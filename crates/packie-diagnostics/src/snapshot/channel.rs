//! One channel, its traffic and last errors, and the bridge detail.

use packie_config::{ConnectStrategy, TapFramer};
use packie_core::{
    BridgeSide, ChannelCommand, ChannelId, ChannelKind, ChannelState, EndpointId, EndpointKind,
    EndpointState, SessionState, StopReason, Timestamp,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::MessageDetail;

/// An operator-facing failure: what failed and what the engine does next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ErrorSummary {
    /// When it happened.
    pub at: Timestamp,
    /// What failed, such as `Channel x A endpoint "LIS" connect failed: connection refused`.
    pub summary: String,
    /// What happens next, such as `Retrying in 4s`.
    pub next_action: Option<String>,
}

impl ErrorSummary {
    /// A summary dated now.
    pub fn new(summary: impl Into<String>, next_action: Option<&str>) -> Self {
        Self {
            at: Timestamp::now(),
            summary: summary.into(),
            next_action: next_action.map(str::to_owned),
        }
    }
}

/// Traffic counters at one instant; see [`Counters`](crate::Counters).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TrafficSnapshot {
    /// Bytes received.
    #[ts(type = "number")]
    pub bytes_in: u64,
    /// Bytes sent.
    #[ts(type = "number")]
    pub bytes_out: u64,
    /// Messages received.
    #[ts(type = "number")]
    pub messages_in: u64,
    /// Messages sent.
    #[ts(type = "number")]
    pub messages_out: u64,
    /// Failures.
    #[ts(type = "number")]
    pub errors: u64,
    /// Last byte or message in either direction.
    pub last_activity: Option<Timestamp>,
}

/// One channel (§4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChannelSnapshot {
    /// The channel.
    #[serde(rename = "channel_id")]
    pub id: ChannelId,
    /// Its label.
    pub label: String,
    /// Bridge or message.
    pub kind: ChannelKind,
    /// `enabled` in configuration.
    pub enabled: bool,
    /// Supervision state.
    pub state: ChannelState,
    /// Why it is stopped, when `state` is `stopped`.
    pub stop_reason: Option<StopReason>,
    /// When `state` last changed.
    pub state_since: Timestamp,
    /// Configuration version of the definition the instance runs.
    #[ts(type = "number")]
    pub config_version: u64,
    /// Start of the running instance.
    pub instance_started_at: Option<Timestamp>,
    /// Endpoints it uses.
    pub endpoints: Vec<EndpointId>,
    /// Traffic through the channel (bridges: in = A→B, out = B→A).
    pub traffic: TrafficSnapshot,
    /// Consecutive failed attempts; resets once a session is stable.
    pub retry_count: u32,
    /// When the next attempt is scheduled, while `retrying`.
    pub next_retry_at: Option<Timestamp>,
    /// Last failure, until cleared.
    pub last_error: Option<ErrorSummary>,
    /// The latest failures, newest first, at most `retain_recent_errors`.
    pub recent_errors: Vec<ErrorSummary>,
    /// Operator commands whose preconditions hold now, so clients do not mirror the rules.
    pub allowed_actions: Vec<ChannelCommand>,
    /// Bridge detail, for bridge channels.
    pub bridge: Option<BridgeDetail>,
    /// Message detail, for message channels.
    pub message: Option<MessageDetail>,
}

impl ChannelSnapshot {
    /// The commands an operator may issue for a channel in this state
    /// (`docs/spec/operator-console.md` §5).
    #[must_use]
    pub fn actions_for(
        enabled: bool,
        state: ChannelState,
        stop_reason: Option<StopReason>,
    ) -> Vec<ChannelCommand> {
        if !enabled {
            return Vec::new();
        }
        match (state, stop_reason) {
            (ChannelState::Stopped, Some(StopReason::OperatorPaused)) => {
                vec![ChannelCommand::Resume]
            }
            (ChannelState::Stopped, _) => Vec::new(),
            _ => vec![ChannelCommand::Pause, ChannelCommand::Restart],
        }
    }
}

/// One endpoint as one channel uses it (§4.2 endpoint reference).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EndpointRef {
    /// The endpoint.
    pub endpoint_id: EndpointId,
    /// Its label.
    pub label: String,
    /// Transport kind.
    pub kind: EndpointKind,
    /// `a`, `b`, `source` or a destination id.
    pub role: String,
    /// State of this channel's use of the endpoint.
    pub state: EndpointState,
    /// Failure streak.
    pub consecutive_failures: u32,
    /// Connected peer, when known.
    pub active_peer: Option<String>,
    /// Last successful I/O of this side.
    pub last_activity: Option<Timestamp>,
}

/// A bridge's session, sides and buffers (§4.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BridgeDetail {
    /// The live session.
    pub session: SessionState,
    /// When `session` last changed.
    pub session_since: Timestamp,
    /// Pairing strategy.
    pub connect_strategy: ConnectStrategy,
    /// The receiver side under `receiver_then_sender`.
    pub receiver: Option<BridgeSide>,
    /// Side A.
    pub endpoint_a: EndpointRef,
    /// Side B.
    pub endpoint_b: EndpointRef,
    /// Bytes A→B.
    pub a_to_b: FlowSnapshot,
    /// Bytes B→A.
    pub b_to_a: FlowSnapshot,
    /// The tap.
    pub tap: TapSnapshot,
}

/// One direction of a bridge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FlowSnapshot {
    /// Bytes accepted into the buffer since the instance started.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Messages framed by the tap; absent without a framer.
    #[ts(type = "number | null")]
    pub messages: Option<u64>,
    /// Segments waiting in the buffer.
    pub buffered_segments: u32,
    /// Bytes waiting in the buffer.
    #[ts(type = "number")]
    pub buffered_bytes: u64,
    /// The buffer's segment limit.
    pub max_segments: u32,
    /// The buffer's byte limit.
    #[ts(type = "number")]
    pub max_bytes: u64,
    /// An overflow episode is in progress.
    pub overflowing: bool,
    /// Overflow episodes since the instance started.
    #[ts(type = "number")]
    pub overflow_count: u64,
}

/// A bridge tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TapSnapshot {
    /// The framer.
    pub framer: TapFramer,
    /// Observed messages are stored.
    pub record_messages: bool,
    /// Unframed runs longer than `max_message_bytes`; absent without a framer.
    #[ts(type = "number | null")]
    pub framing_errors: Option<u64>,
    /// Observations the tap lost; absent without a framer.
    #[ts(type = "number | null")]
    pub tap_dropped: Option<u64>,
    /// Clients viewing the live transcript (0 or 1).
    pub transcript_viewers: u32,
}
