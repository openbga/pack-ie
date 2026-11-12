//! Snapshot DTOs (`docs/spec/diagnostics.md` §3–§5): the point-in-time read model the
//! console and CLI display. They never contain message bodies.
//!
//! - `node`: [`EngineSnapshot`], the [`NodeSnapshot`] with its totals and store status, and
//!   how the node state is derived.
//! - `channel`: [`ChannelSnapshot`], its [`TrafficSnapshot`] and [`ErrorSummary`], and the
//!   [`BridgeDetail`] of bridge channels.
//! - `message`: the [`MessageDetail`] of message channels and their destinations.
//! - `endpoint`: [`EndpointSnapshot`].

mod channel;
mod endpoint;
mod message;
mod node;

pub use channel::{
    BridgeDetail, ChannelSnapshot, EndpointRef, ErrorSummary, FlowSnapshot, TapSnapshot,
    TrafficSnapshot,
};
pub use endpoint::{EndpointSnapshot, PeerRejections};
pub use message::{
    DeliveryQueueSnapshot, DestinationCounters, DestinationSnapshot, MessageCounters,
    MessageDetail, SourceSession, Unresolved,
};
pub use node::{
    AlertCounts, ApplyResult, ConfigStatus, EndpointCounts, EngineSnapshot, HostingMode, LastApply,
    NodeInputs, NodeSnapshot, NodeState, StateCounts, StoreFigures, StoreState, StoreStatus,
    Totals,
};
