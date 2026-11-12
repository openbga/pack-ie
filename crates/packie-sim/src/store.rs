//! `assert_store`: rows of the message store, as the engine reports them.
//!
//! The specification reads the store through a read-only SQLite connection, but the store
//! crate (and so its schema) does not exist yet, while the engine's management interface
//! already serves every field the scenarios assert on (`packie_ipc::messages`). Rows are
//! therefore built from `get_messages`, `get_message` and `get_message_body`, so the
//! engine must be running. Switching to SQLite later changes only [`load_rows`].
//!
//! A row is a flat JSON object. `messages`: the message record (its `summary` fields
//! lifted to the top) plus `original_text` and `original_hex`. `deliveries`: the delivery
//! record, its parent message as `message.<field>`, plus `transformed_text` and
//! `transformed_hex`. `attempts`: the attempt record with `delivery.<field>` and
//! `message.<field>`. `alerts`: the active alerts of the snapshot. Bodies are fetched only
//! when an expectation names them.

use std::collections::BTreeMap;

use packie_core::{DeliveryId, MessageId};
use packie_ipc::messages::{BodyFormat, BodyRequest, BodySelector, MAX_PAGE, MessageQuery};
use packie_ipc::{Request, Response};
use serde_json::{Map, Value};

use crate::engine::EngineHost;
use crate::matcher::Matcher;
use crate::snapshot_view::alerts_view;
use crate::steps::Table;

/// One store row.
pub type Row = Map<String, Value>;

fn object(value: Value) -> Row {
    match value {
        Value::Object(map) => map,
        _ => Row::new(),
    }
}

fn attach(row: &mut Row, prefix: &str, parent: &Row) {
    for (key, value) in parent {
        row.insert(format!("{prefix}.{key}"), value.clone());
    }
}

/// Whether any expectation names a body field (`original_*`, `transformed_*`).
#[must_use]
pub fn needs_bodies<'a>(mut keys: impl Iterator<Item = &'a String>) -> bool {
    keys.any(|k| k.contains("original_") || k.contains("transformed_"))
}

async fn body(
    engine: &EngineHost,
    message: MessageId,
    which: BodySelector,
    format: BodyFormat,
) -> Result<String, String> {
    let request = Request::GetMessageBody(BodyRequest {
        message_id: message,
        which,
        format,
    });
    match engine.call(request).await.map_err(|e| e.to_string())? {
        Response::MessageBody(b) => Ok(b.content),
        _ => Err("unexpected response to get_message_body".into()),
    }
}

/// A message record as a flat row (`summary` lifted, `deliveries` removed).
fn message_row(record: &Value) -> (Row, Vec<Value>) {
    let mut row = object(record.clone());
    let summary = row.remove("summary").map(object).unwrap_or_default();
    let deliveries = match row.remove("deliveries") {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    row.extend(summary);
    (row, deliveries)
}

/// Reads the rows of `table` from the running engine.
pub async fn load_rows(
    engine: &EngineHost,
    table: Table,
    with_bodies: bool,
) -> Result<Vec<Row>, String> {
    if table == Table::Alerts {
        let snapshot = engine.snapshot().await?;
        return Ok(alerts_view(&snapshot).into_iter().map(object).collect());
    }
    let mut rows = Vec::new();
    let mut offset = 0;
    loop {
        let query = MessageQuery {
            limit: MAX_PAGE,
            offset,
            ..MessageQuery::default()
        };
        let Response::Messages(list) = engine
            .call(Request::GetMessages(Box::new(query)))
            .await
            .map_err(|e| e.to_string())?
        else {
            return Err("unexpected response to get_messages".into());
        };
        let page = serde_json::to_value(&list).map_err(|e| e.to_string())?;
        let summaries = page["messages"].as_array().cloned().unwrap_or_default();
        for summary in &summaries {
            collect_message(engine, table, summary, with_bodies, &mut rows).await?;
        }
        offset += u32::try_from(summaries.len()).unwrap_or(u32::MAX);
        if summaries.is_empty() || u64::from(offset) >= page["total"].as_u64().unwrap_or(0) {
            return Ok(rows);
        }
    }
}

async fn collect_message(
    engine: &EngineHost,
    table: Table,
    summary: &Value,
    with_bodies: bool,
    rows: &mut Vec<Row>,
) -> Result<(), String> {
    let id_text = summary["message_id"].as_str().unwrap_or_default();
    let message_id: MessageId = id_text
        .parse()
        .map_err(|_| format!("message id `{id_text}` is not an id this client can send"))?;
    let Response::Message { message } = engine
        .call(Request::GetMessage { message_id })
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("unexpected response to get_message".into());
    };
    let record = serde_json::to_value(&*message).map_err(|e| e.to_string())?;
    let (mut message_fields, deliveries) = message_row(&record);
    if with_bodies {
        message_fields.insert(
            "original_text".into(),
            Value::String(
                body(engine, message_id, BodySelector::Original, BodyFormat::Text).await?,
            ),
        );
        message_fields.insert(
            "original_hex".into(),
            Value::String(body(engine, message_id, BodySelector::Original, BodyFormat::Hex).await?),
        );
    }
    if table == Table::Messages {
        rows.push(message_fields);
        return Ok(());
    }
    for delivery in deliveries {
        let mut delivery_row = object(delivery);
        if with_bodies {
            let delivery_id: DeliveryId = delivery_row
                .get("delivery_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .parse()
                .map_err(|_| "a delivery id cannot be sent".to_owned())?;
            let which = BodySelector::Transformed { delivery_id };
            delivery_row.insert(
                "transformed_text".into(),
                Value::String(
                    body(engine, message_id, which, BodyFormat::Text)
                        .await
                        .unwrap_or_default(),
                ),
            );
            delivery_row.insert(
                "transformed_hex".into(),
                Value::String(
                    body(engine, message_id, which, BodyFormat::Hex)
                        .await
                        .unwrap_or_default(),
                ),
            );
        }
        if table == Table::Attempts {
            let attempts = match delivery_row.remove("attempt_log") {
                Some(Value::Array(items)) => items,
                _ => Vec::new(),
            };
            for attempt in attempts {
                let mut attempt_row = object(attempt);
                attach(&mut attempt_row, "delivery", &delivery_row);
                attach(&mut attempt_row, "message", &message_fields);
                rows.push(attempt_row);
            }
        } else {
            delivery_row.remove("attempt_log");
            attach(&mut delivery_row, "message", &message_fields);
            rows.push(delivery_row);
        }
    }
    Ok(())
}

/// The mismatches of `row` against `matchers`, one line each.
#[must_use]
pub fn row_mismatches(row: &Row, matchers: &BTreeMap<String, Matcher>) -> Vec<String> {
    matchers
        .iter()
        .filter_map(|(field, matcher)| {
            matcher
                .check(row.get(field))
                .err()
                .map(|why| format!("{field}: {why}"))
        })
        .collect()
}

/// Selects rows by `where_`, then checks the count and `each`. `Err` describes the
/// difference between expectation and observation.
pub fn evaluate(
    rows: &[Row],
    where_: &BTreeMap<String, Matcher>,
    count: &Matcher,
    each: &BTreeMap<String, Matcher>,
) -> Result<(), String> {
    let selected: Vec<&Row> = rows
        .iter()
        .filter(|r| row_mismatches(r, where_).is_empty())
        .collect();
    let observed = Value::from(selected.len());
    if let Err(why) = count.check(Some(&observed)) {
        let mut text = format!("row count: {why} (the table has {} rows", rows.len());
        if selected.is_empty() {
            text.push_str(&near_misses(rows, where_));
        }
        text.push(')');
        return Err(text);
    }
    for row in &selected {
        let bad = row_mismatches(row, each);
        if !bad.is_empty() {
            return Err(format!(
                "a selected row does not satisfy `each`: {}\n    row: {}",
                bad.join("; "),
                brief(row)
            ));
        }
    }
    Ok(())
}

/// For an empty selection: the row that matches the most `where` fields.
fn near_misses(rows: &[Row], where_: &BTreeMap<String, Matcher>) -> String {
    let best = rows
        .iter()
        .map(|r| (row_mismatches(r, where_), r))
        .min_by_key(|(bad, _)| bad.len());
    match best {
        Some((bad, row)) => format!(
            "; closest row differs in {}: {}",
            bad.join("; "),
            brief(row)
        ),
        None => String::new(),
    }
}

fn brief(row: &Row) -> String {
    let text = Value::Object(row.clone()).to_string();
    if text.chars().count() > 400 {
        format!("{}…", text.chars().take(400).collect::<String>())
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn rows() -> Vec<Row> {
        vec![
            json!({"id": "1", "channel_id": "c", "state": "routed"}),
            json!({"id": "2", "channel_id": "c", "state": "aborted"}),
        ]
        .into_iter()
        .filter_map(|v| v.as_object().cloned())
        .collect()
    }

    fn m(pairs: &[(&str, Value)]) -> BTreeMap<String, Matcher> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Matcher::from_value(v).unwrap()))
            .collect()
    }

    #[test]
    fn counts_selected_rows() {
        let one = Matcher::from_value(&json!(1)).unwrap();
        assert!(evaluate(&rows(), &m(&[("state", json!("routed"))]), &one, &m(&[])).is_ok());
        let err =
            evaluate(&rows(), &m(&[("state", json!("receiving"))]), &one, &m(&[])).unwrap_err();
        assert!(err.contains("row count"), "{err}");
        assert!(err.contains("closest row"), "{err}");
    }

    #[test]
    fn each_must_hold_for_every_selected_row() {
        let any = Matcher::from_value(&json!({"gte": 1})).unwrap();
        let err = evaluate(
            &rows(),
            &m(&[("channel_id", json!("c"))]),
            &any,
            &m(&[("state", json!("routed"))]),
        )
        .unwrap_err();
        assert!(err.contains("each"), "{err}");
    }

    #[test]
    fn message_records_are_flattened() {
        let record = json!({"summary": {"message_id": "m", "state": "routed"}, "response_code": "AA", "deliveries": [{"delivery_id": "d"}]});
        let (row, deliveries) = message_row(&record);
        assert_eq!(row["state"], "routed");
        assert_eq!(row["response_code"], "AA");
        assert_eq!(deliveries.len(), 1);
        assert!(!row.contains_key("summary"));
    }

    #[test]
    fn body_fields_are_detected() {
        let keys = ["state".to_owned(), "message.original_text".to_owned()];
        assert!(needs_bodies(keys.iter()));
        assert!(!needs_bodies(keys[..1].iter()));
    }
}
