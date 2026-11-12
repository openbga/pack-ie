//! One delivery worker per destination (`docs/spec/message-channels.md` §4, §5, §8):
//! keeps the destination connection, takes the head of the queue in receipt order,
//! commits the attempt before the first byte, waits for the acknowledgement the
//! delivery requires and records how the attempt ended. Protocol sending lives in
//! `conn`, attempt outcomes in `outcome`, file destinations in `file`.

mod conn;
mod file;
mod outcome;

use std::sync::Arc;
use std::time::Duration;

use packie_astm::LinkDriver;
use packie_au::{AuConfig, StreamDecoder};
use packie_config::{AckMode, Destination, EffectiveDelivery, EndpointConfig, ProtocolSettings};
use packie_core::{AckLevel, AttemptResult, DeliveryState, EndpointState, Timestamp};
use packie_hl7::mllp;
use packie_transport::{Acceptor, ByteStream, FileSink, Link, LinkError, Opener};
use tokio::io::{AsyncReadExt, ReadHalf, WriteHalf};
use tokio::time::Instant;

use crate::message::store::{AttemptFinish, PendingDelivery};
use crate::message::transform::OutputError;
use crate::message::{MessageContext, TapPoint, astm, au, drain_deadline};
use crate::retry::{Backoff, describe_delay};
use crate::status::Role;
use crate::stop::{StopMode, StopRx, sleep_unless_stopped, stopped};
use file::send_file;
use outcome::{Outcome, Progress, stopped_outcome};

type Reader = ReadHalf<Box<dyn ByteStream>>;
type Writer = WriteHalf<Box<dyn ByteStream>>;

/// How often an idle worker looks at its queue without being woken.
const IDLE_POLL: Duration = Duration::from_secs(1);
const READ_CHUNK: usize = 16 * 1024;

/// The destination's protocol state on one connection.
enum Speaker {
    Hl7 {
        decoder: mllp::Decoder,
        ack_mode: AckMode,
        max: usize,
        /// MSH-9.1 values accepted as the acknowledgement.
        ack_types: Vec<String>,
    },
    Astm {
        driver: Box<LinkDriver>,
        timer: Option<(packie_astm::TimerId, Instant)>,
    },
    Au {
        config: AuConfig,
        /// Reads the instrument's ACK and NAK; `None` when the profile is not valid.
        decoder: Option<StreamDecoder>,
    },
}

/// An open destination connection.
struct Conn {
    reader: Reader,
    writer: Writer,
    speaker: Speaker,
    /// Every byte of the attempt in flight was written (ASTM: the transfer completed).
    sent: bool,
    tap: TapPoint,
}

/// Runs the worker of `destination` until the channel stops.
pub(crate) async fn run_worker(
    cx: Arc<MessageContext>,
    destination: Destination,
    mut stop: StopRx,
) {
    let Some(endpoint) = cx.config.endpoint(destination.endpoint).cloned() else {
        return;
    };
    if let Some(cause) = unsupported(&destination.protocol) {
        let role = Role::Destination(destination.id);
        cx.shared.set_endpoint(role, EndpointState::Error);
        tracing::error!(event = "destination.unsupported", destination_id = %destination.id, "{cause}");
        let _ = stopped(&mut stop).await;
        return;
    }
    let mut worker = Worker {
        policy: destination.effective_delivery(&cx.channel.delivery),
        opener: cx.transports.opener(&endpoint, cx.shared.id),
        reconnect: Backoff::new(&cx.channel.retry),
        listener: None,
        conn: None,
        sink: None,
        role: Role::Destination(destination.id),
        destination,
        endpoint,
        cx,
    };
    worker.run(&mut stop).await;
}

fn unsupported(protocol: &ProtocolSettings) -> Option<&'static str> {
    match protocol {
        ProtocolSettings::Raw => Some("raw is not a destination protocol"),
        _ => None,
    }
}

struct Worker {
    cx: Arc<MessageContext>,
    destination: Destination,
    policy: EffectiveDelivery,
    opener: Result<Opener, LinkError>,
    reconnect: Backoff,
    listener: Option<Box<dyn Acceptor>>,
    conn: Option<Conn>,
    /// A delimited-file destination writes through the endpoint's file sink instead.
    sink: Option<Box<dyn FileSink>>,
    endpoint: EndpointConfig,
    role: Role,
}

impl Worker {
    async fn run(&mut self, stop: &mut StopRx) {
        let wake = self
            .cx
            .wakeups
            .worker(self.cx.shared.id, self.destination.id);
        let mut failures = 0_u32;
        loop {
            let running = if self.conn.is_none() && self.sink.is_none() {
                self.connect(&mut failures, stop).await
            } else {
                self.serve_queue(&wake, stop).await
            };
            if !running {
                return;
            }
        }
    }

    /// One connect attempt, or the wait after a failed one. `false` when stopping.
    async fn connect(&mut self, failures: &mut u32, stop: &mut StopRx) -> bool {
        let attached = tokio::select! {
            biased;
            _ = stopped(stop) => return false,
            attached = self.attach() => attached,
        };
        let error = match attached {
            Ok(()) => {
                *failures = 0;
                return true;
            }
            Err(error) => error,
        };
        *failures = failures.saturating_add(1);
        let delay = self.connect_failed(&error, *failures);
        sleep_unless_stopped(stop, delay).await
    }

    /// Sends the head of the queue when it is due, else waits. `false` when stopping.
    async fn serve_queue(&mut self, wake: &tokio::sync::Notify, stop: &mut StopRx) -> bool {
        let pending = self
            .cx
            .store
            .next_delivery(self.cx.shared.id, self.destination.id)
            .await
            .unwrap_or_else(|error| {
                tracing::error!(
                    event = "store.error",
                    error_code = "queue_read_failed",
                    "Queue read failed: {error}"
                );
                None
            });
        let due = pending
            .as_ref()
            .and_then(|p| p.next_attempt_at)
            .filter(|at| *at > Timestamp::now());
        match (pending, due) {
            (Some(pending), None) => self.attempt(pending, stop).await,
            (_, due) => self.idle(wake, due, stop).await,
        }
    }

    /// Waits for work, a retry time or the connection to drop. `false` when stopping.
    async fn idle(
        &mut self,
        wake: &tokio::sync::Notify,
        due: Option<Timestamp>,
        stop: &mut StopRx,
    ) -> bool {
        let wait = due.map_or(IDLE_POLL, |at| {
            at.saturating_duration_since(Timestamp::now())
                .min(IDLE_POLL)
        });
        let mut buf = vec![0_u8; READ_CHUNK];
        let Some(conn) = self.conn.as_mut() else {
            // A file sink has no connection to watch.
            return tokio::select! {
                biased;
                _ = stopped(stop) => false,
                () = wake.notified() => true,
                () = tokio::time::sleep(wait) => true,
            };
        };
        let read = tokio::select! {
            biased;
            _ = stopped(stop) => return false,
            () = wake.notified() => return true,
            () = tokio::time::sleep(wait) => return true,
            read = conn.reader.read(&mut buf) => read,
        };
        match read {
            // Anything arriving between attempts matches no message in flight.
            Ok(n) if n > 0 => {
                conn.tap.received(buf.get(..n).unwrap_or_default());
                log_unmatched(&self.cx, self.destination.id);
            }
            _ => self.drop_conn("peer closed the connection"),
        }
        true
    }

    async fn attach(&mut self) -> Result<(), LinkError> {
        if matches!(self.destination.protocol, ProtocolSettings::Delimited(_)) {
            self.sink = Some(self.cx.transports.file_sink(&self.endpoint)?);
            self.cx.shared.connected(self.role, None);
            return Ok(());
        }
        let shared = &self.cx.shared;
        let opener = self.opener.as_ref().map_err(Clone::clone)?;
        let link = crate::listen::attach(shared, self.role, opener, &mut self.listener).await?;
        shared.connected(self.role, link.peer.clone());
        self.conn = Some(self.conn_for(link));
        Ok(())
    }

    fn conn_for(&self, link: Link) -> Conn {
        let (reader, writer) = tokio::io::split(link.stream);
        let speaker = match &self.destination.protocol {
            ProtocolSettings::Astm(settings) => Speaker::Astm {
                driver: Box::new(LinkDriver::new(astm::link_options(settings))),
                timer: None,
            },
            ProtocolSettings::Au(settings) => {
                let config = au::config(settings).0;
                Speaker::Au {
                    decoder: StreamDecoder::new(config.clone()).ok(),
                    config,
                }
            }
            ProtocolSettings::Hl7(settings) => {
                let max = usize::try_from(settings.max_message_bytes).unwrap_or(usize::MAX);
                let decoder = mllp::Decoder::new(max);
                Speaker::Hl7 {
                    max,
                    decoder: if settings.require_end_cr {
                        decoder
                    } else {
                        decoder.with_optional_end_cr()
                    },
                    ack_mode: settings.ack_mode,
                    ack_types: settings.ack_message_types.clone(),
                }
            }
            _ => Speaker::Hl7 {
                max: usize::MAX,
                decoder: mllp::Decoder::new(1 << 20),
                ack_mode: AckMode::None,
                ack_types: Vec::new(),
            },
        };
        Conn {
            reader,
            writer,
            speaker,
            sent: false,
            tap: self.cx.tap_point(self.destination.endpoint),
        }
    }

    fn drop_conn(&mut self, cause: &str) {
        self.conn = None;
        let state = if self.listener.is_some() {
            EndpointState::Listening
        } else {
            EndpointState::Retrying
        };
        self.cx.shared.set_endpoint(self.role, state);
        tracing::info!(
            event = "endpoint.state_changed",
            destination_id = %self.destination.id,
            "Destination {} connection ended: {cause}",
            self.destination.id
        );
    }

    /// Records a connect failure; returns the delay before the next one.
    fn connect_failed(&self, error: &LinkError, failures: u32) -> Duration {
        let shared = &self.cx.shared;
        shared.count_failure(self.role);
        let delay = self.reconnect.after_failure(error.persistent, failures);
        let state = if error.persistent {
            EndpointState::Error
        } else {
            EndpointState::Retrying
        };
        shared.set_endpoint(self.role, state);
        if let Some(cell) = shared.cell(self.role) {
            let summary = cell.failure_summary(shared.id, error.operation, &error.cause);
            let next = format!("Retrying in {}.", describe_delay(delay));
            shared.record_endpoint_error(&self.cx.counters_of(cell.endpoint_id), summary, &next);
        }
        delay
    }

    /// In-place changes (delivery policy, mapping) apply from the next attempt.
    fn refresh_definition(&mut self) {
        let current = self.cx.current();
        if let Some(destination) = current
            .destinations
            .iter()
            .find(|d| d.id == self.destination.id)
        {
            self.policy = destination.effective_delivery(&current.delivery);
            self.destination = destination.clone();
        }
    }

    /// One attempt for `pending`. `false` when the channel stops.
    async fn attempt(&mut self, mut pending: PendingDelivery, stop: &mut StopRx) -> bool {
        self.refresh_definition();
        let store = Arc::clone(&self.cx.store);
        let Some(body) = self.body(&mut pending).await else {
            return true;
        };
        if let Err(error) = store
            .begin_attempt(pending.delivery_id, Timestamp::now())
            .await
        {
            tracing::warn!(event = "delivery.attempt_refused", delivery_id = %pending.delivery_id, "{error}");
            return true;
        }
        let (outcome, stopping) = self.send_until_stopped(&pending, &body, stop).await;
        let keep = outcome.keep;
        self.finish(&pending, outcome, stopping).await;
        if !keep {
            self.drop_conn("the attempt ended the connection");
        }
        stopping.is_none()
    }

    /// Sends; a stop lets the attempt finish within the drain budget.
    async fn send_until_stopped(
        &mut self,
        pending: &PendingDelivery,
        body: &[u8],
        stop: &mut StopRx,
    ) -> (Outcome, Option<StopMode>) {
        let (outcome, mode) = {
            let send = self.send(pending, body);
            let mut send = std::pin::pin!(send);
            let mode = tokio::select! {
                outcome = &mut send => return (outcome, None),
                mode = stopped(stop) => mode,
            };
            let outcome = match drain_deadline(mode) {
                Some(deadline) => tokio::time::timeout_at(deadline, &mut send).await.ok(),
                None => None,
            };
            (outcome, mode)
        };
        let sent = self.conn.as_ref().is_some_and(|c| c.sent);
        (outcome.unwrap_or_else(|| stopped_outcome(sent)), Some(mode))
    }

    /// The bytes to send: the stored output, or the current mapping run again on the
    /// original (a resend after a transform error). A mapping that fails again makes the
    /// delivery `failed` (`transform_error`) without an attempt.
    async fn body(&mut self, pending: &mut PendingDelivery) -> Option<Vec<u8>> {
        if let Some(body) = pending.body.take() {
            return Some(body);
        }
        let (_, compiled) = self.cx.current_compiled();
        let compiled = compiled?;
        let output = compiled
            .parse(&pending.original)
            .map_err(|reason| OutputError(format!("parse: {reason}")))
            .and_then(|(parsed, _)| {
                compiled.output(self.destination.id, &parsed, &pending.original)
            });
        let store = &self.cx.store;
        let recorded = match &output {
            Ok(body) => {
                store
                    .record_transformed_body(pending.delivery_id, body.clone(), compiled.version)
                    .await
            }
            Err(error) => {
                // The store logs `delivery.transform_failed`.
                store
                    .record_transform_error(pending.delivery_id, error.0.clone(), compiled.version)
                    .await
            }
        };
        if let Err(error) = recorded {
            tracing::warn!(event = "delivery.attempt_refused", delivery_id = %pending.delivery_id, "{error}");
            return None;
        }
        output.ok()
    }

    async fn send(&mut self, pending: &PendingDelivery, body: &[u8]) -> Outcome {
        let send_timeout = Duration::from_secs(u64::from(self.policy.send_timeout_seconds));
        let ack_timeout = Duration::from_secs(u64::from(self.policy.ack_timeout_seconds));
        // The level frozen with the delivery, not the current policy: the store completes
        // the delivery only at that level.
        let required = pending.ack_required;
        if let Some(sink) = &self.sink {
            return send_file(
                &self.cx,
                &self.destination,
                sink.as_ref(),
                pending,
                body,
                send_timeout,
            )
            .await;
        }
        let Some(conn) = self.conn.as_mut() else {
            return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, false)
                .cause("not connected");
        };
        conn.sent = false;
        let counters = self.cx.counters_of(self.destination.endpoint);
        let progress = Progress {
            cx: &self.cx,
            delivery: pending.delivery_id,
            destination: self.destination.id,
        };
        let outcome = match &mut conn.speaker {
            Speaker::Hl7 { .. } => {
                conn.send_hl7(body, send_timeout, ack_timeout, required, &progress)
                    .await
            }
            Speaker::Astm { .. } => conn.send_astm(body, send_timeout).await,
            Speaker::Au { .. } => conn.send_au(body, send_timeout, ack_timeout).await,
        };
        if outcome.sent_completely {
            counters.bytes_out(body.len());
            self.cx.shared.message_sent(body.len());
        }
        outcome
    }

    async fn finish(
        &mut self,
        pending: &PendingDelivery,
        outcome: Outcome,
        stopping: Option<StopMode>,
    ) {
        let retry = Backoff::new(&self.policy.retry);
        let retry_at =
            Timestamp::now().checked_add(retry.delay(pending.attempts.saturating_add(1)));
        let reason = stopping.map(|_| self.cx.shared.stop_reason_code().to_owned());
        let finish = AttemptFinish {
            result: outcome.result,
            ack_level: outcome.level,
            ack_code: outcome.code.clone(),
            ack_control_id: outcome.ack_control_id.clone(),
            error_code: (outcome.result != AttemptResult::Acknowledged)
                .then(|| outcome.result.as_str().to_owned()),
            error_summary: outcome.cause.as_ref().map(|cause| self.summary(cause)),
            ended_at: Timestamp::now(),
            retry_at,
            sent_completely: outcome.sent_completely,
            reason_code: reason,
        };
        match self
            .cx
            .store
            .finish_attempt(pending.delivery_id, finish)
            .await
        {
            Ok(state) => log_state(&self.destination, pending, state, &outcome),
            Err(error) => {
                tracing::error!(event = "store.error", delivery_id = %pending.delivery_id, error_code = "finish_failed", "{error}");
            }
        }
    }

    fn summary(&self, cause: &str) -> String {
        let shared = &self.cx.shared;
        shared.cell(self.role).map_or_else(
            || {
                format!(
                    "Channel {} destination {} send failed: {cause}.",
                    shared.id, self.destination.id
                )
            },
            |cell| cell.failure_summary(shared.id, "send", cause),
        )
    }
}

fn log_state(
    destination: &Destination,
    pending: &PendingDelivery,
    state: DeliveryState,
    outcome: &Outcome,
) {
    let attention = matches!(
        state,
        DeliveryState::Rejected
            | DeliveryState::Failed
            | DeliveryState::TimedOut
            | DeliveryState::OutcomeUnknown
    );
    let code = outcome.code.as_deref().unwrap_or("");
    if attention {
        tracing::warn!(
            event = "delivery.state_changed",
            destination_id = %destination.id,
            delivery_id = %pending.delivery_id,
            message_id = %pending.message_id,
            ack_code = code,
            ack_level = outcome.level.as_str(),
            "Delivery {} to {} is {state}",
            pending.delivery_id,
            destination.id
        );
    } else {
        tracing::info!(
            event = "delivery.state_changed",
            destination_id = %destination.id,
            delivery_id = %pending.delivery_id,
            message_id = %pending.message_id,
            ack_code = code,
            ack_level = outcome.level.as_str(),
            "Delivery {} to {} is {state}",
            pending.delivery_id,
            destination.id
        );
    }
}

pub(super) fn log_unmatched(cx: &MessageContext, destination: packie_core::ConnectorId) {
    tracing::warn!(
        event = "delivery.ack_unmatched",
        destination_id = %destination,
        "Channel {}: an acknowledgement from destination {destination} matched no message in flight",
        cx.shared.id
    );
}
