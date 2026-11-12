//! What the message pipeline needs from durable storage (`docs/spec/message-channels.md`).
//!
//! [`MessageStore`] is the narrow seam between message channels and `packie-store`. Method
//! names and semantics follow the store's own API so the store implements it with thin
//! adapters. Every mutating method returns only after its change is durably committed:
//! sources rely on that to acknowledge after the commit, delivery workers to write after
//! the attempt record.

use std::collections::HashMap;

use packie_core::{
    AckLevel, AttemptResult, ChannelId, ConnectorId, DeliveryId, DeliveryState, EndpointId,
    MessageId, MessageState, ProtocolKind, Timestamp,
};
use packie_ipc::messages::{
    BodyRequest, CloseRequest, DeliveryRecord, MessageBody, MessageList, MessageQuery,
    MessageRecord, RejectionCode, ResendRequest, ResendResult,
};
use packie_transport::BoxFuture;

/// A store failure. Text is metadata only, never message content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The store cannot commit now (disk, I/O, writer stopped): no positive
    /// acknowledgement may go out (`message_store_unavailable`).
    #[error("store unavailable: {0}")]
    Unavailable(String),
    /// No such message or delivery.
    #[error("record not found")]
    NotFound,
    /// The operation does not fit the record's state (a programming or race error).
    #[error("invalid operation: {0}")]
    Invalid(String),
    /// An operator command's precondition failed.
    #[error("{message}")]
    Rejected {
        /// Stable code.
        code: RejectionCode,
        /// Operator-facing text.
        message: String,
    },
}

/// Store result.
pub type StoreResult<T> = Result<T, StoreError>;

/// Identifiers the protocol parser extracted (`message-channels.md` §3, §7). Absent
/// values are `None`; `source_identity` is `None` when every part is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identifiers {
    /// Message control id (MSH-10, ASTM H.3).
    pub control_id: Option<String>,
    /// Sample (specimen) id.
    pub sample_id: Option<String>,
    /// Patient id.
    pub patient_id: Option<String>,
    /// Message type (MSH-9, or the protocol name).
    pub message_type: Option<String>,
    /// The sender's identity of the unit, for duplicate detection.
    pub source_identity: Option<String>,
}

/// Where a received unit came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// The receiving channel.
    pub channel_id: ChannelId,
    /// Its label at receipt.
    pub channel_label: String,
    /// The source endpoint.
    pub source_endpoint_id: EndpointId,
    /// The peer address, for connection sources.
    pub source_peer: Option<String>,
    /// The source protocol.
    pub protocol: ProtocolKind,
    /// When the unit arrived.
    pub received_at: Timestamp,
    /// The configuration version the channel ran.
    pub config_version: u64,
    /// Whether the duplicate check applies.
    pub duplicate_detection: bool,
    /// The file a delimited message came from (name, file hash, row range).
    pub source_file: Option<packie_ipc::messages::SourceFile>,
    /// Known at commit for single-unit protocols (HL7, files); multi-frame transfers
    /// supply them at [`MessageStore::complete_transfer`].
    pub identifiers: Identifiers,
}

/// What a commit decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptOutcome {
    /// The stored message.
    pub message_id: MessageId,
    /// Receipt order within the channel.
    pub sequence: u64,
    /// `received` or `duplicate`.
    pub state: MessageState,
    /// The earlier message this one duplicates.
    pub duplicate_of: Option<MessageId>,
    /// The earlier message of the same identity with different content.
    pub revision_of: Option<MessageId>,
}

/// Delivery policy frozen into a delivery when it is created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationPolicy {
    /// The destination.
    pub destination_id: ConnectorId,
    /// Its label.
    pub label: String,
    /// Attempts per cycle before the delivery fails.
    pub max_attempts: u32,
    /// Whether a timed-out or unknown attempt is retried.
    pub retry_unacknowledged: bool,
    /// The acknowledgement level that settles a delivery.
    pub ack_required: AckLevel,
}

/// One delivery the pipeline creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDelivery {
    /// The destination and its frozen policy.
    pub policy: DestinationPolicy,
    /// The output for this destination; `None` with a transform error.
    pub body: Option<Vec<u8>>,
    /// `Some(rule and path)` creates the delivery `failed` with reason
    /// `transform_error` and zero attempts.
    pub transform_error: Option<String>,
    /// The `config_version` whose mapping produced `body`.
    pub mapping_version: u64,
}

/// The head of a destination's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDelivery {
    /// The delivery.
    pub delivery_id: DeliveryId,
    /// Its message.
    pub message_id: MessageId,
    /// Receipt order of the message.
    pub sequence: u64,
    /// Not before this time.
    pub next_attempt_at: Option<Timestamp>,
    /// Attempts used in the current cycle.
    pub attempts: u32,
    /// Attempts allowed per cycle.
    pub max_attempts: u32,
    /// The acknowledgement level that completes this delivery, frozen when it was created
    /// (`message-channels.md` §12): an attempt waits for it even after the destination's
    /// policy changed, because the store completes the delivery only at that level.
    pub ack_required: AckLevel,
    /// The stored output for this destination; `None` after a transform error, in
    /// which case the destination's current mapping runs on `original` again.
    pub body: Option<Vec<u8>>,
    /// The message's original bytes.
    pub original: Vec<u8>,
    /// The message's identifiers, for destination file names.
    pub control_id: Option<String>,
    /// The message's sample id, for destination file names.
    pub sample_id: Option<String>,
}

/// How an attempt ended. The store derives the next state from it and the frozen
/// policy (`message-channels.md` §4, §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptFinish {
    /// How the attempt ended.
    pub result: AttemptResult,
    /// The highest level proven.
    pub ack_level: AckLevel,
    /// The protocol code received (`AA`, `CA`, `AR`, ...).
    pub ack_code: Option<String>,
    /// The acknowledgement message's own control id (HL7 MSH-10 of the ACK).
    pub ack_control_id: Option<String>,
    /// Machine-readable failure code (the result) unless acknowledged.
    pub error_code: Option<String>,
    /// Operator-facing, without message values.
    pub error_summary: Option<String>,
    /// When the attempt ended.
    pub ended_at: Timestamp,
    /// When the next attempt may start if the store requeues the delivery.
    pub retry_at: Option<Timestamp>,
    /// For `engine_stopped`: whether every byte was written. A partly sent attempt that
    /// was stopped returns to `queued` without counting; a fully sent one becomes
    /// `outcome_unknown`.
    pub sent_completely: bool,
    /// Reason recorded on the delivery (`channel_paused`, `engine_stopped`, ...).
    pub reason_code: Option<String>,
}

/// A stored message the pipeline runs again (startup reconciliation, `new_message`
/// resend).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    /// The message.
    pub message_id: MessageId,
    /// Its channel.
    pub channel_id: ChannelId,
    /// Its source protocol.
    pub protocol: ProtocolKind,
    /// Its current state.
    pub state: MessageState,
    /// Original bytes.
    pub bytes: Vec<u8>,
    /// Destinations a `new_message` resend selected; empty means all.
    pub resend_targets: Vec<ConnectorId>,
}

/// What startup reconciliation did (`message-channels.md` §13).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// Transfers whose committed frames formed a complete message.
    pub recovered: u64,
    /// Incomplete transfers, now `aborted`.
    pub aborted: u64,
    /// Messages left `received`: the pipeline runs them again.
    pub requeued: Vec<MessageId>,
    /// Deliveries found `sending`, now `outcome_unknown` (`engine_restarted`).
    pub unknown: u64,
}

/// One channel's retention policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// The channel.
    pub channel_id: ChannelId,
    /// Purge resolved messages older than this.
    pub max_age_days: Option<u32>,
    /// Keep at most this many resolved messages.
    pub max_messages: Option<u64>,
}

/// Queue counts of one destination for snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueCounts {
    /// Deliveries `queued`.
    pub queued: u64,
    /// Deliveries `sending`.
    pub sending: u64,
    /// Deliveries `rejected`, unresolved.
    pub rejected: u64,
    /// Deliveries `timed_out`, unresolved.
    pub timed_out: u64,
    /// Deliveries `outcome_unknown`, unresolved.
    pub outcome_unknown: u64,
    /// Deliveries `failed`, unresolved.
    pub failed: u64,
    /// `failed` deliveries whose mapping failed (`transform_error`).
    pub transform_errors: u64,
    /// When the oldest queued delivery was queued.
    pub oldest_queued_at: Option<Timestamp>,
    /// When the next queued attempt may start.
    pub next_attempt_at: Option<Timestamp>,
    /// Attempts started, across every delivery of the destination.
    pub attempts: u64,
    /// Deliveries acknowledged.
    pub acknowledged: u64,
    /// Bytes of acknowledged deliveries' bodies.
    pub bytes_out: u64,
    /// When the latest attempt started.
    pub last_attempt_at: Option<Timestamp>,
    /// When the latest acknowledgement arrived.
    pub last_ack_at: Option<Timestamp>,
    /// The latest attempt failure summary.
    pub last_error: Option<String>,
}

/// Durable storage for message channels.
pub trait MessageStore: Send + Sync + 'static {
    /// Commits several complete units in one transaction, all or none (the records or
    /// groups of one source file), each with the duplicate check. Outcomes are in order.
    fn commit_batch(
        &self,
        units: Vec<(Receipt, Vec<u8>)>,
    ) -> BoxFuture<'_, StoreResult<Vec<ReceiptOutcome>>>;

    /// Commits one complete unit (HL7 frame, file record) with the duplicate check, as
    /// `received` or `duplicate`, before the caller acknowledges.
    fn commit_received(
        &self,
        receipt: Receipt,
        bytes: Vec<u8>,
    ) -> BoxFuture<'_, StoreResult<ReceiptOutcome>>;

    /// Starts a multi-frame transfer (ASTM, AU): a `receiving` message holding its first
    /// frame, committed before that frame is acknowledged.
    fn begin_transfer(
        &self,
        receipt: Receipt,
        frame: Vec<u8>,
    ) -> BoxFuture<'_, StoreResult<MessageId>>;

    /// Commits one more frame of a `receiving` message.
    fn append_frame(&self, message: MessageId, frame: Vec<u8>) -> BoxFuture<'_, StoreResult<()>>;

    /// Completes a transfer: `bytes` (the assembled records) become the original, and
    /// the duplicate check runs. The message becomes `received` or `duplicate`.
    fn complete_transfer(
        &self,
        message: MessageId,
        bytes: Vec<u8>,
        identifiers: Identifiers,
        duplicate_detection: bool,
    ) -> BoxFuture<'_, StoreResult<ReceiptOutcome>>;

    /// Ends a transfer without a complete message: `aborted` with `reason`.
    fn abort_transfer(
        &self,
        message: MessageId,
        reason: &'static str,
    ) -> BoxFuture<'_, StoreResult<()>>;

    /// Records the acknowledgement code sent back to the sender.
    fn record_response(
        &self,
        message: MessageId,
        code: String,
        at: Timestamp,
    ) -> BoxFuture<'_, StoreResult<()>>;

    /// `received` → `invalid` with a reason code.
    fn record_invalid(&self, message: MessageId, reason: String) -> BoxFuture<'_, StoreResult<()>>;

    /// `received` → `filtered` with a reason code.
    fn record_filtered(&self, message: MessageId, reason: String)
    -> BoxFuture<'_, StoreResult<()>>;

    /// `received` → `routed` with its deliveries, in one transaction.
    fn route(
        &self,
        message: MessageId,
        deliveries: Vec<NewDelivery>,
        config_version: u64,
    ) -> BoxFuture<'_, StoreResult<Vec<DeliveryId>>>;

    /// A `received` message, to run the pipeline again.
    fn stored_message(&self, message: MessageId) -> BoxFuture<'_, StoreResult<StoredMessage>>;

    /// The `queued` delivery of `destination` with the lowest receipt sequence, if any.
    fn next_delivery(
        &self,
        channel: ChannelId,
        destination: ConnectorId,
    ) -> BoxFuture<'_, StoreResult<Option<PendingDelivery>>>;

    /// Stores the output a re-run mapping produced for a delivery that had none.
    fn record_transformed_body(
        &self,
        delivery: DeliveryId,
        body: Vec<u8>,
        mapping_version: u64,
    ) -> BoxFuture<'_, StoreResult<()>>;

    /// The mapping failed again when a bodyless delivery was about to be attempted: the
    /// delivery becomes `failed` (`transform_error`) without an attempt.
    fn record_transform_error(
        &self,
        delivery: DeliveryId,
        summary: String,
        mapping_version: u64,
    ) -> BoxFuture<'_, StoreResult<()>>;

    /// `queued` → `sending`: commits the attempt record before any byte is written.
    /// Returns the attempt number (total across cycles).
    fn begin_attempt(
        &self,
        delivery: DeliveryId,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<u32>>;

    /// A lower acknowledgement level arrived; the attempt goes on.
    fn record_ack_progress(
        &self,
        delivery: DeliveryId,
        level: AckLevel,
        code: Option<String>,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<()>>;

    /// Ends the attempt; returns the delivery's new state.
    fn finish_attempt(
        &self,
        delivery: DeliveryId,
        finish: AttemptFinish,
    ) -> BoxFuture<'_, StoreResult<DeliveryState>>;

    /// Operator resend with the per-destination preconditions (`message-channels.md` §9).
    /// `active` lists the enabled destinations of the active configuration.
    fn resend(
        &self,
        request: ResendRequest,
        active: Vec<DestinationPolicy>,
        config_version: u64,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<ResendResult>>;

    /// Operator close of an unresolved delivery; returns the closed record.
    fn close_delivery(
        &self,
        request: CloseRequest,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<DeliveryRecord>>;

    /// Cancels the unresolved deliveries of a removed destination or channel
    /// (`destination_removed`, `channel_removed`).
    fn cancel_unresolved(
        &self,
        channel: ChannelId,
        destination: Option<ConnectorId>,
        reason: &'static str,
    ) -> BoxFuture<'_, StoreResult<u64>>;

    /// Where each ASTM source channel keeps its specimen id (`AstmSettings::sample_id_position`),
    /// for the identifiers of a transfer that reconciliation completes. Channels that are
    /// not listed use the standard `O.3`.
    fn set_astm_sample_positions(&self, _positions: HashMap<ChannelId, (usize, Option<usize>)>) {}

    /// Startup reconciliation, steps 1–4, before any source opens.
    fn reconcile_at_startup(&self) -> BoxFuture<'_, StoreResult<Reconciliation>>;

    /// Current queue counts of `destination` (a cheap read).
    fn queue_counts(
        &self,
        channel: ChannelId,
        destination: ConnectorId,
    ) -> StoreResult<QueueCounts>;

    /// Store-wide figures for the node snapshot, when the store keeps them.
    fn totals(&self) -> Option<packie_store::StoreTotals>;

    /// Retention (`message-channels.md` §10): purges resolved history of each channel by
    /// its policy (`(channel, max_age_days, max_messages)`); other channels use the
    /// default. Returns the messages purged.
    fn purge(&self, policies: Vec<RetentionPolicy>) -> BoxFuture<'_, StoreResult<u64>>;

    /// The Messages page query.
    fn query_messages(&self, query: MessageQuery) -> BoxFuture<'_, StoreResult<MessageList>>;

    /// Per-destination delivery totals of the messages `filter` selects, for the
    /// transmission report.
    fn transmission_totals(
        &self,
        filter: packie_ipc::messages::MessageFilter,
    ) -> BoxFuture<'_, StoreResult<Vec<packie_ipc::reports::DestinationTransmission>>>;

    /// One message with its deliveries.
    fn message_record(&self, message: MessageId) -> BoxFuture<'_, StoreResult<MessageRecord>>;

    /// A stored body for the Messages detail view.
    fn message_body(&self, request: BodyRequest) -> BoxFuture<'_, StoreResult<MessageBody>>;
}

impl AttemptFinish {
    /// The store's form; the delivery's reason code travels as the error code.
    pub(crate) fn to_store(&self) -> packie_store::AttemptFinish {
        packie_store::AttemptFinish {
            result: self.result,
            ack_level: self.ack_level,
            ack_code: self.ack_code.clone(),
            ack_control_id: self.ack_control_id.clone(),
            error_code: self.reason_code.clone().or_else(|| self.error_code.clone()),
            error_summary: self.error_summary.clone(),
            ended_at: self.ended_at,
            retry_at: self.retry_at,
        }
    }
}
