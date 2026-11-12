//! Stored messages over IPC: the Messages grid query, message detail, bodies, resend and
//! close (`docs/spec/message-channels.md` §3–§5, §9; `docs/spec/operator-console.md` §5–§6).
//!
//! Message bodies appear only in [`MessageBody`], served to the local client that asked;
//! every other type carries identifiers, sizes and hashes, never content.

use packie_core::{
    AckLevel, AttemptResult, ChannelId, ConnectorId, DeliveryId, DeliveryState, DeliverySummary,
    EndpointId, MessageDirection, MessageId, MessageOrigin, MessageState, ProtocolKind, Timestamp,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Default page size of [`MessageQuery`].
pub const DEFAULT_PAGE: u32 = 100;
/// Largest page size of [`MessageQuery`].
pub const MAX_PAGE: u32 = 1000;
/// Shortest accepted [`MessageFilter::text`].
pub const MIN_TEXT: usize = 3;
/// Longest accepted close or resend reason, in characters.
pub const MAX_REASON: usize = 500;
/// Fewest hex digits of [`MessageFilter::message_id_prefix`].
pub const MIN_ID_PREFIX: usize = 8;

/// A page of the Messages grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageQuery {
    /// Which messages.
    #[serde(default)]
    pub filter: MessageFilter,
    /// Order.
    #[serde(default)]
    pub sort: MessageSort,
    /// Rows to skip.
    #[serde(default)]
    pub offset: u32,
    /// Rows to return, 1 to [`MAX_PAGE`]. Default [`DEFAULT_PAGE`].
    #[serde(default = "default_page")]
    pub limit: u32,
}

impl Default for MessageQuery {
    fn default() -> Self {
        Self {
            filter: MessageFilter::default(),
            sort: MessageSort::default(),
            offset: 0,
            limit: DEFAULT_PAGE,
        }
    }
}

const fn default_page() -> u32 {
    DEFAULT_PAGE
}

impl MessageQuery {
    /// Checks the limits a query must respect; the text is for the operator.
    ///
    /// # Errors
    ///
    /// What is out of range, as operator text.
    pub fn check(&self) -> Result<(), String> {
        if !(1..=MAX_PAGE).contains(&self.limit) {
            return Err(format!("limit must be from 1 to {MAX_PAGE}"));
        }
        if let Some(text) = &self.filter.text
            && text.trim().chars().count() < MIN_TEXT
        {
            return Err(format!("text search needs at least {MIN_TEXT} characters"));
        }
        if let Some(prefix) = &self.filter.message_id_prefix {
            id_prefix(prefix)?;
        }
        if let (Some(since), Some(until)) = (self.filter.since, self.filter.until)
            && until <= since
        {
            return Err("until must be after since".to_owned());
        }
        Ok(())
    }
}

/// Messages grid filters. Absent or empty criteria match everything; list criteria match
/// any of their values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct MessageFilter {
    /// Received at or after.
    pub since: Option<Timestamp>,
    /// Received before.
    pub until: Option<Timestamp>,
    /// Any of these channels.
    pub channels: Vec<ChannelId>,
    /// Any of these directions.
    pub directions: Vec<MessageDirection>,
    /// Any of these message states.
    pub states: Vec<MessageState>,
    /// Messages with at least one delivery in any of these states.
    pub delivery_states: Vec<DeliveryState>,
    /// Any of these delivery summaries (`attention` is the "Needs attention" quick filter).
    pub delivery_summaries: Vec<DeliverySummary>,
    /// Control, sample or patient id.
    pub identifier: Option<IdentifierFilter>,
    /// Substring of the control, sample or patient id or the message type (bodies are never
    /// searched); at least 3 characters, bounded by the time range.
    pub text: Option<String>,
    /// Messages whose id starts with these hex digits: at least [`MIN_ID_PREFIX`], dashes
    /// optional, any case. [`id_prefix`] gives the form ids are stored in.
    pub message_id_prefix: Option<String>,
}

/// `text` as the beginning of a message id in its stored form (lowercase, dashes at the
/// UUID positions), or why it cannot be one; the error is for the operator.
///
/// # Errors
///
/// Why `text` is not a message id prefix, as operator text.
pub fn id_prefix(text: &str) -> Result<String, String> {
    let digits: String = text
        .trim()
        .chars()
        .filter(|c| *c != '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("a message id has only hex digits and dashes".to_owned());
    }
    if !(MIN_ID_PREFIX..=32).contains(&digits.len()) {
        return Err(format!(
            "give at least {MIN_ID_PREFIX} and at most 32 hex digits of the message id"
        ));
    }
    let mut stored = String::with_capacity(digits.len() + 4);
    for (index, digit) in digits.chars().enumerate() {
        if matches!(index, 8 | 12 | 16 | 20) {
            stored.push('-');
        }
        stored.push(digit);
    }
    Ok(stored)
}

/// Match on the control, sample or patient id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct IdentifierFilter {
    /// The id or its beginning.
    pub value: String,
    /// Match ids that start with `value` instead of equal it.
    #[serde(default)]
    pub prefix: bool,
}

/// Grid order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageSort {
    /// Column.
    pub field: MessageSortField,
    /// Largest first. Default `true` (newest first).
    pub descending: bool,
}

impl Default for MessageSort {
    fn default() -> Self {
        Self {
            field: MessageSortField::ReceivedAt,
            descending: true,
        }
    }
}

/// Sortable columns. Ties are broken by receipt sequence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MessageSortField {
    /// Received time.
    #[default]
    ReceivedAt,
    /// Channel label.
    Channel,
    /// Message state.
    State,
    /// Message type.
    MessageType,
    /// Size of the original bytes.
    ByteLength,
}

/// One page of messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageList {
    /// The rows.
    pub messages: Vec<MessageSummary>,
    /// Rows matching the filter, all pages.
    #[ts(type = "number")]
    pub total: u64,
    /// As requested.
    pub offset: u32,
    /// As requested.
    pub limit: u32,
}

/// A grid row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageSummary {
    /// The message.
    pub message_id: MessageId,
    /// Its channel.
    pub channel_id: ChannelId,
    /// The channel's label (from the active configuration, or as stored when removed).
    pub channel_label: String,
    /// Receipt sequence within the channel.
    #[ts(type = "number")]
    pub seq: u64,
    /// Inbound, or the bridge direction it was observed in.
    pub direction: MessageDirection,
    /// Message state.
    pub state: MessageState,
    /// How it came to exist.
    pub origin: MessageOrigin,
    /// When the first byte arrived.
    pub received_at: Timestamp,
    /// Protocol of the source.
    pub protocol: ProtocolKind,
    /// Message type (`ORU^R01`, `astm`, `au`, `delimited`), when known.
    pub message_type: Option<String>,
    /// Protocol control id, when present.
    pub control_id: Option<String>,
    /// Sample (specimen) id, when present.
    pub sample_id: Option<String>,
    /// Patient id, when present.
    pub patient_id: Option<String>,
    /// Size of the original bytes.
    #[ts(type = "number")]
    pub byte_length: u64,
    /// Peer address, port or server that sent it.
    pub source_peer: Option<String>,
    /// Configuration version in force at receipt.
    #[ts(type = "number")]
    pub config_version: u64,
    /// The message this one was resent from.
    pub resent_from: Option<MessageId>,
    /// Roll-up of the deliveries.
    pub delivery_summary: DeliverySummary,
    /// One chip per delivery, in destination order.
    pub deliveries: Vec<DeliveryChip>,
}

/// A delivery as shown in a grid row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeliveryChip {
    /// The delivery.
    pub delivery_id: DeliveryId,
    /// Its destination connector.
    pub destination_id: ConnectorId,
    /// The destination's label.
    pub destination_label: String,
    /// Delivery state.
    pub state: DeliveryState,
    /// What was proven.
    pub ack_level: AckLevel,
}

/// Everything about one message except its bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageRecord {
    /// The grid row fields.
    pub summary: MessageSummary,
    /// When the complete original bytes were committed.
    pub committed_at: Option<Timestamp>,
    /// SHA-256 of the original bytes, lowercase hex.
    pub content_hash: String,
    /// Source identity used for duplicate detection, when the message has one.
    pub source_identity: Option<String>,
    /// The source endpoint.
    pub source_endpoint_id: EndpointId,
    /// For messages read from a file.
    pub source_file: Option<SourceFile>,
    /// The acknowledgement code sent back to the sender (`AA`, `CA`, `ACK`, ...).
    pub response_code: Option<String>,
    /// When it was sent.
    pub responded_at: Option<Timestamp>,
    /// Why the transfer was aborted (`receive_timeout`, `message_too_large`, ...).
    pub abort_reason: Option<String>,
    /// Why it was filtered or is invalid (`source_filter`, `parse_error`, ...).
    pub reason_code: Option<String>,
    /// The earlier message this one duplicates.
    pub duplicate_of: Option<MessageId>,
    /// The earlier message with the same identity and other content.
    pub revision_of: Option<MessageId>,
    /// The newest later message that revises this one (directly or through a chain of
    /// revisions): this message is superseded by a corrected one.
    #[serde(default)]
    pub revised_by: Option<MessageId>,
    /// Deliveries with their attempts, in destination order.
    pub deliveries: Vec<DeliveryRecord>,
}

/// Most fields listed in [`ParsedFields`].
pub const MAX_FIELDS: usize = 500;

/// The original message's fields by the paths filters and mappings use
/// (`docs/spec/configuration.md` §8). Carries message content, like [`MessageBody`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ParsedFields {
    /// Path and value in message order, at most [`MAX_FIELDS`]; empty when unavailable.
    pub fields: Vec<FieldValue>,
    /// More fields exist than are listed.
    pub truncated: bool,
    /// Why no fields are listed (the original does not parse, the protocol has no field
    /// list, the body is not available), operator-facing, no message content.
    pub unavailable: Option<String>,
}

/// One field of a [`ParsedFields`] list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FieldValue {
    /// The path in the protocol's syntax (`MSH.9.1`, `R.3.4`).
    pub path: String,
    /// The decoded text at the path.
    pub value: String,
}

/// Where in a file a message came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SourceFile {
    /// File name relative to the source directory.
    pub name: String,
    /// SHA-256 of the whole file, lowercase hex.
    pub file_hash: String,
    /// First row of the record or group, from 1.
    pub first_row: u32,
    /// Last row of the record or group.
    pub last_row: u32,
}

/// One message to one destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeliveryRecord {
    /// The delivery.
    pub delivery_id: DeliveryId,
    /// Its message.
    pub message_id: MessageId,
    /// Its channel.
    pub channel_id: ChannelId,
    /// Its destination connector.
    pub destination_id: ConnectorId,
    /// The destination's label.
    pub destination_label: String,
    /// Delivery state.
    pub state: DeliveryState,
    /// What was proven.
    pub ack_level: AckLevel,
    /// Attempts in the current attempt cycle.
    pub attempts: u32,
    /// Attempts in all cycles.
    pub total_attempts: u32,
    /// When it was created.
    pub created_at: Timestamp,
    /// When its state last changed.
    pub updated_at: Timestamp,
    /// When the next attempt starts, while `queued` for a retry.
    pub next_attempt_at: Option<Timestamp>,
    /// Why it is in this state (`transform_error`, `engine_restarted`, ...).
    pub reason_code: Option<String>,
    /// The operator's reason, when closed.
    pub closed_reason: Option<String>,
    /// The operator's reason for the resend that started the current attempt cycle.
    #[serde(default)]
    pub resend_reason: Option<String>,
    /// The last failure in words: the attempt's error summary, or the failing mapping rule
    /// and path for `transform_error` (diagnostics.md §7.4; never message content).
    #[serde(default)]
    pub last_error: Option<String>,
    /// The configuration version whose mapping produced the transformed body.
    #[ts(type = "number | null")]
    pub mapping_version: Option<u64>,
    /// SHA-256 of the transformed body, lowercase hex.
    pub transformed_hash: Option<String>,
    /// Attempts, oldest first.
    pub attempt_log: Vec<AttemptRecord>,
}

/// One delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AttemptRecord {
    /// Number within the delivery, from 1 across all cycles.
    pub attempt_no: u32,
    /// Committed before the first byte was written.
    pub started_at: Timestamp,
    /// When it ended; absent while in flight.
    pub ended_at: Option<Timestamp>,
    /// How it ended.
    pub result: Option<AttemptResult>,
    /// Protocol acknowledgement code received (`AA`, `CA`, `AR`, ...).
    pub ack_code: Option<String>,
    /// The acknowledgement message's own control id (HL7 MSH-10 of the ACK); MSA-2 in it
    /// echoes the message that was sent.
    #[serde(default)]
    pub ack_control_id: Option<String>,
    /// Error code for failed attempts (`connection_refused`, `write_timeout`, ...).
    pub error_code: Option<String>,
    /// What this attempt proved.
    pub ack_level: AckLevel,
}

/// Which body of a message to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BodySelector {
    /// The bytes as received.
    Original,
    /// The output produced for one delivery.
    Transformed {
        /// The delivery.
        delivery_id: DeliveryId,
    },
}

/// How to render a body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BodyFormat {
    /// Text with control characters made visible (`<STX>`, `<CR>` and a line break),
    /// as `packie_core::visible_text` renders it.
    #[default]
    Text,
    /// Hex dump with offsets, as `packie_core::hex_dump` renders it.
    Hex,
}

/// A body request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BodyRequest {
    /// The message.
    pub message_id: MessageId,
    /// Original or transformed.
    pub which: BodySelector,
    /// Rendering. Default `text`.
    #[serde(default, rename = "as")]
    pub format: BodyFormat,
}

/// A rendered body, for the local client that asked only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageBody {
    /// The message.
    pub message_id: MessageId,
    /// Which body.
    pub which: BodySelector,
    /// How it is rendered.
    #[serde(rename = "as")]
    pub format: BodyFormat,
    /// The rendering.
    pub content: String,
    /// Size of the raw bytes.
    #[ts(type = "number")]
    pub byte_length: u64,
    /// SHA-256 of the raw bytes, lowercase hex.
    pub hash: String,
    /// The rendering stops early because the body exceeds the display limit.
    pub truncated: bool,
}

/// Which destinations a resend targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ResendTargets {
    /// Every destination of the channel in the active configuration.
    All,
    /// These destinations.
    Destinations(Vec<ConnectorId>),
}

/// How to resend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ResendMode {
    /// A new attempt cycle on the existing deliveries, with the stored output.
    #[default]
    SameMessage,
    /// A new message from the same original bytes, through the current configuration.
    NewMessage,
}

/// A resend command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ResendRequest {
    /// The message.
    pub message_id: MessageId,
    /// Which destinations.
    pub destinations: ResendTargets,
    /// Default `same_message`.
    #[serde(default)]
    pub mode: ResendMode,
    /// Required to resend deliveries that may already have been processed
    /// (`timed_out`, `outcome_unknown`, `acknowledged`).
    #[serde(default)]
    pub confirm_duplicate_risk: bool,
    /// Why, optional, 1 to 500 characters; stored with the new attempt cycle as
    /// [`DeliveryRecord::resend_reason`].
    #[serde(default)]
    pub reason: Option<String>,
    /// Required to resend a message that a newer revision supersedes (it may reissue a
    /// corrected result, CLIA §493.1291(k)).
    #[serde(default)]
    pub confirm_superseded: bool,
}

impl ResendRequest {
    /// No reason, or one of 1 to 500 characters.
    #[must_use]
    pub fn has_valid_reason(&self) -> bool {
        self.reason
            .as_deref()
            .is_none_or(|r| (1..=MAX_REASON).contains(&r.trim().chars().count()))
    }
}

/// What a resend did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ResendResult {
    /// The new message, for `new_message`.
    pub new_message_id: Option<MessageId>,
    /// One entry per requested destination.
    pub destinations: Vec<ResendOutcome>,
}

/// What a resend did for one destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ResendOutcome {
    /// The destination.
    pub destination_id: ConnectorId,
    /// Its label.
    pub destination_label: String,
    /// The queued delivery, when one was queued.
    pub delivery_id: Option<DeliveryId>,
    /// Why none was queued.
    pub rejected: Option<Rejection>,
}

/// A command precondition that failed: a stable code and an operator-facing explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Rejection {
    /// Stable code.
    pub code: RejectionCode,
    /// What is wrong and what to do.
    pub message: String,
}

/// Stable rejection codes of operator commands (`docs/spec/operator-console.md` §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCode {
    /// The channel, message, delivery or version does not exist.
    NotFound,
    /// Pause of a channel that is already stopped.
    AlreadyStopped,
    /// Resume of a channel that is not operator-paused.
    NotPaused,
    /// The channel is disabled in the active configuration.
    DisabledInConfig,
    /// Restart of an operator-paused channel (use resume).
    OperatorPaused,
    /// Resend of a `queued` or `sending` delivery, or close of a `sending` one.
    DeliveryInProgress,
    /// The delivery state does not allow the command.
    InvalidState,
    /// The resend needs `confirm_duplicate_risk`.
    DuplicateRiskUnconfirmed,
    /// The destination is not in the active configuration or is disabled.
    DestinationNotFound,
    /// Close without a reason of 1–500 characters.
    ReasonRequired,
    /// The document was edited from a version that is no longer active.
    VersionConflict,
    /// The document has validation errors.
    InvalidConfig,
    /// Another client already views the transcript.
    TranscriptInUse,
    /// A newer revision supersedes the message to resend; needs `confirm_superseded`.
    Superseded,
}

/// A close command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CloseRequest {
    /// The delivery.
    pub delivery_id: DeliveryId,
    /// Why, 1 to 500 characters; stored as `closed_reason`.
    pub reason: String,
}

impl CloseRequest {
    /// The reason is present and short enough.
    #[must_use]
    pub fn has_valid_reason(&self) -> bool {
        (1..=MAX_REASON).contains(&self.reason.trim().chars().count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_defaults_and_limits() {
        let query: MessageQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(query, MessageQuery::default());
        assert_eq!(query.limit, DEFAULT_PAGE);
        assert!(query.sort.descending);
        assert!(query.check().is_ok());

        let mut bad = query.clone();
        bad.limit = MAX_PAGE + 1;
        assert!(bad.check().is_err());
        let mut short = query;
        short.filter.text = Some("ab".into());
        assert!(short.check().unwrap_err().contains("3 characters"));
    }

    #[test]
    fn body_request_uses_as_and_tagged_selector() {
        let delivery = DeliveryId::generate();
        let json = serde_json::json!({
            "message_id": MessageId::generate(),
            "which": {"kind": "transformed", "delivery_id": delivery},
            "as": "hex",
        });
        let request: BodyRequest = serde_json::from_value(json).unwrap();
        assert_eq!(
            request.which,
            BodySelector::Transformed {
                delivery_id: delivery
            }
        );
        assert_eq!(request.format, BodyFormat::Hex);
    }

    #[test]
    fn resend_targets_are_all_or_a_list() {
        assert_eq!(serde_json::to_value(ResendTargets::All).unwrap(), "all");
        let id = ConnectorId::generate();
        let list = serde_json::to_value(ResendTargets::Destinations(vec![id])).unwrap();
        assert_eq!(list, serde_json::json!({"destinations": [id]}));
    }

    #[test]
    fn id_prefixes_take_the_stored_form() {
        assert_eq!(id_prefix("01A1B2CA").unwrap(), "01a1b2ca");
        assert_eq!(id_prefix("01a1b2caecee76").unwrap(), "01a1b2ca-ecee-76");
        assert_eq!(id_prefix(" 01a1b2ca-ecee-7").unwrap(), "01a1b2ca-ecee-7");
        let full = "01a1b2ca-ecee-76cb-a401-8e4001132030";
        assert_eq!(id_prefix(&full.replace('-', "")).unwrap(), full);
        assert!(id_prefix("01a1b2c").is_err());
        assert!(id_prefix("01a1b2cz").is_err());
        let query = MessageQuery {
            filter: MessageFilter {
                message_id_prefix: Some("0123".into()),
                ..MessageFilter::default()
            },
            ..MessageQuery::default()
        };
        assert!(query.check().unwrap_err().contains("at least 8"));
    }

    #[test]
    fn close_reason_bounds() {
        let close = |reason: &str| CloseRequest {
            delivery_id: DeliveryId::generate(),
            reason: reason.into(),
        };
        assert!(!close("  ").has_valid_reason());
        assert!(close("Checked with the LIS team").has_valid_reason());
        assert!(!close(&"x".repeat(501)).has_valid_reason());
    }
}
