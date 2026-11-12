//! The contract between the engine and whatever runs channels.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use packie_config::{
    ApplyReport, Bootstrap, ChannelConfig, ConfigDiff, EndpointConfig, RuntimeConfig, TapFramer,
};
use packie_core::{
    ChannelId, ChannelState, EndpointId, EndpointState, SessionState, StopReason, Timestamp,
};
use packie_diagnostics::{
    BridgeDetail, ChannelSnapshot, DeliveryQueueSnapshot, DestinationCounters, DestinationSnapshot,
    EndpointRef, EndpointSnapshot, FlowSnapshot, MessageCounters, MessageDetail, SourceSession,
    TapSnapshot, TrafficSnapshot, TrafficTap, Unresolved,
};
use packie_ipc::ChannelCommand;
use packie_ipc::checks::{ProbeReport, SerialPort};
use packie_ipc::messages::MessageFilter;
use packie_ipc::messages::{
    BodyRequest, CloseRequest, DeliveryRecord, MessageBody, MessageList, MessageQuery,
    MessageRecord, RejectionCode, ResendRequest, ResendResult,
};
use packie_ipc::reports::DestinationTransmission;
use tokio::sync::Notify;

/// A boxed future, so [`ChannelRegistry`] can be a trait object.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Runs the configured channels and endpoints. The engine holds one as
/// `Arc<dyn ChannelRegistry>`; `packie-routing` provides the real one.
///
/// The engine serializes calls to [`apply`](Self::apply), [`command`](Self::command) and
/// [`shutdown`](Self::shutdown); [`snapshot`](Self::snapshot) may be called at any time
/// from any thread and must be cheap (it runs every publication interval).
pub trait ChannelRegistry: Send + Sync + 'static {
    /// Brings the running channels in line with `config`, which is already validated and
    /// saved. `diff` is relative to the previously applied document (an empty document at
    /// start): start `added`, restart `changed`, stop `removed`, leave `unchanged` alone.
    /// A channel that fails to start reports it in its snapshot; apply itself never fails.
    ///
    /// Returns one outcome per channel of either document (`unchanged`, `updated`,
    /// `started`, `restarted`, `stopped`, `removed`, `failed{reason}`); any `failed` makes
    /// the apply `partial`. A failed channel keeps its previous definition and instance and
    /// is retried by the next apply even when unchanged there.
    fn apply<'a>(
        &'a self,
        config: &'a RuntimeConfig,
        diff: &'a ConfigDiff,
    ) -> BoxFuture<'a, ApplyReport>;

    /// Every configured channel and endpoint, in configuration order.
    fn snapshot(&self) -> RegistrySnapshot;

    /// An operator command on one channel.
    fn command(
        &self,
        channel: ChannelId,
        command: ChannelCommand,
    ) -> BoxFuture<'_, Result<(), RegistryError>>;

    /// Stops every channel, giving them up to `drain` to finish in-flight work.
    fn shutdown(&self, drain: Duration) -> BoxFuture<'_, ()>;

    /// Checks the registry adds to configuration validation (for example `packie-transform`'s
    /// `mapping_invalid` and `filter_invalid`); any error rejects the document. Default:
    /// none.
    fn validate(&self, config: &RuntimeConfig) -> Vec<packie_config::Problem> {
        let _ = config;
        Vec::new()
    }

    /// What the message store reports, for the node snapshot. Default: no store.
    fn store_status(&self) -> Option<packie_diagnostics::StoreFigures> {
        None
    }

    /// A page of stored messages; the query is already checked
    /// ([`MessageQuery::check`]).
    fn query_messages(
        &self,
        query: MessageQuery,
    ) -> BoxFuture<'_, Result<MessageList, RegistryError>> {
        let _ = query;
        not_implemented("Stored messages")
    }

    /// Per-destination delivery totals of the messages `filter` selects (the transmission
    /// report's totals, complete however many rows it lists), destinations in the order of
    /// their first message.
    fn transmission_totals(
        &self,
        filter: MessageFilter,
    ) -> BoxFuture<'_, Result<Vec<DestinationTransmission>, RegistryError>> {
        let _ = filter;
        not_implemented("Transmission reports")
    }

    /// One message with deliveries and attempts.
    fn message(
        &self,
        id: packie_core::MessageId,
    ) -> BoxFuture<'_, Result<MessageRecord, RegistryError>> {
        let _ = id;
        not_implemented("Stored messages")
    }

    /// A rendered body.
    fn message_body(
        &self,
        request: BodyRequest,
    ) -> BoxFuture<'_, Result<MessageBody, RegistryError>> {
        let _ = request;
        not_implemented("Message bodies")
    }

    /// Resend (`docs/spec/message-channels.md` §9). Per-destination precondition failures
    /// go into the result; a failure of the whole command (unknown message) is an error.
    /// Serialized with [`apply`](Self::apply) and [`command`](Self::command).
    fn resend(&self, request: ResendRequest) -> BoxFuture<'_, Result<ResendResult, RegistryError>> {
        let _ = request;
        not_implemented("Resend")
    }

    /// Close a delivery with a reason; the reason is already checked
    /// ([`CloseRequest::has_valid_reason`]). Serialized like [`resend`](Self::resend).
    fn close_delivery(
        &self,
        request: CloseRequest,
    ) -> BoxFuture<'_, Result<DeliveryRecord, RegistryError>> {
        let _ = request;
        not_implemented("Closing deliveries")
    }

    /// The serial ports the operating system reports, sorted by name; `used_by` is left
    /// empty for the engine to fill in.
    fn serial_ports(&self) -> BoxFuture<'_, Result<Vec<SerialPort>, RegistryError>> {
        not_implemented("Serial port lists")
    }

    /// Probes `endpoint` without sending data or changing anything, within `timeout`
    /// (`docs/spec/diagnostics.md` §10). A serial port a running channel holds is reported
    /// as in use, not opened.
    fn probe_endpoint<'a>(
        &'a self,
        endpoint: &'a EndpointConfig,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<ProbeReport, RegistryError>> {
        let _ = (endpoint, timeout);
        not_implemented("Endpoint tests")
    }
}

fn not_implemented<'a, T: Send + 'a>(what: &str) -> BoxFuture<'a, Result<T, RegistryError>> {
    let message = format!("{what} are not available in this build of the engine.");
    Box::pin(async move { Err(RegistryError::NotImplemented(message)) })
}

/// Channel and endpoint snapshots from a registry.
#[derive(Debug, Clone, Default)]
pub struct RegistrySnapshot {
    /// Channels, in configuration order.
    pub channels: Vec<ChannelSnapshot>,
    /// Endpoints, in configuration order.
    pub endpoints: Vec<EndpointSnapshot>,
}

/// A registry command failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// No such channel.
    #[error("no channel {0}")]
    NotFound(ChannelId),
    /// A precondition failed; the text says what is wrong and what to do.
    #[error("{message}")]
    Rejected {
        /// Stable code.
        code: RejectionCode,
        /// Operator-facing explanation.
        message: String,
    },
    /// The registry cannot do this.
    #[error("{0}")]
    NotImplemented(String),
    /// The command was understood but failed; the text is operator-facing.
    #[error("{0}")]
    Failed(String),
    /// The request itself is malformed or out of range; reaches clients as `bad_request`.
    #[error("{0}")]
    BadRequest(String),
}

/// What the engine gives a registry when it creates it.
#[derive(Clone)]
pub struct EngineContext {
    /// The bootstrap document (paths already resolved).
    pub bootstrap: Arc<Bootstrap>,
    /// The state directory, for the message store.
    pub state_dir: PathBuf,
    /// The traffic tap; publish segments here.
    pub tap: TrafficTap,
    /// Notify to publish a snapshot now (debounced), for example on a state change.
    pub snapshot_now: Arc<Notify>,
    /// Present when the bootstrap enables `test_hooks`.
    pub test_hooks: Option<TestHooks>,
    /// The secret store the bootstrap selects; transports resolve secrets through it.
    pub secrets: Arc<dyn packie_config::SecretResolver>,
}

/// Scenario test hooks the store and pipeline observe (`docs/spec/scenarios.md` §7.2).
#[derive(Debug, Clone)]
pub struct TestHooks {
    hold_commits: Arc<tokio::sync::watch::Sender<bool>>,
}

impl TestHooks {
    pub(crate) fn new() -> Self {
        Self {
            hold_commits: Arc::new(tokio::sync::watch::channel(false).0),
        }
    }

    /// `true` while store commits must be held. The store writer waits for `false` before
    /// completing each commit.
    #[must_use]
    pub fn commits_held(&self) -> tokio::sync::watch::Receiver<bool> {
        self.hold_commits.subscribe()
    }

    pub(crate) fn hold_commits(&self, hold: bool) {
        self.hold_commits.send_replace(hold);
    }
}

/// A registry that runs nothing: every enabled channel reports `stopped` with reason
/// `not_implemented`, disabled ones `disabled_by_config`; endpoints read `disconnected`
/// or `disabled`. Used until `packie-routing` provides channels.
#[derive(Debug, Default)]
pub struct NoopRegistry {
    state: Mutex<NoopState>,
}

#[derive(Debug)]
struct NoopState {
    config: Arc<RuntimeConfig>,
    since: HashMap<ChannelId, Timestamp>,
    endpoint_since: HashMap<EndpointId, Timestamp>,
}

impl Default for NoopState {
    fn default() -> Self {
        Self {
            config: Arc::new(RuntimeConfig::empty()),
            since: HashMap::new(),
            endpoint_since: HashMap::new(),
        }
    }
}

impl NoopRegistry {
    /// A registry with nothing configured.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for EngineContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineContext")
            .field("state_dir", &self.state_dir)
            .finish_non_exhaustive()
    }
}

impl ChannelRegistry for NoopRegistry {
    fn apply<'a>(
        &'a self,
        config: &'a RuntimeConfig,
        diff: &'a ConfigDiff,
    ) -> BoxFuture<'a, ApplyReport> {
        Box::pin(async move {
            let now = Timestamp::now();
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            for change in &diff.channels {
                state.since.insert(change.id, now);
            }
            state.since.retain(|id, _| config.channel(*id).is_some());
            for endpoint in &config.endpoints {
                state.endpoint_since.entry(endpoint.id).or_insert(now);
            }
            state
                .endpoint_since
                .retain(|id, _| config.endpoint(*id).is_some());
            state.config = Arc::new(config.clone());
            ApplyReport::planned(diff)
        })
    }

    fn snapshot(&self) -> RegistrySnapshot {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Timestamp::now();
        let config = &state.config;
        let channels = config
            .channels
            .iter()
            .map(|c| stopped_channel(config, c, *state.since.get(&c.id()).unwrap_or(&now)))
            .collect();
        let endpoints = config
            .endpoints
            .iter()
            .map(|e| idle_endpoint(config, e, *state.endpoint_since.get(&e.id).unwrap_or(&now)))
            .collect();
        RegistrySnapshot {
            channels,
            endpoints,
        }
    }

    fn command(
        &self,
        _channel: ChannelId,
        _command: ChannelCommand,
    ) -> BoxFuture<'_, Result<(), RegistryError>> {
        Box::pin(async {
            Err(RegistryError::NotImplemented(
                "This build of the engine does not run channels yet.".into(),
            ))
        })
    }

    fn shutdown(&self, _drain: Duration) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

fn idle_state(enabled: bool) -> EndpointState {
    if enabled {
        EndpointState::Disconnected
    } else {
        EndpointState::Disabled
    }
}

fn idle_endpoint(config: &RuntimeConfig, e: &EndpointConfig, since: Timestamp) -> EndpointSnapshot {
    EndpointSnapshot {
        id: e.id,
        label: e.label.clone(),
        kind: e.kind(),
        enabled: e.enabled,
        state: idle_state(e.enabled),
        state_since: since,
        peer: None,
        held_by: None,
        settings_summary: e.settings_summary(),
        consecutive_failures: 0,
        retry_count: 0,
        next_retry_at: None,
        traffic: TrafficSnapshot::default(),
        last_error: None,
        rejections: None,
        channels: config
            .channels
            .iter()
            .filter(|c| c.endpoint_ids().contains(&e.id))
            .map(ChannelConfig::id)
            .collect(),
    }
}

/// An idle reference to endpoint `id` in `role`.
fn idle_ref(config: &RuntimeConfig, id: EndpointId, role: &str) -> EndpointRef {
    let endpoint = config.endpoint(id);
    EndpointRef {
        endpoint_id: id,
        label: endpoint.map_or_else(|| id.to_string(), |e| e.label.clone()),
        kind: endpoint.map_or(packie_core::EndpointKind::TcpClient, EndpointConfig::kind),
        role: role.to_owned(),
        state: idle_state(endpoint.is_some_and(|e| e.enabled)),
        consecutive_failures: 0,
        active_peer: None,
        last_activity: None,
    }
}

fn idle_flow(buffer: &packie_config::BufferPolicy, tap: bool) -> FlowSnapshot {
    FlowSnapshot {
        max_segments: buffer.max_buffered_segments,
        max_bytes: buffer.max_bytes,
        messages: tap.then_some(0),
        ..FlowSnapshot::default()
    }
}

fn bridge_detail(
    config: &RuntimeConfig,
    b: &packie_config::BridgeChannel,
    since: Timestamp,
) -> BridgeDetail {
    let tap = b.tap.framer != TapFramer::None;
    BridgeDetail {
        session: SessionState::Closed,
        session_since: since,
        connect_strategy: b.connection.connect_strategy,
        receiver: b.connection.receiver,
        endpoint_a: idle_ref(config, b.endpoint_a, "a"),
        endpoint_b: idle_ref(config, b.endpoint_b, "b"),
        a_to_b: idle_flow(&b.buffer, tap),
        b_to_a: idle_flow(&b.buffer, tap),
        tap: TapSnapshot {
            framer: b.tap.framer,
            record_messages: b.tap.record_messages,
            framing_errors: tap.then_some(0),
            tap_dropped: tap.then_some(0),
            transcript_viewers: 0,
        },
    }
}

fn message_detail(config: &RuntimeConfig, m: &packie_config::MessageChannel) -> MessageDetail {
    MessageDetail {
        source: idle_ref(config, m.source.endpoint, "source"),
        protocol: m.source.protocol.kind(),
        session: SourceSession::Closed,
        counters: MessageCounters::default(),
        last_received_at: None,
        destinations: m
            .destinations
            .iter()
            .map(|d| DestinationSnapshot {
                destination_id: d.id,
                label: d.label.clone(),
                enabled: d.enabled,
                endpoint: idle_ref(config, d.endpoint, d.id.as_str()),
                protocol: d.protocol.kind(),
                queue: DeliveryQueueSnapshot::default(),
                unresolved: Unresolved::default(),
                counters: DestinationCounters::default(),
                last_attempt_at: None,
                last_ack_at: None,
                last_error: None,
            })
            .collect(),
        unresolved: Unresolved::default(),
    }
}

fn stopped_channel(
    config: &RuntimeConfig,
    channel: &ChannelConfig,
    since: Timestamp,
) -> ChannelSnapshot {
    let reason = if channel.enabled() {
        StopReason::NotImplemented
    } else {
        StopReason::DisabledByConfig
    };
    ChannelSnapshot {
        id: channel.id(),
        label: channel.label().to_owned(),
        kind: channel.kind(),
        enabled: channel.enabled(),
        state: ChannelState::Stopped,
        stop_reason: Some(reason),
        state_since: since,
        config_version: config.config_version,
        instance_started_at: None,
        endpoints: channel.endpoint_ids(),
        traffic: TrafficSnapshot::default(),
        retry_count: 0,
        next_retry_at: None,
        last_error: None,
        recent_errors: Vec::new(),
        allowed_actions: Vec::new(),
        bridge: match channel {
            ChannelConfig::Bridge(b) => Some(bridge_detail(config, b, since)),
            ChannelConfig::Message(_) => None,
        },
        message: match channel {
            ChannelConfig::Message(m) => Some(message_detail(config, m)),
            ChannelConfig::Bridge(_) => None,
        },
    }
}
