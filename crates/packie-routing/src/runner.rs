//! Channel runners and the per-instance supervisor task.
//!
//! A [`ChannelRunner`] runs one channel instance until its stop signal fires. The
//! registry starts every instance through [`spawn`], which runs the runner in its own
//! task: a panic is caught there, reported on that channel only, and the runner is
//! started again after the channel's retry delay. Bridge channels use
//! [`crate::bridge::BridgeRunner`]; message channels get a runner once the message
//! pipeline exists (see [`crate::RoutingRegistry`]).

use std::collections::HashMap;
use std::sync::Arc;

use packie_config::{ChannelConfig, RuntimeConfig};
use packie_core::{EndpointId, Timestamp};
use packie_diagnostics::{Counters, ErrorSummary, TrafficTap};
use packie_engine::BoxFuture;
use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::TapSink;
use crate::retry::{Backoff, describe_delay};
use crate::status::{ChannelShared, Phase};
use crate::stop::{StopRx, is_requested, sleep_unless_stopped};
use packie_transport::Transports;

/// Engine-lifetime counters per endpoint, shared by the channel instances that use it.
pub(crate) type EndpointCounters = Arc<HashMap<EndpointId, Arc<Counters>>>;

/// Everything a runner gets for one channel instance.
#[derive(Clone)]
pub(crate) struct RunContext {
    /// State the runner reports through.
    pub(crate) shared: Arc<ChannelShared>,
    /// The definition the instance runs.
    pub(crate) channel: ChannelConfig,
    /// The whole active document (endpoint definitions).
    pub(crate) config: Arc<RuntimeConfig>,
    pub(crate) transports: Arc<dyn Transports>,
    /// Engine-lifetime counters per endpoint.
    pub(crate) endpoint_counters: EndpointCounters,
    pub(crate) traffic_tap: TrafficTap,
    pub(crate) tap_sink: Option<Arc<dyn TapSink>>,
    /// Fires once when the instance must stop.
    pub(crate) stop: StopRx,
}

impl RunContext {
    /// The counters of `endpoint` (fresh ones if the registry has none).
    pub(crate) fn counters_of(&self, endpoint: EndpointId) -> Arc<Counters> {
        self.endpoint_counters
            .get(&endpoint)
            .cloned()
            .unwrap_or_default()
    }
}

/// Runs one kind of channel.
pub(crate) trait ChannelRunner: Send + Sync + 'static {
    /// Runs the instance until `cx.stop` fires; returns once everything it opened is
    /// closed. Reports state through `cx.shared`.
    fn run(&self, cx: RunContext) -> BoxFuture<'static, ()>;
}

/// Starts the instance in its own task and keeps it running across panics.
pub(crate) fn spawn(runner: Arc<dyn ChannelRunner>, cx: RunContext) -> JoinHandle<()> {
    let span = cx.shared.span.clone();
    tokio::spawn(supervise(runner, cx).instrument(span))
}

async fn supervise(runner: Arc<dyn ChannelRunner>, cx: RunContext) {
    let shared = Arc::clone(&cx.shared);
    let mut stop = cx.stop.clone();
    let backoff = match &cx.channel {
        ChannelConfig::Bridge(bridge) => Backoff::new(&bridge.retry),
        ChannelConfig::Message(message) => Backoff::new(&message.retry),
    };
    let mut crashes = 0_u32;
    tracing::info!(
        event = "channel.started",
        config_version = shared.config_version,
        "Channel {} started from configuration version {}",
        shared.id,
        shared.config_version
    );
    loop {
        let task = tokio::spawn(runner.run(cx.clone()).instrument(tracing::Span::current()));
        let error = match task.await {
            Ok(()) => return,
            Err(error) if error.is_panic() => error,
            Err(_) => return,
        };
        if is_requested(&stop) || shared.is_stopping() {
            return;
        }
        crashes = crashes.saturating_add(1);
        let delay = backoff.delay(crashes);
        let next_action = format!("Restarting the channel in {}.", describe_delay(delay));
        let summary = ErrorSummary::new(
            format!("Channel {} failed unexpectedly: internal error.", shared.id),
            Some(&next_action),
        );
        tracing::error!(
            event = "channel.failed",
            error_summary = %summary.summary,
            "Channel {} runner failed: {error}. {next_action}",
            shared.id
        );
        shared.record_error(&summary);
        shared.schedule_retry(Timestamp::now().checked_add(delay));
        shared.set_phase(Phase::Retrying);
        if !sleep_unless_stopped(&mut stop, delay).await {
            return;
        }
    }
}
