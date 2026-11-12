//! How stored messages, deliveries and attempts read in the terminal: the list rows and
//! the detail view of `packie messages`. Wording follows the console and
//! `docs/spec/operator-console.md` §2 and §6 (no "delivered" or "success" below an
//! accepted acknowledgement).

use packie_core::{AckLevel, ByteSize, DeliveryState};
use packie_ipc::messages::{
    AttemptRecord, DeliveryRecord, MessageRecord, MessageSummary, ParsedFields,
};

use crate::output::{opt_text, opt_time, short_time, table};

/// Column headers of `packie messages list`.
pub(crate) const LIST_HEADERS: [&str; 7] = [
    "RECEIVED",
    "CHANNEL",
    "STATE",
    "TYPE",
    "IDENTIFIER",
    "DELIVERIES",
    "MESSAGE ID",
];

/// One row of `packie messages list`.
pub(crate) fn list_row(message: &MessageSummary) -> Vec<String> {
    vec![
        short_time(message.received_at),
        message.channel_id.to_string(),
        message.state.to_string(),
        opt_text(message.message_type.as_deref()),
        opt_text(identifier(message).as_deref()),
        deliveries_text(message),
        message.message_id.to_string(),
    ]
}

/// The control id, else the sample id (the console's Identifier column).
fn identifier(message: &MessageSummary) -> Option<String> {
    message
        .control_id
        .clone()
        .or_else(|| message.sample_id.clone())
}

/// The roll-up with one entry per delivery: `attention (lis rejected, his acknowledged)`.
fn deliveries_text(message: &MessageSummary) -> String {
    if message.deliveries.is_empty() {
        return message.delivery_summary.to_string();
    }
    let chips: Vec<String> = message
        .deliveries
        .iter()
        .map(|c| format!("{} {}", c.destination_id, c.state))
        .collect();
    format!("{} ({})", message.delivery_summary, chips.join(", "))
}

/// What a delivery proved, in words (`ack_level`; a lower level is never success).
pub(crate) fn ack_text(level: AckLevel) -> &'static str {
    match level {
        AckLevel::None => "No acknowledgement was received.",
        AckLevel::HandedOff => "Written to the destination. Not confirmed by it.",
        AckLevel::Received => "Received by the destination. Processing not confirmed.",
        AckLevel::Accepted => "Accepted by the destination's application.",
    }
}

/// The state label of operator-console.md §3 ("Delivery state"), with the
/// acknowledgement wording of §6 for acknowledged deliveries.
fn delivery_label(delivery: &DeliveryRecord) -> String {
    match delivery.state {
        DeliveryState::Queued => "Queued".to_owned(),
        DeliveryState::Sending => "Sending".to_owned(),
        DeliveryState::Rejected => "Rejected by destination".to_owned(),
        DeliveryState::TimedOut => "No acknowledgement in time, may have been delivered".to_owned(),
        DeliveryState::OutcomeUnknown => "Outcome unknown, may have been delivered".to_owned(),
        DeliveryState::Failed => "Failed, not delivered".to_owned(),
        DeliveryState::Cancelled => "Closed".to_owned(),
        DeliveryState::Acknowledged => {
            format!("Acknowledged. {}", ack_text(delivery.ack_level))
        }
    }
}

/// Why a failed delivery failed and what to do, when its reason says more than the label.
fn failure_cause(delivery: &DeliveryRecord) -> Option<&'static str> {
    if delivery.state != DeliveryState::Failed {
        return None;
    }
    match delivery.reason_code.as_deref() {
        Some("transform_error") => Some(
            "The destination's mapping failed, so no attempt was made. Correct the mapping, \
             apply the configuration, then resend.",
        ),
        Some("mapping_required") => Some(
            "The destination needs a mapping to its protocol and has none, so no attempt was \
             made. Add a mapping, apply, then resend.",
        ),
        _ if delivery.total_attempts == 0 => {
            Some("No attempt was made. Fix the error below, then resend.")
        }
        _ => Some("Every attempt failed. Fix the error below, then resend."),
    }
}

/// The detail view of one message as lines: facts, then each delivery with its attempts.
pub(crate) fn detail_lines(record: &MessageRecord) -> Vec<String> {
    let mut lines = facts(record);
    lines.push(String::new());
    if record.deliveries.is_empty() {
        lines.push("Deliveries: none. This message has no destination to deliver to.".to_owned());
    } else {
        lines.push(format!("Deliveries ({})", record.deliveries.len()));
        for delivery in &record.deliveries {
            lines.push(String::new());
            lines.extend(delivery_lines(delivery));
        }
    }
    lines
}

/// The "Fields" section of `packie messages show --fields`: path and value columns, or why
/// no fields are listed.
pub(crate) fn field_lines(parsed: Option<&ParsedFields>) -> Vec<String> {
    let Some(parsed) = parsed else {
        return vec!["Fields: the engine did not list the fields of this message.".to_owned()];
    };
    if let Some(reason) = &parsed.unavailable {
        return vec![format!("Fields: not listed. {reason}")];
    }
    if parsed.fields.is_empty() {
        return vec!["Fields: the original has no non-empty fields.".to_owned()];
    }
    let rows: Vec<Vec<String>> = parsed
        .fields
        .iter()
        .map(|f| vec![f.path.clone(), f.value.clone()])
        .collect();
    let mut lines = vec![format!("Fields ({})", rows.len()), String::new()];
    lines.extend(table(&["PATH", "VALUE"], &rows));
    if parsed.truncated {
        lines.push(format!(
            "Only the first {} fields are listed; the message has more.",
            rows.len()
        ));
    }
    lines
}

fn facts(record: &MessageRecord) -> Vec<String> {
    let m = &record.summary;
    let mut lines = vec![
        format!("Message      {}", m.message_id),
        format!("Channel      {} ({})", m.channel_label, m.channel_id),
        format!("State        {}", m.state),
        format!(
            "Received     {} ({}, from {})",
            short_time(m.received_at),
            m.direction,
            opt_text(m.source_peer.as_deref())
        ),
        format!("Origin       {}", m.origin),
        format!(
            "Protocol     {}, type {}",
            m.protocol,
            opt_text(m.message_type.as_deref())
        ),
        format!(
            "Identifiers  control {}, sample {}, patient {}",
            opt_text(m.control_id.as_deref()),
            opt_text(m.sample_id.as_deref()),
            opt_text(m.patient_id.as_deref())
        ),
        format!(
            "Original     {}, sha256 {}",
            ByteSize(m.byte_length),
            record.content_hash
        ),
        format!("Config       version {}", m.config_version),
    ];
    if let Some(code) = &record.response_code {
        lines.push(format!(
            "Response     {code} sent at {}",
            opt_time(record.responded_at)
        ));
    }
    lines.extend(links(record));
    lines.extend(reasons(record));
    lines
}

/// Related messages and the source file.
fn links(record: &MessageRecord) -> Vec<String> {
    let mut lines = Vec::new();
    let related = [
        ("Resent from", record.summary.resent_from),
        ("Duplicate of", record.duplicate_of),
        ("Revision of", record.revision_of),
    ];
    for (label, id) in related {
        if let Some(id) = id {
            lines.push(format!("{label:<12} {id}"));
        }
    }
    if let Some(file) = &record.source_file {
        lines.push(format!(
            "Source file  {} (rows {}-{})",
            file.name, file.first_row, file.last_row
        ));
    }
    lines
}

/// Why the message was aborted, filtered or found invalid.
fn reasons(record: &MessageRecord) -> Vec<String> {
    [
        ("Aborted", &record.abort_reason),
        ("Reason", &record.reason_code),
    ]
    .iter()
    .filter_map(|(label, reason)| reason.as_ref().map(|r| format!("{label:<12} {r}")))
    .collect()
}

fn delivery_lines(d: &DeliveryRecord) -> Vec<String> {
    let mut lines = vec![
        format!("Delivery     {}", d.delivery_id),
        format!(
            "  Destination  {} ({})",
            d.destination_label, d.destination_id
        ),
        format!("  State        {}", delivery_label(d)),
        format!(
            "  Attempts     {} in this cycle, {} in total; last change {}",
            d.attempts,
            d.total_attempts,
            short_time(d.updated_at)
        ),
    ];
    if let Some(at) = d.next_attempt_at {
        lines.push(format!("  Next attempt {}", short_time(at)));
    }
    if let Some(cause) = failure_cause(d) {
        lines.push(format!("  Cause        {cause}"));
    }
    if let Some(reason) = &d.reason_code {
        lines.push(format!("  Reason       {reason}"));
    }
    if let Some(error) = &d.last_error {
        lines.push(format!("  Error        {error}"));
    }
    if let Some(reason) = &d.closed_reason {
        lines.push(format!("  Closed       {reason}"));
    }
    if let Some(reason) = &d.resend_reason {
        lines.push(format!("  Resent       {reason}"));
    }
    if !d.attempt_log.is_empty() {
        let rows: Vec<Vec<String>> = d.attempt_log.iter().map(attempt_row).collect();
        let headers = [
            "ATTEMPT", "STARTED", "ENDED", "RESULT", "ACK CODE", "ACK ID", "ERROR",
        ];
        lines.extend(table(&headers, &rows).into_iter().map(|l| format!("  {l}")));
    }
    lines
}

fn attempt_row(a: &AttemptRecord) -> Vec<String> {
    vec![
        a.attempt_no.to_string(),
        short_time(a.started_at),
        opt_time(a.ended_at),
        a.result
            .map_or_else(|| "in progress".to_owned(), |r| r.to_string()),
        opt_text(a.ack_code.as_deref()),
        opt_text(a.ack_control_id.as_deref()),
        opt_text(a.error_code.as_deref()),
    ]
}

#[cfg(test)]
mod tests {
    use packie_core::{
        AckLevel, DeliverySummary, MessageDirection, MessageId, MessageOrigin, MessageState,
        ProtocolKind, Timestamp,
    };
    use packie_ipc::messages::{DeliveryChip, MessageSummary};

    use super::{ack_text, list_row};

    fn summary() -> MessageSummary {
        MessageSummary {
            message_id: MessageId::generate(),
            channel_id: "orders".parse().unwrap(),
            channel_label: "Orders".to_owned(),
            seq: 1,
            direction: MessageDirection::Inbound,
            state: MessageState::Routed,
            origin: MessageOrigin::Received,
            received_at: Timestamp::parse("2026-11-06T12:00:00Z").unwrap(),
            protocol: ProtocolKind::Hl7,
            message_type: Some("ORU^R01".to_owned()),
            control_id: Some("MSG-00000".to_owned()),
            sample_id: None,
            patient_id: Some("MRN-00000".to_owned()),
            byte_length: 120,
            source_peer: None,
            config_version: 1,
            resent_from: None,
            delivery_summary: DeliverySummary::Attention,
            deliveries: vec![DeliveryChip {
                delivery_id: packie_core::DeliveryId::generate(),
                destination_id: "lis".parse().unwrap(),
                destination_label: "LIS".to_owned(),
                state: packie_core::DeliveryState::Rejected,
                ack_level: AckLevel::None,
            }],
        }
    }

    #[test]
    fn a_list_row_shows_the_identifier_and_each_delivery() {
        let message = summary();
        let row = list_row(&message);
        assert_eq!(
            row[..6],
            [
                "2026-11-06 12:00:00Z",
                "orders",
                "routed",
                "ORU^R01",
                "MSG-00000",
                "attention (lis rejected)"
            ]
        );
        assert_eq!(row[6], message.message_id.to_string());
    }

    #[test]
    fn a_row_without_deliveries_or_identifiers_uses_placeholders() {
        let mut message = summary();
        message.deliveries.clear();
        message.delivery_summary = DeliverySummary::None;
        message.control_id = None;
        message.message_type = None;
        let row = list_row(&message);
        assert_eq!(row[3], "-");
        assert_eq!(row[4], "-");
        assert_eq!(row[5], "none");
    }

    #[test]
    fn acknowledgement_levels_never_read_as_success_below_accepted() {
        assert_eq!(
            ack_text(AckLevel::HandedOff),
            "Written to the destination. Not confirmed by it."
        );
        for level in [AckLevel::None, AckLevel::HandedOff, AckLevel::Received] {
            let text = ack_text(level).to_lowercase();
            assert!(
                !text.contains("delivered") && !text.contains("success"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_failed_mapping_names_its_cause_and_error() {
        use packie_core::{DeliveryId, DeliveryState};
        use packie_ipc::messages::DeliveryRecord;

        let delivery = DeliveryRecord {
            delivery_id: DeliveryId::generate(),
            message_id: MessageId::generate(),
            channel_id: "orders".parse().unwrap(),
            destination_id: "lis".parse().unwrap(),
            destination_label: "LIS".to_owned(),
            state: DeliveryState::Failed,
            ack_level: AckLevel::None,
            attempts: 0,
            total_attempts: 0,
            created_at: Timestamp::parse("2026-11-06T12:00:00Z").unwrap(),
            updated_at: Timestamp::parse("2026-11-06T12:00:00Z").unwrap(),
            next_attempt_at: None,
            reason_code: Some("transform_error".to_owned()),
            closed_reason: None,
            resend_reason: None,
            last_error: Some("rule 2 (map): OBX.5 is not a number".to_owned()),
            mapping_version: Some(3),
            transformed_hash: None,
            attempt_log: Vec::new(),
        };
        let lines = super::delivery_lines(&delivery).join("\n");
        assert!(
            lines.contains("State        Failed, not delivered"),
            "{lines}"
        );
        assert!(
            lines.contains("mapping failed, so no attempt was made"),
            "{lines}"
        );
        assert!(
            lines.contains("Error        rule 2 (map): OBX.5 is not a number"),
            "{lines}"
        );
        assert!(!lines.contains("after the last attempt"), "{lines}");
    }
}
