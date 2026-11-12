//! Observability for Positive ACK (`docs/architecture.md` §6).
//!
//! - [`init`] installs the process-wide `tracing` subscriber: JSON lines to
//!   `<logs>/packie.YYYY-MM-DD[.N].log` (rotation at local midnight and at a size limit,
//!   appending across restarts, retention in days), the in-memory
//!   [`LogRing`] (default 20k events, served over IPC with [`LogFilter`]) and, for
//!   foreground runs, a console stream. `channel_id`, `endpoint_id` and `message_id`
//!   fields are lifted from events and enclosing spans into [`LogEvent`], so channel work
//!   that runs inside `info_span!("channel", channel_id = %id)` is attributed.
//! - [`Counters`] are the cheap atomic traffic counters every endpoint and channel keeps;
//!   their [`TrafficSnapshot`] and last [`ErrorSummary`] feed the snapshots.
//! - [`EngineSnapshot`] and its parts ([`NodeSnapshot`], [`ChannelSnapshot`],
//!   [`EndpointSnapshot`], [`DeliveryQueueSnapshot`]) are the read model the console and
//!   CLI display. They never contain message bodies.
//! - [`AlertBook`] derives [`Alert`]s (catalog codes of `docs/spec/diagnostics.md` §6)
//!   from snapshots, [`NodeConditions`] and the runtime thresholds, and keeps 30 days of
//!   cleared alerts for the history.
//! - [`TrafficTap`] copies channel traffic to one local viewer per channel, only while it
//!   watches, through rings of at most 1000 segments or 1 MiB per direction.
//!
//! Message bodies never go to logs: log identifiers and sizes, and use the tap for bytes.

#![warn(missing_docs)]

mod alert;
mod counters;
mod logging;
mod ring;
mod rotate;
mod snapshot;
mod tap;

pub use alert::{
    Alert, AlertBook, AlertKind, AlertState, FailingTarget, NodeConditions, StaleBackup,
    free_space, total_space,
};
pub use counters::Counters;
pub use logging::{LOG_ENV, LogHandle, LogInitError, LogOptions, init};
pub use ring::{LogEvent, LogFilter, LogLevel, LogRing, RingLayer};
pub use rotate::first_log_file_today;
pub use snapshot::{
    AlertCounts, ApplyResult, BridgeDetail, ChannelSnapshot, ConfigStatus, DeliveryQueueSnapshot,
    DestinationCounters, DestinationSnapshot, EndpointCounts, EndpointRef, EndpointSnapshot,
    EngineSnapshot, ErrorSummary, FlowSnapshot, HostingMode, LastApply, MessageCounters,
    MessageDetail, NodeInputs, NodeSnapshot, NodeState, PeerRejections, SourceSession, StateCounts,
    StoreFigures, StoreState, StoreStatus, TapSnapshot, Totals, TrafficSnapshot, Unresolved,
};
pub use tap::{TapBusy, TapRecvError, TapRole, TapSegment, TapViewer, TrafficTap};
