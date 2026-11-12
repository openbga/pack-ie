use packie_core::{
    AckLevel, AttemptResult, DeliveryId, MessageDirection, MessageId, MessageOrigin, MessageState,
    ProtocolKind, Timestamp,
};
use std::collections::BTreeMap;
use tokio::sync::watch;

/// Opening settings; pass `EngineContext.test_hooks.map(|h| h.commits_held())` to the hook.
#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// Largest body kept in the database; larger ones spill to files.
    pub inline_body_cap: usize,
    /// Pooled read connections (at least one).
    pub read_connections: usize,
    /// The engine test hook that holds commits while it reads `true`.
    pub commits_held: Option<watch::Receiver<bool>>,
}
impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            inline_body_cap: 1024 * 1024,
            read_connections: 3,
            commits_held: None,
        }
    }
}

/// Identifiers supplied by the protocol parser. `source_identity` is the protocol's
/// complete composite identity, not merely its control id; empty components alone do
/// not form an identity. Search text is explicit metadata, never automatically raw bytes.
#[derive(Debug, Clone, Default)]
pub struct Identifiers {
    /// The protocol's message control identifier.
    pub control_id: Option<String>,
    /// The sample (specimen) identifier.
    pub sample_id: Option<String>,
    /// The patient identifier.
    pub patient_id: Option<String>,
    /// The protocol's message type.
    pub message_type: Option<String>,
    /// The composite identity duplicate detection compares.
    pub source_identity: Option<String>,
    /// Safe text the grid's text filter searches.
    pub searchable_text: String,
}

/// Receipt metadata and the configured destinations selected by the pipeline.
#[derive(Debug, Clone)]
pub struct Receipt {
    /// The receiving channel.
    pub channel_id: String,
    /// The channel's label when the message arrived.
    pub channel_label: String,
    /// The source connector.
    pub source_connector_id: String,
    /// The source endpoint.
    pub source_endpoint_id: String,
    /// The remote peer, for network sources.
    pub source_peer: Option<String>,
    /// The file the message came from, for file sources.
    pub source_file: Option<packie_ipc::messages::SourceFile>,
    /// The message protocol.
    pub protocol: ProtocolKind,
    /// The direction through the channel.
    pub direction: MessageDirection,
    /// How the message entered the store.
    pub origin: MessageOrigin,
    /// The state it is stored in; only `Received` may carry destinations.
    pub initial_state: MessageState,
    /// Identifiers the protocol parser extracted.
    pub identifiers: Identifiers,
    /// When the first byte arrived.
    pub received_at: Timestamp,
    /// The configuration version in force.
    pub config_version: u64,
    /// Whether to look for an earlier identical message.
    pub duplicate_detection: bool,
    /// Destinations selected at receipt, or empty to leave routing to the pipeline.
    pub destinations: Vec<Destination>,
}

/// Frozen delivery policy. The scheduler supplies the calculated backoff timestamp;
/// the store alone decides whether that attempt is eligible for an automatic retry.
#[derive(Debug, Clone)]
pub struct Destination {
    /// The destination connector.
    pub id: String,
    /// Its label when the delivery was created.
    pub label: String,
    /// Attempts allowed per cycle (at least one).
    pub max_attempts: u32,
    /// Whether unacknowledged attempts are retried.
    pub retry_unacknowledged: bool,
    /// The acknowledgement level that completes a delivery (not `None`).
    pub ack_required: AckLevel,
    /// A mapping failure to record instead of queueing the delivery.
    pub transform_error: Option<String>,
}
impl Destination {
    /// A destination with the default policy: five attempts, `Accepted` required,
    /// no unacknowledged retry.
    pub fn new(id: impl ToString, label: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            label: label.into(),
            max_attempts: 5,
            retry_unacknowledged: false,
            ack_required: AckLevel::Accepted,
            transform_error: None,
        }
    }
}

/// What a committed receipt became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptOutcome {
    /// The stored message.
    pub message_id: MessageId,
    /// Its per-channel sequence number.
    pub sequence: u64,
    /// The earlier identical message, when this one is a duplicate.
    pub duplicate_of: Option<MessageId>,
    /// The earlier message with the same identity and different content.
    pub revision_of: Option<MessageId>,
    /// The deliveries created at receipt.
    pub deliveries: Vec<DeliveryId>,
}

/// A completed attempt. Summaries must describe conditions without message values.
#[derive(Debug, Clone)]
pub struct AttemptFinish {
    /// How the attempt ended.
    pub result: AttemptResult,
    /// The highest acknowledgement level reached.
    pub ack_level: AckLevel,
    /// The destination's acknowledgement code.
    pub ack_code: Option<String>,
    /// The acknowledgement message's own control id (HL7 MSH-10 of the ACK).
    pub ack_control_id: Option<String>,
    /// A machine-readable failure code.
    pub error_code: Option<String>,
    /// A failure summary for operators.
    pub error_summary: Option<String>,
    /// When the attempt ended.
    pub ended_at: Timestamp,
    /// The next attempt's due time; required when the result leaves the delivery queued.
    pub retry_at: Option<Timestamp>,
}

/// Store-wide figures for the node snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreTotals {
    /// Messages currently stored.
    pub messages: u64,
    /// Deliveries `rejected`, `failed`, `timed_out` or `outcome_unknown`.
    pub deliveries_unresolved: u64,
    /// Deliveries `queued`.
    pub deliveries_queued: u64,
    /// Database (with its write-ahead log) and spilled bodies, in bytes.
    pub size_bytes: u64,
}

/// The head of a destination's queue (`Store::next_delivery`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedDelivery {
    /// The delivery.
    pub delivery_id: DeliveryId,
    /// Its message.
    pub message_id: MessageId,
    /// The message's per-channel sequence number.
    pub sequence: u64,
    /// When the next attempt is due; `None` when immediately.
    pub next_attempt_at: Option<Timestamp>,
    /// Attempts made in the current cycle.
    pub attempts: u32,
    /// Attempts allowed per cycle.
    pub max_attempts: u32,
    /// The acknowledgement level that completes the delivery, frozen when it was created;
    /// an attempt finishes acknowledged only at this level or above.
    pub ack_required: AckLevel,
    /// The stored output; `None` until one is recorded (transform error, mapping required).
    pub body: Option<Vec<u8>>,
    /// The message's original bytes.
    pub original: Vec<u8>,
    /// The message's identifiers, for destination file names.
    pub control_id: Option<String>,
    /// See `control_id`.
    pub sample_id: Option<String>,
}

/// The outcome of startup reconciliation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// Interrupted transfers the host's parser recovered as complete.
    pub recovered: u64,
    /// Partial receipts aborted.
    pub aborted: u64,
    /// Complete receipts the pipeline must process again.
    pub requeued: Vec<MessageId>,
    /// In-flight attempts left with an unknown outcome.
    pub unknown: u64,
    /// Channels the operator had paused.
    pub paused_channels: Vec<String>,
}

/// How long resolved history is kept; a limit left `None` is not applied.
#[derive(Debug, Clone)]
pub struct Retention {
    /// Oldest age, in days, of a message that is kept.
    pub max_age_days: Option<u32>,
    /// Most messages a channel keeps.
    pub max_messages: Option<u64>,
}
impl Default for Retention {
    fn default() -> Self {
        Self {
            max_age_days: Some(90),
            max_messages: None,
        }
    }
}

/// What a retention run removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurgeCounts {
    /// Messages.
    pub messages: u64,
    /// Their deliveries.
    pub deliveries: u64,
    /// Their attempts.
    pub attempts: u64,
    /// Their body references.
    pub bodies: u64,
    /// Cleared alert episodes.
    pub alerts: u64,
    /// Unreferenced body files.
    pub files: u64,
}

/// Persistent monotonic totals survive retention. Derive instance counters by baseline
/// subtraction when starting a channel instance. Errors count invalid/aborted receipts,
/// failed transformations and unsuccessful delivery attempts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counters {
    /// Messages stored, including duplicates.
    pub received: u64,
    /// Messages the pipeline filtered.
    pub filtered: u64,
    /// Messages stored as duplicates.
    pub duplicates: u64,
    /// Invalid or aborted receipts, failed transformations and unsuccessful attempts.
    pub errors: u64,
    /// Messages found invalid.
    pub invalid: u64,
    /// Transfers aborted.
    pub aborted: u64,
    /// Bytes received.
    pub bytes_in: u64,
    /// Delivery attempts started.
    pub attempts: u64,
    /// Attempts acknowledged.
    pub acknowledged: u64,
    /// Attempts the destination rejected.
    pub rejected: u64,
    /// Attempts whose acknowledgement timed out.
    pub timed_out: u64,
    /// Attempts whose outcome is unknown.
    pub outcome_unknown: u64,
    /// Deliveries that failed.
    pub failed: u64,
    /// Bytes handed to destinations.
    pub bytes_out: u64,
}

/// Queue state and totals of one destination.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DestinationCounts {
    /// Deliveries per unresolved state (`queued`, `sending`, `rejected`, `timed_out`,
    /// `outcome_unknown`, `failed`); resolved history is in `totals`.
    pub states: BTreeMap<String, u64>,
    /// `failed` deliveries whose mapping failed (reason `transform_error`).
    pub transform_errors: u64,
    /// When the oldest queued message was received.
    pub oldest_queued_at: Option<Timestamp>,
    /// How long ago that was, in seconds.
    pub oldest_queued_age_seconds: Option<u64>,
    /// The earliest due retry among queued deliveries.
    pub next_attempt_at: Option<Timestamp>,
    /// The latest attempt start.
    pub last_attempt_at: Option<Timestamp>,
    /// The latest acknowledgement.
    pub last_ack_at: Option<Timestamp>,
    /// The latest failure summary.
    pub last_error: Option<String>,
    /// The persistent totals.
    pub totals: Counters,
}

/// One alert episode: a condition from its first sighting until it clears.
#[derive(Debug, Clone)]
pub struct Alert {
    /// The episode's identifier (a fresh `UUIDv7` for each episode).
    pub id: packie_core::AlertId,
    /// The stable identity of the condition.
    pub identity: String,
    /// The alert code.
    pub code: String,
    /// How serious it is.
    pub severity: packie_core::Severity,
    /// The channel concerned.
    pub channel_id: Option<String>,
    /// The endpoint concerned.
    pub endpoint_id: Option<String>,
    /// The destination concerned.
    pub destination_id: Option<String>,
    /// What happened.
    pub summary: String,
    /// What the operator should do.
    pub next_action: String,
    /// When the condition was first seen.
    pub first_seen_at: Timestamp,
    /// When it was last seen.
    pub last_seen_at: Timestamp,
    /// When it cleared; `None` while active.
    pub cleared_at: Option<Timestamp>,
    /// How many times it was raised.
    pub occurrences: u64,
    /// Structured context.
    pub details: serde_json::Value,
}

/// Selects alert episodes; unset fields match everything.
#[derive(Debug, Clone, Default)]
pub struct AlertFilter {
    /// Only this channel.
    pub channel_id: Option<String>,
    /// Only this endpoint.
    pub endpoint_id: Option<String>,
    /// Only this code.
    pub code: Option<String>,
    /// Only episodes that have not cleared.
    pub active_only: bool,
    /// Only episodes last seen at or after this time.
    pub since: Option<Timestamp>,
    /// Only episodes first seen at or before this time.
    pub until: Option<Timestamp>,
    /// Rows to skip.
    pub offset: u32,
    /// Most rows to return (0 means 100; at most 1000).
    pub limit: u32,
}
