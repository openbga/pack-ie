//! Reconciliation: no message may be lost or duplicated at a receiving peer.
//!
//! Every message the source peer had acknowledged must reach the receiving peer exactly
//! once. A message that did not arrive is checked against the engine's own record:
//!
//! | Engine says | Meaning | Verdict |
//! | --- | --- | --- |
//! | `outcome_unknown`, `timed_out`, `failed` | The engine could not prove the delivery and says so; an operator must decide. Expected after a kill or a receiver that dropped the connection. | Reported, not a failure |
//! | `acknowledged`, no delivery, or no message | The engine claims it delivered, or never created a delivery. | Failure: lost |
//! | `rejected` | The peers never reject. | Failure |
//! | `queued`, `sending` | Still in flight. | Pending; a failure in the final check |
//!
//! A duplicate at the receiving peer or a damaged message is always a failure. Bridges are
//! judged by [`Ledger::reconcile_bridge`](super::ledger::Ledger::reconcile_bridge).

use std::collections::BTreeMap;
use std::time::Duration;

use packie_core::ChannelId;
use packie_ipc::messages::{IdentifierFilter, MessageFilter, MessageQuery};
use packie_ipc::{Request, Response};
use serde_json::{Value, json};

use super::chan::ChannelRt;
use super::rig::{Owner, Rig};
use super::snap;

/// Messages acknowledged less than this long ago are not judged yet.
pub(crate) const SETTLE_MS: u64 = 60_000;
/// Engine lookups per check; the rest wait for the next one.
const MAX_LOOKUPS: usize = 300;

/// The result for one message channel.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelRecon {
    pub(crate) channel: String,
    pub(crate) acked: u64,
    pub(crate) received: u64,
    pub(crate) pending: u64,
    pub(crate) unacknowledged: u64,
    pub(crate) duplicates: u64,
    pub(crate) corrupt: u64,
    /// Not received, and the engine reports the delivery as unresolved.
    pub(crate) unresolved: u64,
    /// Of `unresolved`, discarded on purpose by the receiving peer.
    pub(crate) discarded: u64,
    /// Not received, and the engine claims to have delivered it or knows nothing of it.
    pub(crate) lost: u64,
    pub(crate) stuck: u64,
}

/// The result for one bridge.
#[derive(Debug, Clone, Default)]
pub(crate) struct BridgeRow {
    pub(crate) channel: String,
    pub(crate) sent: u64,
    pub(crate) received: u64,
    pub(crate) lost_at_break: u64,
    pub(crate) lost_unexplained: u64,
    pub(crate) duplicates: u64,
    pub(crate) out_of_order: u64,
    pub(crate) corrupt: u64,
    pub(crate) pending: u64,
}

/// One reconciliation.
#[derive(Debug, Clone, Default)]
pub(crate) struct Summary {
    pub(crate) label: String,
    pub(crate) at_ms: u64,
    pub(crate) channels: Vec<ChannelRecon>,
    pub(crate) bridges: Vec<BridgeRow>,
    pub(crate) ok: bool,
}

/// Waits until no message channel has deliveries queued or in flight; `false` on timeout.
pub(crate) async fn wait_drained(rig: &Rig, within: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if let Some(snapshot) = rig.snapshot().await
            && rig
                .channels
                .iter()
                .filter(|c| !c.plan.kind.is_bridge())
                .all(|c| snap::channel(&snapshot, &c.plan.id).is_some_and(|v| snap::queued(v) == 0))
        {
            return true;
        }
        if tokio::time::Instant::now() >= deadline
            || !rig.env.halt.sleep(Duration::from_secs(2)).await
        {
            return false;
        }
    }
}

/// What the engine knows about the message `seq` of `ch`: the state of its delivery, or
/// `absent` when the engine has no such message.
async fn engine_state(rig: &Rig, ch: &ChannelRt, seq: u64) -> String {
    let Ok(channel_id) = ch.plan.id.parse::<ChannelId>() else {
        return "unknown_channel_id".to_owned();
    };
    let query = MessageQuery {
        filter: MessageFilter {
            channels: vec![channel_id],
            identifier: Some(IdentifierFilter {
                value: ch.plan.control_id(seq),
                prefix: false,
            }),
            ..MessageFilter::default()
        },
        limit: 10,
        ..MessageQuery::default()
    };
    let Ok(Response::Messages(list)) = rig.call(Request::GetMessages(Box::new(query))).await else {
        return "lookup_failed".to_owned();
    };
    let Ok(value) = serde_json::to_value(&list) else {
        return "lookup_failed".to_owned();
    };
    delivery_state(&value)
}

/// The delivery state of the first routed message of a message list.
fn delivery_state(list: &Value) -> String {
    let messages = list["messages"].as_array().cloned().unwrap_or_default();
    let routed = messages
        .iter()
        .find(|m| matches!(m["state"].as_str(), Some("routed" | "received")));
    let Some(message) = routed else {
        return if messages.is_empty() {
            "absent"
        } else {
            "not_routed"
        }
        .to_owned();
    };
    message["deliveries"]
        .get(0)
        .and_then(|d| d["state"].as_str())
        .unwrap_or("no_delivery")
        .to_owned()
}

/// How a missing message is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Lost,
    Unresolved,
    Rejected,
    InFlight,
}

fn judge(state: &str) -> Verdict {
    match state {
        "outcome_unknown" | "timed_out" | "failed" => Verdict::Unresolved,
        "rejected" => Verdict::Rejected,
        "queued" | "sending" | "lookup_failed" => Verdict::InFlight,
        _ => Verdict::Lost,
    }
}

/// Compares the peers' records of one message channel with the engine's.
async fn check_channel(rig: &Rig, ch: &ChannelRt, last: bool, settle_ms: u64) -> ChannelRecon {
    let recon = ch.ledger.reconcile(rig.now_ms(), settle_ms);
    let mut row = ChannelRecon {
        channel: ch.plan.id.clone(),
        acked: recon.acked,
        received: recon.received,
        pending: recon.pending,
        unacknowledged: recon.unacknowledged,
        duplicates: recon.duplicates.len() as u64,
        corrupt: recon.corrupt,
        ..ChannelRecon::default()
    };
    if recon.corrupt > 0 {
        rig.fail(
            "corrupt_message",
            Owner::RoutingStore,
            format!(
                "{}: {} message(s) reached the receiving peer with changed content",
                row.channel, recon.corrupt
            ),
        );
    }
    report_duplicates(rig, ch, &recon.duplicates).await;
    classify_missing(rig, ch, &recon, last, &mut row).await;
    row
}

async fn report_duplicates(rig: &Rig, ch: &ChannelRt, duplicates: &[u64]) {
    if duplicates.is_empty() {
        return;
    }
    let first = duplicates[0];
    let state = engine_state(rig, ch, first).await;
    rig.fail(
        "duplicate_at_peer",
        Owner::RoutingStore,
        format!(
            "{}: {} message(s) reached the receiving peer more than once, first {} (engine delivery state: {state})",
            ch.plan.id,
            duplicates.len(),
            ch.plan.control_id(first)
        ),
    );
}

async fn classify_missing(
    rig: &Rig,
    ch: &ChannelRt,
    recon: &super::ledger::Recon,
    last: bool,
    row: &mut ChannelRecon,
) {
    let mut lookups = 0;
    let mut lost = Vec::new();
    let mut by_state: BTreeMap<String, u64> = BTreeMap::new();
    for seq in &recon.missing {
        let key = (ch.plan.id.clone(), *seq);
        let cached = lock(rig).get(&key).cloned();
        let state = match cached {
            Some(state) => state,
            None if lookups >= MAX_LOOKUPS => continue,
            None => {
                lookups += 1;
                engine_state(rig, ch, *seq).await
            }
        };
        match judge(&state) {
            Verdict::InFlight if !last => {
                row.pending += 1;
                continue;
            }
            Verdict::InFlight => row.stuck += 1,
            Verdict::Unresolved => row.unresolved += 1,
            Verdict::Rejected | Verdict::Lost => {
                row.lost += 1;
                lost.push(*seq);
            }
        }
        if recon.discarded.contains(seq) && judge(&state) == Verdict::Unresolved {
            row.discarded += 1;
        }
        *by_state.entry(state.clone()).or_default() += 1;
        lock(rig).insert(key, state);
    }
    report_missing(rig, ch, &lost, row, &by_state);
}

fn lock(rig: &Rig) -> std::sync::MutexGuard<'_, std::collections::HashMap<(String, u64), String>> {
    rig.verdicts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn report_missing(
    rig: &Rig,
    ch: &ChannelRt,
    lost: &[u64],
    row: &ChannelRecon,
    by_state: &BTreeMap<String, u64>,
) {
    if let Some(first) = lost.first() {
        let states: Vec<String> = by_state.iter().map(|(s, n)| format!("{s}: {n}")).collect();
        rig.fail(
            "message_lost",
            Owner::RoutingStore,
            format!(
                "{}: {} message(s) were acknowledged to the source peer but never reached the receiving peer, and the engine does not report them as unresolved ({}), first {}",
                row.channel,
                lost.len(),
                states.join(", "),
                ch.plan.control_id(*first)
            ),
        );
    }
    if row.stuck > 0 {
        rig.fail(
            "delivery_stuck",
            Owner::RoutingStore,
            format!(
                "{}: {} delivery(ies) are still queued or sending after the run drained",
                row.channel, row.stuck
            ),
        );
    }
}

fn check_bridge(rig: &Rig, ch: &ChannelRt, settle_ms: u64) -> BridgeRow {
    let r = ch.ledger.reconcile_bridge(rig.now_ms(), settle_ms);
    let row = BridgeRow {
        channel: ch.plan.id.clone(),
        sent: r.sent,
        received: r.received,
        lost_at_break: r.lost_at_break,
        lost_unexplained: r.lost_unexplained.len() as u64,
        duplicates: r.duplicates,
        out_of_order: r.out_of_order,
        corrupt: r.corrupt,
        pending: r.pending,
    };
    if row.duplicates + row.out_of_order + row.corrupt > 0 {
        rig.fail(
            "bridge_integrity",
            Owner::RoutingStore,
            format!(
                "{}: the sink saw {} duplicate, {} reordered and {} damaged record(s)",
                row.channel, row.duplicates, row.out_of_order, row.corrupt
            ),
        );
    }
    if let Some(first) = r.lost_unexplained.first() {
        rig.fail(
            "bridge_loss",
            Owner::RoutingStore,
            format!(
                "{}: {} record(s) never reached the sink although no connection broke near them, first record {first}",
                row.channel,
                r.lost_unexplained.len()
            ),
        );
    }
    row
}

/// Runs one reconciliation of every channel. `last` is the check after the run has
/// drained: in-flight deliveries then count as failures.
pub(crate) async fn run(rig: &Rig, label: &str, last: bool, settle_ms: u64) -> Summary {
    let mut summary = Summary {
        label: label.to_owned(),
        at_ms: rig.now_ms(),
        ok: true,
        ..Summary::default()
    };
    for ch in &rig.channels {
        if ch.plan.kind.is_bridge() {
            summary.bridges.push(check_bridge(rig, ch, settle_ms));
        } else {
            summary
                .channels
                .push(check_channel(rig, ch, last, settle_ms).await);
        }
    }
    summary.ok = summary
        .channels
        .iter()
        .all(|c| c.lost + c.stuck + c.duplicates + c.corrupt == 0)
        && summary
            .bridges
            .iter()
            .all(|b| b.lost_unexplained + b.duplicates + b.out_of_order + b.corrupt == 0);
    rig.journal.reconcile(&json!({
        "label": label, "t_ms": summary.at_ms, "ok": summary.ok,
        "channels": summary.channels.iter().map(channel_json).collect::<Vec<_>>(),
        "bridges": summary.bridges.iter().map(bridge_json).collect::<Vec<_>>(),
    }));
    if let Ok(mut all) = rig.recons.lock() {
        all.push(summary.clone());
    }
    summary
}

fn channel_json(c: &ChannelRecon) -> Value {
    json!({ "channel": c.channel, "acked": c.acked, "received": c.received, "pending": c.pending,
            "unacknowledged": c.unacknowledged, "duplicates": c.duplicates, "corrupt": c.corrupt,
            "unresolved": c.unresolved, "discarded": c.discarded, "lost": c.lost, "stuck": c.stuck })
}

fn bridge_json(b: &BridgeRow) -> Value {
    json!({ "channel": b.channel, "sent": b.sent, "received": b.received,
            "lost_at_break": b.lost_at_break, "lost_unexplained": b.lost_unexplained,
            "duplicates": b.duplicates, "out_of_order": b.out_of_order, "corrupt": b.corrupt,
            "pending": b.pending })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_states_are_judged() {
        assert_eq!(judge("outcome_unknown"), Verdict::Unresolved);
        assert_eq!(judge("timed_out"), Verdict::Unresolved);
        assert_eq!(judge("failed"), Verdict::Unresolved);
        assert_eq!(judge("rejected"), Verdict::Rejected);
        assert_eq!(judge("queued"), Verdict::InFlight);
        assert_eq!(judge("acknowledged"), Verdict::Lost);
        assert_eq!(judge("absent"), Verdict::Lost);
        assert_eq!(judge("no_delivery"), Verdict::Lost);
    }

    #[test]
    fn the_delivery_state_comes_from_the_routed_message() {
        let list = json!({ "messages": [
            { "state": "duplicate", "deliveries": [] },
            { "state": "routed", "deliveries": [{ "state": "outcome_unknown" }] } ] });
        assert_eq!(delivery_state(&list), "outcome_unknown");
        assert_eq!(delivery_state(&json!({ "messages": [] })), "absent");
        assert_eq!(
            delivery_state(&json!({ "messages": [{ "state": "routed", "deliveries": [] }] })),
            "no_delivery"
        );
        assert_eq!(
            delivery_state(&json!({ "messages": [{ "state": "aborted" }] })),
            "not_routed"
        );
    }
}
