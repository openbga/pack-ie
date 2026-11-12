//! Bridge channels (`docs/spec/configuration.md` §6.2): live bytes between two endpoints.

use std::collections::BTreeMap;

use packie_core::{BridgeSide, ChannelId, EndpointId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::policy::{RetryPolicy, TapRetention};
use crate::defaults::{mib_1, ms_5000, yes};

/// A bridge channel (§6.2): endpoint A and endpoint B, one live session, bytes unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct BridgeChannel {
    /// Stable operator-chosen id.
    #[serde(rename = "channel_id")]
    pub id: ChannelId,
    /// Operator-facing name, 1–100 characters.
    pub label: String,
    /// Default `true`.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Free operator metadata.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// Side A (serial, `tcp_client` or `tcp_listener`).
    pub endpoint_a: EndpointId,
    /// Side B (serial, `tcp_client` or `tcp_listener`).
    pub endpoint_b: EndpointId,
    /// How the two sides are paired into a session.
    #[serde(default)]
    pub connection: Connection,
    /// Per-direction buffer bounds.
    #[serde(default)]
    pub buffer: BufferPolicy,
    /// Reconnection backoff, for both endpoints.
    #[serde(default)]
    pub retry: RetryPolicy,
    /// Optional framer that observes the stream without altering it.
    #[serde(default)]
    pub tap: Tap,
}

/// How a bridge pairs its two sides into one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    /// Default `parallel`.
    #[serde(default)]
    pub connect_strategy: ConnectStrategy,
    /// The side that attaches first; required for `receiver_then_sender` only.
    #[serde(default)]
    pub receiver: Option<BridgeSide>,
    /// Time for the second side to attach once the first has (the first wait is
    /// unbounded); > 0. Default 5000.
    #[serde(default = "ms_5000")]
    pub pairing_timeout_ms: u32,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            connect_strategy: ConnectStrategy::default(),
            receiver: None,
            pairing_timeout_ms: ms_5000(),
        }
    }
}

/// Session pairing strategy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStrategy {
    /// Both sides start at once.
    #[default]
    Parallel,
    /// Behaves like `parallel`; requires a `tcp_listener` side.
    WaitForListenerThenPair,
    /// The `receiver` side attaches first, then the other side connects.
    ReceiverThenSender,
}

/// Bounds of each direction's buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct BufferPolicy {
    /// Segments per direction, > 0. Default 1000.
    #[serde(default = "segments_1000")]
    pub max_buffered_segments: u32,
    /// Bytes per direction, > 0, enforced with the segment limit. Default 10 MiB.
    #[serde(default = "mib_10")]
    pub max_bytes: u64,
    /// Only `block_then_degrade`.
    #[serde(default)]
    pub overflow_action: OverflowAction,
    /// How long a write into a full buffer blocks before the channel reads `degraded`; it
    /// keeps waiting afterwards, nothing is dropped. > 0. Default 5000.
    #[serde(default = "ms_5000")]
    pub block_timeout_ms: u32,
}

impl Default for BufferPolicy {
    fn default() -> Self {
        Self {
            max_buffered_segments: segments_1000(),
            max_bytes: mib_10(),
            overflow_action: OverflowAction::default(),
            block_timeout_ms: ms_5000(),
        }
    }
}

/// What happens when a buffer is full.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum OverflowAction {
    /// Apply backpressure; after `block_timeout_ms` report `degraded`. Never drop.
    #[default]
    BlockThenDegrade,
}

/// A bridge tap: a framer that counts (and optionally records) messages without altering
/// the stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Tap {
    /// Default `none`.
    #[serde(default)]
    pub framer: TapFramer,
    /// Store each observed message (state `observed`); needs a framer. Default `false`.
    #[serde(default)]
    pub record_messages: bool,
    /// A longer unframed run is a framing error and the framer resynchronizes;
    /// 1024–67108864. Default 1 MiB.
    #[serde(default = "mib_1")]
    pub max_message_bytes: u64,
    /// How long recorded messages are kept. Default 30 days, no count limit.
    #[serde(default)]
    pub retention: TapRetention,
}

impl Default for Tap {
    fn default() -> Self {
        Self {
            framer: TapFramer::default(),
            record_messages: false,
            max_message_bytes: mib_1(),
            retention: TapRetention::default(),
        }
    }
}

/// Framer of a bridge tap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TapFramer {
    /// No tap.
    #[default]
    None,
    /// ASTM E1381 frames.
    Astm,
    /// HL7 MLLP frames.
    Hl7Mllp,
    /// AU frames.
    Au,
}

const fn segments_1000() -> u32 {
    1000
}
const fn mib_10() -> u64 {
    10 * 1024 * 1024
}
