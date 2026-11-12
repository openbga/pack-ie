//! `packie status`: the node, its channels and its endpoints, as the console dashboard shows
//! them.

use packie_core::{ByteSize, Timestamp};
use packie_diagnostics::{
    ChannelSnapshot, EndpointSnapshot, EngineSnapshot, MessageDetail, StoreStatus, Totals,
};
use packie_ipc::{Request, Response};

use crate::Failure;
use crate::client::{Target, ask, unexpected};
use crate::output::{
    count, duration, json_text, opt_text, opt_time, print_json, print_line, print_table,
};

const NO_CHANNELS: &str = "No channels configured. `packie config apply <file>` adds some.";
const NO_ENDPOINTS: &str = "No endpoints configured. `packie config apply <file>` adds some.";

pub(crate) async fn status(target: &Target, json: bool) -> Result<(), Failure> {
    let client = target.connect().await?;
    let Response::Snapshot { snapshot } = ask(&client, Request::GetSnapshot).await? else {
        return Err(unexpected());
    };
    if json {
        print_json(&snapshot)
    } else {
        print_status(&snapshot);
        Ok(())
    }
}

fn print_status(snapshot: &EngineSnapshot) {
    print_node(snapshot);
    print_section(
        "Channels",
        NO_CHANNELS,
        &[
            "CHANNEL",
            "LABEL",
            "KIND",
            "STATE",
            "TRAFFIC",
            "LAST ACTIVITY",
            "LAST ERROR",
        ],
        &snapshot.channels,
        channel_row,
    );
    print_section(
        "Endpoints",
        NO_ENDPOINTS,
        &[
            "ENDPOINT",
            "KIND",
            "STATE",
            "SETTINGS",
            "PEER",
            "LAST ACTIVITY",
            "LAST ERROR",
        ],
        &snapshot.endpoints,
        endpoint_row,
    );
}

/// A blank line, `Title (n)`, then a table with one row per item, or `empty_text`.
fn print_section<T>(
    title: &str,
    empty_text: &str,
    headers: &[&str],
    items: &[T],
    row: fn(&T) -> Vec<String>,
) {
    print_line("");
    print_line(&format!("{title} ({})", items.len()));
    if items.is_empty() {
        print_line(empty_text);
    } else {
        let rows: Vec<Vec<String>> = items.iter().map(row).collect();
        print_table(headers, &rows);
    }
}

fn print_node(snapshot: &EngineSnapshot) {
    let node = &snapshot.node;
    let counts = &node.channel_counts;
    let uptime = node.captured_at.saturating_duration_since(node.started_at);
    print_line(&format!(
        "Node      {} ({})",
        node.display_name, node.node_id
    ));
    print_line(&format!("State     {}", json_text(&node.node_state)));
    let version = node
        .config
        .active_version
        .map_or_else(|| "none".to_owned(), |v| v.to_string());
    print_line(&format!(
        "Engine    {}, up {}, configuration version {version}",
        node.version,
        duration(uptime),
    ));
    print_line(&format!(
        "Store     {}",
        store_text(&node.store, &node.totals)
    ));
    print_line(&format!(
        "Channels  {} running, {} starting, {} retrying, {} degraded, {} stopped",
        counts.running, counts.starting, counts.retrying, counts.degraded, counts.stopped
    ));
    let alerts = &node.totals.alerts_active;
    print_line(&format!(
        "Alerts    {} critical, {} warning",
        alerts.critical, alerts.warning
    ));
    for alert in &snapshot.alerts {
        print_line(&format!(
            "  [{}] {} {}",
            alert.severity, alert.summary, alert.next_action
        ));
    }
}

/// `ok, 12 messages stored, 1 delivery unresolved, 0 queued`; figures the store does not
/// report are left out.
fn store_text(store: &StoreStatus, totals: &Totals) -> String {
    let mut parts = vec![json_text(&store.state)];
    if let Some(n) = totals.messages_stored {
        parts.push(format!("{} stored", count(n, "message", "messages")));
    }
    if let Some(n) = totals.deliveries_unresolved {
        parts.push(format!("{} unresolved", count(n, "delivery", "deliveries")));
    }
    if let Some(n) = totals.deliveries_queued {
        parts.push(format!("{n} queued"));
    }
    if let Some(bytes) = store.size_bytes {
        parts.push(ByteSize(bytes).to_string());
    }
    parts.join(", ")
}

fn channel_row(channel: &ChannelSnapshot) -> Vec<String> {
    vec![
        channel.id.to_string(),
        channel.label.clone(),
        channel.kind.to_string(),
        state_text(channel),
        traffic_text(channel),
        opt_time(channel_activity(channel)),
        opt_text(channel.last_error.as_ref().map(|e| e.summary.as_str())),
    ]
}

/// The last byte or message of the channel; message channels also report their last
/// committed message.
fn channel_activity(channel: &ChannelSnapshot) -> Option<Timestamp> {
    let received = channel.message.as_ref().and_then(|m| m.last_received_at);
    channel.traffic.last_activity.max(received)
}

/// The state, with the stop reason of a stopped channel or the retry number of a retrying
/// one.
fn state_text(channel: &ChannelSnapshot) -> String {
    match (channel.stop_reason, channel.retry_count) {
        (Some(reason), _) => format!("{} ({reason})", channel.state),
        (None, retries) if retries > 0 => format!("{} (retry {retries})", channel.state),
        _ => channel.state.to_string(),
    }
}

/// Bridges: bytes each way. Message channels: what was received and where it stands.
fn traffic_text(channel: &ChannelSnapshot) -> String {
    if let Some(detail) = &channel.message {
        return message_traffic(detail);
    }
    let (a_to_b, b_to_a) = channel
        .bridge
        .as_ref()
        .map_or((channel.traffic.bytes_in, channel.traffic.bytes_out), |b| {
            (b.a_to_b.bytes, b.b_to_a.bytes)
        });
    format!("A->B {}, B->A {}", ByteSize(a_to_b), ByteSize(b_to_a))
}

/// `5 received, 4 acked, 1 unresolved, 0 queued`, plus invalid and duplicate counts when
/// there are any. Received and acked count since the channel instance started; unresolved
/// and queued are current.
fn message_traffic(detail: &MessageDetail) -> String {
    let acked = detail
        .destinations
        .iter()
        .fold(0_u64, |sum, d| sum.saturating_add(d.counters.acknowledged));
    let queued = detail.destinations.iter().fold(0_u64, |sum, d| {
        sum.saturating_add(d.queue.queued)
            .saturating_add(d.queue.sending)
    });
    let counters = &detail.counters;
    let mut parts = vec![
        format!("{} received", counters.received),
        format!("{acked} acked"),
        format!("{} unresolved", detail.unresolved.total()),
        format!("{queued} queued"),
    ];
    for (n, what) in [
        (counters.invalid, "invalid"),
        (counters.duplicates, "duplicate"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    parts.join(", ")
}

fn endpoint_row(endpoint: &EndpointSnapshot) -> Vec<String> {
    vec![
        endpoint.id.to_string(),
        endpoint.kind.to_string(),
        endpoint.state.to_string(),
        endpoint.settings_summary.clone(),
        opt_text(endpoint.peer.as_deref()),
        opt_time(endpoint.traffic.last_activity),
        opt_text(endpoint.last_error.as_ref().map(|e| e.summary.as_str())),
    ]
}

#[cfg(test)]
mod tests {
    use packie_diagnostics::{AlertCounts, StoreState, StoreStatus, Totals};

    use super::store_text;

    #[test]
    fn the_store_line_leaves_out_unknown_figures() {
        let store = StoreStatus {
            state: StoreState::Ok,
            size_bytes: Some(2048),
            free_disk_bytes: None,
            total_disk_bytes: None,
        };
        let totals = Totals {
            messages_stored: Some(1),
            deliveries_unresolved: Some(2),
            deliveries_queued: Some(0),
            alerts_active: AlertCounts::default(),
            ..Totals::default()
        };
        assert_eq!(
            store_text(&store, &totals),
            "ok, 1 message stored, 2 deliveries unresolved, 0 queued, 2.0 KiB"
        );
        let none = Totals::default();
        assert_eq!(store_text(&store, &none), "ok, 2.0 KiB");
    }
}
