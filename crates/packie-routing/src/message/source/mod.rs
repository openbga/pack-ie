//! The source connector session (`docs/spec/message-channels.md` §2): attach the source
//! endpoint, receive protocol units, commit each before acknowledging it, run the
//! pipeline, and reconnect by the channel's retry policy. HL7 runs in `mllp`; ASTM and
//! AU share the driver loop in `framed`.

mod framed;
mod mllp;

use std::sync::Arc;
use std::time::Duration;

use packie_au::ReceiveDriver;
use packie_config::ProtocolSettings;
use packie_core::{EndpointState, MessageId, ProtocolKind, Timestamp};
use packie_transport::{
    Acceptor, ByteStream, Link, LinkError, Opener, Operation, PeerProbe, describe_io,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::time::Instant;

use crate::message::store::{Identifiers, Receipt, ReceiptOutcome, StoreError};
use crate::message::{MessageContext, TapPoint, astm, au, drain_deadline, files};
use crate::retry::{Backoff, describe_delay};
use crate::status::{MessageCounters, Phase, Role};
use crate::stop::{StopRx, sleep_unless_stopped, stopped};

pub(crate) use framed::{Completion, FrameDriver, Step, Verdict};

type Reader = ReadHalf<Box<dyn ByteStream>>;
type Writer = WriteHalf<Box<dyn ByteStream>>;

/// Bytes read per call.
const READ_CHUNK: usize = 16 * 1024;

/// Why a source session ended.
enum End {
    /// The channel stops.
    Stopped,
    /// The connection ended or failed.
    Closed { operation: Operation, cause: String },
}

/// Runs the source until the channel stops.
pub(crate) async fn run_source(cx: Arc<MessageContext>, mut stop: StopRx) {
    let Some(endpoint) = cx.config.endpoint(cx.channel.source.endpoint).cloned() else {
        return;
    };
    if let ProtocolSettings::Delimited(settings) = &cx.channel.source.protocol {
        let settings = settings.clone();
        return files::run_file_source(cx, endpoint, settings, stop).await;
    }
    let opener = cx.transports.opener(&endpoint, cx.shared.id);
    let backoff = Backoff::new(&cx.channel.retry);
    let mut listener: Option<Box<dyn Acceptor>> = None;
    loop {
        let attached = tokio::select! {
            biased;
            _ = stopped(&mut stop) => return,
            attached = attach(&cx, &opener, &mut listener) => attached,
        };
        let failure = match attached {
            Err(error) => error,
            Ok(link) => {
                let listens = listener.is_some();
                let end = session(&cx, link, &mut stop).await;
                if listens {
                    cx.shared.set_peer(Role::Source, None);
                }
                match end {
                    End::Stopped => return,
                    // A listener goes back to waiting for its peer: not a failure.
                    End::Closed { .. } if listens => continue,
                    End::Closed { operation, cause } => LinkError::new(operation, cause),
                }
            }
        };
        let delay = schedule_retry(&cx, &backoff, &failure, listener.is_some());
        if !sleep_unless_stopped(&mut stop, delay).await {
            return;
        }
    }
}

/// Dials, or binds and accepts one peer.
async fn attach(
    cx: &MessageContext,
    opener: &Result<Opener, LinkError>,
    listener: &mut Option<Box<dyn Acceptor>>,
) -> Result<Link, LinkError> {
    let shared = &cx.shared;
    let opener = opener.as_ref().map_err(Clone::clone)?;
    let link = crate::listen::attach(shared, Role::Source, opener, listener).await?;
    match opener {
        Opener::Dial(_) => shared.connected(Role::Source, link.peer.clone()),
        // The listener keeps listening: the endpoint stays `listening`, the source
        // session shows the attached peer (`diagnostics.md` §4.3).
        Opener::Listen(_) => shared.set_peer(Role::Source, link.peer.clone()),
    }
    Ok(link)
}

/// Records the failure and returns the delay before the next attempt.
pub(crate) fn schedule_retry(
    cx: &MessageContext,
    backoff: &Backoff,
    failure: &LinkError,
    listens: bool,
) -> Duration {
    let shared = &cx.shared;
    shared.count_failure(Role::Source);
    let delay = backoff.after_failure(failure.persistent, shared.retry_count().saturating_add(1));
    let count = shared.schedule_retry(Timestamp::now().checked_add(delay));
    let state = match (failure.persistent, listens) {
        (true, _) => EndpointState::Error,
        (false, true) => EndpointState::Listening,
        (false, false) => EndpointState::Retrying,
    };
    shared.set_endpoint(Role::Source, state);
    shared.set_phase(Phase::Retrying);
    let Some(cell) = shared.cell(Role::Source) else {
        return delay;
    };
    let summary = cell.failure_summary(shared.id, failure.operation, &failure.cause);
    let next_action = format!("Retrying in {}.", describe_delay(delay));
    shared.record_endpoint_error(
        &cx.counters_of(cell.endpoint_id),
        summary.clone(),
        &next_action,
    );
    tracing::warn!(
        event = "endpoint.retry_scheduled",
        endpoint_id = %cell.endpoint_id,
        endpoint_role = "source",
        endpoint_kind = cell.kind.as_str(),
        retry_count = count,
        error_summary = format!("{summary} {next_action}"),
        "Channel {} source endpoint {}: retry scheduled",
        shared.id,
        cell.endpoint_id
    );
    delay
}

/// One connection's protocol session.
async fn session(cx: &MessageContext, link: Link, stop: &mut StopRx) -> End {
    let peer = link.peer.clone();
    let probe = link.stream.peer_probe();
    let (reader, writer) = tokio::io::split(link.stream);
    let session = Session {
        cx,
        peer,
        reader,
        writer,
        probe,
        probing: false,
        stable: false,
        tap: cx.tap_point(cx.channel.source.endpoint),
        stop: stop.clone(),
        write_limit: response_write_limit(&cx.channel.source.protocol),
    };
    match &cx.channel.source.protocol {
        ProtocolSettings::Hl7(settings) => session.hl7(settings, stop).await,
        ProtocolSettings::Astm(settings) => {
            let driver = packie_astm::LinkDriver::new(astm::link_options(settings));
            session.framed(driver, stop).await
        }
        ProtocolSettings::Au(settings) => {
            let (config, options) = au::config(settings);
            let Ok(driver) = ReceiveDriver::new(config, options) else {
                return unsupported(cx, "the AU settings do not form a valid profile", stop).await;
            };
            let instrument = cx.channel.source.endpoint.to_string();
            session
                .framed(au::SourceDriver::new(driver, instrument), stop)
                .await
        }
        // Delimited sources read files (`files`); raw is refused by validation.
        ProtocolSettings::Delimited(_) | ProtocolSettings::Raw => {
            unsupported(cx, "the source protocol does not run over a stream", stop).await
        }
    }
}

async fn unsupported(cx: &MessageContext, cause: &str, stop: &mut StopRx) -> End {
    tracing::error!(
        event = "source.unsupported",
        "Channel {}: {cause}",
        cx.shared.id
    );
    let _ = stopped(stop).await;
    End::Stopped
}

struct Session<'a> {
    cx: &'a MessageContext,
    peer: Option<String>,
    reader: Reader,
    writer: Writer,
    /// Probes the peer while a message is half received (TCP), so a reset the system
    /// ignored still ends the session (`message-channels.md` §2.2).
    probe: Option<Box<dyn PeerProbe>>,
    /// The probe is on.
    probing: bool,
    /// The failure streak was cleared for this connection.
    stable: bool,
    tap: TapPoint,
    /// Observed while a response is written.
    stop: StopRx,
    /// How long one response (acknowledgement) may take to write.
    write_limit: Duration,
}

/// The longest a source response may take to write: the protocol's frame or receive
/// timeout. A peer that stops reading its acknowledgements loses the connection after it,
/// so it cannot hold the channel's only session (`message-channels.md` §2.3).
fn response_write_limit(protocol: &ProtocolSettings) -> Duration {
    let ms = match protocol {
        ProtocolSettings::Hl7(settings) => settings.frame_timeout_ms,
        ProtocolSettings::Astm(settings) => settings.receive_timeout_ms,
        ProtocolSettings::Au(settings) => settings.frame_timeout_ms,
        ProtocolSettings::Delimited(_) | ProtocolSettings::Raw => 30_000,
    };
    Duration::from_millis(u64::from(ms.max(1)))
}

impl Session<'_> {
    fn receipt(&self, protocol: ProtocolKind, identifiers: Identifiers) -> Receipt {
        Receipt {
            channel_id: self.cx.shared.id,
            channel_label: self.cx.current().label,
            source_endpoint_id: self.cx.channel.source.endpoint,
            source_peer: self.peer.clone(),
            protocol,
            received_at: Timestamp::now(),
            config_version: self.cx.config.config_version,
            duplicate_detection: self.cx.channel.source.duplicate_detection,
            identifiers,
            source_file: None,
        }
    }

    fn counters(&self) -> Option<&MessageCounters> {
        self.cx.shared.messages.as_ref()
    }

    /// The first committed unit proves the connection: the failure streak clears.
    fn mark_stable(&mut self) {
        if !self.stable {
            self.stable = true;
            self.cx.shared.session_stable();
        }
    }

    /// Reports a commit failure (`message_store_unavailable`) or clears it.
    fn store_health(&self, result: &Result<impl Sized, StoreError>) {
        let Some(counters) = self.counters() else {
            return;
        };
        let unavailable = matches!(result, Err(StoreError::Unavailable(_)));
        let before = counters
            .store_unavailable
            .swap(unavailable, std::sync::atomic::Ordering::Relaxed);
        if unavailable && !before {
            tracing::error!(
                event = "store.error",
                error_code = "commit_failed",
                "Channel {}: the store did not commit; no positive acknowledgement is sent",
                self.cx.shared.id
            );
        }
        if before != unavailable {
            self.cx.shared.refresh();
        }
    }

    /// Whether the session waits for the rest of a message: the peer is probed meanwhile.
    fn waiting_for_rest(&mut self, waiting: bool) {
        if waiting != self.probing
            && let Some(probe) = &self.probe
        {
            probe.waiting(waiting);
            self.probing = waiting;
        }
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, End> {
        match self.reader.read(buf).await {
            Ok(0) => Err(End::Closed {
                operation: Operation::Read,
                cause: "peer closed the connection".into(),
            }),
            Ok(n) => {
                self.tap.received(buf.get(..n).unwrap_or_default());
                self.cx
                    .counters_of(self.cx.channel.source.endpoint)
                    .bytes_in(n);
                if let Some(cell) = self.cx.shared.cell(Role::Source) {
                    cell.touch();
                }
                Ok(n)
            }
            Err(error) => Err(End::Closed {
                operation: Operation::Read,
                cause: describe_io(&error),
            }),
        }
    }

    /// A response the peer did not read in time: logged and shown as the source endpoint's
    /// last error; the connection closes.
    fn record_stall(&self, cause: &str) {
        let shared = &self.cx.shared;
        let Some(cell) = shared.cell(Role::Source) else {
            return;
        };
        let summary = cell.failure_summary(shared.id, Operation::Write, cause);
        shared.record_endpoint_error(
            &self.cx.counters_of(cell.endpoint_id),
            summary.clone(),
            "The connection was closed; the sender can connect again.",
        );
        tracing::warn!(
            event = "source.response_stalled",
            endpoint_id = %cell.endpoint_id,
            endpoint_role = "source",
            error_summary = summary.as_str(),
            "Channel {} source endpoint {}: response not read in time; connection closed",
            shared.id,
            cell.endpoint_id
        );
    }

    /// Writes a response within `write_limit`. A stop ends the write at once, a drain at
    /// its deadline; a peer that does not read in time loses the connection.
    async fn write(&mut self, bytes: &[u8]) -> Result<(), End> {
        let limit = Instant::now() + self.write_limit;
        let writer = &mut self.writer;
        let written = async {
            writer.write_all(bytes).await?;
            writer.flush().await
        };
        let outcome = {
            let mut written = std::pin::pin!(written);
            tokio::select! {
                biased;
                result = tokio::time::timeout_at(limit, &mut written) => result,
                mode = stopped(&mut self.stop) => {
                    let Some(drain) = drain_deadline(mode) else {
                        return Err(End::Stopped);
                    };
                    tokio::time::timeout_at(drain.min(limit), &mut written).await
                }
            }
        };
        let cause = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(describe_io(&error)),
            Err(_) => {
                let cause = format!(
                    "the peer did not read the response within {} ms",
                    self.write_limit.as_millis()
                );
                self.record_stall(&cause);
                Some(cause)
            }
        };
        if let Some(cause) = cause {
            return Err(End::Closed {
                operation: Operation::Write,
                cause,
            });
        }
        self.tap.sent(bytes);
        self.cx
            .counters_of(self.cx.channel.source.endpoint)
            .bytes_out(bytes.len());
        Ok(())
    }
}

/// A duplicate was recorded (`message.duplicate`); it is not processed.
pub(crate) fn log_duplicate(counters: &MessageCounters, message: MessageId) {
    MessageCounters::bump(&counters.duplicates);
    tracing::info!(event = "message.duplicate", message_id = %message, "Duplicate recorded, not processed");
}

/// `message.committed` (debug) and, for a new version of an earlier message,
/// `message.identity_reused`.
pub(crate) fn log_committed(outcome: &ReceiptOutcome, bytes: usize, ack_code: &str) {
    tracing::debug!(
        event = "message.committed",
        message_id = %outcome.message_id,
        bytes,
        ack_code,
        "Message {} committed and acknowledged",
        outcome.message_id
    );
    if let Some(earlier) = outcome.revision_of {
        tracing::info!(
            event = "message.identity_reused",
            message_id = %outcome.message_id,
            "Message {} reuses the identity of {earlier} with different content",
            outcome.message_id
        );
    }
}
