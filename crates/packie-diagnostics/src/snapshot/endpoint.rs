//! One endpoint across the channels that use it.

use packie_core::{ChannelId, EndpointId, EndpointKind, EndpointState, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ErrorSummary, TrafficSnapshot};

/// One endpoint (§5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EndpointSnapshot {
    /// The endpoint.
    #[serde(rename = "endpoint_id")]
    pub id: EndpointId,
    /// Its label.
    pub label: String,
    /// Transport kind.
    pub kind: EndpointKind,
    /// `enabled` in configuration.
    pub enabled: bool,
    /// Least healthy state over the channels using it.
    pub state: EndpointState,
    /// When `state` last changed.
    pub state_since: Timestamp,
    /// Connected peer (address, port name or server), when known.
    pub peer: Option<String>,
    /// The channel holding this exclusive endpoint (serial port, listener), if any.
    pub held_by: Option<ChannelId>,
    /// Non-secret one-line description, such as `COM3 9600 8N1` or `0.0.0.0:3001`.
    pub settings_summary: String,
    /// Failure streak.
    pub consecutive_failures: u32,
    /// Retry count as seen by the holding channel.
    pub retry_count: u32,
    /// Next attempt of the holding channel.
    pub next_retry_at: Option<Timestamp>,
    /// Traffic counters, all channels, since start.
    pub traffic: TrafficSnapshot,
    /// Last failure, until cleared.
    pub last_error: Option<ErrorSummary>,
    /// Peers a listener refused, when it is a listener.
    pub rejections: Option<PeerRejections>,
    /// Channels using this endpoint.
    pub channels: Vec<ChannelId>,
}

/// Peers a listener refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PeerRejections {
    /// Refused since the listener was bound.
    #[ts(type = "number")]
    pub count: u64,
    /// When the latest refusal happened.
    pub last_at: Option<Timestamp>,
    /// The latest refused peer.
    pub last_peer: Option<String>,
    /// `not_allowed` or `session_active`.
    pub last_reason: Option<String>,
}
