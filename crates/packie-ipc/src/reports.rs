//! Read-only reports over the message store: the selected-message export
//! (requirement P9 in `docs/requirements.md`) and the transmission verification report (P7).
//! Both are audited; message bodies appear only in an export that asks for them.

use packie_core::{
    AckLevel, ChannelId, ConnectorId, DeliveryState, EndpointId, MessageId, MessageState, Timestamp,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::messages::MessageFilter;

/// Messages per export chunk without bodies.
pub const EXPORT_CHUNK: u32 = 1000;
/// Messages per export chunk with bodies.
pub const EXPORT_CHUNK_WITH_BODIES: u32 = 100;
/// Most rows in one transmission report; a longer period is reported in parts.
pub const MAX_REPORT_ROWS: usize = 10_000;
/// The last line of the CSV rows of a truncated transmission report.
pub const TRUNCATED_MARKER: &str = "# truncated: more than 10000 deliveries in the period; the rows above are the first \
     10000. Report a shorter period to list every delivery.";

/// File format of an export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Comma-separated values with a header row.
    #[default]
    Csv,
    /// One JSON object per line.
    Jsonl,
}

/// One chunk of an export of the messages a filter selects, oldest first. The client asks
/// again with `offset` set to the previous chunk's `next_offset` until it is absent, and
/// writes the chunks in order (the CSV header comes with the first chunk only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExportRequest {
    /// Which messages, as in the Messages grid.
    #[serde(default)]
    pub filter: MessageFilter,
    /// CSV or JSON Lines.
    #[serde(default)]
    pub format: ExportFormat,
    /// Also export each original body (as UTF-8 text); a content view, audited as such.
    #[serde(default)]
    pub include_bodies: bool,
    /// Messages already exported; 0 for the first chunk (the one that is audited).
    #[serde(default)]
    pub offset: u32,
    /// The first chunk's `export_id`, required with every later chunk: only the first chunk
    /// is audited, so a continuation must belong to an export that was.
    #[serde(default)]
    pub export_id: Option<String>,
}

/// One chunk of an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExportChunk {
    /// CSV or JSON Lines text to append to the file.
    pub content: String,
    /// Messages in this chunk.
    pub count: u32,
    /// Messages the filter selects in total.
    #[ts(type = "number")]
    pub total: u64,
    /// The `offset` of the next chunk; absent after the last one.
    pub next_offset: Option<u32>,
    /// Pass with every later chunk of this export; absent after the last one.
    #[serde(default)]
    pub export_id: Option<String>,
}

/// A transmission verification report request: what one channel received and delivered in
/// a period, so the laboratory can document that results arrived unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TransmissionQuery {
    /// The channel.
    pub channel_id: ChannelId,
    /// Messages received at or after.
    pub since: Timestamp,
    /// Messages received before; default now.
    #[serde(default)]
    pub until: Option<Timestamp>,
}

/// The transmission verification report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TransmissionReport {
    /// The channel.
    pub channel_id: ChannelId,
    /// The period.
    pub since: Timestamp,
    /// The end of the period.
    pub until: Timestamp,
    /// When the report was made.
    pub generated_at: Timestamp,
    /// Messages received in the period.
    #[ts(type = "number")]
    pub messages: u64,
    /// Per destination.
    pub destinations: Vec<DestinationTransmission>,
    /// One row per delivery, oldest message first.
    pub rows: Vec<TransmissionRow>,
    /// More than [`MAX_REPORT_ROWS`] deliveries: only the first are listed (`messages` and
    /// `destinations` still cover the whole period); report a shorter period for every row.
    pub truncated: bool,
    /// The most rows one report lists ([`MAX_REPORT_ROWS`]).
    pub rows_limit: u32,
    /// Deliveries in the period, the rows a complete report lists; more than `rows.len()`
    /// when `truncated`.
    #[ts(type = "number")]
    pub period_rows_total: u64,
}

/// What one destination was sent and what it confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DestinationTransmission {
    /// The destination.
    pub destination_id: ConnectorId,
    /// Its label.
    pub label: String,
    /// Deliveries in the period.
    #[ts(type = "number")]
    pub deliveries: u64,
    /// Acknowledged at `accepted` (the destination's application accepted it).
    #[ts(type = "number")]
    pub accepted: u64,
    /// Acknowledged at `received`.
    #[ts(type = "number")]
    pub received: u64,
    /// Acknowledged at `handed_off` (written; not confirmed by the destination).
    #[ts(type = "number")]
    pub handed_off: u64,
    /// Rejected, failed, timed out or outcome unknown.
    #[ts(type = "number")]
    pub unresolved: u64,
    /// Closed by an operator.
    #[ts(type = "number")]
    pub closed: u64,
    /// Still queued or sending.
    #[ts(type = "number")]
    pub pending: u64,
}

/// One delivery in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TransmissionRow {
    /// The message.
    pub message_id: MessageId,
    /// When it was received.
    pub received_at: Timestamp,
    /// The endpoint the message was received on.
    pub source_endpoint_id: EndpointId,
    /// The connected peer it came from (address or device), as recorded at receipt.
    pub source_peer: Option<String>,
    /// The configuration version active when the message was received.
    #[ts(type = "number")]
    pub config_version: u64,
    /// The configuration version whose mapping produced the bytes sent to this destination.
    #[ts(type = "number")]
    pub mapping_version: Option<u64>,
    /// The message's state.
    pub message_state: MessageState,
    /// The sender's control id (HL7 MSH-10, ASTM H.3, ...).
    pub control_id: Option<String>,
    /// SHA-256 of the original bytes, lowercase hex.
    pub content_hash: String,
    /// The destination.
    pub destination_id: ConnectorId,
    /// The delivery's state.
    pub state: DeliveryState,
    /// What the destination proved.
    pub ack_level: AckLevel,
    /// The acknowledgement code of the last attempt (`AA`, `CA`, ...).
    pub ack_code: Option<String>,
    /// The acknowledgement message's own control id (HL7 MSH-10 of the ACK).
    pub ack_control_id: Option<String>,
    /// SHA-256 of the bytes sent, lowercase hex (absent for a pass-through or unsent
    /// delivery).
    pub sent_hash: Option<String>,
    /// Attempts made.
    pub attempts: u32,
    /// When the first attempt started.
    pub first_attempt_at: Option<Timestamp>,
    /// When the acknowledgement arrived.
    pub acknowledged_at: Option<Timestamp>,
}

impl TransmissionReport {
    /// The rows as CSV with a header row; a truncated report ends with [`TRUNCATED_MARKER`]
    /// as its last line.
    #[must_use]
    pub fn rows_csv(&self) -> String {
        let mut out = String::from(
            "message_id,received_at,channel_id,source_endpoint_id,source_peer,config_version,\
             mapping_version,message_state,control_id,content_hash,destination_id,state,\
             ack_level,ack_code,ack_control_id,sent_hash,attempts,first_attempt_at,\
             acknowledged_at\n",
        );
        let text = |value: Option<u64>| value.map(|v| v.to_string()).unwrap_or_default();
        for row in &self.rows {
            let fields = [
                row.message_id.to_string(),
                row.received_at.to_string(),
                self.channel_id.to_string(),
                row.source_endpoint_id.to_string(),
                row.source_peer.clone().unwrap_or_default(),
                row.config_version.to_string(),
                text(row.mapping_version),
                row.message_state.to_string(),
                row.control_id.clone().unwrap_or_default(),
                row.content_hash.clone(),
                row.destination_id.to_string(),
                row.state.to_string(),
                row.ack_level.to_string(),
                row.ack_code.clone().unwrap_or_default(),
                row.ack_control_id.clone().unwrap_or_default(),
                row.sent_hash.clone().unwrap_or_default(),
                row.attempts.to_string(),
                row.first_attempt_at
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
                row.acknowledged_at
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            ];
            out.push_str(&fields.map(|f| csv_field(&f)).join(","));
            out.push('\n');
        }
        if self.truncated {
            out.push_str(TRUNCATED_MARKER);
            out.push('\n');
        }
        out
    }
}

/// One CSV field, quoted when it holds a comma, quote or line break; a leading `=`, `+`,
/// `-` or `@` is prefixed with `'` so spreadsheets do not run it as a formula.
#[must_use]
pub fn csv_field(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_are_quoted_and_formula_safe() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }
}
