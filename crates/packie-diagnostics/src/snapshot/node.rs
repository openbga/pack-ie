//! The node and its aggregates: totals, store status, configuration versions.

use packie_core::{ChannelState, EndpointState, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ChannelSnapshot, EndpointSnapshot};
use crate::alert::Alert;

/// Everything the engine publishes once per interval and on change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EngineSnapshot {
    /// The node.
    pub node: NodeSnapshot,
    /// Every configured channel, in configuration order.
    pub channels: Vec<ChannelSnapshot>,
    /// Every configured endpoint, in configuration order.
    pub endpoints: Vec<EndpointSnapshot>,
    /// Active alerts, newest first.
    pub alerts: Vec<Alert>,
}

/// Aggregate state of the node (§3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// A configuration is active, the store works, disk is not critical, and every enabled
    /// channel is running or starting.
    Healthy,
    /// Something needs attention (see the node state rules).
    Degraded,
    /// Shutdown completed; the final snapshot.
    Stopped,
}

/// How the engine is hosted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HostingMode {
    /// `packie run` in a terminal.
    Console,
    /// An OS service (Windows service, launchd, systemd).
    Service,
}

/// Counts per state; every key is always present.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StateCounts {
    /// `starting`.
    pub starting: u32,
    /// `running`.
    pub running: u32,
    /// `retrying`.
    pub retrying: u32,
    /// `degraded`.
    pub degraded: u32,
    /// `stopped`.
    pub stopped: u32,
}

impl StateCounts {
    /// Counts `states`.
    #[must_use]
    pub fn of(states: impl IntoIterator<Item = ChannelState>) -> Self {
        let mut counts = Self::default();
        for state in states {
            let slot = match state {
                ChannelState::Starting => &mut counts.starting,
                ChannelState::Running => &mut counts.running,
                ChannelState::Retrying => &mut counts.retrying,
                ChannelState::Degraded => &mut counts.degraded,
                ChannelState::Stopped => &mut counts.stopped,
            };
            *slot = slot.saturating_add(1);
        }
        counts
    }
}

/// Endpoint counts per state; every key is always present.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EndpointCounts {
    /// `disabled`.
    pub disabled: u32,
    /// `connecting`.
    pub connecting: u32,
    /// `listening`.
    pub listening: u32,
    /// `connected`.
    pub connected: u32,
    /// `retrying`.
    pub retrying: u32,
    /// `disconnected`.
    pub disconnected: u32,
    /// `error`.
    pub error: u32,
}

impl EndpointCounts {
    /// Counts `states`.
    #[must_use]
    pub fn of(states: impl IntoIterator<Item = EndpointState>) -> Self {
        let mut counts = Self::default();
        for state in states {
            let slot = match state {
                EndpointState::Disabled => &mut counts.disabled,
                EndpointState::Connecting => &mut counts.connecting,
                EndpointState::Listening => &mut counts.listening,
                EndpointState::Connected => &mut counts.connected,
                EndpointState::Retrying => &mut counts.retrying,
                EndpointState::Disconnected => &mut counts.disconnected,
                EndpointState::Error => &mut counts.error,
            };
            *slot = slot.saturating_add(1);
        }
        counts
    }
}

/// The node (§3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NodeSnapshot {
    /// Node id from the bootstrap document.
    pub node_id: String,
    /// Node name.
    pub display_name: String,
    /// Engine version.
    pub version: String,
    /// How the engine is hosted.
    pub hosting_mode: HostingMode,
    /// When the engine started.
    pub started_at: Timestamp,
    /// When this snapshot was taken.
    pub captured_at: Timestamp,
    /// Seconds since start.
    #[ts(type = "number")]
    pub uptime_seconds: u64,
    /// Aggregate state.
    pub node_state: NodeState,
    /// Configuration versions.
    pub config: ConfigStatus,
    /// Channels per state.
    pub channel_counts: StateCounts,
    /// Endpoints per state.
    pub endpoint_counts: EndpointCounts,
    /// Node-wide totals.
    pub totals: Totals,
    /// The message store.
    pub store: StoreStatus,
}

/// Configuration versions of the node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConfigStatus {
    /// The version in effect; absent when none is active.
    #[ts(type = "number | null")]
    pub active_version: Option<u64>,
    /// The version being applied, during an apply.
    #[ts(type = "number | null")]
    pub pending_version: Option<u64>,
    /// The latest apply.
    pub last_apply: Option<LastApply>,
}

/// The latest apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LastApply {
    /// When it finished.
    pub at: Timestamp,
    /// How it ended.
    pub result: ApplyResult,
}

/// How an apply ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ApplyResult {
    /// Every channel took its new definition.
    Applied,
    /// The document was invalid; nothing changed.
    Rejected,
    /// Some channels failed; the others took effect.
    Partial,
}

/// Node-wide totals. Store figures are absent until the message store reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Totals {
    /// Bytes received by all endpoints since start.
    #[ts(type = "number")]
    pub bytes_in: u64,
    /// Bytes sent by all endpoints since start.
    #[ts(type = "number")]
    pub bytes_out: u64,
    /// Messages currently stored.
    #[ts(type = "number | null")]
    pub messages_stored: Option<u64>,
    /// Deliveries rejected, failed, timed out or with unknown outcome.
    #[ts(type = "number | null")]
    pub deliveries_unresolved: Option<u64>,
    /// Deliveries queued.
    #[ts(type = "number | null")]
    pub deliveries_queued: Option<u64>,
    /// Active alerts by severity.
    pub alerts_active: AlertCounts,
}

/// Active alerts by severity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AlertCounts {
    /// `warning`.
    pub warning: u32,
    /// `critical`.
    pub critical: u32,
}

/// The message store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StoreStatus {
    /// Whether it works.
    pub state: StoreState,
    /// Database and bodies size, when known.
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
    /// Free space on the state volume, when known.
    #[ts(type = "number | null")]
    pub free_disk_bytes: Option<u64>,
    /// Total size of the state volume, when known.
    #[ts(type = "number | null")]
    pub total_disk_bytes: Option<u64>,
}

/// What the message store reports about itself, through the channel registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreFigures {
    /// Whether it works.
    pub state: StoreState,
    /// Database and bodies size.
    pub size_bytes: Option<u64>,
    /// Messages currently stored.
    pub messages_stored: Option<u64>,
    /// Deliveries rejected, failed, timed out or with unknown outcome.
    pub deliveries_unresolved: Option<u64>,
    /// Deliveries queued.
    pub deliveries_queued: Option<u64>,
}

/// State of the message store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StoreState {
    /// Open and committing.
    Ok,
    /// Cannot be opened or a commit failed.
    Unavailable,
    /// This build has no message store.
    NotConfigured,
}

/// What the node state is derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeInputs {
    /// A runtime configuration is active.
    pub config_active: bool,
    /// The store is not `unavailable`.
    pub store_ok: bool,
    /// Free disk is below `disk_critical_megabytes`.
    pub disk_critical: bool,
    /// Shutdown completed.
    pub stopped: bool,
}

impl NodeState {
    /// The node state from the channels and the node inputs (§3 node state rules).
    #[must_use]
    pub fn derive(channels: &[ChannelSnapshot], inputs: NodeInputs) -> Self {
        if inputs.stopped {
            return Self::Stopped;
        }
        let channel_trouble = channels.iter().any(|c| match c.state {
            ChannelState::Retrying | ChannelState::Degraded => true,
            ChannelState::Stopped => c.stop_reason.is_none_or(|r| !r.is_intentional()),
            ChannelState::Starting | ChannelState::Running => false,
        });
        if channel_trouble || !inputs.config_active || !inputs.store_ok || inputs.disk_critical {
            Self::Degraded
        } else {
            Self::Healthy
        }
    }
}
