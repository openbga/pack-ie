//! Read-only reports over the store: the selected-message export (P9) and the transmission
//! verification report (P7). Both read through the registry's message queries.

use packie_core::{DeliveryState, Timestamp};
use packie_ipc::messages::{
    MessageFilter, MessageList, MessageQuery, MessageRecord, MessageSort, MessageSummary,
};
use packie_ipc::reports::{
    EXPORT_CHUNK, EXPORT_CHUNK_WITH_BODIES, ExportChunk, ExportFormat, ExportRequest,
    MAX_REPORT_ROWS, TransmissionQuery, TransmissionReport, TransmissionRow, csv_field,
};
use serde_json::{Map, Value, json};

use crate::registry::{ChannelRegistry, RegistryError};

/// The exported columns, in order; `body` follows when bodies are included.
const COLUMNS: [&str; 12] = [
    "message_id",
    "received_at",
    "channel_id",
    "direction",
    "state",
    "protocol",
    "message_type",
    "control_id",
    "sample_id",
    "patient_id",
    "byte_length",
    "deliveries",
];

/// One export chunk: the messages the filter selects, oldest first, from `offset`.
pub(crate) async fn export(
    registry: &dyn ChannelRegistry,
    request: &ExportRequest,
) -> Result<ExportChunk, RegistryError> {
    let limit = if request.include_bodies {
        EXPORT_CHUNK_WITH_BODIES
    } else {
        EXPORT_CHUNK
    };
    let list = registry
        .query_messages(oldest_first(&request.filter, request.offset, limit))
        .await?;
    let mut content = String::new();
    if request.format == ExportFormat::Csv && request.offset == 0 {
        content.push_str(&COLUMNS.join(","));
        if request.include_bodies {
            content.push_str(",body");
        }
        content.push('\n');
    }
    for summary in &list.messages {
        let body = if request.include_bodies {
            Some(body_text(registry, summary).await?)
        } else {
            None
        };
        content.push_str(&row(summary, body.as_deref(), request.format));
        content.push('\n');
    }
    let (count, next_offset) = page(request.offset, &list);
    Ok(ExportChunk {
        content,
        count,
        total: list.total,
        next_offset,
        export_id: None,
    })
}

/// The messages on a page that starts at `offset`, and the offset of the next page if any.
fn page(offset: u32, list: &MessageList) -> (u32, Option<u32>) {
    let count = u32::try_from(list.messages.len()).unwrap_or(u32::MAX);
    let next = offset.saturating_add(count);
    (
        count,
        (count > 0 && u64::from(next) < list.total).then_some(next),
    )
}

fn oldest_first(filter: &MessageFilter, offset: u32, limit: u32) -> MessageQuery {
    MessageQuery {
        filter: filter.clone(),
        sort: MessageSort {
            descending: false,
            ..MessageSort::default()
        },
        offset,
        limit,
    }
}

/// The original body as UTF-8 text (invalid sequences replaced). A body that cannot be read
/// in full fails the export rather than exporting empty text.
async fn body_text(
    registry: &dyn ChannelRegistry,
    summary: &MessageSummary,
) -> Result<String, RegistryError> {
    let record = registry.message(summary.message_id).await?;
    let bytes = crate::fields::original(registry, &record)
        .await
        .map_err(|cause| {
            RegistryError::Failed(format!(
                "The original body of message {} cannot be read ({cause}), so the export \
                 stopped. Export without bodies, or check the store.",
                summary.message_id
            ))
        })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `lis:acknowledged:accepted;lims:queued:none`.
fn deliveries(summary: &MessageSummary) -> String {
    summary
        .deliveries
        .iter()
        .map(|d| format!("{}:{}:{}", d.destination_id, d.state, d.ack_level))
        .collect::<Vec<_>>()
        .join(";")
}

/// The values of [`COLUMNS`] for one message: CSV text and JSON value.
fn values(summary: &MessageSummary) -> [(String, Value); 12] {
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    [
        (summary.message_id.to_string(), json!(summary.message_id)),
        (summary.received_at.to_string(), json!(summary.received_at)),
        (summary.channel_id.to_string(), json!(summary.channel_id)),
        (summary.direction.to_string(), json!(summary.direction)),
        (summary.state.to_string(), json!(summary.state)),
        (summary.protocol.to_string(), json!(summary.protocol)),
        (text(&summary.message_type), json!(summary.message_type)),
        (text(&summary.control_id), json!(summary.control_id)),
        (text(&summary.sample_id), json!(summary.sample_id)),
        (text(&summary.patient_id), json!(summary.patient_id)),
        (summary.byte_length.to_string(), json!(summary.byte_length)),
        (deliveries(summary), json!(summary.deliveries)),
    ]
}

fn row(summary: &MessageSummary, body: Option<&str>, format: ExportFormat) -> String {
    let values = values(summary);
    match format {
        ExportFormat::Csv => values
            .iter()
            .map(|(text, _)| text.as_str())
            .chain(body)
            .map(csv_field)
            .collect::<Vec<_>>()
            .join(","),
        ExportFormat::Jsonl => {
            let mut object: Map<String, Value> = COLUMNS
                .iter()
                .zip(values)
                .map(|(name, (_, value))| ((*name).to_owned(), value))
                .collect();
            if let Some(body) = body {
                object.insert("body".into(), json!(body));
            }
            Value::Object(object).to_string()
        }
    }
}

/// Whether `channel` can be reported: configured now, or with stored messages (a removed
/// channel's history stays reportable).
pub(crate) async fn reportable(
    registry: &dyn ChannelRegistry,
    configured: bool,
    channel: packie_core::ChannelId,
) -> Result<bool, RegistryError> {
    if configured {
        return Ok(true);
    }
    let filter = MessageFilter {
        channels: vec![channel],
        ..MessageFilter::default()
    };
    let stored = registry.query_messages(oldest_first(&filter, 0, 1)).await?;
    Ok(stored.total > 0)
}

/// The transmission report of one channel and period.
pub(crate) async fn transmission(
    registry: &dyn ChannelRegistry,
    query: &TransmissionQuery,
) -> Result<TransmissionReport, RegistryError> {
    let until = query.until.unwrap_or_else(Timestamp::now);
    let filter = MessageFilter {
        channels: vec![query.channel_id],
        since: Some(query.since),
        until: Some(until),
        ..MessageFilter::default()
    };
    let destinations = registry.transmission_totals(filter.clone()).await?;
    let mut report = TransmissionReport {
        channel_id: query.channel_id,
        since: query.since,
        until,
        generated_at: Timestamp::now(),
        messages: 0,
        period_rows_total: destinations.iter().map(|d| d.deliveries).sum(),
        destinations,
        rows: Vec::new(),
        truncated: false,
        rows_limit: u32::try_from(MAX_REPORT_ROWS).unwrap_or(u32::MAX),
    };
    let mut offset = 0;
    loop {
        let list = registry
            .query_messages(oldest_first(&filter, offset, EXPORT_CHUNK))
            .await?;
        report.messages = list.total;
        for summary in &list.messages {
            let record = registry.message(summary.message_id).await?;
            add_message(&mut report, &record);
        }
        match page(offset, &list) {
            (_, Some(next)) if !report.truncated => offset = next,
            _ => return Ok(report),
        }
    }
}

/// The rows of `record`'s deliveries, up to [`MAX_REPORT_ROWS`]; the totals come from the
/// store's aggregate over the whole period.
fn add_message(report: &mut TransmissionReport, record: &MessageRecord) {
    for delivery in &record.deliveries {
        if report.rows.len() >= MAX_REPORT_ROWS {
            report.truncated = true;
            return;
        }
        let last = delivery.attempt_log.last();
        report.rows.push(TransmissionRow {
            message_id: record.summary.message_id,
            received_at: record.summary.received_at,
            source_endpoint_id: record.source_endpoint_id,
            source_peer: record.summary.source_peer.clone(),
            config_version: record.summary.config_version,
            mapping_version: delivery.mapping_version,
            message_state: record.summary.state,
            control_id: record.summary.control_id.clone(),
            content_hash: record.content_hash.clone(),
            destination_id: delivery.destination_id,
            state: delivery.state,
            ack_level: delivery.ack_level,
            ack_code: last.and_then(|a| a.ack_code.clone()),
            ack_control_id: last.and_then(|a| a.ack_control_id.clone()),
            sent_hash: delivery.transformed_hash.clone(),
            attempts: delivery.total_attempts,
            first_attempt_at: delivery.attempt_log.first().map(|a| a.started_at),
            acknowledged_at: (delivery.state == DeliveryState::Acknowledged)
                .then(|| last.and_then(|a| a.ended_at))
                .flatten(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> MessageSummary {
        serde_json::from_value(json!({
            "message_id": packie_core::MessageId::generate(), "channel_id": "orders",
            "channel_label": "Orders", "seq": 1, "direction": "inbound", "state": "routed",
            "origin": "received", "received_at": "2026-11-06T12:00:00Z", "protocol": "hl7_mllp",
            "message_type": "ORU^R01", "control_id": "=MSG-00001", "sample_id": null,
            "patient_id": "MRN-00000", "byte_length": 42, "source_peer": null,
            "config_version": 1, "resent_from": null, "delivery_summary": "complete",
            "deliveries": [],
        }))
        .unwrap()
    }

    #[test]
    fn csv_and_json_lines_carry_the_same_columns() {
        let summary = summary();
        let csv = row(&summary, Some("MSH|"), ExportFormat::Csv);
        let fields: Vec<&str> = csv.split(',').collect();
        assert_eq!(fields.len(), COLUMNS.len() + 1, "{csv}");
        assert_eq!(fields[7], "'=MSG-00001", "formula-like values are guarded");
        assert_eq!(fields[8], "", "absent values are empty");

        let json: Value = serde_json::from_str(&row(&summary, None, ExportFormat::Jsonl)).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut expected = COLUMNS.to_vec();
        keys.sort_unstable();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(json["sample_id"], Value::Null);
        assert_eq!(json["deliveries"], json!([]));
    }
}
