//! Channel supervisors for Positive ACK (`docs/architecture.md` §7): the engine's
//! [`ChannelRegistry`](packie_engine::ChannelRegistry) implementation.
//!
//! - [`RoutingRegistry`] applies configuration per channel, serializes apply, pause,
//!   resume, restart and shutdown, persists pause flags under the state directory, and
//!   answers snapshots from atomics without waiting for any operation. [`registry`] is
//!   the factory the `packie` binary hands to `packie_engine::run`.
//! - Each channel instance runs in its own supervisor task. A panic or error stays in
//!   that channel: it is reported in its snapshot and the instance starts again after
//!   its retry delay.
//! - Bridge channels follow `docs/spec/bridge-channels.md`: pairing strategies,
//!   bounded per-direction buffers with `block_then_degrade`, close propagation,
//!   exponential backoff with jitter, single-peer listeners with allowlists, and drain
//!   on shutdown. Bytes are forwarded unchanged and in order.
//! - An optional tap runs a protocol framer (HL7 MLLP, ASTM, AU) on copies of each
//!   direction to count messages; observed messages go to a [`TapSink`] for recording.
//!   The live transcript is published through the engine's `TrafficTap`.
//! - Message channels ([`message`]) run when the registry has a [`message::MessageStore`]:
//!   HL7 MLLP, ASTM and AU sources commit every unit before acknowledging it, delimited
//!   file sources commit every record or group of a file before consuming it, the
//!   pipeline runs the filters and mappings compiled with `packie-transform`, and one worker
//!   per destination (HL7, ASTM, AU or file) delivers in receipt order with the spec's
//!   attempt, acknowledgement and outcome rules. Without a store they report `stopped`
//!   with reason `not_implemented`; with a store that could not be opened, `stopped` with
//!   `store_unavailable` while the registry retries opening it ([`StoreOpener`]).
//! - Transports come through `packie_transport::Transports`: `SystemTransports` on a real
//!   host, [`memory::MemoryNetwork`] for in-memory tests and simulations.

#![warn(missing_docs)]

mod bridge;
mod buffer;
mod failure;
mod framer;
mod listen;
pub mod memory;
pub mod message;
mod paused;
mod registry;
mod retry;
mod runner;
mod session;
mod status;
mod stop;
mod store_open;

pub use registry::{RegistryOptions, RoutingRegistry, registry, registry_with_store};
pub use store_open::{OpenedStore, StoreOpener, StoreRetry, sqlite_store_opener};

pub(crate) use failure::Failure;

use packie_core::{BridgeSide, ChannelId, EndpointId, ProtocolKind};

/// Receives every message a bridge tap observes, for recording in the store (state
/// `observed`, direction `a_to_b` or `b_to_a`, no deliveries, never deduplicated).
///
/// Called from the tap's own task; it must not block. A sink that cannot keep up
/// should queue or drop, never hold up the caller: the bridge never waits for a tap.
pub trait TapSink: Send + Sync + 'static {
    /// A complete message the tap framed in one direction.
    fn observed(&self, observation: TapObservation);
}

/// One message a bridge tap observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapObservation {
    /// The bridge channel.
    pub channel_id: ChannelId,
    /// Its label at the time.
    pub channel_label: String,
    /// The side it came from: `A` for `a_to_b`, `B` for `b_to_a`.
    pub from: BridgeSide,
    /// The endpoint of that side.
    pub endpoint_id: EndpointId,
    /// The tap framer's protocol.
    pub protocol: ProtocolKind,
    /// The configuration version the channel ran.
    pub config_version: u64,
    /// The message, framing removed.
    pub message: Vec<u8>,
}
