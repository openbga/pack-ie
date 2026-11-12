//! Message grid and detail reads. Bodies are deliberately outside this module.

use std::{fmt::Display, str::FromStr};

use crate::Cached;
use packie_core::{DeliveryId, MessageId};
use packie_ipc::messages::{
    AttemptRecord, DeliveryChip, DeliveryRecord, MessageFilter, MessageList, MessageQuery,
    MessageRecord, MessageSort, MessageSortField, MessageSummary,
};
use packie_ipc::reports::DestinationTransmission;
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter, types::Value};

const DELIVERY_SUMMARY: &str = "CASE
    WHEN EXISTS (SELECT 1 FROM deliveries d WHERE d.message_id = m.id
        AND d.state IN ('rejected', 'failed', 'timed_out', 'outcome_unknown'))
        THEN 'attention'
    WHEN EXISTS (SELECT 1 FROM deliveries d WHERE d.message_id = m.id
        AND d.state IN ('queued', 'sending')) THEN 'pending'
    WHEN EXISTS (SELECT 1 FROM deliveries d WHERE d.message_id = m.id) THEN 'complete'
    ELSE 'none' END";

pub(crate) fn messages(conn: &mut Connection, query: &MessageQuery) -> crate::Result<MessageList> {
    query.check().map_err(crate::Error::Invalid)?;
    let filter = FilterSql::new(&query.filter);
    let transaction = conn.transaction()?;
    let total = transaction.query_row(
        &format!("SELECT count(*) FROM messages m {}", filter.clause()),
        params_from_iter(&filter.values),
        |row| row.get(0),
    )?;
    let mut values = filter.values.clone();
    values.extend([Value::from(query.limit), Value::from(query.offset)]);
    let sql = format!(
        "SELECT m.*, {DELIVERY_SUMMARY} AS delivery_summary FROM messages m {} {} LIMIT ? OFFSET ?",
        filter.clause(),
        order(query.sort)
    );
    let mut rows = transaction
        .prepare(&sql)?
        .query_map(params_from_iter(&values), summary_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    attach_chips(&transaction, &mut rows)?;
    transaction.commit()?;
    Ok(MessageList {
        messages: rows,
        total,
        offset: query.offset,
        limit: query.limit,
    })
}

/// Per-destination delivery totals of the messages `filter` selects, destinations in the
/// order of their first message: the transmission report's totals, complete however many
/// rows the report lists.
pub(crate) fn transmission_totals(
    conn: &mut Connection,
    filter: &MessageFilter,
) -> crate::Result<Vec<DestinationTransmission>> {
    let filter = FilterSql::new(filter);
    // With one min() aggregate, SQLite takes the bare label from the row holding the minimum:
    // the label of the destination's first delivery in the period.
    let sql = format!(
        "SELECT d.destination_id, d.destination_label, count(*),
            sum(d.state = 'acknowledged' AND d.ack_level = 'accepted'),
            sum(d.state = 'acknowledged' AND d.ack_level = 'received'),
            sum(d.state = 'acknowledged' AND d.ack_level IN ('handed_off', 'none')),
            sum(d.state IN ('rejected', 'failed', 'timed_out', 'outcome_unknown')),
            sum(d.state = 'cancelled'),
            sum(d.state IN ('queued', 'sending')),
            min(m.seq)
         FROM deliveries d JOIN messages m ON m.id = d.message_id {}
         GROUP BY d.destination_id ORDER BY min(m.seq), d.destination_id",
        filter.clause()
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(&filter.values), |row| {
        Ok(DestinationTransmission {
            destination_id: parse(row.get(0)?)?,
            label: row.get(1)?,
            deliveries: row.get(2)?,
            accepted: row.get(3)?,
            received: row.get(4)?,
            handed_off: row.get(5)?,
            unresolved: row.get(6)?,
            closed: row.get(7)?,
            pending: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn message(conn: &Connection, id: MessageId) -> crate::Result<MessageRecord> {
    let sql = format!(
        "SELECT m.*, {DELIVERY_SUMMARY} AS delivery_summary FROM messages m WHERE m.id = ?"
    );
    let mut record = conn
        .query_row(&sql, [id.to_string()], message_row)
        .optional()?
        .ok_or(crate::Error::NotFound)?;
    let ids = delivery_ids(conn, id)?;
    for id in ids {
        record.deliveries.push(delivery(conn, id)?);
    }
    record.summary.deliveries = record.deliveries.iter().map(chip_from_record).collect();
    record.revised_by = newest_revision(conn, id)?;
    Ok(record)
}

/// The newest message in the chain of revisions that starts at `id`, if any.
fn newest_revision(conn: &Connection, id: MessageId) -> crate::Result<Option<MessageId>> {
    let newest: Option<String> = conn
        .query_row_cached(
            "WITH RECURSIVE chain(id, received_at, seq) AS (
               SELECT id, received_at, seq FROM messages WHERE revision_of = ?1
               UNION SELECT m.id, m.received_at, m.seq FROM messages m JOIN chain c ON m.revision_of = c.id)
             SELECT id FROM chain ORDER BY received_at DESC, seq DESC LIMIT 1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    newest
        .map(|text| {
            text.parse()
                .map_err(|_| crate::Error::Invalid(format!("bad message id {text}")))
        })
        .transpose()
}

pub(crate) fn delivery(conn: &Connection, id: DeliveryId) -> crate::Result<DeliveryRecord> {
    let mut record = conn
        .query_row_cached(
            "SELECT * FROM deliveries WHERE id = ?",
            [id.to_string()],
            delivery_row,
        )
        .optional()?
        .ok_or(crate::Error::NotFound)?;
    record.attempt_log = conn
        .prepare("SELECT * FROM delivery_attempts WHERE delivery_id = ? ORDER BY attempt_no, id")?
        .query_map([id.to_string()], attempt_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(record)
}

fn summary_row(row: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    Ok(MessageSummary {
        message_id: parse(row.get("id")?)?,
        channel_id: parse(row.get("channel_id")?)?,
        channel_label: row.get("channel_label")?,
        seq: row.get("seq")?,
        direction: parse(row.get("direction")?)?,
        state: parse(row.get("state")?)?,
        origin: parse(row.get("origin")?)?,
        received_at: row.get("received_at")?,
        protocol: parse(row.get("protocol")?)?,
        message_type: row.get("message_type")?,
        control_id: row.get("control_id")?,
        sample_id: row.get("sample_id")?,
        patient_id: row.get("patient_id")?,
        byte_length: row.get("byte_length")?,
        source_peer: row.get("source_peer")?,
        config_version: row.get("config_version")?,
        resent_from: optional_parse(row.get("resent_from")?)?,
        delivery_summary: parse(row.get("delivery_summary")?)?,
        deliveries: Vec::new(),
    })
}

fn message_row(row: &Row<'_>) -> rusqlite::Result<MessageRecord> {
    let source_file: Option<String> = row.get("source_file")?;
    let source_file = source_file
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    row.as_ref().column_index("source_file").unwrap_or(0),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?;
    Ok(MessageRecord {
        summary: summary_row(row)?,
        committed_at: row.get("committed_at")?,
        content_hash: row.get("content_hash")?,
        source_identity: row.get("source_identity")?,
        source_endpoint_id: parse(row.get("source_endpoint_id")?)?,
        source_file,
        response_code: row.get("response_code")?,
        responded_at: row.get("responded_at")?,
        abort_reason: row.get("abort_reason")?,
        reason_code: row.get("reason_code")?,
        duplicate_of: optional_parse(row.get("duplicate_of")?)?,
        revision_of: optional_parse(row.get("revision_of")?)?,
        revised_by: None,
        deliveries: Vec::new(),
    })
}

fn delivery_row(row: &Row<'_>) -> rusqlite::Result<DeliveryRecord> {
    Ok(DeliveryRecord {
        delivery_id: parse(row.get("id")?)?,
        message_id: parse(row.get("message_id")?)?,
        channel_id: parse(row.get("channel_id")?)?,
        destination_id: parse(row.get("destination_id")?)?,
        destination_label: row.get("destination_label")?,
        state: parse(row.get("state")?)?,
        ack_level: parse(row.get("ack_level")?)?,
        attempts: row.get("attempts")?,
        total_attempts: row.get("total_attempts")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        next_attempt_at: row.get("next_attempt_at")?,
        reason_code: row.get("reason_code")?,
        closed_reason: row.get("closed_reason")?,
        resend_reason: row.get("resend_reason")?,
        last_error: row.get("last_error")?,
        mapping_version: row.get("mapping_version")?,
        transformed_hash: row.get("transformed_hash")?,
        attempt_log: Vec::new(),
    })
}

fn attempt_row(row: &Row<'_>) -> rusqlite::Result<AttemptRecord> {
    Ok(AttemptRecord {
        attempt_no: row.get("attempt_no")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        result: optional_parse(row.get("result")?)?,
        ack_code: row.get("ack_code")?,
        ack_control_id: row.get("ack_control_id")?,
        error_code: row.get("error_code")?,
        ack_level: parse(row.get("ack_level")?)?,
    })
}

fn attach_chips(conn: &Connection, rows: &mut [MessageSummary]) -> rusqlite::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let ids: Vec<_> = rows.iter().map(|row| row.message_id.to_string()).collect();
    let sql = format!(
        "SELECT message_id, id, destination_id, destination_label, state, ack_level
         FROM deliveries WHERE message_id IN ({}) ORDER BY rowid",
        placeholders(ids.len())
    );
    let mut positions = std::collections::HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        positions.insert(row.message_id, index);
    }
    let mut statement = conn.prepare(&sql)?;
    let chips = statement.query_map(params_from_iter(&ids), |row| {
        Ok((parse::<MessageId>(row.get(0)?)?, chip_row(row)?))
    })?;
    for chip in chips {
        let (id, chip) = chip?;
        if let Some(index) = positions.get(&id) {
            rows[*index].deliveries.push(chip);
        }
    }
    Ok(())
}

fn chip_row(row: &Row<'_>) -> rusqlite::Result<DeliveryChip> {
    Ok(DeliveryChip {
        delivery_id: parse(row.get("id")?)?,
        destination_id: parse(row.get("destination_id")?)?,
        destination_label: row.get("destination_label")?,
        state: parse(row.get("state")?)?,
        ack_level: parse(row.get("ack_level")?)?,
    })
}

fn chip_from_record(record: &DeliveryRecord) -> DeliveryChip {
    DeliveryChip {
        delivery_id: record.delivery_id,
        destination_id: record.destination_id,
        destination_label: record.destination_label.clone(),
        state: record.state,
        ack_level: record.ack_level,
    }
}

fn delivery_ids(conn: &Connection, id: MessageId) -> rusqlite::Result<Vec<DeliveryId>> {
    conn.prepare("SELECT id FROM deliveries WHERE message_id = ? ORDER BY rowid")?
        .query_map([id.to_string()], |row| parse(row.get(0)?))?
        .collect()
}

pub(crate) fn parse<T: FromStr>(value: String) -> rusqlite::Result<T> {
    value.parse().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid stored {}", std::any::type_name::<T>()),
            )),
        )
    })
}

fn optional_parse<T: FromStr>(value: Option<String>) -> rusqlite::Result<Option<T>> {
    value.map(parse).transpose()
}

fn order(sort: MessageSort) -> String {
    let column = match sort.field {
        MessageSortField::ReceivedAt => "m.received_at",
        MessageSortField::Channel => "m.channel_label",
        MessageSortField::State => "m.state",
        MessageSortField::MessageType => "m.message_type",
        MessageSortField::ByteLength => "m.byte_length",
    };
    let direction = if sort.descending { "DESC" } else { "ASC" };
    format!("ORDER BY {column} {direction}, m.seq {direction}, m.id {direction}")
}

#[derive(Default)]
struct FilterSql {
    predicates: Vec<String>,
    values: Vec<Value>,
}

impl FilterSql {
    fn new(filter: &MessageFilter) -> Self {
        let mut sql = Self::default();
        if let Some(since) = filter.since {
            sql.predicate("m.received_at >= ?", since.to_string());
        }
        if let Some(until) = filter.until {
            sql.predicate("m.received_at < ?", until.to_string());
        }
        sql.any("m.channel_id", &filter.channels);
        sql.any("m.direction", &filter.directions);
        sql.any("m.state", &filter.states);
        if !filter.delivery_states.is_empty() {
            sql.predicates.push(format!(
                "EXISTS (SELECT 1 FROM deliveries d WHERE d.message_id = m.id AND d.state IN ({}))",
                placeholders(filter.delivery_states.len())
            ));
            sql.values.extend(
                filter
                    .delivery_states
                    .iter()
                    .map(|s| Value::from(s.to_string())),
            );
        }
        sql.any(DELIVERY_SUMMARY, &filter.delivery_summaries);
        sql.search(filter);
        if let Some(prefix) = &filter.message_id_prefix {
            // The stored form (`packie_ipc::messages::id_prefix`): lowercase hex and dashes, so
            // every id with this prefix sorts in [prefix, prefix + '~'), an index range on
            // the primary key (a LIKE on a binary-collated column cannot use it).
            let start = packie_ipc::messages::id_prefix(prefix).unwrap_or_else(|_| prefix.clone());
            let end = format!("{start}~");
            sql.predicate("m.id >= ?", start);
            sql.predicate("m.id < ?", end);
        }
        sql
    }

    fn search(&mut self, filter: &MessageFilter) {
        if let Some(identifier) = &filter.identifier {
            let (operator, value) = if identifier.prefix {
                (
                    "LIKE ? ESCAPE '\\'",
                    format!("{}%", escape_like(&identifier.value)),
                )
            } else {
                ("= ?", identifier.value.clone())
            };
            self.match_columns(&["control_id", "sample_id", "patient_id"], operator, &value);
        }
        if let Some(text) = &filter.text {
            self.match_columns(
                &[
                    "control_id",
                    "sample_id",
                    "patient_id",
                    "message_type",
                    "searchable_text",
                ],
                "LIKE ? ESCAPE '\\'",
                &format!("%{}%", escape_like(text)),
            );
        }
    }

    fn match_columns(&mut self, columns: &[&str], operator: &str, value: &str) {
        let predicate = columns
            .iter()
            .map(|c| format!("m.{c} {operator}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        self.predicates.push(format!("({predicate})"));
        self.values
            .extend(columns.iter().map(|_| Value::from(value.to_owned())));
    }

    fn predicate(&mut self, expression: &str, value: String) {
        self.predicates.push(expression.to_owned());
        self.values.push(Value::from(value));
    }

    fn any<T: Display>(&mut self, column: &str, values: &[T]) {
        if !values.is_empty() {
            self.predicates
                .push(format!("({column}) IN ({})", placeholders(values.len())));
            self.values
                .extend(values.iter().map(|s| Value::from(s.to_string())));
        }
    }

    fn clause(&self) -> String {
        if self.predicates.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.predicates.join(" AND "))
        }
    }
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use packie_core::{ChannelId, ConnectorId, DeliveryState, DeliverySummary, EndpointId};
    use packie_ipc::messages::IdentifierFilter;

    fn seeded() -> (Connection, Vec<MessageId>) {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        let channel = ChannelId::generate();
        let endpoint = EndpointId::generate();
        let mut ids = Vec::new();
        for seq in 1..=4 {
            let id = MessageId::generate();
            conn.execute(
                "INSERT INTO messages (id, channel_id, channel_label, seq, received_at,
                 direction, origin, protocol, state, byte_length, content_hash,
                 source_endpoint_id, config_version, control_id, searchable_text)
                 VALUES (?, ?, 'Channel', ?, ?, 'inbound', 'received', 'hl7_mllp', 'received',
                 12, 'hash', ?, 1, ?, ?)",
                rusqlite::params![
                    id.to_string(),
                    channel.to_string(),
                    seq,
                    format!("2026-11-06T12:00:0{seq}.000000000Z"),
                    endpoint.to_string(),
                    format!("control_{seq}%"),
                    format!("searchable text {seq}")
                ],
            )
            .unwrap();
            ids.push(id);
        }
        (conn, ids)
    }

    fn insert_delivery(conn: &Connection, message: MessageId, state: DeliveryState) -> DeliveryId {
        let id = DeliveryId::generate();
        conn.execute(
            "INSERT INTO deliveries (id, message_id, channel_id, destination_id,
             destination_label, state, ack_level, attempts, total_attempts, created_at,
             updated_at, max_attempts, retry_unacknowledged, ack_required)
             SELECT ?, id, channel_id, ?, 'Destination', ?, 'none', 0, 0,
             received_at, received_at, 3, 0, 'accepted' FROM messages WHERE id = ?",
            rusqlite::params![
                id.to_string(),
                ConnectorId::generate().to_string(),
                state.as_str(),
                message.to_string()
            ],
        )
        .unwrap();
        id
    }

    #[test]
    fn transmission_totals_count_every_delivery_of_the_period_per_destination() {
        let (mut conn, ids) = seeded();
        let states = [
            DeliveryState::Acknowledged,
            DeliveryState::TimedOut,
            DeliveryState::Queued,
            DeliveryState::Cancelled,
        ];
        for (id, state) in ids.iter().zip(states) {
            let delivery = insert_delivery(&conn, *id, state);
            let destination = if state == DeliveryState::Cancelled {
                "archive"
            } else {
                "lis"
            };
            conn.execute(
                "UPDATE deliveries SET destination_id = ?, ack_level = 'accepted' WHERE id = ?",
                rusqlite::params![destination, delivery.to_string()],
            )
            .unwrap();
        }
        let all = transmission_totals(&mut conn, &MessageFilter::default()).unwrap();
        let lis = &all[0];
        assert_eq!(
            (lis.destination_id.as_str(), lis.deliveries, lis.accepted),
            ("lis", 3, 1)
        );
        assert_eq!((lis.unresolved, lis.pending, lis.closed), (1, 1, 0));
        assert_eq!(
            (all[1].destination_id.as_str(), all[1].closed),
            ("archive", 1)
        );

        let later = MessageFilter {
            since: Some("2026-11-06T12:00:02Z".parse().unwrap()),
            ..MessageFilter::default()
        };
        let totals = transmission_totals(&mut conn, &later).unwrap();
        assert_eq!((totals[0].deliveries, totals[0].accepted), (2, 0));
    }

    #[test]
    fn pages_and_counts_and_orders_stable_ties() {
        let (mut conn, ids) = seeded();
        conn.execute("UPDATE messages SET byte_length = 10", [])
            .unwrap();
        let query = MessageQuery {
            sort: MessageSort {
                field: MessageSortField::ByteLength,
                descending: false,
            },
            offset: 1,
            limit: 2,
            ..MessageQuery::default()
        };
        let page = messages(&mut conn, &query).unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(page.offset, 1);
        assert_eq!(page.limit, 2);
        assert_eq!(
            page.messages
                .iter()
                .map(|m| m.message_id)
                .collect::<Vec<_>>(),
            ids[1..3]
        );
    }

    #[test]
    fn delivery_rollups_prioritize_attention_and_filter_before_paging() {
        let (mut conn, ids) = seeded();
        insert_delivery(&conn, ids[0], DeliveryState::Queued);
        insert_delivery(&conn, ids[0], DeliveryState::Rejected);
        insert_delivery(&conn, ids[1], DeliveryState::Sending);
        insert_delivery(&conn, ids[2], DeliveryState::Acknowledged);
        insert_delivery(&conn, ids[2], DeliveryState::Cancelled);
        let rows = messages(&mut conn, &MessageQuery::default())
            .unwrap()
            .messages;
        assert_eq!(
            rows.iter().map(|m| m.delivery_summary).collect::<Vec<_>>(),
            [
                DeliverySummary::None,
                DeliverySummary::Complete,
                DeliverySummary::Pending,
                DeliverySummary::Attention
            ]
        );
        assert_eq!(rows[3].deliveries.len(), 2);
        let mut query = MessageQuery::default();
        query.filter.delivery_summaries = vec![DeliverySummary::Attention, DeliverySummary::None];
        query.filter.delivery_states = vec![DeliveryState::Rejected, DeliveryState::Failed];
        assert_eq!(messages(&mut conn, &query).unwrap().total, 1);
        assert_eq!(
            messages(&mut conn, &query).unwrap().messages[0].message_id,
            ids[0]
        );
    }

    #[test]
    fn literal_identifier_prefix_and_safe_text_are_combined() {
        let (mut conn, ids) = seeded();
        conn.execute(
            "UPDATE messages SET control_id = 'controlX1X' WHERE id = ?",
            [ids[1].to_string()],
        )
        .unwrap();
        let mut query = MessageQuery::default();
        query.filter.identifier = Some(IdentifierFilter {
            value: "control_1%".into(),
            prefix: true,
        });
        query.filter.text = Some("text 1".into());
        assert_eq!(messages(&mut conn, &query).unwrap().total, 1);
        query.filter.text = Some("text 2".into());
        assert_eq!(messages(&mut conn, &query).unwrap().total, 0);
        query.filter.identifier = None;
        query.filter.text = Some("control_1%".into());
        assert_eq!(
            messages(&mut conn, &query).unwrap().messages[0].message_id,
            ids[0]
        );
        query.filter.text = Some("hash".into());
        assert_eq!(messages(&mut conn, &query).unwrap().total, 0);
    }

    #[test]
    fn time_bounds_and_list_filters_match_all_criteria() {
        let (mut conn, ids) = seeded();
        conn.execute(
            "UPDATE messages SET sample_id = 'specimen' WHERE id = ?",
            [ids[1].to_string()],
        )
        .unwrap();
        conn.execute(
            "UPDATE messages SET state = 'invalid' WHERE id = ?",
            [ids[2].to_string()],
        )
        .unwrap();
        let row = messages(&mut conn, &MessageQuery::default())
            .unwrap()
            .messages
            .remove(0);
        let mut query = MessageQuery::default();
        query.filter.since = Some("2026-11-06T12:00:02Z".parse().unwrap());
        query.filter.until = Some("2026-11-06T12:00:04Z".parse().unwrap());
        query.filter.channels = vec![row.channel_id];
        query.filter.directions = vec![
            packie_core::MessageDirection::Inbound,
            packie_core::MessageDirection::AToB,
        ];
        query.filter.states = vec![
            packie_core::MessageState::Invalid,
            packie_core::MessageState::Received,
        ];
        assert_eq!(messages(&mut conn, &query).unwrap().total, 2);
        query.filter.identifier = Some(IdentifierFilter {
            value: "specimen".into(),
            prefix: false,
        });
        assert_eq!(
            messages(&mut conn, &query).unwrap().messages[0].message_id,
            ids[1]
        );
        query.filter.states = vec![packie_core::MessageState::Invalid];
        assert_eq!(messages(&mut conn, &query).unwrap().total, 0);
    }

    #[test]
    fn details_read_attempts_in_order_and_keep_grid_independent() {
        let (mut conn, ids) = seeded();
        let id = insert_delivery(&conn, ids[0], DeliveryState::Rejected);
        for number in [2, 1] {
            conn.execute(
                "INSERT INTO delivery_attempts (id, delivery_id, attempt_no, started_at, result,
                 ack_code, ack_level) VALUES (?, ?, ?, '2026-11-06T12:00:00.000000000Z',
                 'rejected', 'AR', 'received')",
                rusqlite::params![DeliveryId::generate().to_string(), id.to_string(), number],
            )
            .unwrap();
        }
        let record = message(&conn, ids[0]).unwrap();
        assert_eq!(
            record.deliveries[0]
                .attempt_log
                .iter()
                .map(|a| a.attempt_no)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(record.summary.deliveries[0].delivery_id, id);
        conn.execute("UPDATE delivery_attempts SET result = 'invalid enum'", [])
            .unwrap();
        assert!(messages(&mut conn, &MessageQuery::default()).is_ok());
        assert!(delivery(&conn, id).is_err());
        assert!(matches!(
            message(&conn, MessageId::generate()),
            Err(crate::Error::NotFound)
        ));
        assert!(matches!(
            delivery(&conn, DeliveryId::generate()),
            Err(crate::Error::NotFound)
        ));
    }
}
