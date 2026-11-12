//! Message channels (`docs/spec/message-channels.md`): a source connector session that
//! commits every unit before acknowledging it, the pipeline, and one delivery worker per
//! destination.
//!
//! `MessageRunner` is the channel runner for message channels. Storage goes
//! through [`MessageStore`] (implemented over `packie-store`);
//! filters and mappings are compiled with `packie-transform` (`transform`).

pub(crate) mod astm;
mod au;
mod delivery;
mod files;
mod hl7;
pub(crate) mod pipeline;
mod source;
mod sqlite_store;
pub mod store;
pub(crate) mod transform;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use packie_config::{ChannelConfig, MessageChannel, RuntimeConfig};
use packie_core::{ChannelId, ConnectorId, Direction, EndpointId, MessageId};
use packie_diagnostics::{Counters, TrafficTap};
use packie_transport::{BoxFuture, Transports};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinSet;
use tracing::Instrument;

pub use sqlite_store::{SqliteStore, StoreTapSink};
pub use store::MessageStore;

use crate::runner::{ChannelRunner, EndpointCounters, RunContext};
use crate::status::ChannelShared;
use crate::stop::{StopMode, StopRx};

/// Wakes delivery workers when their queue may have changed (new deliveries, resends).
#[derive(Debug, Default)]
pub(crate) struct Wakeups {
    workers: Mutex<HashMap<(ChannelId, ConnectorId), Arc<Notify>>>,
    pipelines: Mutex<HashMap<ChannelId, mpsc::UnboundedSender<MessageId>>>,
    /// Messages to run through the pipeline once their channel runs (startup
    /// reconciliation, `new_message` resends of a stopped channel).
    pending: Mutex<HashMap<ChannelId, Vec<MessageId>>>,
}

impl Wakeups {
    /// The notifier of one destination worker.
    pub(crate) fn worker(&self, channel: ChannelId, destination: ConnectorId) -> Arc<Notify> {
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(workers.entry((channel, destination)).or_default())
    }

    /// Wakes the worker of `destination`.
    pub(crate) fn wake(&self, channel: ChannelId, destination: ConnectorId) {
        self.worker(channel, destination).notify_one();
    }

    /// Runs `message` through `channel`'s pipeline now, or when the channel next runs.
    pub(crate) fn run_pipeline(&self, channel: ChannelId, message: MessageId) {
        let sent = self
            .pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&channel)
            .is_some_and(|tx| tx.send(message).is_ok());
        if !sent {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(channel)
                .or_default()
                .push(message);
        }
    }

    /// Registers a running instance's pipeline queue and hands it what was pending.
    fn attach(&self, channel: ChannelId, tx: mpsc::UnboundedSender<MessageId>) {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&channel)
            .unwrap_or_default();
        for message in pending {
            let _ = tx.send(message);
        }
        self.pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(channel, tx);
    }

    fn detach(&self, channel: ChannelId) {
        self.pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&channel);
    }
}

/// Everything the source session and the delivery workers of one instance share.
pub(crate) struct MessageContext {
    pub(crate) shared: Arc<ChannelShared>,
    pub(crate) channel: MessageChannel,
    pub(crate) config: Arc<RuntimeConfig>,
    pub(crate) store: Arc<dyn MessageStore>,
    /// The compiled filters and mappings of the definition version last used.
    pub(crate) compiled: Mutex<Option<Arc<transform::Compiled>>>,
    pub(crate) transports: Arc<dyn Transports>,
    pub(crate) endpoint_counters: EndpointCounters,
    pub(crate) wakeups: Arc<Wakeups>,
    pub(crate) traffic_tap: TrafficTap,
}

/// Where one endpoint's traffic goes to the live transcript: inbound is what the peer
/// sent, outbound what the engine sent (a source's acknowledgements, a destination's
/// messages).
#[derive(Clone)]
pub(crate) struct TapPoint {
    tap: TrafficTap,
    channel: ChannelId,
    endpoint: EndpointId,
}

impl TapPoint {
    pub(crate) fn received(&self, bytes: &[u8]) {
        self.tap
            .publish(self.channel, self.endpoint, Direction::Inbound, bytes);
    }

    pub(crate) fn sent(&self, bytes: &[u8]) {
        self.tap
            .publish(self.channel, self.endpoint, Direction::Outbound, bytes);
    }
}

impl MessageContext {
    /// The transcript tap point of `endpoint`.
    pub(crate) fn tap_point(&self, endpoint: EndpointId) -> TapPoint {
        TapPoint {
            tap: self.traffic_tap.clone(),
            channel: self.shared.id,
            endpoint,
        }
    }

    /// Engine-lifetime counters of `endpoint`.
    pub(crate) fn counters_of(&self, endpoint: EndpointId) -> Arc<Counters> {
        self.endpoint_counters
            .get(&endpoint)
            .cloned()
            .unwrap_or_default()
    }

    /// The definition in effect now: in-place changes (filters, mappings, delivery
    /// policy) apply between messages.
    pub(crate) fn current(&self) -> MessageChannel {
        self.current_compiled().0
    }

    /// The definition in effect now with its compiled filters and mappings, compiled
    /// again when the definition's version changed. The apply already compiled it, so a
    /// failure here keeps the previous form (`None` before any).
    pub(crate) fn current_compiled(&self) -> (MessageChannel, Option<Arc<transform::Compiled>>) {
        let (definition, version) = self.shared.versioned_definition();
        let channel = match &*definition {
            ChannelConfig::Message(channel) => channel.clone(),
            ChannelConfig::Bridge(_) => self.channel.clone(),
        };
        let mut cached = self.compiled.lock().unwrap_or_else(PoisonError::into_inner);
        if cached.as_ref().is_none_or(|c| c.version != version) {
            match transform::Compiled::compile(&channel, version, "channel") {
                Ok(compiled) => *cached = Some(Arc::new(compiled)),
                Err(problems) => tracing::error!(
                    event = "channel.compile_failed",
                    problems = problems.len(),
                    "Channel {} filters or mappings do not compile; the previous form stays",
                    self.shared.id
                ),
            }
        }
        (channel, cached.clone())
    }

    /// The pipeline for a committed `received` message; wakes the destinations it routed
    /// to. A store failure leaves the message `received` for reconciliation.
    pub(crate) async fn route(
        &self,
        message: MessageId,
        original: &[u8],
        targets: &[ConnectorId],
    ) -> Option<pipeline::Routed> {
        let (channel, compiled) = self.current_compiled();
        let compiled = compiled?;
        let routed = pipeline::process(
            pipeline::Pipeline {
                store: self.store.as_ref(),
                compiled: &compiled,
                channel: &channel,
            },
            message,
            original,
            targets,
        )
        .await;
        let Some(counters) = &self.shared.messages else {
            return routed.ok();
        };
        match &routed {
            Ok(pipeline::Routed::To(destinations)) => {
                for destination in destinations {
                    self.wakeups.wake(self.shared.id, *destination);
                }
            }
            Ok(pipeline::Routed::Filtered(_)) => store_counter(&counters.filtered),
            Ok(pipeline::Routed::Invalid(reason)) => {
                store_counter(&counters.invalid);
                tracing::warn!(
                    event = "message.invalid",
                    message_id = %message,
                    error_code = reason.as_str(),
                    "Channel {} message {message} is invalid ({reason})",
                    self.shared.id
                );
            }
            Err(error) => pipeline::log_pipeline_failure(message, error),
        }
        routed.ok()
    }
}

fn store_counter(counter: &std::sync::atomic::AtomicU64) {
    crate::status::MessageCounters::bump(counter);
}

/// Runs message channels.
pub(crate) struct MessageRunner {
    pub(crate) store: Arc<dyn MessageStore>,
    pub(crate) wakeups: Arc<Wakeups>,
}

impl ChannelRunner for MessageRunner {
    fn run(&self, cx: RunContext) -> BoxFuture<'static, ()> {
        let ChannelConfig::Message(channel) = cx.channel.clone() else {
            return Box::pin(async {});
        };
        let context = Arc::new(MessageContext {
            shared: Arc::clone(&cx.shared),
            channel,
            config: Arc::clone(&cx.config),
            store: Arc::clone(&self.store),
            compiled: Mutex::new(None),
            transports: Arc::clone(&cx.transports),
            endpoint_counters: Arc::clone(&cx.endpoint_counters),
            wakeups: Arc::clone(&self.wakeups),
            traffic_tap: cx.traffic_tap.clone(),
        });
        Box::pin(run(context, cx.stop))
    }
}

/// One instance: the pipeline queue, the source and the delivery workers, until stopped.
async fn run(cx: Arc<MessageContext>, stop: StopRx) {
    let span = tracing::Span::current();
    let (pipeline_tx, pipeline_rx) = mpsc::unbounded_channel();
    cx.wakeups.attach(cx.shared.id, pipeline_tx);
    let mut tasks = JoinSet::new();
    tasks
        .spawn(pipeline_queue(Arc::clone(&cx), pipeline_rx, stop.clone()).instrument(span.clone()));
    for destination in cx.channel.destinations.iter().filter(|d| d.enabled) {
        tasks.spawn(
            delivery::run_worker(Arc::clone(&cx), destination.clone(), stop.clone())
                .instrument(span.clone()),
        );
    }
    tasks.spawn(source::run_source(Arc::clone(&cx), stop.clone()).instrument(span));
    while let Some(joined) = tasks.join_next().await {
        if let Err(error) = joined
            && error.is_panic()
        {
            // A part that panics takes the instance down; the supervisor restarts it.
            cx.wakeups.detach(cx.shared.id);
            std::panic::resume_unwind(error.into_panic());
        }
    }
    cx.wakeups.detach(cx.shared.id);
}

/// Runs queued pipeline work (reconciled and resent messages) until stopped.
async fn pipeline_queue(
    cx: Arc<MessageContext>,
    mut rx: mpsc::UnboundedReceiver<MessageId>,
    mut stop: StopRx,
) {
    loop {
        let message = tokio::select! {
            biased;
            _ = crate::stop::stopped(&mut stop) => return,
            message = rx.recv() => message,
        };
        let Some(message) = message else {
            return;
        };
        match cx.store.stored_message(message).await {
            Ok(stored) if stored.state == packie_core::MessageState::Received => {
                cx.route(message, &stored.bytes, &stored.resend_targets)
                    .await;
            }
            Ok(_) => {}
            Err(error) => pipeline::log_pipeline_failure(message, &error),
        }
    }
}

/// The drain deadline of a stop, if it drains.
pub(crate) const fn drain_deadline(mode: StopMode) -> Option<tokio::time::Instant> {
    match mode {
        StopMode::Now => None,
        StopMode::Drain(deadline) => Some(deadline),
    }
}
