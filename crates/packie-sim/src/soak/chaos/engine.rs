//! Episodes that act on the engine: config applies, pause and resume, a kill with restart,
//! and a restart with the store blocked.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use packie_core::ChannelId;
use packie_ipc::{ChannelCommand, Request};
use serde_json::Value;

use super::Outcome;
use super::check::{Baseline, Need, all_with_baselines, view_of, wait_recovered};
use crate::soak::chan::ChannelRt;
use crate::soak::reconcile::{self, SETTLE_MS};
use crate::soak::rig::{Owner, Rig};
use crate::soak::snap;
use crate::soak::topology::{Change, runtime_document};
use crate::soak::util::{Rng, hms};

/// Budget for a channel that restarts because of an apply.
const RESTART_BUDGET: Duration = Duration::from_secs(60);
/// Budget for a channel that changes in place.
const IN_PLACE_BUDGET: Duration = Duration::from_secs(30);
/// Budget from the start of a killed engine until every channel passes traffic again.
const KILL_BUDGET: Duration = Duration::from_secs(180);
/// The engine opens the store again at most 5 minutes after the last failed attempt.
const STORE_BUDGET: Duration = Duration::from_secs(330);
/// How long the check after a restart waits for the queues to empty.
const DRAIN_WITHIN: Duration = Duration::from_secs(300);

fn pick_change(ch: &ChannelRt, rng: &mut Rng) -> Change {
    if ch.plan.kind.is_bridge() {
        if rng.chance(50) {
            Change::Label
        } else {
            Change::Replace
        }
    } else {
        [Change::Label, Change::Policy, Change::Replace][rng.index(3)]
    }
}

/// `instance_started_at` of every channel of a fresh snapshot.
async fn instances(rig: &Rig) -> BTreeMap<String, String> {
    let Some(snapshot) = rig.snapshot().await else {
        return BTreeMap::new();
    };
    snapshot["channels"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|c| {
                    (
                        snap::text(c, &["channel_id"]).to_owned(),
                        snap::text(c, &["instance_started_at"]).to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Problems in the engine's answer to an apply that changed only `changed`.
fn check_answer(answer: &Value, changed: &str, change: Change) -> Vec<String> {
    let mut problems = Vec::new();
    if answer["result"] != "applied" {
        problems.push(format!("the apply result was {}", answer["result"]));
    }
    for outcome in answer["outcomes"].as_array().cloned().unwrap_or_default() {
        let id = snap::text(&outcome, &["channel_id"]);
        let got = snap::text(&outcome, &["outcome"]);
        let want = if id == changed {
            change.expected_outcome()
        } else {
            "unchanged"
        };
        if got != want {
            problems.push(format!("{id}: expected {want}, the engine reported {got}"));
        }
    }
    problems
}

/// One channel changes through a config apply; every other channel must be left alone.
pub(crate) async fn config_apply(rig: &Rig, ch: &Arc<ChannelRt>, rng: &mut Rng) -> Outcome {
    let change = pick_change(ch, rng);
    let id = ch.plan.id.clone();
    let detail = format!(
        "{change:?} change, expected outcome {}",
        change.expected_outcome()
    );
    let window_from = rig.now_ms();
    let before = instances(rig).await;
    let mut knobs = rig.knobs.lock().map(|k| k.clone()).unwrap_or_default();
    knobs.entry(id.clone()).or_default().apply(change);
    let document = runtime_document(&rig.plans(), &knobs, &rig.doc_options);
    let note = format!("soak chaos: {change:?} change of {id}");
    let answer = apply_with_retry(rig, &document, &note).await;
    let budget = if change == Change::Replace {
        RESTART_BUDGET
    } else {
        IN_PLACE_BUDGET
    };
    let answer = match answer {
        Ok(answer) => answer,
        Err(why) => {
            rig.fail(
                "config_apply_rejected",
                Owner::Core,
                format!("applying a {change:?} change of {id} failed: {why}"),
            );
            let mut o = Outcome::new(detail, budget).note(why);
            o.flagged = true;
            return o;
        }
    };
    if let Ok(mut current) = rig.knobs.lock() {
        *current = knobs;
    }
    let base = vec![(Arc::clone(ch), Baseline::of(ch))];
    let problems = check_answer(&answer, &id, change);
    let disturbed = disturbed_channels(rig, &before, &answer, &id, window_from).await;
    let mut outcome = Outcome::new(detail, budget);
    if !problems.is_empty() || !disturbed.is_empty() {
        let all: Vec<String> = problems.into_iter().chain(disturbed).collect();
        rig.fail(
            "config_apply_outcome",
            Owner::Core,
            format!("applying a {change:?} change of {id}: {}", all.join("; ")),
        );
        outcome.flagged = true;
        outcome = outcome.note(all.join("; "));
    }
    let recovered = wait_recovered(rig, &base, budget, Need::Flowing).await;
    outcome.recovered(recovered)
}

async fn apply_with_retry(rig: &Rig, document: &Value, note: &str) -> Result<Value, String> {
    match rig.put_config(document.clone(), note).await {
        Err(why) if why.contains("onflict") => {
            rig.env.stop.sleep(Duration::from_secs(2)).await;
            rig.put_config(document.clone(), note).await
        }
        other => other,
    }
}

/// Channels the engine reported `unchanged` whose instance restarted anyway, with no other
/// episode on them.
///
/// The engine's own per-channel `outcomes` are what it did; instance times only show that
/// something else happened, and an episode that works on a channel during the window (a
/// pause and resume, a cut) restarts it on purpose. Those channels are not judged, and
/// neither are channels the engine reported as changed (the answer check covers them).
fn disturbed(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
    answer: &Value,
    changed: &str,
    touched: &std::collections::HashSet<String>,
) -> Vec<String> {
    let unchanged: Vec<&str> = answer["outcomes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| snap::text(o, &["outcome"]) == "unchanged")
        .map(|o| snap::text(o, &["channel_id"]))
        .collect();
    before
        .iter()
        .filter(|(id, started)| {
            id.as_str() != changed
                && unchanged.contains(&id.as_str())
                && !touched.contains(*id)
                && after.get(*id).is_some_and(|a| a != *started)
        })
        .map(|(id, _)| {
            format!("{id} was restarted although the engine reported it unchanged and no episode touched it")
        })
        .collect()
}

/// Waits a moment after the apply, then lists what [`disturbed`] finds. `from_ms` is the run
/// time at which the check began.
async fn disturbed_channels(
    rig: &Rig,
    before: &BTreeMap<String, String>,
    answer: &Value,
    changed: &str,
    from_ms: u64,
) -> Vec<String> {
    rig.env.stop.sleep(Duration::from_secs(3)).await;
    let after = instances(rig).await;
    let touched = rig
        .targets
        .lock()
        .map(|t| t.touched(from_ms, rig.now_ms()))
        .unwrap_or_default();
    disturbed(before, &after, answer, changed, &touched)
}

/// The state and stop reason of a channel.
async fn state_and_reason(rig: &Rig, id: &str) -> (String, String) {
    match view_of(rig, id).await {
        Some(v) => (
            snap::channel_state(&v).to_owned(),
            snap::text(&v, &["stop_reason"]).to_owned(),
        ),
        None => (String::new(), String::new()),
    }
}

async fn wait_stopped(rig: &Rig, id: &str, within: Duration) -> (String, String) {
    let started = tokio::time::Instant::now();
    loop {
        let (state, reason) = state_and_reason(rig, id).await;
        if state == "stopped"
            || started.elapsed() >= within
            || !rig.env.stop.sleep(Duration::from_millis(500)).await
        {
            return (state, reason);
        }
    }
}

/// A channel is paused for a while and resumed.
pub(crate) async fn pause_resume(rig: &Rig, ch: &Arc<ChannelRt>, rng: &mut Rng) -> Outcome {
    let secs = rng.between(10, 60);
    let detail = format!("paused for {}", hms(secs));
    let budget = Duration::from_secs(60);
    let Ok(channel_id) = ch.plan.id.parse::<ChannelId>() else {
        return Outcome::new(detail, budget).note("the channel id is not a valid id");
    };
    let command = |command| Request::ChannelCommand {
        channel_id,
        command,
    };
    if let Err(why) = rig.call(command(ChannelCommand::Pause)).await {
        rig.fail(
            "pause_rejected",
            Owner::Core,
            format!("{}: pause was rejected: {why}", ch.plan.id),
        );
        let mut o = Outcome::new(detail, budget).note(why);
        o.flagged = true;
        return o;
    }
    let (state, reason) = wait_stopped(rig, &ch.plan.id, Duration::from_secs(30)).await;
    let mut flagged = false;
    if state != "stopped" || reason != "operator_paused" {
        rig.fail(
            "pause_not_applied",
            Owner::Core,
            format!(
                "{}: 30 s after a pause the channel is `{state}` with stop reason `{reason}`",
                ch.plan.id
            ),
        );
        flagged = true;
    }
    rig.env.stop.sleep(Duration::from_secs(secs)).await;
    let resumed = rig.call(command(ChannelCommand::Resume)).await;
    let base = vec![(Arc::clone(ch), Baseline::of(ch))];
    let recovered = wait_recovered(rig, &base, budget, Need::Flowing).await;
    let mut outcome = Outcome::new(detail, budget)
        .recovered(recovered)
        .note(format!("while paused: {state}"));
    outcome.flagged = flagged || resumed.is_err();
    outcome
}

/// The text after `"message":"` in a log line.
fn message_of(line: &str) -> String {
    line.split("\"message\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("")
        .to_owned()
}

/// The last startup reconciliation the engine logged since `since_ms` (Unix time).
fn reconciliation_since(rig: &Rig, since_ms: u64) -> String {
    let Ok(mut data) = rig.monitor.lock() else {
        return String::new();
    };
    data.scan.refresh();
    data.scan
        .reconciliation
        .as_ref()
        .filter(|l| l.unix_ms >= since_ms)
        .map(|l| message_of(&l.text))
        .unwrap_or_default()
}

/// Waits for the queues to empty, then compares the peers' records.
async fn reconcile_after(rig: &Rig, label: &str) -> String {
    let drained = reconcile::wait_drained(rig, DRAIN_WITHIN).await;
    let summary = reconcile::run(rig, label, false, SETTLE_MS).await;
    let unresolved: u64 = summary.channels.iter().map(|c| c.unresolved).sum();
    format!(
        "reconciliation after {label}: {}, {unresolved} unresolved by the engine's own account{}",
        if summary.ok {
            "no message lost or duplicated"
        } else {
            "FAILED"
        },
        if drained {
            ""
        } else {
            " (queues had not emptied)"
        }
    )
}

/// The engine is killed with SIGKILL and started again.
pub(crate) async fn crash_restart(rig: &Rig, rng: &mut Rng) -> Outcome {
    let down = Duration::from_secs(rng.between(3, 15));
    let detail = format!("kill -9, restart after {}", hms(down.as_secs()));
    if let Err(why) = rig.kill_engine().await {
        return Outcome::new(detail, KILL_BUDGET).note(format!("kill failed: {why}"));
    }
    rig.env.stop.sleep(down).await;
    let since = crate::soak::util::unix_ms();
    let began = tokio::time::Instant::now();
    let ready = match rig.start_engine().await {
        Ok(took) => took,
        Err(why) => {
            rig.fail(
                "restart_failed",
                Owner::Core,
                format!("the engine did not start after a kill: {why}"),
            );
            rig.env.stop.set();
            let mut o = Outcome::new(detail, KILL_BUDGET).note(why);
            o.flagged = true;
            return o;
        }
    };
    let base = all_with_baselines(rig);
    let recovered = wait_recovered(rig, &base, KILL_BUDGET, Need::Flowing).await;
    let total = recovered.map(|_| began.elapsed());
    let engine_note = format!("engine ready after {:.1} s", ready.as_secs_f64());
    let recon = reconciliation_since(rig, since);
    let check = reconcile_after(rig, "crash restart").await;
    Outcome::new(detail, KILL_BUDGET)
        .recovered(total)
        .owner(Owner::RoutingStore)
        .note(engine_note)
        .note(recon)
        .note(check)
}

/// Moves the store aside and puts a file where the store directory belongs.
fn block_store(state: &Path) -> std::io::Result<()> {
    std::fs::rename(state.join("store"), state.join("store.aside"))?;
    std::fs::write(state.join("store"), b"blocked by the soak runner\n")
}

fn unblock_store(state: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(state.join("store"));
    if state.join("store.aside").exists() {
        std::fs::rename(state.join("store.aside"), state.join("store"))?;
    }
    Ok(())
}

/// What the engine shows while the store cannot be opened; `Err` describes what is wrong.
async fn blocked_state(rig: &Rig) -> Result<(), String> {
    let snapshot = rig.snapshot().await.ok_or("no snapshot")?;
    let store = snap::text(&snapshot["node"], &["store", "state"]);
    if store != "unavailable" {
        return Err(format!("the node reports the store as `{store}`"));
    }
    for ch in rig.channels.iter().filter(|c| !c.plan.kind.is_bridge()) {
        let view = snap::channel(&snapshot, &ch.plan.id).ok_or("a channel is missing")?;
        let (state, reason) = (
            snap::channel_state(view),
            snap::text(view, &["stop_reason"]),
        );
        if state != "stopped" || reason != "store_unavailable" {
            return Err(format!(
                "{} is `{state}` with stop reason `{reason}`",
                ch.plan.id
            ));
        }
    }
    let alerted = snapshot["alerts"].as_array().is_some_and(|a| {
        a.iter()
            .any(|x| snap::text(x, &["code"]) == "message_store_unavailable")
    });
    if alerted {
        Ok(())
    } else {
        Err("the alert message_store_unavailable is not raised".to_owned())
    }
}

async fn wait_blocked(rig: &Rig) -> Result<(), String> {
    let started = tokio::time::Instant::now();
    loop {
        match blocked_state(rig).await {
            Ok(()) => return Ok(()),
            Err(why) if started.elapsed() >= Duration::from_secs(45) => return Err(why),
            Err(_) => {
                rig.env.stop.sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Bridges that did not pass traffic since `base`.
fn stalled_bridges(base: &[(Arc<ChannelRt>, Baseline)]) -> Vec<String> {
    base.iter()
        .filter(|(ch, b)| ch.plan.kind.is_bridge() && ch.delivered() <= b.delivered())
        .map(|(ch, _)| ch.plan.id.clone())
        .collect()
}

/// The engine is killed, the store is blocked by a file, and the engine starts without it.
pub(crate) async fn store_block(rig: &Rig, rng: &mut Rng) -> Outcome {
    let hold = Duration::from_secs(rng.between(60, 150));
    let detail = format!(
        "kill -9, store blocked by a file for {}",
        hms(hold.as_secs())
    );
    let state = rig.paths.state.clone();
    if let Err(why) = rig.kill_engine().await {
        return Outcome::new(detail, STORE_BUDGET).note(format!("kill failed: {why}"));
    }
    if let Err(why) = block_store(&state) {
        let _ = unblock_store(&state);
        return Outcome::new(detail, STORE_BUDGET).note(format!("cannot block the store: {why}"));
    }
    let blocked = hold_blocked(rig, hold).await;
    let restored = unblock_store(&state);
    let began = tokio::time::Instant::now();
    let (note, flagged) = match &blocked {
        Ok(note) => (note.clone(), false),
        Err(why) => (why.clone(), true),
    };
    if let Err(why) = restored {
        rig.fail(
            "runner",
            Owner::Runner,
            format!("cannot put the store back: {why}"),
        );
        rig.env.stop.set();
    }
    if rig.pid().is_none() {
        // The engine did not start with the store blocked; start it now that it is free.
        let _ = rig.start_engine().await;
    }
    let recovered = recover_from_block(rig).await;
    let total = recovered.map(|_| began.elapsed());
    let check = reconcile_after(rig, "store block").await;
    let mut outcome = Outcome::new(detail, STORE_BUDGET)
        .recovered(total)
        .owner(Owner::RoutingStore)
        .note(note)
        .note(check);
    outcome.flagged = flagged;
    outcome
}

/// Starts the engine with the store blocked and checks what it shows until `hold` is over.
async fn hold_blocked(rig: &Rig, hold: Duration) -> Result<String, String> {
    let began = tokio::time::Instant::now();
    rig.start_engine()
        .await
        .map_err(|why| format!("the engine did not start with the store blocked: {why}"))
        .inspect_err(|why| rig.fail("store_block_start", Owner::Core, why.clone()))?;
    if let Err(why) = wait_blocked(rig).await {
        rig.fail(
            "store_block_state",
            Owner::RoutingStore,
            format!("with the store blocked: {why}"),
        );
        return Err(why);
    }
    let base = all_with_baselines(rig);
    let left = hold.saturating_sub(began.elapsed());
    rig.env.stop.sleep(left).await;
    let stalled = stalled_bridges(&base);
    if !stalled.is_empty() {
        let why = format!(
            "bridges stopped passing traffic while the store was blocked: {}",
            stalled.join(", ")
        );
        rig.fail(
            "bridge_stopped_without_store",
            Owner::RoutingStore,
            why.clone(),
        );
        return Err(why);
    }
    Ok("while blocked: message channels stopped (store_unavailable), alert raised, every bridge kept passing traffic".to_owned())
}

/// After the file is gone the engine must open the store, start the message channels and
/// clear the alert.
async fn recover_from_block(rig: &Rig) -> Option<Duration> {
    let started = tokio::time::Instant::now();
    loop {
        let base_ready = rig.snapshot().await.is_some_and(|s| {
            snap::text(&s["node"], &["store", "state"]) == "ok"
                && snap::channel_states(&s)
                    .iter()
                    .all(|(_, state)| state == "running")
                && !s["alerts"].as_array().is_some_and(|a| {
                    a.iter()
                        .any(|x| snap::text(x, &["code"]) == "message_store_unavailable")
                })
        });
        if base_ready {
            let base = all_with_baselines(rig);
            let left = STORE_BUDGET.saturating_sub(started.elapsed());
            return wait_recovered(rig, &base, left.max(Duration::from_secs(30)), Need::Flowing)
                .await
                .map(|_| started.elapsed());
        }
        if started.elapsed() >= STORE_BUDGET || !rig.env.stop.sleep(Duration::from_secs(2)).await {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn instances_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    fn answer_with(unchanged: &[&str], changed: &str) -> Value {
        let mut outcomes: Vec<Value> = unchanged
            .iter()
            .map(|id| json!({ "channel_id": id, "outcome": "unchanged" }))
            .collect();
        outcomes.push(json!({ "channel_id": changed, "outcome": "updated" }));
        json!({ "result": "applied", "outcomes": outcomes })
    }

    #[test]
    fn a_pause_and_resume_during_an_apply_is_not_a_side_effect() {
        // Episode 103: hl7-04 was changed in place, hl7-07 was paused and resumed by another
        // episode in the same window, so its instance restarted. The engine restarted nothing.
        let before = instances_of(&[("hl7-04", "t1"), ("hl7-07", "t1"), ("hl7-01", "t1")]);
        let after = instances_of(&[("hl7-04", "t1"), ("hl7-07", "t2"), ("hl7-01", "t1")]);
        let answer = answer_with(&["hl7-07", "hl7-01"], "hl7-04");
        let touched: std::collections::HashSet<String> = ["hl7-07".to_owned()].into();
        assert!(disturbed(&before, &after, &answer, "hl7-04", &touched).is_empty());
    }

    #[test]
    fn an_unchanged_channel_that_restarts_with_no_episode_on_it_is_a_side_effect() {
        let before = instances_of(&[("hl7-04", "t1"), ("hl7-07", "t1"), ("hl7-01", "t1")]);
        let after = instances_of(&[("hl7-04", "t1"), ("hl7-07", "t2"), ("hl7-01", "t1")]);
        let answer = answer_with(&["hl7-07", "hl7-01"], "hl7-04");
        // Another episode on a different channel does not excuse it.
        let touched: std::collections::HashSet<String> = ["hl7-01".to_owned()].into();
        let found = disturbed(&before, &after, &answer, "hl7-04", &touched);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("hl7-07 "));
    }

    #[test]
    fn only_channels_the_engine_called_unchanged_are_judged_by_their_instance() {
        let before = instances_of(&[("hl7-04", "t1"), ("hl7-07", "t1")]);
        let after = instances_of(&[("hl7-04", "t2"), ("hl7-07", "t2")]);
        // hl7-07 is reported restarted: the answer check owns that, not this one. The
        // changed channel itself is never listed.
        let answer = json!({ "result": "applied", "outcomes": [
            { "channel_id": "hl7-04", "outcome": "restarted" },
            { "channel_id": "hl7-07", "outcome": "restarted" } ] });
        assert!(disturbed(&before, &after, &answer, "hl7-04", &Default::default()).is_empty());
        assert_eq!(check_answer(&answer, "hl7-04", Change::Replace).len(), 1);
    }

    #[test]
    fn an_apply_must_change_only_the_named_channel() {
        let answer = json!({ "result": "applied", "outcomes": [
            { "channel_id": "a", "outcome": "restarted" },
            { "channel_id": "b", "outcome": "unchanged" } ] });
        assert!(check_answer(&answer, "a", Change::Replace).is_empty());
        let wrong = json!({ "result": "applied", "outcomes": [
            { "channel_id": "a", "outcome": "updated" },
            { "channel_id": "b", "outcome": "restarted" } ] });
        assert_eq!(check_answer(&wrong, "a", Change::Replace).len(), 2);
        let partial = json!({ "result": "partial", "outcomes": [] });
        assert_eq!(check_answer(&partial, "a", Change::Label).len(), 1);
    }

    #[test]
    fn the_reconciliation_message_is_found_in_a_log_line() {
        let line = r#"{"timestamp":"t","level":"INFO","message":"Startup reconciliation: 0 recovered, 1 aborted","event":"x"}"#;
        assert_eq!(
            message_of(line),
            "Startup reconciliation: 0 recovered, 1 aborted"
        );
        assert_eq!(message_of("no message"), "");
    }

    #[test]
    fn blocking_the_store_replaces_its_directory_by_a_file_and_back() {
        let dir = tempfile::tempdir().map(|d| d.keep()).unwrap_or_default();
        let _ = std::fs::create_dir_all(dir.join("store"));
        let _ = std::fs::write(dir.join("store/messages.sqlite3"), b"x");
        assert!(block_store(&dir).is_ok());
        assert!(dir.join("store").is_file());
        assert!(unblock_store(&dir).is_ok());
        assert!(dir.join("store/messages.sqlite3").is_file());
        assert!(!dir.join("store.aside").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
