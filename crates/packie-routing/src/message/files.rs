//! Delimited files (`docs/spec/message-channels.md` §2.4, §8): the source reads complete
//! files from a folder or SFTP endpoint and commits every record or group of a file in
//! one transaction before the file is consumed; the destination writes one file per
//! delivery through the endpoint's file sink.

use std::sync::Arc;

use packie_config::{DelimitedSettings, EndpointConfig};
use packie_core::{MessageId, MessageState, ProtocolKind, Timestamp};
use packie_ipc::messages::SourceFile;
use packie_transport::{FileSource, FileUnit, LinkError, Operation};

use crate::message::MessageContext;
use crate::message::source::{log_committed, log_duplicate, schedule_retry};
use crate::message::store::{Identifiers, PendingDelivery, Receipt, ReceiptOutcome, StoreError};
use crate::message::transform::{
    delimited_identifiers, delimited_reason, row_of, stored_template, template,
};
use crate::retry::Backoff;
use crate::status::{MessageCounters, Role};
use crate::stop::{StopRx, sleep_unless_stopped, stopped};

/// The templates a source reads files with and stores their messages with.
struct Templates {
    read: packie_files::Template,
    stored: packie_files::Template,
}

/// What one source file becomes.
enum FileVerdict {
    /// One message per record or group (none for a file without rows).
    Messages(Vec<(Receipt, Vec<u8>)>),
    /// Over `max_file_bytes`: not read; one `aborted` message records it.
    TooLarge,
    /// Unreadable as configured: the whole file is kept as one `invalid` message.
    Invalid(&'static str),
}

/// Runs a delimited-file source until the channel stops.
pub(crate) async fn run_file_source(
    cx: Arc<MessageContext>,
    endpoint: EndpointConfig,
    settings: DelimitedSettings,
    mut stop: StopRx,
) {
    let backoff = Backoff::new(&cx.channel.retry);
    let templates = Templates {
        read: template(&settings),
        stored: stored_template(&settings),
    };
    loop {
        let failure = match cx.transports.file_source(&endpoint) {
            Err(error) => error,
            Ok(mut source) => {
                // Larger files are not read into memory; they arrive `too_large`.
                source.set_max_file_bytes(settings.max_file_bytes);
                cx.shared.connected(Role::Source, None);
                match watch(&cx, source.as_mut(), &templates, &mut stop).await {
                    None => return,
                    Some(error) => error,
                }
            }
        };
        let delay = schedule_retry(&cx, &backoff, &failure, false);
        if !sleep_unless_stopped(&mut stop, delay).await {
            return;
        }
    }
}

/// Takes complete files until the channel stops (`None`) or the source fails.
async fn watch(
    cx: &MessageContext,
    source: &mut dyn FileSource,
    templates: &Templates,
    stop: &mut StopRx,
) -> Option<LinkError> {
    loop {
        let unit = tokio::select! {
            biased;
            _ = stopped(stop) => return None,
            unit = source.next() => unit,
        };
        let unit = match unit {
            Ok(unit) => unit,
            Err(error) => return Some(error),
        };
        if let Err(error) = take_file(cx, source, &unit, templates).await {
            // Not committed: the file stays and is found again by a new source.
            return Some(LinkError::new(
                Operation::Read,
                format!("the file could not be committed ({error})"),
            ));
        }
    }
}

/// Commits what the file holds, consumes it, then runs the pipeline for each message.
async fn take_file(
    cx: &MessageContext,
    source: &mut dyn FileSource,
    unit: &FileUnit,
    templates: &Templates,
) -> Result<(), StoreError> {
    let store = &cx.store;
    let committed = match verdict(cx, unit, templates) {
        FileVerdict::Messages(units) => store
            .commit_batch(units.clone())
            .await
            .map(|o| (o.iter().map(|o| o.message_id).collect(), o, units)),
        FileVerdict::TooLarge => {
            let receipt = receipt(cx, unit, delimited_identifiers(None), None);
            match store.begin_transfer(receipt, Vec::new()).await {
                Ok(message) => store
                    .abort_transfer(message, "file_too_large")
                    .await
                    .map(|()| (vec![message], Vec::new(), Vec::new())),
                Err(error) => Err(error),
            }
        }
        FileVerdict::Invalid(reason) => {
            let receipt = receipt(cx, unit, delimited_identifiers(None), None);
            match store.commit_received(receipt, unit.bytes.clone()).await {
                Ok(outcome) => {
                    record_invalid(cx, &outcome, reason, unit.bytes.len()).await;
                    Ok((vec![outcome.message_id], Vec::new(), Vec::new()))
                }
                Err(error) => Err(error),
            }
        }
    };
    store_health(cx, committed.as_ref().err());
    let (recorded, outcomes, units) = committed?;
    cx.shared.session_stable();
    if let Err(error) = source.consumed(unit).await {
        log_consume_failed(cx, &recorded, &error, &unit.name);
    }
    for (outcome, (_, bytes)) in outcomes.iter().zip(&units) {
        after_commit(cx, outcome, bytes).await;
    }
    Ok(())
}

/// A committed file that could not be moved or deleted. Names the file by the messages
/// recorded from it: a file name may carry identifiers (`diagnostics.md` §8).
fn log_consume_failed(cx: &MessageContext, recorded: &[MessageId], error: &LinkError, name: &str) {
    let first = recorded
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    tracing::warn!(
        event = "file.consume_failed",
        message_id = first.as_str(),
        messages = recorded.len(),
        operation = %error.operation,
        "Channel {}: the file of message {first} was committed but could not be moved or \
         deleted ({}); reading it again records duplicates",
        cx.shared.id,
        without_name(&error.cause, name)
    );
}

/// `cause` with every mention of the file `name` replaced by `<file>`: its relative path,
/// its last component and its stem (a suffixed `stem-1.ext` variant included), so text from
/// a file transport never carries a message-derived name into logs or summaries.
pub(crate) fn without_name(cause: &str, name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    let mut text = cause.to_owned();
    for part in [name, base, stem] {
        if !part.is_empty() {
            text = text.replace(part, "<file>");
        }
    }
    text
}

/// Counts and logs a committed message and runs its pipeline.
async fn after_commit(cx: &MessageContext, outcome: &ReceiptOutcome, bytes: &[u8]) {
    log_committed(outcome, bytes.len(), "none");
    if let Some(counters) = &cx.shared.messages {
        cx.shared.message_received(bytes.len());
        if outcome.state == MessageState::Duplicate {
            log_duplicate(counters, outcome.message_id);
            return;
        }
    }
    if outcome.state != MessageState::Duplicate {
        drop(cx.route(outcome.message_id, bytes, &[]).await);
    }
}

async fn record_invalid(cx: &MessageContext, outcome: &ReceiptOutcome, reason: &str, len: usize) {
    if let Some(counters) = &cx.shared.messages {
        cx.shared.message_received(len);
        MessageCounters::bump(&counters.invalid);
    }
    drop(
        cx.store
            .record_invalid(outcome.message_id, reason.to_owned())
            .await,
    );
    tracing::warn!(
        event = "message.invalid",
        message_id = %outcome.message_id,
        error_code = reason,
        "Channel {} file message {} is invalid ({reason})",
        cx.shared.id,
        outcome.message_id
    );
}

/// Reports a commit failure (`message_store_unavailable`) or clears it.
fn store_health(cx: &MessageContext, error: Option<&StoreError>) {
    let Some(counters) = &cx.shared.messages else {
        return;
    };
    let unavailable = matches!(error, Some(StoreError::Unavailable(_)));
    counters
        .store_unavailable
        .store(unavailable, std::sync::atomic::Ordering::Relaxed);
}

/// Reads the file and splits it into messages per `record_mode`. Each message's original
/// is its rows written with the source columns (and header line, with `header_row`).
fn verdict(cx: &MessageContext, unit: &FileUnit, templates: &Templates) -> FileVerdict {
    if unit.too_large.is_some() {
        return FileVerdict::TooLarge;
    }
    let document = match packie_files::read(&unit.bytes, &templates.read) {
        Ok(document) => document,
        Err(packie_files::Error::FileTooLarge { .. }) => return FileVerdict::TooLarge,
        Err(error) => return FileVerdict::Invalid(delimited_reason(&error)),
    };
    let groups = match document.messages(&templates.read) {
        Ok(groups) => groups,
        Err(error) => return FileVerdict::Invalid(delimited_reason(&error)),
    };
    let file_hash = packie_store::content_hash(&unit.bytes);
    let mut first_row = 1_u32;
    let mut units = Vec::with_capacity(groups.len());
    for group in groups {
        let Ok(bytes) = packie_files::write(group, &templates.stored) else {
            return FileVerdict::Invalid("malformed_file");
        };
        let rows = u32::try_from(group.len()).unwrap_or(u32::MAX);
        let last_row = first_row.saturating_add(rows.saturating_sub(1));
        let mut identifiers = delimited_identifiers(group.first().map(row_of).as_deref());
        identifiers.source_identity = Some(format!("{}|{first_row}-{last_row}", unit.name));
        let file = SourceFile {
            name: unit.name.clone(),
            file_hash: file_hash.clone(),
            first_row,
            last_row,
        };
        units.push((receipt(cx, unit, identifiers, Some(file)), bytes));
        first_row = last_row.saturating_add(1);
    }
    FileVerdict::Messages(units)
}

fn receipt(
    cx: &MessageContext,
    unit: &FileUnit,
    identifiers: Identifiers,
    file: Option<SourceFile>,
) -> Receipt {
    let source_file = file.or_else(|| {
        Some(SourceFile {
            name: unit.name.clone(),
            // A file over the size limit was not read: its hash is unknown.
            file_hash: if unit.too_large.is_some() {
                String::new()
            } else {
                packie_store::content_hash(&unit.bytes)
            },
            first_row: 0,
            last_row: 0,
        })
    });
    Receipt {
        channel_id: cx.shared.id,
        channel_label: cx.current().label,
        source_endpoint_id: cx.channel.source.endpoint,
        source_peer: None,
        protocol: ProtocolKind::Delimited,
        received_at: unit.modified.min(Timestamp::now()),
        config_version: cx.config.config_version,
        duplicate_detection: cx.channel.source.duplicate_detection,
        identifiers,
        source_file,
    }
}

/// A destination file name from the `file_name` template: `{message_id}`,
/// `{control_id}`, `{sample_id}`, `{date}` (`yyyyMMdd`), `{time}` (`HHmmss`) and `{seq}`
/// (the message's receipt sequence). Characters that cannot be part of a file name
/// become `_`.
pub(crate) fn file_name(pattern: &str, pending: &PendingDelivery, now: Timestamp) -> String {
    let stamp = crate::message::hl7::hl7_timestamp(now);
    let (date, time) = stamp.split_at(stamp.len().min(8));
    let values = [
        ("{message_id}", pending.message_id.to_string()),
        (
            "{control_id}",
            pending.control_id.clone().unwrap_or_default(),
        ),
        ("{sample_id}", pending.sample_id.clone().unwrap_or_default()),
        ("{date}", date.to_owned()),
        ("{time}", time.get(..6).unwrap_or(time).to_owned()),
        ("{seq}", pending.sequence.to_string()),
    ];
    let mut name = pattern.to_owned();
    for (key, value) in values {
        name = name.replace(key, &safe(&value));
    }
    name
}

fn safe(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use packie_core::MessageId;

    fn pending() -> PendingDelivery {
        PendingDelivery {
            delivery_id: packie_core::DeliveryId::generate(),
            message_id: MessageId::generate(),
            sequence: 7,
            next_attempt_at: None,
            attempts: 0,
            max_attempts: 1,
            ack_required: packie_core::AckLevel::HandedOff,
            body: None,
            original: Vec::new(),
            control_id: Some("CTRL/1".into()),
            sample_id: Some("SID-1".into()),
        }
    }

    #[test]
    fn file_names_fill_the_template_with_safe_text() {
        let now = Timestamp::from_unix_millis(1_767_225_600_000).unwrap_or_else(Timestamp::now);
        let pending = pending();
        let name = file_name(
            "r-{control_id}-{sample_id}-{seq}-{date}{time}.csv",
            &pending,
            now,
        );
        assert!(name.starts_with("r-CTRL_1-SID-1-7-"), "{name}");
        assert!(name.ends_with(".csv"));
        let id = file_name("orig-{message_id}.csv", &pending, now);
        assert_eq!(id, format!("orig-{}.csv", pending.message_id));
    }
}
