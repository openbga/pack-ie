//! `packie config diff <a> <b>`: what changes from document `a` to document `b`, as the
//! console's version comparison shows it: endpoints and channels added, removed or changed
//! (matched by id), and each changed field as `field: old → new`. Either side is a saved
//! version, `active`, or a runtime document file.

use std::path::Path;

use packie_config::RuntimeConfig;
use packie_ipc::{Request, Response};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::Failure;
use crate::client::{Target, ask, unexpected};
use crate::output::{print_json, print_line};

/// Top-level members that describe a version rather than configure anything.
const METADATA: [&str; 3] = ["schema_version", "config_version", "generated_at"];
/// Members that identify an array element, in order of preference.
const IDS: [&str; 4] = ["endpoint_id", "channel_id", "destination_id", "id"];

/// One differing field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FieldChange {
    /// `Settings › Baud rate`.
    pub(crate) field: String,
    /// The value in `a`, in words.
    pub(crate) before: String,
    /// The value in `b`, in words.
    pub(crate) after: String,
}

/// An endpoint or channel that differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ItemChange {
    pub(crate) id: String,
    pub(crate) label: String,
    /// `added`, `removed` or `changed`.
    pub(crate) change: &'static str,
    /// What differs, for a changed item.
    pub(crate) fields: Vec<FieldChange>,
}

/// Everything that differs between two documents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Comparison {
    pub(crate) endpoints: Vec<ItemChange>,
    pub(crate) channels: Vec<ItemChange>,
    /// Sections other than endpoints and channels.
    pub(crate) other: Vec<FieldChange>,
}

impl Comparison {
    fn is_empty(&self) -> bool {
        self.endpoints.is_empty() && self.channels.is_empty() && self.other.is_empty()
    }
}

pub(crate) async fn config_diff(
    target: &Target,
    a: &str,
    b: &str,
    json: bool,
) -> Result<(), Failure> {
    let before = load(target, a).await?;
    let after = load(target, b).await?;
    let comparison = compare(&before, &after);
    if json {
        return print_json(&comparison);
    }
    if comparison.is_empty() {
        print_line(&format!("{a} and {b} configure the same."));
        return Ok(());
    }
    for line in describe(&comparison) {
        print_line(&line);
    }
    Ok(())
}

/// A saved version (`7`), the active document (`active`) or a file, as JSON with every
/// default filled in so that writing a default out is not a change.
async fn load(target: &Target, side: &str) -> Result<Value, Failure> {
    let document = if side == "active" {
        ask_document(target, Request::GetConfig).await?
    } else if let Ok(config_version) = side.parse::<u64>() {
        ask_document(target, Request::GetConfigVersion { config_version }).await?
    } else {
        let path = Path::new(side);
        let text = std::fs::read_to_string(path)
            .map_err(|e| Failure::invalid(format!("cannot read {side}: {e}")))?;
        RuntimeConfig::from_value(
            serde_json::from_str(&text)
                .map_err(|e| Failure::invalid(format!("{side} is not JSON: {e}")))?,
        )
        .map_err(|e| Failure::invalid(format!("{side} is not a valid runtime document:\n{e}")))?
    };
    serde_json::to_value(&document).map_err(|e| Failure::new(e.to_string()))
}

async fn ask_document(target: &Target, request: Request) -> Result<RuntimeConfig, Failure> {
    let client = target.connect().await?;
    match ask(&client, request).await? {
        Response::Config { document } => Ok(*document),
        _ => Err(unexpected()),
    }
}

/// What changes from `before` to `after`.
pub(crate) fn compare(before: &Value, after: &Value) -> Comparison {
    let empty = Map::new();
    let before = before.as_object().unwrap_or(&empty);
    let after = after.as_object().unwrap_or(&empty);
    let mut comparison = Comparison {
        endpoints: items(before.get("endpoints"), after.get("endpoints")),
        channels: items(before.get("channels"), after.get("channels")),
        other: Vec::new(),
    };
    let keys = union(before, after);
    for key in keys {
        if key == "endpoints" || key == "channels" || METADATA.contains(&key) {
            continue;
        }
        diff(
            &humanize(key),
            before.get(key),
            after.get(key),
            &mut comparison.other,
        );
    }
    comparison
}

/// Endpoints or channels, matched by id: those of `after` in order, then the removed ones.
fn items(before: Option<&Value>, after: Option<&Value>) -> Vec<ItemChange> {
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let (before, after) = (list(before), list(after));
    let mut changes = Vec::new();
    for item in &after {
        let old = before.iter().find(|o| identity(o) == identity(item));
        let mut fields = Vec::new();
        match old {
            None => changes.push(item_change(item, "added", fields)),
            Some(old) => {
                diff("", Some(old), Some(item), &mut fields);
                if !fields.is_empty() {
                    changes.push(item_change(item, "changed", fields));
                }
            }
        }
    }
    for old in &before {
        if !after.iter().any(|item| identity(item) == identity(old)) {
            changes.push(item_change(old, "removed", Vec::new()));
        }
    }
    changes
}

fn item_change(item: &Value, change: &'static str, fields: Vec<FieldChange>) -> ItemChange {
    ItemChange {
        id: identity(item).unwrap_or_default(),
        label: item
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        change,
        fields,
    }
}

/// The id member of an array element, if it has one.
fn identity(value: &Value) -> Option<String> {
    IDS.iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .map(str::to_owned)
}

/// Appends the differences between two values at `field`.
fn diff(field: &str, before: Option<&Value>, after: Option<&Value>, out: &mut Vec<FieldChange>) {
    match (before, after) {
        (Some(b), Some(a)) if b == a => {}
        (Some(Value::Object(b)), Some(Value::Object(a))) => {
            for key in union(b, a) {
                diff(&join(field, &humanize(key)), b.get(key), a.get(key), out);
            }
        }
        (Some(Value::Array(b)), Some(Value::Array(a)))
            if b.iter().chain(a).any(Value::is_object) =>
        {
            members(field, b, a, out);
        }
        _ => out.push(FieldChange {
            field: field.to_owned(),
            before: before.map_or_else(|| "not set".to_owned(), show),
            after: after.map_or_else(|| "not set".to_owned(), show),
        }),
    }
}

/// The elements of two arrays of records: by id when every element has one, else by
/// position.
fn members(field: &str, before: &[Value], after: &[Value], out: &mut Vec<FieldChange>) {
    if before.iter().chain(after).all(|v| identity(v).is_some()) {
        for item in after {
            let id = identity(item).unwrap_or_default();
            let old = before.iter().find(|o| identity(o).as_deref() == Some(&id));
            diff(&join(field, &id), old, Some(item), out);
        }
        for old in before {
            let id = identity(old).unwrap_or_default();
            if !after.iter().any(|a| identity(a).as_deref() == Some(&id)) {
                diff(&join(field, &id), Some(old), None, out);
            }
        }
        return;
    }
    for index in 0..before.len().max(after.len()) {
        let at = join(field, &format!("#{}", index + 1));
        diff(&at, before.get(index), after.get(index), out);
    }
}

fn union<'a>(a: &'a Map<String, Value>, b: &'a Map<String, Value>) -> Vec<&'a str> {
    let mut keys: Vec<&str> = a.keys().map(String::as_str).collect();
    for key in b.keys() {
        if !keys.contains(&key.as_str()) {
            keys.push(key);
        }
    }
    keys
}

fn join(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent} › {child}")
    }
}

/// `baud_rate` → `Baud rate`.
fn humanize(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A value in words: `yes`/`no`, `none`, `empty`, lists joined, records as `name: value`.
fn show(value: &Value) -> String {
    match value {
        Value::Null => "none".to_owned(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.to_owned(),
        Value::String(s) if s.is_empty() => "empty".to_owned(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Array(items) if items.is_empty() => "none".to_owned(),
        Value::Array(items) => items.iter().map(show).collect::<Vec<_>>().join(", "),
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| format!("{k}: {}", show(v)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The comparison as lines: one heading per section, one line per item and field.
fn describe(comparison: &Comparison) -> Vec<String> {
    let mut lines = Vec::new();
    for (title, items) in [
        ("Endpoints", &comparison.endpoints),
        ("Channels", &comparison.channels),
    ] {
        if items.is_empty() {
            continue;
        }
        lines.push(title.to_owned());
        for item in items {
            lines.push(format!("  {:<8} {} ({})", item.change, item.id, item.label));
            lines.extend(item.fields.iter().map(|f| format!("    {}", field_line(f))));
        }
    }
    if !comparison.other.is_empty() {
        lines.push("Other settings".to_owned());
        lines.extend(
            comparison
                .other
                .iter()
                .map(|f| format!("  {}", field_line(f))),
        );
    }
    lines
}

fn field_line(change: &FieldChange) -> String {
    format!("{}: {} → {}", change.field, change.before, change.after)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document(baud: u32, channel: &str) -> Value {
        json!({
            "schema_version": 1, "config_version": 3,
            "endpoints": [{ "endpoint_id": "analyzer", "label": "Analyzer", "kind": "serial",
                "settings": { "port_name": "COM3", "baud_rate": baud, "parity": "none" } }],
            "channels": [{ "channel_id": channel, "label": "Results", "kind": "message",
                "destinations": [{ "destination_id": "d1", "endpoint": "lis" }] }],
            "alerts": { "disk_low_megabytes": 500 },
        })
    }

    #[test]
    fn items_are_matched_by_id_and_fields_shown_as_old_and_new() {
        let mut after = document(19_200, "results-2");
        after["alerts"]["disk_low_megabytes"] = json!(1000);
        let comparison = compare(&document(9600, "results"), &after);
        assert_eq!(comparison.endpoints.len(), 1);
        assert_eq!(comparison.endpoints[0].change, "changed");
        assert_eq!(
            comparison.endpoints[0].fields,
            [FieldChange {
                field: "Settings › Baud rate".into(),
                before: "9600".into(),
                after: "19200".into(),
            }]
        );
        let channels: Vec<(&str, &str)> = comparison
            .channels
            .iter()
            .map(|c| (c.id.as_str(), c.change))
            .collect();
        assert_eq!(channels, [("results-2", "added"), ("results", "removed")]);
        assert_eq!(
            field_line(&comparison.other[0]),
            "Alerts › Disk low megabytes: 500 → 1000"
        );
    }

    #[test]
    fn array_members_with_ids_are_compared_by_id_and_metadata_is_ignored() {
        let before = document(9600, "results");
        let mut after = before.clone();
        after["config_version"] = json!(4);
        after["channels"][0]["destinations"][0]["endpoint"] = json!("lis-2");
        after["channels"][0]["destinations"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "destination_id": "d2", "endpoint": "archive" }));
        let comparison = compare(&before, &after);
        assert!(comparison.other.is_empty());
        let lines: Vec<String> = comparison.channels[0]
            .fields
            .iter()
            .map(field_line)
            .collect();
        assert_eq!(
            lines,
            [
                "Destinations › d1 › Endpoint: lis → lis-2",
                "Destinations › d2: not set → destination_id: d2, endpoint: archive",
            ]
        );
        assert!(compare(&before, &before).is_empty());
    }
}
