//! The assertions of a soak run, evaluated over what was recorded.

use super::analysis::{
    self, Judged, SnapshotTiming, counter_issues, judge_fds, judge_memory, snapshot_timing,
    stretches,
};
use super::chaos::Episode;
use super::chaos::plan::Kind;
use super::monitor::MonitorData;
use super::reconcile::Summary;
use super::rig::Anomaly;

/// The snapshot may be this late at the 99th percentile, milliseconds.
pub(crate) const MAX_SNAPSHOT_AGE_MS: u64 = 3_000;
/// No two snapshot publications may be further apart, milliseconds.
pub(crate) const MAX_SNAPSHOT_GAP_MS: u64 = 6_000;
/// Seconds after the run's start and after each engine start that memory and descriptors
/// are not judged.
pub(crate) const RESIDENT_WARMUP_S: u64 = 900;
/// The log ring never holds more events than this (the configured size).
pub(crate) const RING_EVENTS: u64 = 20_000;

/// One assertion and what was observed.
#[derive(Debug, Clone)]
pub(crate) struct Check {
    pub(crate) name: &'static str,
    pub(crate) pass: bool,
    pub(crate) detail: String,
}

/// The state of the engine after the final drain.
#[derive(Debug, Clone, Default)]
pub(crate) struct FinalState {
    pub(crate) channels: usize,
    pub(crate) running: usize,
    pub(crate) store_ok: bool,
    pub(crate) queued: u64,
    pub(crate) node_state: String,
}

/// What the assertions look at.
pub(crate) struct Facts<'a> {
    pub(crate) data: &'a MonitorData,
    pub(crate) episodes: &'a [Episode],
    pub(crate) recons: &'a [Summary],
    pub(crate) anomalies: &'a [Anomaly],
    pub(crate) engine_failures: &'a [String],
    pub(crate) final_state: &'a FinalState,
    pub(crate) chaos_level: u8,
    pub(crate) warmup_s: u64,
}

fn fatal(anomalies: &[Anomaly], categories: &[&str]) -> Vec<String> {
    anomalies
        .iter()
        .filter(|a| a.fatal && categories.contains(&a.category))
        .map(|a| format!("{} (x{})", a.summary, a.count))
        .collect()
}

fn verdict(name: &'static str, problems: &[String], ok: String) -> Check {
    if problems.is_empty() {
        Check {
            name,
            pass: true,
            detail: ok,
        }
    } else {
        Check {
            name,
            pass: false,
            detail: problems.join(" | "),
        }
    }
}

fn judged_text(judged: &[Judged]) -> (Vec<String>, String) {
    let line = |j: &Judged| match j.growth {
        Some(g) => format!("{}: {} -> {} ({:+}%)", j.what, g.early, g.late, g.percent()),
        None => format!("{}: too few samples to judge", j.what),
    };
    let bad = judged.iter().filter(|j| !j.ok).map(line).collect();
    let whole = judged.last().map(line).unwrap_or_default();
    (bad, whole)
}

fn memory(f: &Facts<'_>) -> (Check, Check) {
    // The engine's memory settles as its caches fill: judge it 15 minutes after each start.
    let s = stretches(
        &f.data.samples,
        f.warmup_s.max(RESIDENT_WARMUP_S),
        RESIDENT_WARMUP_S,
    );
    let (bad, whole) = judged_text(&judge_memory(&s));
    let rss = verdict(
        "Memory does not grow beyond the bound after warm-up",
        &bad,
        format!(
            "{whole} (KiB of footprint, the memory the process is responsible for; the resident size is recorded next to it); bound {}%",
            analysis::MEMORY_GROWTH_PERCENT
        ),
    );
    let (bad, whole) = judged_text(&judge_fds(&s));
    let fds = verdict("No descriptor leak", &bad, whole);
    (rss, fds)
}

fn episodes(f: &Facts<'_>) -> Check {
    let late: Vec<String> = f
        .episodes
        .iter()
        .filter(|e| !e.recovered)
        .map(|e| {
            format!(
                "#{} {} on {} ({}): {}",
                e.id,
                e.kind,
                e.target,
                e.detail,
                e.recovered_ms
                    .map_or("not recovered".to_owned(), |ms| format!(
                        "recovered after {} s, budget {} s",
                        ms / 1000,
                        e.budget_ms / 1000
                    ))
            )
        })
        .collect();
    let missing: Vec<&str> = if f.chaos_level == 0 {
        Vec::new()
    } else {
        Kind::ALL
            .iter()
            .filter(|k| !f.episodes.iter().any(|e| e.kind == k.name()))
            .map(|k| k.name())
            .collect()
    };
    let mut problems = late;
    problems.extend(fatal(f.anomalies, &["episode_not_recovered"]));
    problems.dedup();
    let ok = format!(
        "{} episodes, all recovered within their budgets{}",
        f.episodes.len(),
        if missing.is_empty() {
            String::new()
        } else {
            format!(" (not exercised in this run: {})", missing.join(", "))
        }
    );
    verdict(
        "Every chaos episode recovers within its budget",
        &problems,
        ok,
    )
}

fn reconciliation(f: &Facts<'_>) -> (Check, Check) {
    let cats = [
        "message_lost",
        "duplicate_at_peer",
        "corrupt_message",
        "delivery_stuck",
    ];
    let mut problems = fatal(f.anomalies, &cats);
    let last = f.recons.last();
    if last.is_none() {
        problems.push("no reconciliation ran".to_owned());
    }
    let (acked, received, unresolved) = last.map_or((0, 0, 0), |s| {
        (
            s.channels.iter().map(|c| c.acked).sum::<u64>(),
            s.channels.iter().map(|c| c.received).sum::<u64>(),
            s.channels.iter().map(|c| c.unresolved).sum::<u64>(),
        )
    });
    let messages = verdict(
        "No message lost or duplicated at a receiving peer",
        &problems,
        format!(
            "{} reconciliations; at the end {acked} messages acknowledged to sources, {received} received by peers, {unresolved} reported unresolved by the engine itself (outcome unknown after a kill or a dropped connection)",
            f.recons.len()
        ),
    );
    let mut problems = fatal(f.anomalies, &["bridge_integrity", "bridge_loss"]);
    problems.dedup();
    let (sent, got, lost) = last.map_or((0, 0, 0), |s| {
        (
            s.bridges.iter().map(|b| b.sent).sum::<u64>(),
            s.bridges.iter().map(|b| b.received).sum::<u64>(),
            s.bridges.iter().map(|b| b.lost_at_break).sum::<u64>(),
        )
    });
    let bridges = verdict(
        "Bridges deliver every record intact, in order, without duplicates",
        &problems,
        format!(
            "{sent} records written, {got} read by sinks, {lost} lost with a connection break nearby (expected); none unexplained"
        ),
    );
    (messages, bridges)
}

fn counters(f: &Facts<'_>) -> Check {
    let issues = counter_issues(&f.data.chan_samples);
    let problems: Vec<String> = issues
        .iter()
        .take(10)
        .map(|i| format!("{} {}..{} s: {}", i.channel, i.from_s, i.to_s, i.what))
        .collect();
    verdict(
        "Engine counters agree with the peers' records",
        &problems,
        format!(
            "{} channel samples compared; message counters within {} of the peers, bridge bytes within {} KiB",
            f.data.chan_samples.len(),
            analysis::COUNTER_TOLERANCE,
            analysis::BYTE_SLACK / 1024
        ),
    )
}

fn snapshots(f: &Facts<'_>) -> Check {
    let t: SnapshotTiming = snapshot_timing(&f.data.samples);
    let mut problems = Vec::new();
    if t.worst_age_p99_ms > MAX_SNAPSHOT_AGE_MS {
        problems.push(format!(
            "a window's 99th percentile snapshot age was {} ms (limit {MAX_SNAPSHOT_AGE_MS})",
            t.worst_age_p99_ms
        ));
    }
    if t.worst_gap_ms > MAX_SNAPSHOT_GAP_MS {
        problems.push(format!(
            "two publications were {} ms apart (limit {MAX_SNAPSHOT_GAP_MS})",
            t.worst_gap_ms
        ));
    }
    verdict(
        "Snapshots are published on time",
        &problems,
        format!(
            "{} windows; worst p99 age {} ms, worst gap {} ms, worst request round trip {} ms",
            t.windows, t.worst_age_p99_ms, t.worst_gap_ms, t.worst_rtt_ms
        ),
    )
}

fn alerts(f: &Facts<'_>) -> Check {
    let unexplained: Vec<String> = f
        .anomalies
        .iter()
        .filter(|a| a.category == "alert_unexplained")
        .map(|a| a.summary.clone())
        .collect();
    verdict(
        "No alert is raised without a cause",
        &unexplained,
        "every watched alert coincided with a chaos episode or an engine start".to_owned(),
    )
}

fn applies(f: &Facts<'_>) -> Check {
    let n = f
        .episodes
        .iter()
        .filter(|e| e.kind == "config_apply")
        .count();
    let problems = fatal(
        f.anomalies,
        &["config_apply_outcome", "config_apply_rejected"],
    );
    verdict(
        "A config apply changes only the named channel, with the stated outcome",
        &problems,
        format!(
            "{n} applies; each reported `updated` or `restarted` for its channel and `unchanged` for every other, and no other channel restarted"
        ),
    )
}

fn faults(f: &Facts<'_>) -> Check {
    let cats = [
        "failed_while_waiting",
        "partial_file_read",
        "pause_not_applied",
        "pause_rejected",
        "store_block_start",
        "store_block_state",
        "bridge_stopped_without_store",
        "restart_failed",
        "astm_enq_ignored",
        "source_no_answer",
        "session_held_after_cut",
        "inflight_cut_state",
    ];
    verdict(
        "The engine reacts to each fault as specified",
        &fatal(f.anomalies, &cats),
        "deliveries waited instead of failing, partial files were not read, pause stopped the channel, a blocked store left bridges running".to_owned(),
    )
}

fn stability(f: &Facts<'_>) -> (Check, Check) {
    let mut problems = fatal(f.anomalies, &["engine_exit"]);
    if f.data.unexpected_exits > 0 {
        problems.push(format!("{} unplanned exit(s)", f.data.unexpected_exits));
    }
    let up = verdict(
        "The engine stays up except where the soak kills it",
        &problems,
        "no unplanned exit".to_owned(),
    );
    let mut problems: Vec<String> = f
        .data
        .scan
        .panics
        .iter()
        .map(|p| format!("log: {p}"))
        .collect();
    problems.extend(
        f.engine_failures
            .iter()
            .filter(|m| m.contains("panick"))
            .cloned(),
    );
    let panic = verdict(
        "No `panicked` in the engine's logs or output",
        &problems,
        format!(
            "{} MiB of engine log scanned, no line mentions a panic",
            f.data.scan.bytes >> 20
        ),
    );
    (up, panic)
}

fn healthy_end(f: &Facts<'_>) -> Check {
    let s = f.final_state;
    let mut problems = Vec::new();
    if s.running != s.channels {
        problems.push(format!("{} of {} channels run", s.running, s.channels));
    }
    if !s.store_ok {
        problems.push("the store is not ok".to_owned());
    }
    if s.queued > 0 {
        problems.push(format!("{} deliveries are still queued", s.queued));
    }
    verdict(
        "The engine ends healthy and drained",
        &problems,
        format!(
            "{} of {} channels running, store ok, nothing queued, node `{}`",
            s.running, s.channels, s.node_state
        ),
    )
}

fn bounded_logs(f: &Facts<'_>) -> Check {
    let mut problems = Vec::new();
    if f.data.ring_max > RING_EVENTS {
        problems.push(format!(
            "the log ring held {} events, more than its size {RING_EVENTS}",
            f.data.ring_max
        ));
    }
    let files = f
        .data
        .samples
        .iter()
        .map(|s| s.log_files)
        .max()
        .unwrap_or(0);
    if files > 60 {
        problems.push(format!("{files} log files exist"));
    }
    verdict(
        "The log ring and the log files stay bounded",
        &problems,
        format!(
            "ring peak {} of {RING_EVENTS} events, at most {files} log files",
            f.data.ring_max
        ),
    )
}

fn leftovers(f: &Facts<'_>) -> Check {
    const COVERED: &[&str] = &[
        "inflight_cut_state",
        "source_no_answer",
        "session_held_after_cut",
        "engine_exit",
        "restart_failed",
        "episode_not_recovered",
        "message_lost",
        "duplicate_at_peer",
        "corrupt_message",
        "delivery_stuck",
        "bridge_integrity",
        "bridge_loss",
        "config_apply_outcome",
        "config_apply_rejected",
        "failed_while_waiting",
        "partial_file_read",
        "pause_not_applied",
        "pause_rejected",
        "store_block_start",
        "store_block_state",
        "bridge_stopped_without_store",
        "alert_unexplained",
        "down_not_noticed",
        "astm_enq_ignored",
    ];
    let other: Vec<String> = f
        .anomalies
        .iter()
        .filter(|a| a.fatal && !COVERED.contains(&a.category))
        .map(|a| format!("[{}] {}", a.category, a.summary))
        .collect();
    verdict(
        "No other failure was recorded",
        &other,
        format!(
            "{} anomalies in total, {} of them failures",
            f.anomalies.len(),
            f.anomalies.iter().filter(|a| a.fatal).count()
        ),
    )
}

/// Evaluates every assertion.
pub(crate) fn evaluate(f: &Facts<'_>) -> Vec<Check> {
    let (up, panic) = stability(f);
    let (rss, fds) = memory(f);
    let (messages, bridges) = reconciliation(f);
    vec![
        up,
        panic,
        rss,
        fds,
        episodes(f),
        faults(f),
        messages,
        bridges,
        counters(f),
        snapshots(f),
        alerts(f),
        applies(f),
        healthy_end(f),
        bounded_logs(f),
        leftovers(f),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::probe::Fds;
    use super::super::rig::Owner;
    use super::super::samples::Sample;
    use super::*;

    fn anomaly(category: &'static str, fatal: bool) -> Anomaly {
        Anomaly {
            at_ms: 1,
            unix_ms: 1,
            category,
            owner: Owner::RoutingStore,
            summary: format!("{category} happened"),
            count: 1,
            fatal,
        }
    }

    fn facts<'a>(
        data: &'a MonitorData,
        anomalies: &'a [Anomaly],
        state: &'a FinalState,
    ) -> Facts<'a> {
        Facts {
            data,
            episodes: &[],
            recons: &[],
            anomalies,
            engine_failures: &[],
            final_state: state,
            chaos_level: 0,
            warmup_s: 0,
        }
    }

    fn healthy() -> FinalState {
        FinalState {
            channels: 30,
            running: 30,
            store_ok: true,
            queued: 0,
            node_state: "healthy".to_owned(),
        }
    }

    fn by_name<'a>(checks: &'a [Check], name: &str) -> &'a Check {
        checks
            .iter()
            .find(|c| c.name.contains(name))
            .expect("check")
    }

    #[test]
    fn a_lost_message_fails_the_reconciliation_check_only() {
        let data = MonitorData::default();
        let anomalies = [anomaly("message_lost", true)];
        let state = healthy();
        let checks = evaluate(&facts(&data, &anomalies, &state));
        assert!(!by_name(&checks, "No message lost").pass);
        assert!(by_name(&checks, "Bridges").pass);
        assert!(by_name(&checks, "No other failure").pass);
    }

    #[test]
    fn an_unknown_category_fails_the_catch_all() {
        let data = MonitorData::default();
        let anomalies = [anomaly("something_new", true)];
        let state = healthy();
        let checks = evaluate(&facts(&data, &anomalies, &state));
        assert!(!by_name(&checks, "No other failure").pass);
    }

    #[test]
    fn a_panic_in_the_logs_is_found() {
        let mut data = MonitorData::default();
        data.scan.panics.push("thread panicked at".to_owned());
        let state = healthy();
        let checks = evaluate(&facts(&data, &[], &state));
        assert!(!by_name(&checks, "panicked").pass);
    }

    #[test]
    fn an_undrained_end_state_fails() {
        let data = MonitorData::default();
        let state = FinalState {
            running: 29,
            queued: 4,
            ..healthy()
        };
        let checks = evaluate(&facts(&data, &[], &state));
        assert!(!by_name(&checks, "ends healthy").pass);
    }

    #[test]
    fn slow_snapshots_fail_their_check() {
        let mut data = MonitorData::default();
        data.samples.push(Sample {
            up: true,
            snapshots: 30,
            age_p99_ms: 9_000,
            gap_max_ms: 1_000,
            fds: Fds::default(),
            ..Sample::default()
        });
        let state = healthy();
        let checks = evaluate(&facts(&data, &[], &state));
        assert!(!by_name(&checks, "Snapshots").pass);
    }

    #[test]
    fn a_clean_run_passes_everything_it_can_judge() {
        let data = MonitorData::default();
        let state = healthy();
        let mut checks = evaluate(&facts(&data, &[], &state));
        checks.retain(|c| !c.name.contains("No message lost"));
        assert!(checks.iter().all(|c| c.pass), "{checks:?}");
    }
}
