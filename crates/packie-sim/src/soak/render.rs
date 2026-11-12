//! `report.md`: the assertions, the chaos episodes, the reconciliations, graphs of the
//! sampled series and every anomaly with the engine log around it.

use std::fmt::Write;
use std::path::PathBuf;

use super::chaos::plan::Planned;
use super::checks::{Check, Facts};
use super::graphs::graph;
use super::logscan::{self, Line};
use super::reconcile::Summary;
use super::rig::Anomaly;
use super::util::{hms, median, utc};

/// How many episodes the report lists one by one.
const LISTED_EPISODES: usize = 120;
/// Log lines quoted per anomaly.
const EXCERPT_LINES: usize = 14;

/// The engine under test.
#[derive(Debug, Clone, Default)]
pub(crate) struct EngineInfo {
    pub(crate) path: PathBuf,
    pub(crate) copy: PathBuf,
    pub(crate) version: String,
    pub(crate) bytes: u64,
}

/// Everything the report is built from.
pub(crate) struct Inputs<'a> {
    pub(crate) finished: bool,
    pub(crate) started_unix_ms: u64,
    pub(crate) ended_unix_ms: u64,
    pub(crate) elapsed_s: u64,
    pub(crate) planned_s: u64,
    pub(crate) chaos_level: u8,
    pub(crate) seed: u64,
    pub(crate) command: String,
    pub(crate) host: String,
    pub(crate) engine: &'a EngineInfo,
    pub(crate) plan: &'a [Planned],
    pub(crate) facts: &'a Facts<'a>,
    pub(crate) census: Option<&'a super::census::Census>,
    pub(crate) checks: &'a [Check],
    pub(crate) out: &'a std::path::Path,
    pub(crate) logs: &'a std::path::Path,
    pub(crate) load: String,
}

fn w(out: &mut String, text: impl AsRef<str>) {
    let _ = writeln!(out, "{}", text.as_ref());
}

/// Whether every assertion passed.
pub(crate) fn passed(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.pass)
}

/// The whole report.
pub(crate) fn render(i: &Inputs<'_>) -> String {
    let mut out = String::new();
    header(&mut out, i);
    assertions(&mut out, i);
    findings(&mut out, i);
    episodes(&mut out, i);
    reconciliations(&mut out, i);
    resources(&mut out, i);
    engine_log(&mut out, i);
    store(&mut out, i);
    anomalies(&mut out, i);
    plan(&mut out, i);
    files(&mut out, i);
    out
}

fn header(out: &mut String, i: &Inputs<'_>) {
    let status = match (i.finished, passed(i.checks)) {
        (false, _) => "RUNNING (interim report)",
        (true, true) => "PASS",
        (true, false) => "FAIL",
    };
    w(out, "# Soak test report\n");
    w(out, format!("**Result: {status}**\n"));
    w(out, "| | |\n| --- | --- |");
    let rows = [
        ("Started", utc(i.started_unix_ms)),
        ("Ended", utc(i.ended_unix_ms)),
        (
            "Load ran for",
            format!("{} of {} planned", hms(i.elapsed_s), hms(i.planned_s)),
        ),
        (
            "Chaos level / seed",
            format!("{} / {}", i.chaos_level, i.seed),
        ),
        ("Load", i.load.clone()),
        (
            "Engine",
            format!(
                "{} ({}), {} MiB, copied to `{}`",
                i.engine.path.display(),
                i.engine.version,
                i.engine.bytes >> 20,
                i.engine.copy.display()
            ),
        ),
        ("Host", i.host.clone()),
        ("Command", format!("`{}`", i.command)),
    ];
    for (k, v) in rows {
        w(out, format!("| {k} | {v} |"));
    }
    w(out, "");
}

fn assertions(out: &mut String, i: &Inputs<'_>) {
    w(out, "## Assertions\n");
    w(
        out,
        "| # | Assertion | Result | Evidence |\n| --- | --- | --- | --- |",
    );
    for (n, c) in i.checks.iter().enumerate() {
        let result = if c.pass { "pass" } else { "**FAIL**" };
        w(
            out,
            format!(
                "| {} | {} | {result} | {} |",
                n + 1,
                c.name,
                c.detail.replace('|', "/")
            ),
        );
    }
    w(out, "");
}

/// The anomalies grouped by who should look at them.
fn findings(out: &mut String, i: &Inputs<'_>) {
    let list = i.facts.anomalies;
    if list.is_empty() {
        return;
    }
    w(out, "## Findings by owner\n");
    w(
        out,
        "| Owner | Kind | Failure | Times | First line |\n| --- | --- | --- | --- | --- |",
    );
    let mut sorted: Vec<&Anomaly> = list.iter().collect();
    sorted.sort_by_key(|a| (a.owner.label(), !a.fatal, a.category));
    for a in sorted {
        let first: String = a.summary.chars().take(150).collect();
        w(
            out,
            format!(
                "| {} | `{}` | {} | {} | {} |",
                a.owner.label(),
                a.category,
                if a.fatal { "yes" } else { "no" },
                a.count,
                first.replace('|', "/")
            ),
        );
    }
    w(
        out,
        "\nThe details and the engine log around each are under Anomalies below.\n",
    );
}

fn episodes(out: &mut String, i: &Inputs<'_>) {
    let list = i.facts.episodes;
    w(out, "## Chaos episodes\n");
    if list.is_empty() {
        w(out, "None (chaos level 0, or the run was too short).\n");
        return;
    }
    w(
        out,
        "| Kind | Count | Recovered | Median recovery (s) | Slowest (s) | Budget (s) |\n| --- | --- | --- | --- | --- | --- |",
    );
    let mut kinds: Vec<&str> = list.iter().map(|e| e.kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    for kind in kinds {
        let of: Vec<_> = list.iter().filter(|e| e.kind == kind).collect();
        let times: Vec<u64> = of
            .iter()
            .filter_map(|e| e.recovered_ms)
            .map(|ms| ms / 1000)
            .collect();
        let budget = of.iter().map(|e| e.budget_ms).max().unwrap_or(0) / 1000;
        w(
            out,
            format!(
                "| {kind} | {} | {} | {} | {} | {budget} |",
                of.len(),
                of.iter().filter(|e| e.ok).count(),
                median(&times),
                times.iter().max().unwrap_or(&0)
            ),
        );
    }
    w(out, "");
    w(
        out,
        "| # | Run time | Kind | Target | What | Recovery / budget (s) | Result | Note |\n| --- | --- | --- | --- | --- | --- | --- | --- |",
    );
    for e in list
        .iter()
        .filter(|e| !e.ok)
        .chain(list.iter().filter(|e| e.ok))
        .take(LISTED_EPISODES)
    {
        let rec = e
            .recovered_ms
            .map_or("-".to_owned(), |ms| (ms / 1000).to_string());
        w(
            out,
            format!(
                "| {} | {} | {} | {} | {} | {rec} / {} | {} | {} |",
                e.id,
                hms(e.start_ms / 1000),
                e.kind,
                e.target,
                e.detail,
                e.budget_ms / 1000,
                if e.ok { "ok" } else { "**FAILED**" },
                e.note.replace('|', "/")
            ),
        );
    }
    if list.len() > LISTED_EPISODES {
        w(
            out,
            format!(
                "\nOnly {LISTED_EPISODES} of {} episodes are listed (failed ones first); all are in `chaos.jsonl`.",
                list.len()
            ),
        );
    }
    w(out, "");
}

fn reconciliations(out: &mut String, i: &Inputs<'_>) {
    w(out, "## Reconciliation of sent and received messages\n");
    let recons = i.facts.recons;
    w(
        out,
        "| After | Run time | Messages acked | Received by peers | Unresolved (engine says so) | Lost | Duplicated | Result |\n| --- | --- | --- | --- | --- | --- | --- | --- |",
    );
    for s in recons {
        w(out, summary_row(s));
    }
    w(out, "");
    if let Some(last) = recons.last() {
        final_tables(out, last);
    }
}

fn summary_row(s: &Summary) -> String {
    let sum = |f: fn(&super::reconcile::ChannelRecon) -> u64| s.channels.iter().map(f).sum::<u64>();
    format!(
        "| {} | {} | {} | {} | {} | {} | {} | {} |",
        s.label,
        hms(s.at_ms / 1000),
        sum(|c| c.acked),
        sum(|c| c.received),
        sum(|c| c.unresolved),
        sum(|c| c.lost),
        sum(|c| c.duplicates),
        if s.ok { "ok" } else { "**FAILED**" }
    )
}

fn final_tables(out: &mut String, s: &Summary) {
    w(
        out,
        format!(
            "Message channels at the last check (`{}`), compared by control id:\n",
            s.label
        ),
    );
    w(
        out,
        "| Channel | Acked to source | Received by peer | Unresolved | of which discarded by peer | Lost | Duplicated | Damaged | Not yet judged |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    );
    for c in &s.channels {
        w(
            out,
            format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                c.channel,
                c.acked,
                c.received,
                c.unresolved,
                c.discarded,
                c.lost,
                c.duplicates,
                c.corrupt,
                c.pending
            ),
        );
    }
    w(out, "\nBridges at the last check:\n");
    w(
        out,
        "| Bridge | Records sent | Read by sink | Lost at a connection break | Lost unexplained | Duplicated | Reordered | Damaged |\n| --- | --- | --- | --- | --- | --- | --- | --- |",
    );
    for b in &s.bridges {
        w(
            out,
            format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |",
                b.channel,
                b.sent,
                b.received,
                b.lost_at_break,
                b.lost_unexplained,
                b.duplicates,
                b.out_of_order,
                b.corrupt
            ),
        );
    }
    w(out, "");
}

fn resources(out: &mut String, i: &Inputs<'_>) {
    let samples = &i.facts.data.samples;
    w(out, "## Resources\n");
    w(
        out,
        format!(
            "{} samples, one every {} s; the full series are in `samples.csv` (engine process, store, logs, snapshot timing, totals) and `channels.csv` (every channel's counters next to its peers' counts). In the graphs each character is the largest value of its share of the run; `K` kill, `S` blocked store, `D` destination down, `P` pause, `A` config apply.\n",
            samples.len(),
            samples
                .get(1)
                .map_or(0, |s| s.t_s.saturating_sub(samples[0].t_s))
        ),
    );
    w(out, "```");
    let series = |f: fn(&super::samples::Sample) -> u64| -> Vec<u64> {
        samples.iter().filter(|s| s.up).map(f).collect()
    };
    let total = i.elapsed_s.max(1);
    let e = i.facts.episodes;
    let rows: [(&str, &str, Vec<u64>); 10] = [
        (
            "Engine memory footprint (judged)",
            "MiB",
            series(|s| s.memory_kib() >> 10),
        ),
        (
            "Engine RSS (resident, secondary)",
            "MiB",
            series(|s| s.rss_kib >> 10),
        ),
        ("Engine CPU", "percent", series(|s| s.cpu_tenths / 10)),
        ("Open descriptors", "count", series(|s| s.fds.total)),
        ("Store size", "MiB", series(|s| s.store_bytes >> 20)),
        ("Log files", "MiB", series(|s| s.logs_bytes >> 20)),
        ("Snapshot age p99", "ms", series(|s| s.age_p99_ms)),
        ("Snapshot request", "ms", series(|s| s.rtt_ms)),
        ("End to end latency p99", "ms", series(|s| s.e2e_p99_ms)),
        (
            "Deliveries queued",
            "count",
            series(|s| s.deliveries_queued),
        ),
    ];
    for (label, unit, values) in &rows {
        out.push_str(&graph(label, unit, values, e, total));
    }
    w(out, "```\n");
    growth_rates(out, i);
}

fn growth_rates(out: &mut String, i: &Inputs<'_>) {
    let s = &i.facts.data.samples;
    let (Some(first), Some(last)) = (s.first(), s.last()) else {
        return;
    };
    let per_hour = |a: u64, b: u64| {
        let tenths = (b.saturating_sub(a) >> 20) * 36_000 / i.elapsed_s.max(1);
        format!("{}.{}", tenths / 10, tenths % 10)
    };
    w(
        out,
        format!(
            "Store growth {} MiB per hour (now {} MiB, {} messages stored); engine log growth {} MiB per hour in {} file(s); {} messages and {} bridge bytes sent by the peers.\n",
            per_hour(first.store_bytes, last.store_bytes),
            last.store_bytes >> 20,
            last.messages_stored,
            per_hour(first.logs_bytes, last.logs_bytes),
            last.log_files,
            last.acked,
            last.bytes_sent
        ),
    );
}

fn engine_log(out: &mut String, i: &Inputs<'_>) {
    let scan = &i.facts.data.scan;
    w(out, "## Engine log\n");
    w(
        out,
        format!(
            "{} MiB scanned: {} INFO, {} WARN, {} ERROR lines, {} lines mentioning a panic.\n",
            scan.bytes >> 20,
            scan.level_total("INFO"),
            scan.level_total("WARN"),
            scan.level_total("ERROR"),
            scan.panics.len()
        ),
    );
    let mut top: Vec<(&(String, String), &u64)> = scan
        .counts
        .iter()
        .filter(|((l, _), _)| l == "WARN" || l == "ERROR")
        .collect();
    top.sort_by(|a, b| b.1.cmp(a.1));
    if top.is_empty() {
        return;
    }
    w(out, "| Level | Event | Lines |\n| --- | --- | --- |");
    for ((level, event), n) in top.into_iter().take(15) {
        w(out, format!("| {level} | {event} | {n} |"));
    }
    w(out, "");
}

fn store(out: &mut String, i: &Inputs<'_>) {
    let Some(census) = i.census else { return };
    w(out, "## Store and retention\n");
    let states: Vec<String> = census
        .by_state
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(s, n)| format!("{s} {n}"))
        .collect();
    w(
        out,
        format!(
            "Messages in the store at the end, by state: {}.\n",
            states.join(", ")
        ),
    );
    w(
        out,
        "| Channel | Stored | Recorded by a tap | Purged by retention |\n| --- | --- | --- | --- |",
    );
    for c in &census.channels {
        w(
            out,
            format!(
                "| {} | {} | {} | {} |",
                c.channel, c.stored, c.observed, c.purged
            ),
        );
    }
    w(out, "");
}

fn anomalies(out: &mut String, i: &Inputs<'_>) {
    let list = i.facts.anomalies;
    w(out, "## Anomalies\n");
    if list.is_empty() {
        w(out, "None.\n");
        return;
    }
    let windows: Vec<(u64, u64)> = list
        .iter()
        .map(|a| (a.unix_ms.saturating_sub(10_000), a.unix_ms + 20_000))
        .collect();
    let found = if i.finished {
        logscan::excerpts(i.logs, &windows)
    } else {
        windows.iter().map(|_| Vec::new()).collect()
    };
    for (n, (a, lines)) in list.iter().zip(found).enumerate() {
        anomaly(out, n + 1, a, lines);
    }
}

fn anomaly(out: &mut String, n: usize, a: &Anomaly, lines: Vec<Line>) {
    w(
        out,
        format!(
            "### {n}. {} `{}`, owner: {}\n\n{} (run time {}, seen {} time{}; {})\n",
            if a.fatal { "Failure" } else { "Note" },
            a.category,
            a.owner.label(),
            utc(a.unix_ms),
            hms(a.at_ms / 1000),
            a.count,
            if a.count == 1 { "" } else { "s" },
            if a.fatal {
                "fails the run"
            } else {
                "does not fail the run"
            }
        ),
    );
    w(out, format!("{}\n", a.summary));
    let picked = logscan::pick(lines, EXCERPT_LINES, channel_of(&a.summary).as_deref());
    if picked.is_empty() {
        w(
            out,
            "No engine log lines within 10 s before and 20 s after (the final report adds them if the engine logged any).\n",
        );
        return;
    }
    w(out, "Engine log from 10 s before to 20 s after:\n\n```");
    for l in picked {
        w(out, &l.text);
    }
    w(out, "```\n");
}

/// The first channel id (`bridge-07`, `hl7-02`, `astm-01`, `folder-01`) a text mentions.
fn channel_of(text: &str) -> Option<String> {
    ["bridge-", "hl7-", "astm-", "folder-"]
        .iter()
        .filter_map(|prefix| {
            let at = text.find(prefix)?;
            let digits: String = text[at + prefix.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .take(2)
                .collect();
            (digits.len() == 2).then(|| (at, format!("{prefix}{digits}")))
        })
        .min_by_key(|(at, _)| *at)
        .map(|(_, id)| id)
}

fn plan(out: &mut String, i: &Inputs<'_>) {
    w(out, "## Chaos plan\n");
    w(
        out,
        format!(
            "Drawn from seed {} at level {}: {} episodes. The same seed and options give the same plan; which channel an episode hits is drawn from the same seed when it starts, so it repeats too unless an earlier episode kept a channel busy.\n",
            i.seed,
            i.chaos_level,
            i.plan.len()
        ),
    );
    let mut counts: Vec<(&str, usize)> = super::chaos::plan::Kind::ALL
        .iter()
        .map(|k| (k.name(), i.plan.iter().filter(|p| p.kind == *k).count()))
        .collect();
    counts.retain(|(_, n)| *n > 0);
    for (name, n) in counts {
        w(out, format!("- {name}: {n} planned"));
    }
    w(out, "");
}

fn files(out: &mut String, i: &Inputs<'_>) {
    w(out, "## Files\n");
    let dir = i.out.display();
    for (name, what) in [
        (
            "samples.csv",
            "engine process and snapshot figures every sample interval",
        ),
        ("channels.csv", "per channel counters, engine and peers"),
        ("plan.jsonl", "the chaos plan drawn from the seed"),
        (
            "chaos.jsonl",
            "every episode as it started and finished, and what the peers noticed (waits that timed out)",
        ),
        ("reconcile.jsonl", "every reconciliation, per channel"),
        (
            "alerts.jsonl",
            "alerts raised and cleared, with whether a cause was known",
        ),
        ("engine/logs/", "the engine's own log files"),
    ] {
        w(out, format!("- `{dir}/{name}`: {what}"));
    }
    w(out, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_channel_mentioned_is_found() {
        assert_eq!(
            channel_of("sink_disconnect on astm-01 (reset)").as_deref(),
            Some("astm-01")
        );
        assert_eq!(
            channel_of("hl7-03: and bridge-12 too").as_deref(),
            Some("hl7-03")
        );
        assert_eq!(channel_of("the engine did not start"), None);
        assert_eq!(channel_of("bridge-1 is short"), None);
    }
}
