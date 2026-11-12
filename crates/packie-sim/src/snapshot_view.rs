//! The scenario view of an engine snapshot.
//!
//! Scenarios address snapshots with the field names of `docs/spec/diagnostics.md`
//! (`session_state`, `endpoint_a.state`, `counters.bytes_a_to_b`, ...). The engine's
//! `EngineSnapshot` (packie-diagnostics) nests the same facts differently (a bridge's
//! counters live in its two flows, an endpoint reference is a struct), so this module is
//! the single place that translates one into the other. Alerts and the node snapshot
//! already carry the specified names and pass through. It works on the snapshot's JSON, so
//! a renamed or missing DTO field surfaces as an absent path in an assertion failure, not
//! as a compile error here.

use serde_json::{Map, Value, json};

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn text_of(value: &Value) -> Option<&str> {
    value.as_str()
}

fn find<'a>(items: &'a Value, id: &str, key: &str) -> Option<&'a Value> {
    items
        .as_array()?
        .iter()
        .find(|i| text_of(get(i, key)) == Some(id))
}

/// A target of `assert_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewTarget<'a> {
    /// A channel id.
    Channel(&'a str),
    /// An endpoint id.
    Endpoint(&'a str),
    /// The node.
    Node,
}

/// Builds the view of `target` from a snapshot's JSON.
pub fn view(snapshot: &Value, target: &ViewTarget<'_>) -> Result<Value, String> {
    match target {
        ViewTarget::Node => Ok(get(snapshot, "node").clone()),
        ViewTarget::Channel(id) => {
            let channels = get(snapshot, "channels");
            let channel = find(channels, id, "channel_id").ok_or_else(|| {
                format!(
                    "channel `{id}` is not in the snapshot (channels: {})",
                    ids(channels, "channel_id")
                )
            })?;
            Ok(channel_view(snapshot, channel))
        }
        ViewTarget::Endpoint(id) => {
            let endpoints = get(snapshot, "endpoints");
            let endpoint = find(endpoints, id, "endpoint_id").ok_or_else(|| {
                format!(
                    "endpoint `{id}` is not in the snapshot (endpoints: {})",
                    ids(endpoints, "endpoint_id")
                )
            })?;
            Ok(endpoint_view(snapshot, endpoint))
        }
    }
}

fn ids(items: &Value, key: &str) -> String {
    let list: Vec<&str> = items
        .as_array()
        .map(|a| a.iter().filter_map(|i| text_of(get(i, key))).collect())
        .unwrap_or_default();
    if list.is_empty() {
        "none".to_owned()
    } else {
        list.join(", ")
    }
}

/// Alerts of the snapshot (already in the specification's shape).
#[must_use]
pub fn alerts_view(snapshot: &Value) -> Vec<Value> {
    get(snapshot, "alerts")
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// An endpoint reference with the specified field names.
fn endpoint_ref(reference: &Value) -> Value {
    if reference.is_null() {
        return Value::Null;
    }
    let mut out = reference.as_object().cloned().unwrap_or_default();
    if let Some(n) = out.remove("consecutive_failures") {
        out.insert("consecutive_failure_count".into(), n);
    }
    Value::Object(out)
}

fn endpoint_view(snapshot: &Value, endpoint: &Value) -> Value {
    let traffic = get(endpoint, "traffic");
    let error = get(endpoint, "last_error");
    json!({
        "endpoint_id": get(endpoint, "endpoint_id"),
        "label": get(endpoint, "label"),
        "kind": get(endpoint, "kind"),
        "enabled": get(endpoint, "enabled"),
        "captured_at": get(get(snapshot, "node"), "captured_at"),
        "state": get(endpoint, "state"),
        "state_changed_at": get(endpoint, "state_since"),
        "channels": get(endpoint, "channels"),
        "held_by": get(endpoint, "held_by"),
        "bytes_in": get(traffic, "bytes_in"),
        "bytes_out": get(traffic, "bytes_out"),
        "messages_in": get(traffic, "messages_in"),
        "messages_out": get(traffic, "messages_out"),
        "last_activity_at": get(traffic, "last_activity"),
        "consecutive_failure_count": get(endpoint, "consecutive_failures"),
        "retry_count": get(endpoint, "retry_count"),
        "next_retry_at": get(endpoint, "next_retry_at"),
        "active_peer": get(endpoint, "peer"),
        "last_error_summary": get(error, "summary"),
        "last_error_at": get(error, "at"),
        "settings_summary": get(endpoint, "settings_summary"),
        "rejections": get(endpoint, "rejections"),
    })
}

fn channel_view(snapshot: &Value, channel: &Value) -> Value {
    let error = get(channel, "last_error");
    let channel_id = get(channel, "channel_id");
    let alerts: Vec<Value> = alerts_view(snapshot)
        .into_iter()
        .filter(|a| get(a, "channel_id") == channel_id)
        .map(|a| get(&a, "code").clone())
        .collect();
    let mut out = json!({
        "channel_id": channel_id,
        "label": get(channel, "label"),
        "kind": get(channel, "kind"),
        "enabled": get(channel, "enabled"),
        "captured_at": get(get(snapshot, "node"), "captured_at"),
        "state": get(channel, "state"),
        "stop_reason": get(channel, "stop_reason"),
        "config_version": get(channel, "config_version"),
        "instance_started_at": get(channel, "instance_started_at"),
        "retry_count": get(channel, "retry_count"),
        "next_retry_at": get(channel, "next_retry_at"),
        "last_error_summary": get(error, "summary"),
        "last_error_at": get(error, "at"),
        "recent_errors": get(channel, "recent_errors"),
        "alerts": alerts,
    });
    let Some(map) = out.as_object_mut() else {
        return out;
    };
    if let Some(bridge) = channel.get("bridge").filter(|b| !b.is_null()) {
        bridge_fields(bridge, map);
    }
    if let Some(message) = channel.get("message").filter(|m| !m.is_null()) {
        message_fields(message, map);
    }
    out
}

fn bridge_fields(bridge: &Value, map: &mut Map<String, Value>) {
    let (a_to_b, b_to_a) = (get(bridge, "a_to_b"), get(bridge, "b_to_a"));
    let (a, b) = (get(bridge, "endpoint_a"), get(bridge, "endpoint_b"));
    let tap = get(bridge, "tap");
    let overflow = get(a_to_b, "overflow_count").as_u64().unwrap_or(0)
        + get(b_to_a, "overflow_count").as_u64().unwrap_or(0);
    let buffer = |f: &Value| {
        json!({
            "segments": get(f, "buffered_segments"),
            "bytes": get(f, "buffered_bytes"),
            "max_segments": get(f, "max_segments"),
            "max_bytes": get(f, "max_bytes"),
            "overflowing": get(f, "overflowing"),
        })
    };
    map.insert("session_state".into(), get(bridge, "session").clone());
    map.insert(
        "connect_strategy".into(),
        get(bridge, "connect_strategy").clone(),
    );
    map.insert("receiver".into(), get(bridge, "receiver").clone());
    map.insert("endpoint_a".into(), endpoint_ref(a));
    map.insert("endpoint_b".into(), endpoint_ref(b));
    map.insert(
        "counters".into(),
        json!({
            "bytes_a_to_b": get(a_to_b, "bytes"),
            "bytes_b_to_a": get(b_to_a, "bytes"),
            "messages_a_to_b": get(a_to_b, "messages"),
            "messages_b_to_a": get(b_to_a, "messages"),
            "framing_errors": get(tap, "framing_errors"),
            "tap_dropped": get(tap, "tap_dropped"),
            "overflow_count": overflow,
        }),
    );
    map.insert(
        "buffers".into(),
        json!({ "a_to_b": buffer(a_to_b), "b_to_a": buffer(b_to_a) }),
    );
    map.insert("last_activity_a_at".into(), get(a, "last_activity").clone());
    map.insert("last_activity_b_at".into(), get(b, "last_activity").clone());
    map.insert("tap".into(), tap.clone());
}

fn message_fields(message: &Value, map: &mut Map<String, Value>) {
    let destinations: Vec<Value> = get(message, "destinations")
        .as_array()
        .map(|list| {
            list.iter()
                .map(|d| {
                    let mut out = d.as_object().cloned().unwrap_or_default();
                    out.insert("endpoint".into(), endpoint_ref(get(d, "endpoint")));
                    let summary = get(get(d, "last_error"), "summary").clone();
                    out.insert("last_error_summary".into(), summary);
                    Value::Object(out)
                })
                .collect()
        })
        .unwrap_or_default();
    map.insert(
        "source".into(),
        json!({
            "endpoint": endpoint_ref(get(message, "source")),
            "protocol": get(message, "protocol"),
            "session": get(message, "session"),
        }),
    );
    map.insert("counters".into(), get(message, "counters").clone());
    map.insert(
        "last_received_at".into(),
        get(message, "last_received_at").clone(),
    );
    map.insert("destinations".into(), Value::Array(destinations));
    map.insert("unresolved".into(), get(message, "unresolved").clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(bytes: u64) -> Value {
        json!({"bytes": bytes, "messages": null, "buffered_segments": 0, "buffered_bytes": 0,
               "max_segments": 8, "max_bytes": 1024, "overflowing": false, "overflow_count": 0})
    }

    fn endpoint_ref_json(id: &str, kind: &str, activity: Value) -> Value {
        json!({"endpoint_id": id, "label": id, "kind": kind, "role": "a", "state": "connected",
               "consecutive_failures": 2, "active_peer": "p", "last_activity": activity})
    }

    fn sample() -> Value {
        json!({
            "node": {"node_id": "n", "node_state": "healthy", "captured_at": "t1",
                     "channel_counts": {"running": 1}, "config": {"active_version": 1}},
            "channels": [{"channel_id": "bridge-1", "label": "B", "kind": "bridge", "enabled": true, "state": "running",
                          "stop_reason": null, "config_version": 1, "instance_started_at": "t0", "retry_count": 0,
                          "next_retry_at": null, "last_error": null, "recent_errors": [],
                          "bridge": {"session": "bridged", "connect_strategy": "parallel", "receiver": null,
                                     "endpoint_a": endpoint_ref_json("a", "tcp_listener", json!("t3")),
                                     "endpoint_b": endpoint_ref_json("b", "tcp_client", Value::Null),
                                     "a_to_b": flow(5), "b_to_a": flow(7),
                                     "tap": {"framer": "none", "framing_errors": null, "tap_dropped": null}},
                          "message": null}],
            "endpoints": [{"endpoint_id": "a", "label": "A", "kind": "tcp_listener", "enabled": true, "state": "connected",
                           "state_since": "t0", "channels": ["bridge-1"], "held_by": "bridge-1", "consecutive_failures": 0,
                           "peer": "p", "traffic": {"bytes_in": 5, "last_activity": "t3"}, "last_error": null}],
            "alerts": [{"alert_id": "x", "code": "bridge_buffer_pressure", "channel_id": "bridge-1", "state": "active"}],
        })
    }

    #[test]
    fn bridge_view_uses_specification_names() {
        let v = view(&sample(), &ViewTarget::Channel("bridge-1")).unwrap();
        assert_eq!(v["session_state"], "bridged");
        assert_eq!(v["endpoint_a"]["state"], "connected");
        assert_eq!(v["endpoint_a"]["consecutive_failure_count"], 2);
        assert_eq!(v["counters"]["bytes_a_to_b"], 5);
        assert_eq!(v["counters"]["bytes_b_to_a"], 7);
        assert_eq!(v["counters"]["messages_a_to_b"], Value::Null);
        assert_eq!(v["buffers"]["a_to_b"]["overflowing"], false);
        assert_eq!(v["last_activity_a_at"], "t3");
        assert_eq!(v["alerts"], json!(["bridge_buffer_pressure"]));
    }

    #[test]
    fn node_and_endpoint_views() {
        let node = view(&sample(), &ViewTarget::Node).unwrap();
        assert_eq!(node["node_state"], "healthy");
        assert_eq!(node["channel_counts"]["running"], 1);
        let endpoint = view(&sample(), &ViewTarget::Endpoint("a")).unwrap();
        assert_eq!(endpoint["state_changed_at"], "t0");
        assert_eq!(endpoint["active_peer"], "p");
        assert_eq!(endpoint["bytes_in"], 5);
    }

    #[test]
    fn unknown_channel_lists_the_known_ones() {
        let err = view(&sample(), &ViewTarget::Channel("nope")).unwrap_err();
        assert!(err.contains("bridge-1"), "{err}");
    }
}
