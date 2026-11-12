//! Episodes that act on the peers of a channel.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::Value;

use super::Outcome;
use super::check::{Baseline, Need, view_of, wait_for, wait_recovered};
use super::cut;
use crate::soak::chan::{ChannelRt, Progress};
use crate::soak::rig::{Owner, Rig};
use crate::soak::snap;
use crate::soak::util::{Rng, hms, unix_ms};

/// A cut connection that takes this long to be served again is a finding: a healthy engine
/// notices the loss at once.
const SLOW_RECONNECT: Duration = Duration::from_secs(10);
/// Budget after a peer reconnects.
const RECONNECT_BUDGET: Duration = Duration::from_secs(60);
/// How long a peer may need to get to a requested disconnect: it acts between messages.
const ACT_WITHIN: Duration = Duration::from_secs(120);

fn one(ch: &Arc<ChannelRt>) -> Vec<(Arc<ChannelRt>, Baseline)> {
    vec![(Arc::clone(ch), Baseline::of(ch))]
}

/// The instrument drops its connection in the middle of a message and reconnects.
pub(crate) async fn source_disconnect(rig: &Rig, ch: &Arc<ChannelRt>, rng: &mut Rng) -> Outcome {
    let reset = rng.chance(40);
    ch.ctl.reset.store(reset, Ordering::SeqCst);
    let before = Progress::get(&ch.prog.source_disconnects_done);
    ch.ctl.source_disconnects.fetch_add(1, Ordering::SeqCst);
    let done = wait_for(rig, ACT_WITHIN, || {
        Progress::get(&ch.prog.source_disconnects_done) > before
    })
    .await;
    let detail = format!(
        "instrument cut mid-message ({})",
        if reset { "reset" } else { "close" }
    );
    if !done {
        ch.ctl.source_disconnects.store(0, Ordering::SeqCst);
        return Outcome::new(detail, ACT_WITHIN).note("the instrument never got to disconnect");
    }
    let cut_at = unix_ms();
    let recovered = wait_recovered(rig, &one(ch), RECONNECT_BUDGET, Need::Flowing).await;
    let mut outcome = Outcome::new(detail, RECONNECT_BUDGET)
        .recovered(recovered)
        .owner(Owner::RoutingStore);
    if let Some(took) = recovered.filter(|d| *d >= SLOW_RECONNECT) {
        rig.fail_at(
            "session_held_after_cut",
            Owner::RoutingStore,
            format!(
                "{}: after the instrument dropped its connection with a {} behind half a message, the engine needed {} s before it served the instrument again. The dead session kept the listener's single slot and every reconnect was refused (listener.peer_rejected, alert session_active) until the session ended on its own timeout; an HL7 source ends it at frame_timeout_ms (30 s by default), and a close instead of a reset does not show this",
                ch.plan.id,
                if reset { "reset" } else { "close" },
                took.as_secs()
            ),
            cut_at,
        );
        outcome.flagged = true;
    }
    outcome
}

/// The receiving side drops its connection instead of acknowledging.
pub(crate) async fn sink_disconnect(rig: &Rig, ch: &Arc<ChannelRt>, rng: &mut Rng) -> Outcome {
    let reset = rng.chance(40);
    ch.ctl.reset.store(reset, Ordering::SeqCst);
    let before = Progress::get(&ch.prog.sink_disconnects_done);
    ch.ctl.sink_disconnects.fetch_add(1, Ordering::SeqCst);
    let done = wait_for(rig, ACT_WITHIN, || {
        Progress::get(&ch.prog.sink_disconnects_done) > before
    })
    .await;
    let detail = format!(
        "receiver dropped the connection ({})",
        if reset { "reset" } else { "close" }
    );
    if !done {
        ch.ctl.sink_disconnects.store(0, Ordering::SeqCst);
        return Outcome::new(detail, ACT_WITHIN).note("no message reached the receiver");
    }
    let recovered = wait_recovered(rig, &one(ch), RECONNECT_BUDGET, Need::Flowing).await;
    Outcome::new(detail, RECONNECT_BUDGET).recovered(recovered)
}

/// How long the receiving side stalls. Message destinations answer within the delivery's
/// acknowledgement timeout (60 s) unless the stall is long; bridges with a small buffer
/// need longer to feel back-pressure.
fn stall_secs(ch: &ChannelRt, rng: &mut Rng) -> u64 {
    if ch.plan.kind.is_bridge() {
        if ch.plan.small_buffer {
            rng.between(45, 90)
        } else {
            rng.between(20, 60)
        }
    } else {
        rng.between(15, 45)
    }
}

/// A short description of how loaded the channel is.
fn pressure(view: &Value, ch: &ChannelRt) -> String {
    if ch.plan.kind.is_bridge() {
        format!(
            "bridge {} with {} KiB buffered",
            snap::channel_state(view),
            snap::number(view, &["bridge", "a_to_b", "buffered_bytes"]) / 1024
        )
    } else {
        format!("{} deliveries queued", snap::queued(view))
    }
}

/// The receiving side stops reading for a while.
pub(crate) async fn sink_stall(rig: &Rig, ch: &Arc<ChannelRt>, rng: &mut Rng) -> Outcome {
    let secs = stall_secs(ch, rng);
    ch.ctl
        .stall_until_ms
        .store(rig.now_ms() + secs * 1000, Ordering::SeqCst);
    // Look two seconds before the end, while the receiver is still not reading.
    rig.env
        .stop
        .sleep(Duration::from_secs(secs.saturating_sub(2)))
        .await;
    let under = match view_of(rig, &ch.plan.id).await {
        Some(view) => pressure(&view, ch),
        None => "no snapshot".to_owned(),
    };
    rig.env.stop.sleep(Duration::from_secs(2)).await;
    let base = one(ch);
    let budget = Duration::from_secs(90);
    let recovered = wait_recovered(rig, &base, budget, Need::Drained).await;
    Outcome::new(
        format!("receiver stopped reading for {}", hms(secs)),
        budget,
    )
    .note(format!("2 s before the end of the stall: {under}"))
    .recovered(recovered)
}

/// The engine's own view of the destination endpoint.
fn destination_state(view: &Value, ch: &ChannelRt) -> String {
    if ch.plan.kind.is_bridge() {
        snap::text(view, &["bridge", "endpoint_b", "state"]).to_owned()
    } else {
        snap::text(snap::destination(view), &["endpoint", "state"]).to_owned()
    }
}

/// The destination does not listen for minutes.
pub(crate) async fn destination_down(rig: &Rig, ch: &Arc<ChannelRt>, rng: &mut Rng) -> Outcome {
    let secs = if ch.plan.kind.is_bridge() {
        rng.between(60, 180)
    } else {
        rng.between(120, 300)
    };
    let unresolved_before = view_of(rig, &ch.plan.id)
        .await
        .map_or(0, |v| snap::unresolved(&v));
    let outage = Outage::begin(rig, ch).await;
    ch.ctl.down.store(true, Ordering::SeqCst);
    rig.env.stop.sleep(Duration::from_secs(secs / 2)).await;
    let noted = check_while_down(rig, ch, unresolved_before).await;
    rig.env
        .stop
        .sleep(Duration::from_secs(secs - secs / 2))
        .await;
    let (tolerated, mut failed) = outage.during(rig, ch).await;
    ch.ctl.down.store(false, Ordering::SeqCst);
    let base = one(ch);
    let backlog = secs * 2 / 10;
    let budget = Duration::from_secs(120 + backlog);
    let recovered = wait_recovered(rig, &base, budget, Need::Drained).await;
    failed |= outage.after(rig, ch, &tolerated).await;
    let mut outcome = Outcome::new(format!("destination down for {}", hms(secs)), budget)
        .note(noted)
        .recovered(recovered)
        .owner(Owner::RoutingStore);
    if !tolerated.is_empty() && !failed {
        outcome = outcome.note(format!(
            "the attempt in flight at the cut ({}) ended outcome_unknown, as specified",
            tolerated[0].control_id
        ));
    }
    outcome.flagged = failed;
    outcome
}

/// Half way through: the engine must have noticed, and nothing may have failed.
async fn check_while_down(rig: &Rig, ch: &ChannelRt, unresolved_before: u64) -> String {
    let Some(view) = view_of(rig, &ch.plan.id).await else {
        return "no snapshot half way".to_owned();
    };
    let state = destination_state(&view, ch);
    if state == "connected" {
        rig.warn(
            "down_not_noticed",
            Owner::Core,
            format!(
                "{}: the destination endpoint still reads connected half way through an outage of its peer",
                ch.plan.id
            ),
        );
    }
    let queued = if ch.plan.kind.is_bridge() {
        String::new()
    } else {
        format!(", {} queued", snap::queued(&view))
    };
    let grew = snap::unresolved(&view).saturating_sub(unresolved_before);
    format!(
        "half way: destination endpoint {state}{queued}, {grew} new unresolved (one attempt in flight at the cut may be among them)"
    )
}

/// What the outage did to the channel's deliveries, judged by [`cut`]: queued deliveries
/// stay queued, and the one attempt in flight at the cut may end `outcome_unknown`.
struct Outage {
    /// Unresolved deliveries before the cut; `None` for a bridge or when the engine could
    /// not say.
    before: Option<Vec<cut::Listed>>,
    cut_ms: u64,
}

impl Outage {
    async fn begin(rig: &Rig, ch: &ChannelRt) -> Self {
        let before = if ch.plan.kind.is_bridge() {
            None
        } else {
            cut::list_unresolved(rig, ch).await
        };
        if before.is_none() && !ch.plan.kind.is_bridge() {
            rig.warn(
                "cut_check_skipped",
                Owner::Runner,
                format!(
                    "{}: the engine could not list the unresolved deliveries before an outage, so the check that queued deliveries stay queued was skipped",
                    ch.plan.id
                ),
            );
        }
        Self {
            before,
            cut_ms: unix_ms(),
        }
    }

    /// Just before the destination returns: the deliveries that became unresolved. Returns
    /// the tolerated ones (the in-flight attempt) and whether the check failed.
    async fn during(&self, rig: &Rig, ch: &ChannelRt) -> (Vec<cut::Listed>, bool) {
        let Some(before) = &self.before else {
            return (Vec::new(), false);
        };
        let Some(now) = cut::list_unresolved(rig, ch).await else {
            return (Vec::new(), false);
        };
        let new = cut::new_since(before, &now);
        let mut with_attempts = Vec::new();
        for delivery in new.iter().take(3) {
            with_attempts.push((delivery.clone(), cut::last_attempt(rig, delivery).await));
        }
        let problems = cut::judge_at_cut(&with_attempts, self.cut_ms);
        if problems.is_empty() {
            return (new, false);
        }
        rig.fail(
            "failed_while_waiting",
            Owner::RoutingStore,
            format!(
                "{}: deliveries changed state while the destination was down. Deliveries that were queued must stay queued (an endpoint that is not ready is waiting, not failing); only the attempt in flight at the cut may end outcome_unknown. {}",
                ch.plan.id,
                problems.join(" ")
            ),
        );
        (Vec::new(), true)
    }

    /// After recovery: the tolerated delivery must still be accounted for.
    async fn after(&self, rig: &Rig, ch: &ChannelRt, tolerated: &[cut::Listed]) -> bool {
        if tolerated.is_empty() {
            return false;
        }
        let Some(now) = cut::list_unresolved(rig, ch).await else {
            return false;
        };
        let problems = cut::judge_after(tolerated, &now);
        if problems.is_empty() {
            return false;
        }
        rig.fail(
            "inflight_cut_state",
            Owner::RoutingStore,
            format!("{}: {}", ch.plan.id, problems.join(" ")),
        );
        true
    }
}

/// A file is written into the source folder in two steps: the engine must wait for it.
pub(crate) async fn slow_folder_write(rig: &Rig, ch: &Arc<ChannelRt>) -> Outcome {
    let invalid_before = invalid(rig, ch).await;
    let before = Progress::get(&ch.prog.source_disconnects_done);
    ch.ctl.slow_writes.fetch_add(1, Ordering::SeqCst);
    let done = wait_for(rig, Duration::from_secs(30), || {
        Progress::get(&ch.prog.source_disconnects_done) > before
    })
    .await;
    let detail = "file written in two steps with a half row between".to_owned();
    if !done {
        ch.ctl.slow_writes.store(0, Ordering::SeqCst);
        return Outcome::new(detail, RECONNECT_BUDGET).note("the writer never wrote a slow file");
    }
    let recovered = wait_recovered(rig, &one(ch), RECONNECT_BUDGET, Need::Flowing).await;
    rig.env.stop.sleep(Duration::from_secs(6)).await;
    let grew = invalid(rig, ch).await.saturating_sub(invalid_before);
    let mut outcome = Outcome::new(detail, RECONNECT_BUDGET)
        .recovered(recovered)
        .owner(Owner::RoutingStore);
    if grew > 0 {
        rig.fail(
            "partial_file_read",
            Owner::RoutingStore,
            format!(
                "{}: the engine read {grew} row(s) of a file that was still being written (invalid messages counted)",
                ch.plan.id
            ),
        );
        outcome.flagged = true;
    }
    outcome
}

async fn invalid(rig: &Rig, ch: &ChannelRt) -> u64 {
    view_of(rig, &ch.plan.id)
        .await
        .map_or(0, |v| snap::number(&v, &["message", "counters", "invalid"]))
}

/// An ASTM transfer ends with EOT and the next ENQ in one write; the engine must
/// acknowledge that ENQ. An ENQ it ignores costs the instrument 20 s per occurrence.
pub(crate) async fn astm_back_to_back(rig: &Rig, ch: &Arc<ChannelRt>) -> Outcome {
    let done_before = Progress::get(&ch.prog.chained_done);
    let ok_before = Progress::get(&ch.prog.chained_ok);
    ch.ctl.back_to_back.fetch_add(1, Ordering::SeqCst);
    let done = wait_for(rig, Duration::from_secs(120), || {
        Progress::get(&ch.prog.chained_done) > done_before
    })
    .await;
    let detail = "EOT and the next ENQ in one write".to_owned();
    if !done {
        ch.ctl.back_to_back.store(0, Ordering::SeqCst);
        return Outcome::new(detail, RECONNECT_BUDGET).note("the instrument never got to send it");
    }
    let acknowledged = Progress::get(&ch.prog.chained_ok) > ok_before;
    let happened = unix_ms().saturating_sub(21_000);
    let recovered = wait_recovered(rig, &one(ch), RECONNECT_BUDGET, Need::Flowing).await;
    let mut outcome = Outcome::new(detail, RECONNECT_BUDGET)
        .recovered(recovered)
        .owner(Owner::RoutingStore);
    if acknowledged {
        return outcome.note("the ENQ was acknowledged");
    }
    rig.fail_at(
        "astm_enq_ignored",
        Owner::RoutingStore,
        format!(
            "{}: an ENQ that arrived in the same read as the EOT of the previous transfer was never acknowledged (the instrument waited 20 s and started over); the ASTM receive driver clears its input buffer when a transfer ends (packie-astm receive.rs, finish_transfer -> reset_transfer), which drops bytes that follow the EOT",
            ch.plan.id
        ),
        happened,
    );
    outcome.flagged = true;
    outcome.note("the ENQ was ignored for 20 s")
}
