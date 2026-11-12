//! ASTM E1381/E1394 for message channels: the original bytes of a received transfer,
//! its identifiers and abort reasons (`docs/spec/message-channels.md` §2.2, §3, §7).
//!
//! Spec paths count the record type as field 1 (`H.3` is the message control id);
//! `packie-astm` counts fields after the record type, so `H.n` is `field(n - 1)`.

use std::time::Duration;

use packie_astm::{
    Action, Event, LineRole, LinkDriver, LinkOptions, ReceiveAbortReason, Record, RecordType,
    SendAbortReason, TimerId, parse_message_records,
};
use packie_config::{AstmRole, AstmSettings};
use packie_core::ProtocolKind;

use crate::message::source::{Completion, FrameDriver, Step};
use crate::message::store::Identifiers;

/// The original bytes of a received message: its records, each ended by CR.
pub(crate) fn original_bytes(records: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(records.iter().map(|r| r.len() + 1).sum());
    for record in records {
        bytes.extend_from_slice(record);
        bytes.push(b'\r');
    }
    bytes
}

/// The records of an outgoing body (records separated by CR or CR LF).
pub(crate) fn records_of(body: &[u8]) -> Vec<Vec<u8>> {
    body.split(|b| *b == b'\r' || *b == b'\n')
        .filter(|r| !r.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// Where the specimen id sits in the first order record (field number counting the record
/// type as 1, optional component): `O.3` unless the source connector says otherwise.
pub(crate) type SamplePosition = (usize, Option<usize>);

/// The standard place of the specimen id, `O.3`.
pub(crate) const STANDARD_SAMPLE: SamplePosition = (3, None);

/// The specimen id position a source connector's settings name; the standard one for a
/// path that validation would have refused.
pub(crate) fn sample_position(settings: &AstmSettings) -> SamplePosition {
    settings.sample_id_position().unwrap_or(STANDARD_SAMPLE)
}

/// Identifiers of a received message, or `Err` when its records do not parse. The
/// specimen id (`sample_id`, and the third part of the source identity) is read at
/// `sample`.
pub(crate) fn identifiers(
    original: &[u8],
    sample: SamplePosition,
) -> Result<Identifiers, &'static str> {
    let records = parse_message_records(original).map_err(|_| "unparseable")?;
    let first = |kind: RecordType| records.iter().find(|r| r.record_type() == kind);
    let field = |record: Option<&Record>, spec_position: usize| {
        record
            .and_then(|r| r.field(spec_position - 1))
            .map(|f| f.decoded().to_owned())
            .filter(|v| !v.is_empty())
    };
    let header = first(RecordType::Header);
    if header.is_none() {
        return Err("header_missing");
    }
    let order = first(RecordType::Order);
    let sample_id = order
        .and_then(|record| record.field(sample.0 - 1))
        .and_then(|f| match sample.1 {
            None => Some(f.decoded()),
            Some(component) => f
                .repeats()
                .first()
                .and_then(|repeat| repeat.components().get(component - 1))
                .map(packie_astm::Component::decoded),
        })
        .map(str::to_owned)
        .filter(|v| !v.is_empty());
    // H.5 whole (sender name with its version components), H.14, the first O.3.
    let parts = [field(header, 5), field(header, 14), sample_id.clone()];
    let source_identity = parts
        .iter()
        .any(Option::is_some)
        .then(|| parts.map(Option::unwrap_or_default).join("|"));
    Ok(Identifiers {
        control_id: field(header, 3),
        sample_id,
        patient_id: field(first(RecordType::Patient), 3),
        message_type: Some("astm".to_owned()),
        source_identity,
    })
}

/// The original bytes of a transfer from its committed wire frames, when they form a
/// complete message: every frame valid, in sequence, the last one ending the terminator
/// record (`L`). `None` for anything incomplete.
pub(crate) fn recover(frames: &[u8]) -> Option<Vec<u8>> {
    let mut text = Vec::new();
    let mut last: Option<packie_astm::Frame> = None;
    for raw in frames.split_inclusive(|b| *b == packie_astm::LF) {
        let frame = packie_astm::parse_frame_within(raw, packie_astm::MAX_LONG_FRAME_TEXT).ok()?;
        text.extend_from_slice(frame.text());
        last = Some(frame);
    }
    let last = last?;
    if last.end() != packie_astm::FrameEnd::Final {
        return None;
    }
    let records = records_of(&text);
    let terminator = records.last()?;
    (terminator.first() == Some(&b'L')).then(|| original_bytes(&records))
}

/// `abort_reason` for a failed receive.
pub(crate) const fn receive_abort_reason(reason: ReceiveAbortReason) -> &'static str {
    match reason {
        ReceiveAbortReason::RetryLimitExceeded => "retry_limit_exceeded",
        ReceiveAbortReason::ReceiveTimeout => "receive_timeout",
        ReceiveAbortReason::IncompleteRecord => "incomplete_record",
        ReceiveAbortReason::EmptyTransfer | ReceiveAbortReason::EmptyFrame => "empty_transfer",
        ReceiveAbortReason::MessageTooLarge
        | ReceiveAbortReason::InputBufferExceeded
        | ReceiveAbortReason::FrameLimitExceeded
        | ReceiveAbortReason::WireBytesExceeded => "message_too_large",
    }
}

/// Operator text for an abandoned send.
pub(crate) const fn send_abort_cause(reason: SendAbortReason) -> &'static str {
    match reason {
        SendAbortReason::EstablishmentFailed => "the receiver did not accept ENQ",
        SendAbortReason::RetryLimitExceeded => "a frame was rejected too often",
        SendAbortReason::ReplyTimeout => "no reply to a frame in time",
        SendAbortReason::ReceiverInterrupted => "the receiver interrupted the transfer",
    }
}

/// Driver options from the connector settings.
pub(crate) fn link_options(settings: &AstmSettings) -> LinkOptions {
    LinkOptions {
        role: match settings.role {
            AstmRole::Computer => LineRole::Computer,
            AstmRole::Instrument => LineRole::Instrument,
        },
        max_message_bytes: usize::try_from(settings.max_message_bytes).unwrap_or(usize::MAX),
        reply_timeout: Duration::from_millis(u64::from(settings.ack_timeout_ms)),
        contention_yield_timeout: Duration::from_millis(u64::from(settings.contention_wait_ms)),
        max_frame_retransmissions: u8::try_from(settings.max_frame_retries).unwrap_or(u8::MAX),
        receive_timeout: Duration::from_millis(u64::from(settings.receive_timeout_ms)),
        max_frame_text: usize::try_from(settings.max_frame_chars).unwrap_or(usize::MAX),
        max_enq_attempts: u8::try_from(settings.enq_attempts).unwrap_or(u8::MAX),
        busy_retry_delay: Duration::from_millis(u64::from(settings.busy_retry_ms)),
        ..LinkOptions::default()
    }
}

/// The source session's view of the ASTM link driver: a received message completes
/// with its records.
impl FrameDriver for LinkDriver {
    type Event = Event;
    type Timer = TimerId;
    const PROTOCOL: ProtocolKind = ProtocolKind::Astm;
    const FAILED: &'static str = "ASTM link error";
    const FRAMES_ARE_MESSAGE: bool = false;

    fn bytes(bytes: Vec<u8>) -> Event {
        Event::Bytes(bytes)
    }

    fn expired(timer: TimerId) -> Event {
        Event::TimerExpired(timer)
    }

    fn steps(&mut self, event: Event) -> Option<Vec<Step<Event, TimerId>>> {
        let actions = self.handle(event).ok()?;
        Some(
            actions
                .into_iter()
                .map(|action| match action {
                    Action::Write(bytes) => Step::Write(bytes),
                    Action::PersistFrame { frame_id, bytes } => Step::Persist {
                        bytes,
                        persisted: Event::Persisted(frame_id),
                    },
                    Action::MessageReceived(records) => {
                        Step::Complete(Completion::Records(records))
                    }
                    Action::ReceiveAborted(reason) => Step::Abort(receive_abort_reason(reason)),
                    Action::StartTimer { timer_id, duration } => {
                        Step::StartTimer(timer_id, duration)
                    }
                    Action::StopTimer => Step::StopTimer,
                    Action::MessageSent | Action::SendAborted(_) => Step::Nothing,
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGE: &[&[u8]] = &[
        b"H|\\^&|CTRL-7||SIMANALYZER^1.0|||||||P|1|20260101120000",
        b"P|1|PID-TEST-7",
        b"O|1|SID-7||^^^GLU",
        b"R|1|^^^GLU|5.4|mmol/L",
        b"L|1|N",
    ];

    #[test]
    fn original_bytes_end_each_record_with_cr() {
        let records: Vec<Vec<u8>> = MESSAGE.iter().map(|r| r.to_vec()).collect();
        let bytes = original_bytes(&records);
        assert!(bytes.ends_with(b"L|1|N\r"));
        assert_eq!(records_of(&bytes), records);
    }

    #[test]
    fn identifiers_follow_the_spec_paths() {
        let records: Vec<Vec<u8>> = MESSAGE.iter().map(|r| r.to_vec()).collect();
        let ids = identifiers(&original_bytes(&records), STANDARD_SAMPLE).unwrap();
        assert_eq!(ids.control_id.as_deref(), Some("CTRL-7"));
        assert_eq!(ids.sample_id.as_deref(), Some("SID-7"));
        assert_eq!(ids.patient_id.as_deref(), Some("PID-TEST-7"));
        assert_eq!(ids.message_type.as_deref(), Some("astm"));
        assert_eq!(
            ids.source_identity.as_deref(),
            Some("SIMANALYZER^1.0|20260101120000|SID-7")
        );
        assert!(
            identifiers(b"P|1\r", STANDARD_SAMPLE).is_err(),
            "records without a header are not a message"
        );
    }

    #[test]
    fn the_specimen_id_is_read_where_the_source_says() {
        let records: Vec<Vec<u8>> = [
            "H|\\^&|||HEMA^1.0|||||||P|1|20260101120000",
            "O|1||^^SYN-0001^B|^^^^WBC",
            "L|1|N",
        ]
        .into_iter()
        .map(|r| r.as_bytes().to_vec())
        .collect();
        let original = original_bytes(&records);
        let at = |path: &str| {
            let settings: AstmSettings =
                serde_json::from_value(serde_json::json!({ "sample_id_path": path })).unwrap();
            identifiers(&original, sample_position(&settings)).unwrap()
        };
        // the standard place holds nothing for this layout
        let standard = at("O.3");
        assert_eq!(standard.sample_id, None);
        assert_eq!(
            standard.source_identity.as_deref(),
            Some("HEMA^1.0|20260101120000|")
        );
        let chosen = at("O.4.3");
        assert_eq!(chosen.sample_id.as_deref(), Some("SYN-0001"));
        assert_eq!(
            chosen.source_identity.as_deref(),
            Some("HEMA^1.0|20260101120000|SYN-0001")
        );
        // the whole field, and a component the field does not have
        assert_eq!(at("O.4").sample_id.as_deref(), Some("^^SYN-0001^B"));
        assert_eq!(at("O.4.9").sample_id, None);
        assert_eq!(at("O.40").sample_id, None);
    }

    #[test]
    fn complete_frames_recover_and_partial_ones_do_not() {
        let records: Vec<Vec<u8>> = MESSAGE.iter().map(|r| r.to_vec()).collect();
        let frames: Vec<u8> = packie_astm::encode_records(&records).unwrap().concat();
        assert_eq!(recover(&frames), Some(original_bytes(&records)));
        let partial: Vec<u8> = packie_astm::encode_records(&records[..3]).unwrap().concat();
        assert_eq!(recover(&partial), None, "no terminator record");
        assert_eq!(recover(&frames[..frames.len() - 3]), None, "a cut frame");
    }

    #[test]
    fn link_options_carry_the_connector_settings() {
        let settings: AstmSettings = serde_json::from_value(serde_json::json!({
            "receive_timeout_ms": 12_000, "max_frame_chars": 64, "ack_timeout_ms": 9000
        }))
        .unwrap();
        let options = link_options(&settings);
        assert_eq!(options.receive_timeout, Duration::from_secs(12));
        assert_eq!(options.max_frame_text, 64);
        assert_eq!(options.reply_timeout, Duration::from_secs(9));
        assert_eq!(options.max_enq_attempts, 3);
        assert_eq!(options.busy_retry_delay, Duration::from_secs(10));
        let settings: AstmSettings = serde_json::from_value(serde_json::json!({
            "enq_attempts": 7, "busy_retry_ms": 4000, "max_frame_chars": 64000
        }))
        .unwrap();
        let options = link_options(&settings);
        assert_eq!(options.max_enq_attempts, 7);
        assert_eq!(options.busy_retry_delay, Duration::from_secs(4));
        assert_eq!(options.max_frame_text, 64_000);
    }

    #[test]
    fn long_frames_recover_like_short_ones() {
        let long = vec![format!("R|1|^^^GLU|{}", "9".repeat(900)).into_bytes()];
        let frames: Vec<u8> = packie_astm::encode_records_within(&long, 2000)
            .unwrap()
            .concat();
        assert_eq!(frames.iter().filter(|b| **b == packie_astm::STX).count(), 1);
        let mut records = vec![b"H|\\^&".to_vec()];
        records.extend(long);
        records.push(b"L|1|N".to_vec());
        let frames: Vec<u8> = packie_astm::encode_records_within(&records, 2000)
            .unwrap()
            .concat();
        assert_eq!(recover(&frames), Some(original_bytes(&records)));
    }
}
