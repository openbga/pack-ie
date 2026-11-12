//! The monitor: every sample interval it reads the engine process, a snapshot and the
//! peers' totals, writes `samples.csv` and `channels.csv`, follows alerts, scans the engine
//! log, and notices an engine that ended without being killed on purpose.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use packie_diagnostics::LogFilter;
use packie_ipc::{Request, Response};
use serde_json::{Value, json};

use super::logscan::LogScan;
use super::probe;
use super::rig::{Owner, Rig};
use super::samples::{ChanSample, Sample};
use super::snap::{self, number, text};
use super::topology::Kind;
use super::util::{percentile, unix_ms};
use crate::bench;

/// The ring is read this often (every n-th sample): the answer is as large as the ring.
const RING_EVERY: u64 = 20;
/// Alert codes whose appearance outside a chaos episode is noteworthy.
const WATCHED: [&str; 7] = [
    "endpoint_down",
    "no_peer_connected",
    "bridge_buffer_pressure",
    "message_store_unavailable",
    "config_apply_failed",
    "invalid_messages",
    "transform_failed",
];
/// An alert is explained by an episode that ended this recently.
const EXPLAIN_MS: u64 = 120_000;
/// Alerts right after an engine start are explained by the start.
const START_GRACE_MS: u64 = 90_000;

/// Everything the monitor has recorded, for the report.
#[derive(Debug, Default, Clone)]
pub(crate) struct MonitorData {
    pub(crate) samples: Vec<Sample>,
    pub(crate) chan_samples: Vec<ChanSample>,
    pub(crate) scan: LogScan,
    pub(crate) unexpected_exits: u32,
    /// The largest number of events seen in the log ring.
    pub(crate) ring_max: u64,
}

struct AlertSeen {
    since_ms: u64,
}

struct State {
    prev_cpu: Option<(u32, Duration, Instant)>,
    alerts: HashMap<String, AlertSeen>,
    incarnation: u32,
    incarnation_start_ms: u64,
    count: u64,
}

struct Files {
    samples: File,
    channels: File,
}

fn open_files(dir: &std::path::Path) -> std::io::Result<Files> {
    let mut samples = File::create(dir.join("samples.csv"))?;
    let mut channels = File::create(dir.join("channels.csv"))?;
    writeln!(samples, "{}", Sample::HEADER)?;
    writeln!(channels, "{}", ChanSample::HEADER)?;
    Ok(Files { samples, channels })
}

/// Samples until the run ends.
pub(crate) async fn run(rig: Arc<Rig>, interval: Duration) {
    let Ok(mut files) = open_files(&rig.paths.out) else {
        rig.fail(
            "runner",
            Owner::Runner,
            "cannot create the sample files".to_owned(),
        );
        return;
    };
    let mut state = State {
        prev_cpu: None,
        alerts: HashMap::new(),
        incarnation: 0,
        incarnation_start_ms: 0,
        count: 0,
    };
    loop {
        sample(&rig, &mut state, &mut files).await;
        if !rig.env.halt.sleep(interval).await {
            break;
        }
    }
    sample(&rig, &mut state, &mut files).await;
}

async fn sample(rig: &Rig, st: &mut State, files: &mut Files) {
    watchdog(rig).await;
    let mut row = Sample {
        t_s: rig.now_ms() / 1000,
        unix_ms: unix_ms(),
        incarnation: rig.incarnation.load(Ordering::SeqCst),
        episodes_active: rig.active_episodes.load(Ordering::SeqCst),
        ..Sample::default()
    };
    if row.incarnation != st.incarnation {
        st.incarnation = row.incarnation;
        st.incarnation_start_ms = rig.now_ms();
        st.prev_cpu = None;
    }
    process_fields(rig, st, &mut row).await;
    let snapshot = snapshot_fields(rig, &mut row).await;
    disk_fields(rig, &mut row);
    let latencies = drain_latencies(rig);
    peer_fields(rig, &mut row, &latencies);
    st.count += 1;
    if st.count % RING_EVERY == 1 {
        row.ring_events = ring_events(rig).await;
    }
    let chans = snapshot
        .as_ref()
        .map(|s| chan_samples(rig, s, &row, &latencies))
        .unwrap_or_default();
    if let Some(snapshot) = &snapshot {
        follow_alerts(rig, st, snapshot);
    }
    record(rig, files, row, chans);
}

fn record(rig: &Rig, files: &mut Files, row: Sample, chans: Vec<ChanSample>) {
    let _ = writeln!(files.samples, "{}", row.csv());
    let _ = files.samples.flush();
    for c in &chans {
        let _ = writeln!(files.channels, "{}", c.csv());
    }
    let _ = files.channels.flush();
    if let Ok(mut data) = rig.monitor.lock() {
        data.ring_max = data.ring_max.max(row.ring_events);
        data.scan.refresh();
        data.samples.push(row);
        data.chan_samples.extend(chans);
    }
}

/// Notices an engine process that ended although nobody killed it, records it with the
/// last lines it printed, and starts it again so the soak can go on.
async fn watchdog(rig: &Rig) {
    if rig.down_on_purpose.load(Ordering::SeqCst) {
        return;
    }
    if rig.alive().await != Some(false) {
        return;
    }
    let tail = {
        let engine = rig.engine.read().await;
        engine.recent_output(6).join(" | ")
    };
    rig.fail(
        "engine_exit",
        Owner::Core,
        format!(
            "the engine process ended without being killed by the soak (start {}); last output: {tail}",
            rig.incarnation.load(Ordering::SeqCst)
        ),
    );
    if let Ok(mut data) = rig.monitor.lock() {
        data.unexpected_exits += 1;
    }
    rig.log_chaos("unplanned_restart", json!({}));
    // Let go of the dead process before starting a new one.
    let _ = rig.kill_engine().await;
    if let Err(why) = rig.start_engine().await {
        rig.fail(
            "engine_exit",
            Owner::Core,
            format!("the engine did not start again after it ended: {why}"),
        );
        rig.env.stop.set();
    }
}

async fn process_fields(rig: &Rig, st: &mut State, row: &mut Sample) {
    let Some(pid) = rig.pid() else { return };
    let probed = tokio::task::spawn_blocking(move || {
        (
            bench::resident_kib(pid),
            probe::footprint_kib(pid),
            bench::cpu_time(pid),
            probe::open_fds(pid),
        )
    })
    .await;
    let Ok((rss, footprint, cpu, fds)) = probed else {
        return;
    };
    row.pid = pid;
    row.up = rss.is_some();
    row.rss_kib = rss.unwrap_or(0);
    row.footprint_kib = footprint.unwrap_or(0);
    row.fds = fds.unwrap_or_default();
    if let Some(cpu) = cpu {
        let now = Instant::now();
        if let Some((prev_pid, prev_cpu, prev_at)) = st.prev_cpu
            && prev_pid == pid
        {
            row.cpu_tenths =
                bench::cpu_percent_tenths(cpu.saturating_sub(prev_cpu), now - prev_at).unwrap_or(0);
        }
        st.prev_cpu = Some((pid, cpu, now));
    }
}

/// Requests a snapshot (timing the request), then reads the node figures and the
/// publication timing of the subscription.
async fn snapshot_fields(rig: &Rig, row: &mut Sample) -> Option<Value> {
    let window = rig.watch.take_window();
    let mut ages = window.ages_ms.clone();
    ages.sort_unstable();
    row.snapshots = window.count;
    row.gap_max_ms = window.max_gap_ms;
    row.age_p50_ms = percentile(&ages, 50);
    row.age_p99_ms = percentile(&ages, 99);
    let started = Instant::now();
    let response = rig.call(Request::GetSnapshot).await.ok()?;
    row.rtt_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let Response::Snapshot { snapshot } = response else {
        return None;
    };
    let value = serde_json::to_value(&*snapshot).ok()?;
    node_fields(&value, row);
    Some(value)
}

fn node_fields(snapshot: &Value, row: &mut Sample) {
    let node = &snapshot["node"];
    text(node, &["node_state"]).clone_into(&mut row.node_state);
    text(node, &["store", "state"]).clone_into(&mut row.store_state);
    row.running = number(node, &["channel_counts", "running"]);
    row.retrying = number(node, &["channel_counts", "retrying"]);
    row.degraded = number(node, &["channel_counts", "degraded"]);
    row.stopped = number(node, &["channel_counts", "stopped"]);
    row.endpoints_up = number(node, &["endpoint_counts", "connected"])
        + number(node, &["endpoint_counts", "listening"]);
    row.endpoints_total = snapshot["endpoints"]
        .as_array()
        .map_or(0, |e| e.len() as u64);
    row.alerts_warning = number(node, &["totals", "alerts_active", "warning"]);
    row.alerts_critical = number(node, &["totals", "alerts_active", "critical"]);
    row.messages_stored = number(node, &["totals", "messages_stored"]);
    row.deliveries_unresolved = number(node, &["totals", "deliveries_unresolved"]);
    row.deliveries_queued = number(node, &["totals", "deliveries_queued"]);
}

fn disk_fields(rig: &Rig, row: &mut Sample) {
    let state = &rig.paths.state;
    row.store_bytes = probe::dir_size(&state.join("store"));
    row.state_bytes = probe::dir_size(state).saturating_sub(row.store_bytes);
    row.logs_bytes = probe::dir_size(&rig.paths.logs);
    row.log_files = probe::count_files(&rig.paths.logs, "packie.");
}

/// Delays of the messages each channel's receiving peer got since the last sample.
fn drain_latencies(rig: &Rig) -> HashMap<String, Vec<u64>> {
    rig.channels
        .iter()
        .map(|ch| {
            let mut lat: Vec<u64> = ch
                .ledger
                .take_latencies()
                .into_iter()
                .map(u64::from)
                .collect();
            lat.sort_unstable();
            (ch.plan.id.clone(), lat)
        })
        .collect()
}

fn peer_fields(rig: &Rig, row: &mut Sample, latencies: &HashMap<String, Vec<u64>>) {
    let mut message_latencies: Vec<u64> = Vec::new();
    for ch in &rig.channels {
        if ch.plan.kind.is_bridge() {
            row.bytes_sent += super::chan::Progress::get(&ch.prog.bytes_sent);
            row.bytes_received += super::chan::Progress::get(&ch.prog.bytes_received);
        } else {
            let counts = ch.ledger.counts();
            row.acked += counts.acked;
            row.received += counts.received;
            message_latencies.extend(latencies.get(&ch.plan.id).into_iter().flatten());
        }
    }
    message_latencies.sort_unstable();
    row.e2e_count = message_latencies.len() as u64;
    row.e2e_p50_ms = percentile(&message_latencies, 50);
    row.e2e_p99_ms = percentile(&message_latencies, 99);
}

/// The newest event's sequence number, or the one of `seq` when asked for exactly that one.
async fn log_seq(rig: &Rig, filter: LogFilter) -> Option<u64> {
    match rig.call(Request::GetLogs { filter }).await {
        Ok(Response::Logs { events }) => events.last().map(|e| e.seq),
        _ => None,
    }
}

/// The number of events in the engine's log ring.
///
/// The ring numbers every event from 1 and keeps the newest `capacity`, so it holds the
/// events `newest - count + 1 ..= newest`. Asking for the whole ring would clone and
/// serialize every event; instead the newest number is read with `limit` 1 and the count is
/// found by asking whether the event with one given number is still there (one event each,
/// about sixteen requests), up to twice the configured size so a ring that grew past it
/// shows.
async fn ring_events(rig: &Rig) -> u64 {
    let newest = log_seq(
        rig,
        LogFilter {
            limit: Some(1),
            ..LogFilter::default()
        },
    )
    .await
    .unwrap_or(0);
    let mut search = RingSearch::new(newest.min(super::checks::RING_EVENTS * 2));
    while let Some(count) = search.next_probe() {
        let seq = newest + 1 - count;
        let found = log_seq(
            rig,
            LogFilter {
                after_seq: Some(seq - 1),
                before_seq: Some(seq + 1),
                limit: Some(1),
                ..LogFilter::default()
            },
        )
        .await;
        search.record(found == Some(seq));
    }
    search.count()
}

/// Binary search for the largest `count` such that the `count` newest events are all in the
/// ring: if the event `count` places back is there, so is every newer one.
struct RingSearch {
    present: u64,
    absent: u64,
}

impl RingSearch {
    /// A search among `1..=most` newest events.
    fn new(most: u64) -> Self {
        Self {
            present: 0,
            absent: most + 1,
        }
    }

    /// The age (1 is the newest event) to ask about next; `None` when the search is over.
    fn next_probe(&self) -> Option<u64> {
        (self.absent - self.present > 1).then(|| self.present + (self.absent - self.present) / 2)
    }

    /// Records whether the event at the age last asked about is in the ring.
    fn record(&mut self, found: bool) {
        if let Some(probe) = self.next_probe() {
            if found {
                self.present = probe;
            } else {
                self.absent = probe;
            }
        }
    }

    fn count(&self) -> u64 {
        self.present
    }
}

fn chan_samples(
    rig: &Rig,
    snapshot: &Value,
    row: &Sample,
    latencies: &HashMap<String, Vec<u64>>,
) -> Vec<ChanSample> {
    rig.channels
        .iter()
        .filter_map(|ch| {
            let view = snap::channel(snapshot, &ch.plan.id)?;
            let mut c = ChanSample {
                t_s: row.t_s,
                incarnation: row.incarnation,
                channel: ch.plan.id.clone(),
                kind: kind_name(ch.plan.kind),
                instance: text(view, &["instance_started_at"]).to_owned(),
                state: snap::channel_state(view).to_owned(),
                retry_count: number(view, &["retry_count"]),
                ..ChanSample::default()
            };
            engine_numbers(view, &mut c);
            peer_numbers(ch, &mut c);
            if let Some(lat) = latencies.get(&ch.plan.id) {
                c.lat_p50_ms = percentile(lat, 50);
                c.lat_p99_ms = percentile(lat, 99);
            }
            Some(c)
        })
        .collect()
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Bridge => "bridge",
        Kind::TapBridge => "tap-bridge",
        Kind::Hl7 => "hl7",
        Kind::Astm => "astm",
        Kind::Folder => "folder",
    }
}

fn engine_numbers(view: &Value, c: &mut ChanSample) {
    if let Some(bridge) = view.get("bridge").filter(|b| !b.is_null()) {
        text(bridge, &["session"]).clone_into(&mut c.session);
        c.bytes_a_to_b = number(bridge, &["a_to_b", "bytes"]);
        c.bytes_b_to_a = number(bridge, &["b_to_a", "bytes"]);
        c.messages_a_to_b = number(bridge, &["a_to_b", "messages"]);
        c.messages_b_to_a = number(bridge, &["b_to_a", "messages"]);
        c.buffered_bytes = number(bridge, &["a_to_b", "buffered_bytes"]);
        c.overflow = number(bridge, &["a_to_b", "overflow_count"])
            + number(bridge, &["b_to_a", "overflow_count"]);
    }
    if let Some(message) = view.get("message").filter(|m| !m.is_null()) {
        text(message, &["session"]).clone_into(&mut c.session);
        c.received = number(message, &["counters", "received"]);
        c.duplicates = number(message, &["counters", "duplicates"]);
        c.invalid = number(message, &["counters", "invalid"]);
        c.aborted = number(message, &["counters", "aborted"]);
        c.queued = snap::queued(view);
        c.acknowledged = number(snap::destination(view), &["counters", "acknowledged"]);
        c.unresolved = snap::unresolved(view);
    }
}

fn peer_numbers(ch: &super::chan::ChannelRt, c: &mut ChanSample) {
    use super::chan::Progress;
    if ch.plan.kind.is_bridge() {
        c.peer_bytes_sent = Progress::get(&ch.prog.bytes_sent);
        c.peer_bytes_received = Progress::get(&ch.prog.bytes_received);
        c.peer_records = Progress::get(&ch.prog.records_received);
    } else {
        let counts = ch.ledger.counts();
        c.peer_acked = counts.acked;
        c.peer_received = counts.received;
    }
}

/// Writes alert transitions to `alerts.jsonl` and notes alerts nothing explains.
fn follow_alerts(rig: &Rig, st: &mut State, snapshot: &Value) {
    let now = rig.now_ms();
    let mut active: HashMap<String, Value> = HashMap::new();
    for alert in snapshot["alerts"].as_array().cloned().unwrap_or_default() {
        let key = format!(
            "{}|{}|{}|{}",
            text(&alert, &["code"]),
            text(&alert, &["channel_id"]),
            text(&alert, &["destination_id"]),
            text(&alert, &["endpoint_id"])
        );
        active.insert(key, alert);
    }
    for (key, alert) in &active {
        if !st.alerts.contains_key(key) {
            st.alerts.insert(key.clone(), AlertSeen { since_ms: now });
            raised(rig, st, key, alert);
        }
    }
    let gone: Vec<String> = st
        .alerts
        .keys()
        .filter(|k| !active.contains_key(*k))
        .cloned()
        .collect();
    for key in gone {
        if let Some(seen) = st.alerts.remove(&key) {
            rig.journal
                .alert(&json!({ "t_ms": now, "event": "cleared", "alert": key,
                                       "lasted_s": (now - seen.since_ms) / 1000 }));
        }
    }
}

fn raised(rig: &Rig, st: &State, key: &str, alert: &Value) {
    let now = rig.now_ms();
    let episodes = rig.active_episodes.load(Ordering::SeqCst);
    let since_end = now.saturating_sub(rig.last_episode_end_ms.load(Ordering::SeqCst));
    let code = text(alert, &["code"]);
    let explained = episodes > 0
        || rig.env.stop.is_set()
        || since_end < EXPLAIN_MS
        || now.saturating_sub(st.incarnation_start_ms) < START_GRACE_MS;
    rig.journal.alert(&json!({
        "t_ms": now, "event": "raised", "alert": key, "severity": text(alert, &["severity"]),
        "summary": text(alert, &["summary"]), "episodes_active": episodes,
        "explained": explained || !WATCHED.contains(&code),
    }));
    if !explained && WATCHED.contains(&code) {
        rig.warn(
            "alert_unexplained",
            Owner::Core,
            format!(
                "alert {key} was raised with no chaos episode active: {}",
                text(alert, &["summary"])
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(most: u64, in_ring: u64) -> (u64, u32) {
        let mut s = RingSearch::new(most);
        let mut probes = 0;
        while let Some(age) = s.next_probe() {
            probes += 1;
            s.record(age <= in_ring);
        }
        (s.count(), probes)
    }

    #[test]
    fn the_ring_search_finds_the_count_in_a_few_probes() {
        let (count, probes) = search(40_000, 20_000);
        assert_eq!(count, 20_000);
        assert!(probes <= 16, "{probes} probes");
        assert_eq!(search(40_000, 1).0, 1);
        assert_eq!(search(300, 300).0, 300, "a young ring holds everything");
        assert_eq!(search(40_000, 0).0, 0);
        assert_eq!(search(0, 0), (0, 0), "no events, nothing to ask");
    }
}
