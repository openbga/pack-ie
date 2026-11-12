//! `packie-sim bench`: measures the performance requirements (`docs/requirements.md`).
//!
//! | Target | Requirement | How it is measured |
//! | --- | --- | --- |
//! | [`Target::Prf01`] | A bridge adds under 2 ms (p99) | Request/response round trips of a small payload through a TCP to TCP bridge to an echo peer, minus the same round trips straight to the echo peer. With `--channels N`, N bridges (each with its own echo peer) are driven at the same time and the worst channel's added p99 is reported. |
//! | [`Target::Prf02`] | 100 messages per second for one durable channel; 500 messages per second aggregate with multiple channels; 50 channels per engine | HL7 MLLP listener → durable store → MLLP client. Each instrument waits for an acknowledgement after commit. With `--channels N`, N independent channels run concurrently; aggregate commit and end-to-end throughput are measured from the first measured send to the last acknowledgement and delivery. Every expected message must arrive. Per-channel rates remain informational. |
//! | [`Target::Prf03`] | The engine idles under 80 MB | Resident memory of the engine process, read from the OS, after startup settles and before traffic. |
//!
//! `--channels N` (default 1) applies to PRF-01 and PRF-02 and needs 2N loopback ports. While
//! PRF-01 (the bridged series) and PRF-02 (the measured messages) run, the engine process is
//! sampled: resident memory every 250 ms (peak of the samples, and the value after the run)
//! and CPU time (user plus system, from `/proc/<pid>/stat` on Linux, `ps -o cputime=`
//! on other Unix systems; unavailable on Windows; average percent of one core over the run).
//! Both are reported as informational
//! rows, since no requirement limits them under load (the 80 MB of PRF-03 is for idle only).
//! PRF-01 also reports aggregate bytes per second in both directions over the concurrent
//! bridged series, including warm-up round trips, as an informational throughput row.
//!
//! Run `packie-sim bench --engine target/release/packie --target prf02 --channels 50 --assert`
//! to exercise all 50 message channels in one engine. `--iterations` and `--warmup` are
//! counts per channel; measured traffic starts after every channel's warm-up messages have
//! arrived and resource sampling has started. Receivers verify each expected payload in
//! order, so a duplicate or changed payload cannot count in place of a missing message.
//! The throughput row passes only when every expected message arrives, both aggregate rates
//! reach 100 msg/s for a single channel or 500 msg/s aggregate for multiple channels.
//! Use `--target prf01 --channels 50` for the concurrent bridge latency measurement.
//!
//! To add a target: add a [`Target`] variant, measure it into a [`BenchRow`] in [`run`],
//! and state its limit next to the others.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Barrier, Notify};
use tokio::task::{JoinHandle, JoinSet};

use crate::engine::{EngineHost, EngineSpec, Needles, bootstrap_document, bootstrap_endpoint};
use crate::ports::PortLease;
use crate::resources::RunDir;

/// What to measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Added bridge latency.
    Prf01,
    /// Message channel throughput.
    Prf02,
    /// Idle memory.
    Prf03,
}

impl Target {
    /// Every target, in order.
    pub const ALL: [Self; 3] = [Self::Prf01, Self::Prf02, Self::Prf03];

    /// The requirement id.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Prf01 => "PRF-01",
            Self::Prf02 => "PRF-02",
            Self::Prf03 => "PRF-03",
        }
    }

    /// Whether a measurement exists.
    #[must_use]
    pub const fn implemented(self) -> bool {
        matches!(self, Self::Prf01 | Self::Prf02 | Self::Prf03)
    }
}

/// Limits of the requirements.
pub const PRF01_LIMIT_MICROS: u64 = 2_000;
/// PRF-02 single-channel limit: messages per second, times ten.
pub const PRF02_LIMIT_TENTHS: u64 = 1_000;
/// PRF-02 aggregate limit when multiple channels run: messages per second, times ten.
pub const PRF02_AGGREGATE_LIMIT_TENTHS: u64 = 5_000;
/// PRF-03 limit in KiB.
pub const PRF03_LIMIT_KIB: u64 = 80 * 1024;

/// Bench parameters.
#[derive(Debug, Clone)]
pub struct BenchOptions {
    /// The `packie` binary.
    pub engine_bin: PathBuf,
    /// Measured round trips per series (PRF-02: measured messages).
    pub iterations: usize,
    /// Round trips discarded before measuring (PRF-02: warm-up messages).
    pub warmup: usize,
    /// Payload bytes of one request.
    pub payload: usize,
    /// Seconds the engine idles before its memory is read.
    pub settle: Duration,
    /// Targets to run.
    pub targets: Vec<Target>,
    /// Channels driven at the same time by PRF-01 and PRF-02 (at least 1).
    pub channels: usize,
}

/// Outcome of one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchStatus {
    /// Within the limit.
    Met,
    /// Over the limit.
    Missed,
    /// Could not be measured; see the note.
    NotMeasured,
}

/// One row of the result table.
#[derive(Debug, Clone)]
pub struct BenchRow {
    /// Requirement id.
    pub id: &'static str,
    /// What was measured.
    pub metric: String,
    /// The measured value.
    pub measured: String,
    /// The limit.
    pub limit: String,
    /// Result.
    pub status: BenchStatus,
    /// Detail for the reader.
    pub note: String,
}

/// Percentiles of one series, in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Median.
    pub p50: u64,
    /// 99th percentile.
    pub p99: u64,
    /// Largest.
    pub max: u64,
}

/// Nearest-rank percentiles of `samples` (microseconds); `None` when empty.
#[must_use]
pub fn stats(samples: &[u64]) -> Option<Stats> {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = |p: usize| {
        let n = sorted.len();
        sorted
            .get(
                (n * p)
                    .div_ceil(100)
                    .saturating_sub(1)
                    .min(n.saturating_sub(1)),
            )
            .copied()
    };
    Some(Stats {
        p50: rank(50)?,
        p99: rank(99)?,
        max: *sorted.last()?,
    })
}

/// Microseconds as milliseconds with three decimals.
#[must_use]
pub fn millis(micros: u64) -> String {
    format!("{}.{:03} ms", micros / 1000, micros % 1000)
}

/// Resident memory of process `pid` in KiB, from the operating system.
/// Returns `None` on Windows, where this probe is not available.
#[must_use]
pub fn resident_kib(pid: u32) -> Option<u64> {
    if cfg!(windows) {
        return None;
    }
    if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
        let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
        return line.split_whitespace().nth(1)?.parse().ok();
    }
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Id suffix of channel `index` of `count`: none for a single channel, so a single-channel run
/// keeps the plain ids.
fn suffix(index: usize, count: usize) -> String {
    if count == 1 {
        String::new()
    } else {
        format!("-{}", index + 1)
    }
}

/// The runtime document of the benchmark bridges (the shape scenarios use): one bridge for each
/// `(bridge port, echo port)` pair.
fn runtime_document(ports: &[(u16, u16)]) -> Value {
    let mut endpoints = Vec::new();
    let mut channels = Vec::new();
    for (i, (bridge, echo)) in ports.iter().enumerate() {
        let s = suffix(i, ports.len());
        endpoints.push(json!({
            "endpoint_id": format!("inst-side{s}"), "label": format!("Instrument side{s}"),
            "kind": "tcp_listener",
            "settings": { "bind_address": "127.0.0.1", "port": bridge }
        }));
        endpoints.push(json!({
            "endpoint_id": format!("sys-side{s}"), "label": format!("System side{s}"),
            "kind": "tcp_client",
            "settings": { "host": "127.0.0.1", "port": echo }
        }));
        channels.push(json!({
            "channel_id": format!("bench-bridge{s}"), "label": format!("Benchmark bridge{s}"),
            "kind": "bridge",
            "endpoint_a": format!("inst-side{s}"), "endpoint_b": format!("sys-side{s}"),
            "connection": { "connect_strategy": "parallel", "pairing_timeout_ms": 3000 },
            "retry": { "initial_delay_ms": 200, "max_delay_ms": 1000, "multiplier": 2.0,
                       "jitter_percent": 0, "max_consecutive_failures_before_alert": 3 }
        }));
    }
    json!({ "schema_version": 1, "endpoints": endpoints, "channels": channels })
}

/// Runs `tasks` at the same time and returns their results in order; the first failure aborts
/// the rest and is returned.
async fn join_all_ordered<T, F>(tasks: Vec<F>) -> Result<Vec<T>, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    let count = tasks.len();
    let mut set = JoinSet::new();
    for (i, task) in tasks.into_iter().enumerate() {
        set.spawn(async move { (i, task.await) });
    }
    let mut results: Vec<Option<T>> = (0..count).map(|_| None).collect();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((i, Ok(value))) => {
                if let Some(slot) = results.get_mut(i) {
                    *slot = Some(value);
                }
            }
            Ok((_, Err(why))) => return Err(why),
            Err(e) => return Err(format!("a bench task stopped: {e}")),
        }
    }
    results
        .into_iter()
        .collect::<Option<Vec<T>>>()
        .ok_or_else(|| "a bench task returned nothing".to_owned())
}

/// An echo peer: answers every byte it receives, until the process ends.
async fn echo_server(listener: TcpListener) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                let _ = stream.set_nodelay(true);
                connections.spawn(echo_connection(stream));
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn echo_connection(stream: TcpStream) {
    let (mut read, mut write) = stream.into_split();
    let mut buffer = vec![0_u8; 64 * 1024];
    while let Ok(n) = read.read(&mut buffer).await {
        if n == 0 || write.write_all(&buffer[..n]).await.is_err() {
            break;
        }
    }
}

async fn round_trip(
    stream: &mut TcpStream,
    request: &[u8],
    reply: &mut [u8],
) -> std::io::Result<Duration> {
    let started = Instant::now();
    stream.write_all(request).await?;
    stream.read_exact(reply).await?;
    Ok(started.elapsed())
}

/// Round trips over a fresh connection to `port`: `warmup` discarded, `iterations` in
/// microseconds.
async fn series(
    port: u16,
    payload: usize,
    warmup: usize,
    iterations: usize,
) -> Result<Vec<u64>, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("connect to port {port}: {e}"))?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let request = vec![0x5a_u8; payload];
    let mut reply = vec![0_u8; payload];
    let mut samples = Vec::with_capacity(iterations);
    for i in 0..warmup + iterations {
        let took = tokio::time::timeout(ACK_WAIT, round_trip(&mut stream, &request, &mut reply))
            .await
            .map_err(|_| format!("round trip {i} on port {port} timed out"))?
            .map_err(|e| format!("round trip {i} on port {port}: {e}"))?;
        if reply != request {
            return Err(format!("round trip {i} on port {port} changed the payload"));
        }
        if i >= warmup {
            samples.push(u64::try_from(took.as_micros()).unwrap_or(u64::MAX));
        }
    }
    Ok(samples)
}

/// Waits until a round trip through the bridge works (it proves the bridge is paired).
async fn wait_until_bridged(port: u16, payload: usize, within: Duration) -> Result<(), String> {
    let deadline = Instant::now() + within;
    loop {
        let attempt =
            tokio::time::timeout(Duration::from_millis(500), series(port, payload, 0, 1)).await;
        if matches!(attempt, Ok(Ok(_))) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the bridge did not carry traffic within {} s",
                within.as_secs()
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct Rig {
    engine: EngineHost,
    run_dir: RunDir,
    /// Ports the engine's bridges listen on, one per channel.
    bridge_ports: Vec<u16>,
    /// Ports of the echo peers, one per channel.
    echo_ports: Vec<u16>,
    /// Stop the echo listeners and their connections when the rig is released.
    _peers: JoinSet<()>,
    /// Keep the ports reserved for the length of the run.
    _leases: Vec<PortLease>,
}

/// Starts a listener on the socket a lease holds, so the port is never unbound.
fn listen_on(lease: &mut PortLease) -> Result<TcpListener, String> {
    lease
        .take_socket()
        .ok_or_else(|| "the port has no socket".to_owned())?
        .listen(1024)
        .map_err(|e| format!("listen on port {}: {e}", lease.port()))
}

async fn start_rig(options: &BenchOptions) -> Result<Rig, String> {
    let channels = options.channels.max(1);
    let mut leases = Vec::with_capacity(channels * 2);
    let mut peers = JoinSet::new();
    let (mut bridge_ports, mut echo_ports) = (Vec::new(), Vec::new());
    for _ in 0..channels {
        let mut bridge = PortLease::reserve_loopback().map_err(|e| e.to_string())?;
        let mut echo_lease = PortLease::reserve_loopback().map_err(|e| e.to_string())?;
        bridge_ports.push(bridge.port());
        echo_ports.push(echo_lease.port());
        peers.spawn(echo_server(listen_on(&mut echo_lease)?));
        // The engine binds the bridge port itself.
        bridge.release_socket();
        leases.push(bridge);
        leases.push(echo_lease);
    }
    let ports: Vec<(u16, u16)> = bridge_ports
        .iter()
        .copied()
        .zip(echo_ports.iter().copied())
        .collect();
    let (engine, run_dir) = launch_engine(options, &runtime_document(&ports)).await?;
    Ok(Rig {
        engine,
        run_dir,
        bridge_ports,
        echo_ports,
        _peers: peers,
        _leases: leases,
    })
}

/// Writes `runtime` and a bootstrap document to a new run directory and starts the engine on
/// them.
async fn launch_engine(
    options: &BenchOptions,
    runtime: &Value,
) -> Result<(EngineHost, RunDir), String> {
    let run_dir = RunDir::create().map_err(|e| e.to_string())?;
    let runtime_path = run_dir.root().join("runtime.json");
    let bootstrap_path = run_dir.root().join("bootstrap.json");
    let bootstrap = bootstrap_document(
        "bench",
        &run_dir.state(),
        &run_dir.logs(),
        &runtime_path,
        false,
        &serde_json::Map::new(),
    );
    for (path, value) in [(&runtime_path, runtime), (&bootstrap_path, &bootstrap)] {
        let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    let spec = EngineSpec {
        bin: options.engine_bin.clone(),
        bootstrap_path,
        state_dir: run_dir.state(),
        logs_dir: run_dir.logs(),
        endpoint: bootstrap_endpoint(&bootstrap),
        drain: Duration::from_secs(5),
    };
    let mut engine = EngineHost::new(spec, Needles::default());
    if let Err(why) = engine.start(Duration::from_secs(20)).await {
        return Err(format!("the engine did not start: {why}"));
    }
    Ok((engine, run_dir))
}

fn not_measured(target: Target, metric: &str, limit: &str, note: String) -> BenchRow {
    BenchRow {
        id: target.id(),
        metric: metric.to_owned(),
        measured: "-".to_owned(),
        limit: limit.to_owned(),
        status: BenchStatus::NotMeasured,
        note,
    }
}

/// The channel with the largest added p99: `(index, direct, bridged, added p99)`.
fn worst_added(direct: &[Vec<u64>], bridged: &[Vec<u64>]) -> Option<(usize, Stats, Stats, u64)> {
    direct
        .iter()
        .zip(bridged)
        .enumerate()
        .filter_map(|(i, (d, b))| {
            let (d, b) = (stats(d)?, stats(b)?);
            Some((i, d, b, b.p99.saturating_sub(d.p99)))
        })
        .max_by_key(|(_, _, _, added)| *added)
}

fn latency_row(
    rig: &Rig,
    options: &BenchOptions,
    direct: &[Vec<u64>],
    bridged: &[Vec<u64>],
) -> BenchRow {
    let channels = rig.bridge_ports.len();
    let metric = if channels == 1 {
        "added latency p99".to_owned()
    } else {
        format!("added latency p99, worst of {channels} bridges")
    };
    let Some((worst, d, b, added)) = worst_added(direct, bridged) else {
        return not_measured(Target::Prf01, &metric, "< 2 ms", "no samples".into());
    };
    let ports = rig.bridge_ports.get(worst).zip(rig.echo_ports.get(worst));
    let mut note = format!(
        "{} round trips of {} bytes; direct p50 {} p99 {}; bridged p50 {} p99 {} max {}; added p50 {}; ports {}->{}",
        options.iterations,
        options.payload,
        millis(d.p50),
        millis(d.p99),
        millis(b.p50),
        millis(b.p99),
        millis(b.max),
        millis(b.p50.saturating_sub(d.p50)),
        ports.map_or(0, |(bridge, _)| *bridge),
        ports.map_or(0, |(_, echo)| *echo),
    );
    if channels > 1 {
        note = format!(
            "{channels} bridges measured at the same time, the figures of the worst (channel {}): {note}",
            worst + 1
        );
    }
    BenchRow {
        id: Target::Prf01.id(),
        metric,
        measured: millis(added),
        limit: format!("< {}", millis(PRF01_LIMIT_MICROS)),
        status: if added < PRF01_LIMIT_MICROS {
            BenchStatus::Met
        } else {
            BenchStatus::Missed
        },
        note,
    }
}

/// Memory in MB with one decimal.
fn megabytes(kib: u64) -> String {
    format!("{}.{} MB", kib / 1024, kib % 1024 * 10 / 1024)
}

/// How often the engine process is sampled while traffic runs.
const SAMPLE_EVERY: Duration = Duration::from_millis(250);

/// CPU time (user plus system) of process `pid`, from the operating system.
/// Returns `None` on Windows, where this probe is not available.
#[must_use]
pub fn cpu_time(pid: u32) -> Option<Duration> {
    if cfg!(windows) {
        return None;
    }
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        return parse_proc_stat(&stat, CLOCK_TICKS_PER_SECOND);
    }
    let out = std::process::Command::new("ps")
        .args(["-o", "cputime=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    parse_cputime(String::from_utf8_lossy(&out.stdout).trim())
}

/// Clock ticks per second of `/proc/<pid>/stat` times (`sysconf(_SC_CLK_TCK)`, 100 on Linux
/// for every architecture the engine ships for).
const CLOCK_TICKS_PER_SECOND: u64 = 100;

/// `utime` plus `stime` of the contents of `/proc/<pid>/stat`. The process name (field 2) may
/// hold spaces and parentheses, so fields count from the last `)`.
fn parse_proc_stat(stat: &str, ticks_per_second: u64) -> Option<Duration> {
    let after_name = stat.rsplit_once(')')?.1;
    let mut fields = after_name.split_whitespace();
    // Fields 14 and 15 of the file; field 3 (the state) is the first after the name.
    let utime: u64 = fields.nth(11)?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    let ticks = utime.checked_add(stime)?;
    Some(Duration::from_micros(
        ticks.checked_mul(1_000_000)? / ticks_per_second.max(1),
    ))
}

/// The `[[DD-]HH:]MM:SS[.ff]` of `ps -o cputime=` (macOS prints `M:SS.ff` or `H:MM:SS.ff`).
fn parse_cputime(text: &str) -> Option<Duration> {
    let (days, clock) = match text.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, text),
    };
    let mut seconds = days.checked_mul(86_400)?;
    let mut micros = 0_u64;
    let parts: Vec<&str> = clock.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let (whole, fraction) = if last {
            part.split_once('.').unwrap_or((part, ""))
        } else {
            (*part, "")
        };
        let unit = 60_u64.pow(u32::try_from(parts.len() - 1 - i).ok()?);
        seconds = seconds.checked_add(whole.parse::<u64>().ok()?.checked_mul(unit)?)?;
        if !fraction.is_empty() {
            let digits: String = fraction.chars().take(6).collect();
            micros =
                digits.parse::<u64>().ok()? * 10_u64.pow(6 - u32::try_from(digits.len()).ok()?);
        }
    }
    Some(Duration::from_secs(seconds) + Duration::from_micros(micros))
}

/// Average CPU use as tenths of a percent of one core; `None` for an empty interval.
#[must_use]
pub fn cpu_percent_tenths(cpu: Duration, wall: Duration) -> Option<u64> {
    let wall = wall.as_micros();
    if wall == 0 {
        return None;
    }
    u64::try_from(cpu.as_micros() * 1_000 / wall).ok()
}

/// Runs a blocking OS query off the async threads.
async fn blocking<T: Send + 'static>(
    query: impl FnOnce() -> Option<T> + Send + 'static,
) -> Option<T> {
    tokio::task::spawn_blocking(query).await.ok().flatten()
}

/// What the engine process used while traffic ran.
#[derive(Debug, Clone, Copy, Default)]
struct Usage {
    /// Resident memory when sampling started, KiB.
    before_kib: Option<u64>,
    /// Largest resident memory sampled, KiB.
    peak_kib: Option<u64>,
    /// Resident memory when traffic ended, KiB.
    after_kib: Option<u64>,
    /// CPU time the engine used during the run.
    cpu: Option<Duration>,
    /// Length of the run.
    wall: Duration,
}

/// Samples the engine process in the background until [`UsageSampler::finish`].
struct UsageSampler {
    pid: Option<u32>,
    started: Instant,
    before_kib: Option<u64>,
    cpu_before: Option<Duration>,
    stop: Arc<Notify>,
    peak: JoinHandle<Option<u64>>,
}

impl UsageSampler {
    async fn start(pid: Option<u32>) -> Self {
        let (before_kib, cpu_before) = match pid {
            Some(pid) => (
                blocking(move || resident_kib(pid)).await,
                blocking(move || cpu_time(pid)).await,
            ),
            None => (None, None),
        };
        let stop = Arc::new(Notify::new());
        let signal = Arc::clone(&stop);
        let peak = tokio::spawn(async move {
            let mut peak = before_kib;
            let pid = pid?;
            loop {
                tokio::select! {
                    () = signal.notified() => return peak,
                    () = tokio::time::sleep(SAMPLE_EVERY) => {
                        if let Some(kib) = blocking(move || resident_kib(pid)).await {
                            peak = peak.max(Some(kib));
                        }
                    }
                }
            }
        });
        Self {
            pid,
            started: Instant::now(),
            before_kib,
            cpu_before,
            stop,
            peak,
        }
    }

    async fn finish(self) -> Usage {
        let (after_kib, cpu_after) = match self.pid {
            Some(pid) => (
                blocking(move || resident_kib(pid)).await,
                blocking(move || cpu_time(pid)).await,
            ),
            None => (None, None),
        };
        let wall = self.started.elapsed();
        self.stop.notify_one();
        let peak = self.peak.await.ok().flatten();
        Usage {
            before_kib: self.before_kib,
            peak_kib: peak.max(after_kib),
            after_kib,
            cpu: cpu_after
                .zip(self.cpu_before)
                .map(|(a, b)| a.saturating_sub(b)),
            wall,
        }
    }
}

/// The informational rows of the engine's resource use while `target` ran with `channels`
/// channels; "not measured" rows when the operating system reported nothing.
fn usage_rows(target: Target, channels: usize, usage: &Usage) -> Vec<BenchRow> {
    const MEMORY: &str = "engine resident memory under load";
    const CPU: &str = "engine cpu under load";
    const LIMIT: &str = "informational";
    let channel_note = format!(
        "{channels} channel{} over {:.1} s",
        if channels == 1 { "" } else { "s" },
        usage.wall.as_secs_f64()
    );
    let memory = match usage.peak_kib {
        None => not_measured(
            target,
            MEMORY,
            LIMIT,
            "the operating system did not report it".into(),
        ),
        Some(peak) => BenchRow {
            id: target.id(),
            metric: MEMORY.to_owned(),
            measured: format!("peak {}", megabytes(peak)),
            limit: LIMIT.to_owned(),
            status: BenchStatus::Met,
            note: format!(
                "before {}, after {}; sampled every {} ms; {channel_note}",
                usage.before_kib.map_or_else(|| "-".to_owned(), megabytes),
                usage.after_kib.map_or_else(|| "-".to_owned(), megabytes),
                SAMPLE_EVERY.as_millis(),
            ),
        },
    };
    let cpu = match usage.cpu.zip(cpu_percent_tenths(
        usage.cpu.unwrap_or_default(),
        usage.wall,
    )) {
        None => not_measured(
            target,
            CPU,
            LIMIT,
            "the operating system did not report it".into(),
        ),
        Some((cpu, tenths)) => BenchRow {
            id: target.id(),
            metric: CPU.to_owned(),
            measured: format!("{}.{}% of one core", tenths / 10, tenths % 10),
            limit: LIMIT.to_owned(),
            status: BenchStatus::Met,
            note: format!("{:.2} cpu seconds; {channel_note}", cpu.as_secs_f64()),
        },
    };
    vec![memory, cpu]
}

fn memory_row(kib: Option<u64>, after: Option<u64>) -> BenchRow {
    let Some(kib) = kib else {
        return not_measured(
            Target::Prf03,
            "idle resident memory",
            "< 80 MB",
            "the operating system did not report it".into(),
        );
    };
    BenchRow {
        id: Target::Prf03.id(),
        metric: "idle resident memory".to_owned(),
        measured: megabytes(kib),
        limit: format!("< {} MB", PRF03_LIMIT_KIB / 1024),
        status: if kib < PRF03_LIMIT_KIB {
            BenchStatus::Met
        } else {
            BenchStatus::Missed
        },
        note: after.map_or_else(String::new, |a| {
            format!("after the latency run: {}", megabytes(a))
        }),
    }
}

const MLLP_START: u8 = 0x0b;
const MLLP_END: [u8; 2] = [0x1c, 0x0d];
/// How long one acknowledgement may take before the run is abandoned.
const ACK_WAIT: Duration = Duration::from_secs(10);
/// How long the receiver may take to catch up after the last acknowledgement.
const DELIVERY_WAIT: Duration = Duration::from_secs(30);
const PRF02_METRIC: &str = "message channel throughput";

/// The PRF-02 metric name for `channels` channels.
fn prf02_metric(channels: usize) -> String {
    if channels == 1 {
        PRF02_METRIC.to_owned()
    } else {
        format!("{PRF02_METRIC}, {channels} channels")
    }
}

/// The PRF-02 threshold: one channel at 100 msg/s, or multiple channels at 500 aggregate.
fn prf02_threshold(channels: usize) -> u64 {
    if channels == 1 {
        PRF02_LIMIT_TENTHS
    } else {
        PRF02_AGGREGATE_LIMIT_TENTHS
    }
}

fn prf02_limit(channels: usize) -> String {
    let rate = prf02_threshold(channels) / 10;
    let scope = if channels == 1 { "" } else { " aggregate" };
    format!(">= {rate} msg/s{scope}")
}

/// The runtime document of the PRF-02 channels: for each `(source port, destination port)`
/// pair, an MLLP listener in and an MLLP client out.
fn throughput_document(ports: &[(u16, u16)]) -> Value {
    let mut endpoints = Vec::new();
    let mut channels = Vec::new();
    for (i, (source, destination)) in ports.iter().enumerate() {
        let s = suffix(i, ports.len());
        endpoints.push(json!({
            "endpoint_id": format!("instrument{s}"), "label": format!("Instrument{s}"),
            "kind": "tcp_listener",
            "settings": { "bind_address": "127.0.0.1", "port": source }
        }));
        endpoints.push(json!({
            "endpoint_id": format!("lis{s}"), "label": format!("Receiving system{s}"),
            "kind": "tcp_client",
            "settings": { "host": "127.0.0.1", "port": destination }
        }));
        channels.push(json!({
            "channel_id": format!("bench-messages{s}"),
            "label": format!("Benchmark message channel{s}"),
            "kind": "message",
            "source": { "endpoint": format!("instrument{s}"), "protocol": "hl7_mllp",
                        "settings": { "ack_mode": "original" } },
            "destinations": [
              { "destination_id": format!("lis{s}"), "label": format!("Receiving system{s}"),
                "endpoint": format!("lis{s}"), "protocol": "hl7_mllp",
                "settings": { "ack_mode": "original" },
                "delivery": { "max_attempts": 3 } }
            ],
            "retention": { "max_age_days": 1 }
        }));
    }
    json!({ "schema_version": 1, "endpoints": endpoints, "channels": channels })
}

/// A synthetic ORU^R01 with the unique control id `B<seq>`; placeholder patient only.
fn hl7_message(seq: usize) -> String {
    [
        format!("MSH|^~\\&|BENCH|LAB|LIS|HOSP|20260101120000||ORU^R01|B{seq:09}|P|2.5.1"),
        "PID|1||MRN-00000^^^TEST||TEST^PATIENT||19700101|U".to_owned(),
        format!("OBR|1|SAMPLE-{seq:09}||GLU^Glucose|||20260101115900"),
        "OBX|1|NM|GLU^Glucose||5.4|mmol/L|3.9-5.6|N|||F".to_owned(),
        String::new(),
    ]
    .join("\r")
}

/// The control id sent in `B<seq>` form.
fn control_id(seq: usize) -> String {
    format!("B{seq:09}")
}

/// An acknowledgement of `control_id` with code `AA`.
fn ack_message(control_id: &str) -> String {
    format!(
        "MSH|^~\\&|LIS|HOSP|BENCH|LAB|20260101120000||ACK^R01|A{control_id}|P|2.5.1\rMSA|AA|{control_id}\r"
    )
}

fn mllp_frame(message: &str) -> Vec<u8> {
    let mut frame = Vec::with_capacity(message.len() + 3);
    frame.push(MLLP_START);
    frame.extend_from_slice(message.as_bytes());
    frame.extend_from_slice(&MLLP_END);
    frame
}

/// Removes the first complete MLLP frame from `buffer` and returns its message text.
fn take_frame(buffer: &mut Vec<u8>) -> Option<String> {
    let start = buffer.iter().position(|b| *b == MLLP_START)?;
    let end = buffer[start..]
        .windows(MLLP_END.len())
        .position(|w| w == MLLP_END)?
        + start;
    let text = String::from_utf8_lossy(&buffer[start + 1..end]).into_owned();
    buffer.drain(..end + MLLP_END.len());
    Some(text)
}

/// The text of field `n` (HL7 numbering) of the first `segment`; MSH-1 is the separator.
fn hl7_field<'a>(message: &'a str, segment: &str, n: usize) -> Option<&'a str> {
    let line = message
        .split(['\r', '\n'])
        .find(|l| l.split('|').next() == Some(segment))?;
    let index = if segment == "MSH" { n - 1 } else { n };
    line.split('|').nth(index)
}

/// The code and acknowledged control id of an acknowledgement (MSA-1, MSA-2).
fn ack_of(message: &str) -> Option<(&str, &str)> {
    Some((hl7_field(message, "MSA", 1)?, hl7_field(message, "MSA", 2)?))
}

/// Reads from `stream` until one complete MLLP frame is available.
async fn read_frame(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> std::io::Result<String> {
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(text) = take_frame(buffer) {
            return Ok(text);
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
}

/// Receiver evidence: count only the expected, ordered synthetic messages. Duplicates,
/// corruption and missing messages cannot substitute for a delivery.
#[derive(Default)]
struct Received {
    count: AtomicUsize,
    invalid: AtomicBool,
}

/// The receiving system: acknowledges every delivered message with `AA` and counts them.
async fn receiving_system(listener: TcpListener, delivered: Arc<Received>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                let _ = stream.set_nodelay(true);
                connections.spawn(receiving_connection(stream, Arc::clone(&delivered)));
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn receiving_connection(mut stream: TcpStream, delivered: Arc<Received>) {
    let mut buffer = Vec::new();
    while let Ok(message) = read_frame(&mut stream, &mut buffer).await {
        let seq = delivered.count.load(Ordering::SeqCst);
        if message != hl7_message(seq) {
            delivered.invalid.store(true, Ordering::SeqCst);
            break;
        }
        let id = hl7_field(&message, "MSH", 10).unwrap_or_default();
        let reply = mllp_frame(&ack_message(id));
        if stream.write_all(&reply).await.is_err() {
            break;
        }
        delivered.count.fetch_add(1, Ordering::SeqCst);
    }
}

/// Connects to the channel's source listener, retrying until it listens.
async fn connect_source(port: u16, within: Duration) -> Result<TcpStream, String> {
    let deadline = Instant::now() + within;
    loop {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(stream) => {
                stream.set_nodelay(true).map_err(|e| e.to_string())?;
                return Ok(stream);
            }
            Err(e) if Instant::now() >= deadline => {
                return Err(format!(
                    "the channel did not listen on port {port} within {} s: {e}",
                    within.as_secs()
                ));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

/// Sends message `seq` and waits for its acknowledgement; returns the time it took.
async fn send_message(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    seq: usize,
) -> Result<Duration, String> {
    let frame = mllp_frame(&hl7_message(seq));
    let id = control_id(seq);
    let started = Instant::now();
    stream
        .write_all(&frame)
        .await
        .map_err(|e| format!("send message {seq}: {e}"))?;
    let reply = tokio::time::timeout(ACK_WAIT, read_frame(stream, buffer))
        .await
        .map_err(|_| {
            format!(
                "no acknowledgement of message {seq} within {} s",
                ACK_WAIT.as_secs()
            )
        })?
        .map_err(|e| format!("acknowledgement of message {seq}: {e}"))?;
    let took = started.elapsed();
    match ack_of(&reply) {
        Some(("AA", acked)) if acked == id => Ok(took),
        _ => Err(format!(
            "message {seq} was not acknowledged with AA: {}",
            reply
                .replace('\r', " ")
                .chars()
                .take(120)
                .collect::<String>()
        )),
    }
}

/// Messages per second times ten; `None` for an empty interval.
#[must_use]
pub fn tenths_per_second(count: usize, elapsed: Duration) -> Option<u64> {
    let micros = elapsed.as_micros();
    if micros == 0 {
        return None;
    }
    u64::try_from(count as u128 * 10_000_000 / micros).ok()
}

fn per_second(tenths: u64) -> String {
    format!("{}.{} msg/s", tenths / 10, tenths % 10)
}

/// What the PRF-02 run observed, over all channels.
#[derive(Debug, Clone)]
struct Throughput {
    /// Channels driven at the same time.
    channels: usize,
    /// Measured messages of all channels, all acknowledged by the engine.
    measured: usize,
    /// Warm-up messages of all channels, sent before the measured ones.
    warmup: usize,
    /// First measured send to the last acknowledgement of a measured message, over all channels.
    committed_in: Duration,
    /// Send to acknowledgement of each measured message of all channels, microseconds.
    latencies: Vec<u64>,
    /// Messages that reached the receiving systems, warm-up included.
    delivered: usize,
    /// First measured send to the last measured message at its receiver, when all arrived.
    delivered_in: Option<Duration>,
    /// Commit rate of each channel on its own (messages per second times ten).
    channel_tenths: Vec<u64>,
    /// End-to-end rate of each channel (messages per second times ten).
    channel_delivery_tenths: Vec<u64>,
}

/// One channel's part of the PRF-02 run.
#[derive(Debug, Clone)]
struct ChannelRun {
    /// When its measured messages started.
    started: Instant,
    /// When the last of them was acknowledged.
    committed_at: Instant,
    /// Send to acknowledgement of each, microseconds.
    latencies: Vec<u64>,
    /// Messages that reached its receiving system.
    delivered: usize,
    /// When the receiver had every message, if it did.
    delivered_at: Option<Instant>,
}

/// Combines the channels' runs: all measured messages over the time from the earliest start to
/// the last acknowledgement (and to the last delivery).
fn aggregate(runs: &[ChannelRun], measured_each: usize, warmup_each: usize) -> Throughput {
    let first = runs.iter().map(|r| r.started).min();
    let since_first =
        |at: Instant| first.map_or(Duration::ZERO, |f| at.saturating_duration_since(f));
    let delivered_in = runs
        .iter()
        .map(|r| r.delivered_at)
        .collect::<Option<Vec<Instant>>>()
        .and_then(|all| all.into_iter().max())
        .map(since_first);
    Throughput {
        channels: runs.len(),
        measured: measured_each * runs.len(),
        warmup: warmup_each * runs.len(),
        committed_in: runs
            .iter()
            .map(|r| since_first(r.committed_at))
            .max()
            .unwrap_or_default(),
        latencies: runs
            .iter()
            .flat_map(|r| r.latencies.iter().copied())
            .collect(),
        delivered: runs.iter().map(|r| r.delivered).sum(),
        delivered_in,
        channel_tenths: runs
            .iter()
            .filter_map(|r| {
                tenths_per_second(
                    measured_each,
                    r.committed_at.saturating_duration_since(r.started),
                )
            })
            .collect(),
        channel_delivery_tenths: runs
            .iter()
            .filter_map(|r| {
                tenths_per_second(
                    measured_each,
                    r.delivered_at?.saturating_duration_since(r.started),
                )
            })
            .collect(),
    }
}

fn throughput_rows(t: &Throughput) -> Vec<BenchRow> {
    let channels = t.channels.max(1);
    let limit = prf02_threshold(channels);
    let total = t.measured + t.warmup;
    let commit = tenths_per_second(t.measured, t.committed_in);
    let deliver = t
        .delivered_in
        .and_then(|d| tenths_per_second(t.measured, d));
    let complete = t.delivered == total;
    let fast = commit.is_some_and(|r| r >= limit) && deliver.is_some_and(|r| r >= limit);
    let delivered_note = format!(
        "delivered end to end {}",
        deliver.map_or_else(|| "not completed".to_owned(), per_second),
    );
    let mut note = if channels == 1 {
        format!(
            "{} measured and {} warm-up HL7 messages one at a time, each acknowledged after its commit; {delivered_note}",
            t.measured, t.warmup,
        )
    } else {
        let slowest = t.channel_tenths.iter().min().copied();
        let average = u64::try_from(t.channel_tenths.len())
            .ok()
            .filter(|n| *n > 0)
            .map(|n| t.channel_tenths.iter().sum::<u64>() / n);
        format!(
            "{channels} channels at the same time, each {} measured and {} warm-up HL7 messages one at a time, each acknowledged after its commit; per channel slowest {} average {}; {delivered_note}",
            t.measured / channels,
            t.warmup / channels,
            slowest.map_or_else(|| "-".to_owned(), per_second),
            average.map_or_else(|| "-".to_owned(), per_second),
        )
    };
    if channels > 1 {
        note.push_str(&format!(
            "; slowest channel end to end {}",
            t.channel_delivery_tenths
                .iter()
                .min()
                .copied()
                .map_or_else(|| "not completed".to_owned(), per_second),
        ));
    }
    if !complete {
        note.push_str(&format!(
            "; MISSED: {} of {total} messages reached the receiving systems",
            t.delivered
        ));
    }
    let throughput = BenchRow {
        id: Target::Prf02.id(),
        metric: prf02_metric(channels),
        measured: commit.map_or_else(|| "-".to_owned(), per_second),
        limit: prf02_limit(channels),
        status: if complete && fast {
            BenchStatus::Met
        } else {
            BenchStatus::Missed
        },
        note,
    };
    let latency = stats(&t.latencies).map(|s| BenchRow {
        id: Target::Prf02.id(),
        metric: "commit latency p99".to_owned(),
        measured: millis(s.p99),
        limit: "informational".to_owned(),
        status: BenchStatus::Met,
        note: format!("p50 {} max {}", millis(s.p50), millis(s.max)),
    });
    std::iter::once(throughput).chain(latency).collect()
}

/// Sends `count` messages from `first` on and returns their latencies in microseconds.
async fn send_series(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    first: usize,
    count: usize,
) -> Result<Vec<u64>, String> {
    let mut latencies = Vec::with_capacity(count);
    for seq in first..first + count {
        let took = send_message(stream, buffer, seq).await?;
        latencies.push(u64::try_from(took.as_micros()).unwrap_or(u64::MAX));
    }
    Ok(latencies)
}

async fn wait_for_count(counter: &AtomicUsize, target: usize, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while counter.load(Ordering::SeqCst) < target {
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    true
}

/// One instrument: sends `warmup` messages, waits at `ready` until every channel is warm (and
/// sampling has started), then sends `iterations` measured messages.
async fn drive_channel(
    warmup: usize,
    iterations: usize,
    source_port: u16,
    delivered: Arc<Received>,
    ready: Arc<Barrier>,
) -> Result<ChannelRun, String> {
    let mut stream = connect_source(source_port, Duration::from_secs(15)).await?;
    let mut buffer = Vec::new();
    send_series(&mut stream, &mut buffer, 0, warmup).await?;
    if !wait_for_count(&delivered.count, warmup, DELIVERY_WAIT).await {
        return Err(format!(
            "not all warm-up messages on port {source_port} were delivered"
        ));
    }
    ready.wait().await;
    // The coordinator starts resource sampling between the two barrier rounds.
    ready.wait().await;
    let started = Instant::now();
    let latencies = send_series(&mut stream, &mut buffer, warmup, iterations).await?;
    let committed_at = Instant::now();
    let all = wait_for_count(&delivered.count, warmup + iterations, DELIVERY_WAIT).await;
    if delivered.invalid.load(Ordering::SeqCst) {
        return Err(format!(
            "the receiver of port {source_port} saw an unexpected message"
        ));
    }
    Ok(ChannelRun {
        started,
        committed_at,
        latencies,
        delivered: delivered.count.load(Ordering::SeqCst),
        delivered_at: all.then(Instant::now),
    })
}

/// One channel of the PRF-02 run: the ports of the engine's source listener and of the
/// receiving system, kept reserved, and the receiver's count.
struct Lane {
    source: PortLease,
    destination: PortLease,
    delivered: Arc<Received>,
}

/// The instruments' side of the run, after the engine and the receiving systems are up: every
/// channel at the same time, the engine sampled from the common start.
async fn drive_all(
    options: &BenchOptions,
    pid: Option<u32>,
    lanes: &[Lane],
) -> Result<(Throughput, Usage), String> {
    let ready = Arc::new(Barrier::new(lanes.len() + 1));
    let tasks = join_all_ordered(
        lanes
            .iter()
            .map(|lane| {
                drive_channel(
                    options.warmup,
                    options.iterations,
                    lane.source.port(),
                    Arc::clone(&lane.delivered),
                    Arc::clone(&ready),
                )
            })
            .collect(),
    );
    tokio::pin!(tasks);
    let sampler = tokio::select! {
        () = async { ready.wait().await; } => UsageSampler::start(pid).await,
        early = &mut tasks => {
            // Nobody can finish before the barrier opens unless a channel failed.
            return Err(early.err().unwrap_or_else(|| "the channels ended before measuring".to_owned()));
        }
    };
    ready.wait().await;
    let runs = tasks.await;
    let usage = sampler.finish().await;
    Ok((aggregate(&runs?, options.iterations, options.warmup), usage))
}

/// PRF-02: the rows of the message channel run (or one row saying why it did not run).
async fn throughput(options: &BenchOptions) -> Vec<BenchRow> {
    let channels = options.channels.max(1);
    let mut receivers = JoinSet::new();
    match message_run(options, channels, &mut receivers).await {
        Ok(rows) => rows,
        Err(why) => vec![not_measured(
            Target::Prf02,
            &prf02_metric(channels),
            &prf02_limit(channels),
            why,
        )],
    }
}

async fn message_run(
    options: &BenchOptions,
    channels: usize,
    receivers: &mut JoinSet<()>,
) -> Result<Vec<BenchRow>, String> {
    let mut lanes = Vec::with_capacity(channels);
    for _ in 0..channels {
        let reserve = || PortLease::reserve_loopback().map_err(|e| format!("no free port: {e}"));
        let (mut source, mut destination) = (reserve()?, reserve()?);
        let listener =
            listen_on(&mut destination).map_err(|e| format!("start the receiving system: {e}"))?;
        // The engine binds the source port itself.
        source.release_socket();
        let delivered = Arc::new(Received::default());
        receivers.spawn(receiving_system(listener, Arc::clone(&delivered)));
        lanes.push(Lane {
            source,
            destination,
            delivered,
        });
    }
    let ports: Vec<(u16, u16)> = lanes
        .iter()
        .map(|l| (l.source.port(), l.destination.port()))
        .collect();
    let (mut engine, mut run_dir) = launch_engine(options, &throughput_document(&ports)).await?;
    let observed = drive_all(options, engine.pid(), &lanes).await;
    engine.shutdown().await;
    run_dir.remove();
    let (observed, usage) = observed?;
    let mut rows = throughput_rows(&observed);
    rows.extend(usage_rows(Target::Prf02, channels, &usage));
    Ok(rows)
}

struct LatencyRun {
    direct: Vec<Vec<u64>>,
    bridged: Vec<Vec<u64>>,
    elapsed: Duration,
    usage: Usage,
}

fn bridge_throughput_row(options: &BenchOptions, channels: usize, elapsed: Duration) -> BenchRow {
    let count = options
        .iterations
        .checked_add(options.warmup)
        .and_then(|count| count.checked_mul(channels));
    let rate = count
        .and_then(|count| count.checked_mul(options.payload))
        .and_then(|bytes| bytes.checked_mul(2))
        .and_then(|bytes| tenths_per_second(bytes, elapsed));
    let Some(rate) = rate else {
        return not_measured(
            Target::Prf01,
            "aggregate bridge throughput",
            "informational",
            "byte count overflowed or the measured interval was empty".to_owned(),
        );
    };
    let kib_tenths = rate / 1024;
    BenchRow {
        id: Target::Prf01.id(),
        metric: "aggregate bridge throughput".to_owned(),
        measured: format!("{}.{} KiB/s", kib_tenths / 10, kib_tenths % 10),
        limit: "informational".to_owned(),
        status: BenchStatus::Met,
        note: format!(
            "{channels} channels, {} round trips each including warm-up, {} bytes per direction, over {:.3} s",
            options.iterations + options.warmup,
            options.payload,
            elapsed.as_secs_f64(),
        ),
    }
}

/// PRF-01 round trips on every channel at the same time: first straight to the echo peers, then
/// through the bridges, with the engine sampled during the bridged series.
async fn latency_runs(rig: &Rig, options: &BenchOptions) -> Result<LatencyRun, String> {
    let all = |ports: &[u16]| {
        join_all_ordered(
            ports
                .iter()
                .map(|port| series(*port, options.payload, options.warmup, options.iterations))
                .collect(),
        )
    };
    let direct = all(&rig.echo_ports).await?;
    let sampler = UsageSampler::start(rig.engine.pid()).await;
    let started = Instant::now();
    let bridged = all(&rig.bridge_ports).await;
    let elapsed = started.elapsed();
    let usage = sampler.finish().await;
    Ok(LatencyRun {
        direct,
        bridged: bridged?,
        elapsed,
        usage,
    })
}

/// Runs the selected targets and returns one row each.
pub async fn run(options: &BenchOptions) -> Vec<BenchRow> {
    let mut rows = Vec::new();
    let wants = |t: Target| options.targets.contains(&t);
    if wants(Target::Prf02) {
        rows.extend(throughput(options).await);
    }
    if !wants(Target::Prf01) && !wants(Target::Prf03) {
        return rows;
    }
    let mut rig = match start_rig(options).await {
        Ok(rig) => rig,
        Err(why) => {
            for t in [Target::Prf01, Target::Prf03]
                .into_iter()
                .filter(|t| wants(*t))
            {
                rows.push(not_measured(t, "-", "-", why.clone()));
            }
            rows.sort_by_key(|r| r.id);
            return rows;
        }
    };
    let channels = rig.bridge_ports.len();
    let metric = if channels == 1 {
        "added latency p99".to_owned()
    } else {
        format!("added latency p99, worst of {channels} bridges")
    };
    let payload = options.payload;
    let bridged_up = join_all_ordered(
        rig.bridge_ports
            .iter()
            .map(|port| wait_until_bridged(*port, payload, Duration::from_secs(15)))
            .collect(),
    )
    .await;
    if let Err(why) = bridged_up
        && wants(Target::Prf01)
    {
        rows.push(not_measured(Target::Prf01, &metric, "< 2 ms", why));
    }
    tokio::time::sleep(options.settle).await;
    let idle = rig.engine.pid().and_then(resident_kib);
    if wants(Target::Prf01) && rows.iter().all(|r| r.id != "PRF-01") {
        match latency_runs(&rig, options).await {
            Ok(run) => {
                rows.push(latency_row(&rig, options, &run.direct, &run.bridged));
                rows.push(bridge_throughput_row(options, channels, run.elapsed));
                rows.extend(usage_rows(Target::Prf01, channels, &run.usage));
            }
            Err(why) => rows.push(not_measured(Target::Prf01, &metric, "< 2 ms", why)),
        }
    }
    if wants(Target::Prf03) {
        let after = rig.engine.pid().and_then(resident_kib);
        rows.push(memory_row(idle, after));
    }
    rig.engine.shutdown().await;
    rig.run_dir.remove();
    rows.sort_by_key(|r| r.id);
    rows
}

/// Renders the rows as an aligned table.
#[must_use]
pub fn table(rows: &[BenchRow]) -> Vec<String> {
    let header = ["target", "metric", "measured", "limit", "result"];
    let cells: Vec<[String; 5]> = rows
        .iter()
        .map(|r| {
            let result = match r.status {
                BenchStatus::Met => "met",
                BenchStatus::Missed => "MISSED",
                BenchStatus::NotMeasured => "not measured",
            };
            [
                r.id.to_owned(),
                r.metric.clone(),
                r.measured.clone(),
                r.limit.clone(),
                result.to_owned(),
            ]
        })
        .collect();
    let width = |i: usize| {
        cells
            .iter()
            .map(|c| c[i].chars().count())
            .chain([header[i].len()])
            .max()
            .unwrap_or(0)
    };
    let widths: Vec<usize> = (0..5).map(width).collect();
    let line = |c: &[String]| {
        c.iter()
            .zip(&widths)
            .map(|(s, w)| format!("{s:<w$}"))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_owned()
    };
    let mut out = vec![line(&header.map(str::to_owned))];
    for (row, c) in rows.iter().zip(&cells) {
        out.push(line(c));
        if !row.note.is_empty() {
            out.push(format!("    {}", row.note));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        let samples: Vec<u64> = (1..=100).collect();
        let s = stats(&samples).unwrap();
        assert_eq!((s.p50, s.p99, s.max), (50, 99, 100));
        assert_eq!(stats(&[7]).unwrap().p99, 7);
        assert!(stats(&[]).is_none());
    }

    #[test]
    fn milliseconds_keep_three_decimals() {
        assert_eq!(millis(1_999), "1.999 ms");
        assert_eq!(millis(50), "0.050 ms");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn series_measure_round_trips_through_an_echo_peer() {
        let echo = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = echo.local_addr().unwrap().port();
        tokio::spawn(echo_server(echo));
        let samples = series(port, 32, 5, 50).await.unwrap();
        assert_eq!(samples.len(), 50);
        assert!(
            stats(&samples).unwrap().p99 < 1_000_000,
            "a loopback echo takes well under a second"
        );
        assert!(
            series(1, 32, 0, 1).await.is_err(),
            "nothing listens on port 1"
        );
    }

    #[tokio::test]
    async fn a_corrupt_echo_cannot_pass_a_latency_run() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0_u8; 32];
            stream.read_exact(&mut bytes).await.unwrap();
            bytes[0] ^= 1;
            stream.write_all(&bytes).await.unwrap();
        });
        assert!(
            series(port, 32, 0, 1)
                .await
                .unwrap_err()
                .contains("changed the payload")
        );
        peer.await.unwrap();
    }

    #[test]
    fn rows_render_as_a_table_with_notes() {
        let rows = vec![
            memory_row(Some(50 * 1024), None),
            memory_row(Some(90 * 1024), Some(95 * 1024)),
            memory_row(None, None),
        ];
        assert_eq!(rows[0].status, BenchStatus::Met);
        assert_eq!(rows[1].status, BenchStatus::Missed);
        assert_eq!(rows[2].status, BenchStatus::NotMeasured);
        let text = table(&rows).join("\n");
        assert!(
            text.contains("MISSED")
                && text.contains("not measured")
                && text.contains("after the latency run"),
            "{text}"
        );
    }

    #[test]
    fn own_process_memory_is_readable() {
        let memory = resident_kib(std::process::id());
        if cfg!(windows) {
            assert_eq!(memory, None);
        } else {
            assert!(memory.is_some_and(|k| k > 0));
        }
    }

    #[test]
    fn generated_messages_are_unique_synthetic_hl7() {
        let (a, b) = (hl7_message(1), hl7_message(2));
        assert_eq!(hl7_field(&a, "MSH", 10), Some("B000000001"));
        assert_eq!(hl7_field(&a, "MSH", 10), Some(control_id(1).as_str()));
        assert_ne!(hl7_field(&a, "MSH", 10), hl7_field(&b, "MSH", 10));
        assert_eq!(hl7_field(&a, "MSH", 9), Some("ORU^R01"));
        assert_eq!(hl7_field(&a, "PID", 3), Some("MRN-00000^^^TEST"));
        assert!(a.ends_with('\r') && !a.contains('\n'));
    }

    #[test]
    fn frames_are_cut_from_a_partial_buffer() {
        let mut buffer = mllp_frame("MSH|a");
        buffer.extend_from_slice(&mllp_frame("MSH|b")[..4]);
        assert_eq!(take_frame(&mut buffer).as_deref(), Some("MSH|a"));
        assert_eq!(
            take_frame(&mut buffer),
            None,
            "the second frame is incomplete"
        );
        buffer.extend_from_slice(&mllp_frame("MSH|b")[4..]);
        assert_eq!(take_frame(&mut buffer).as_deref(), Some("MSH|b"));
        assert!(buffer.is_empty());
    }

    #[test]
    fn acknowledgements_name_code_and_control_id() {
        let ack = ack_message("B000000007");
        assert_eq!(ack_of(&ack), Some(("AA", "B000000007")));
        assert_eq!(ack_of("MSH|^~\\&|X\rMSA|AE|B1|boom\r"), Some(("AE", "B1")));
        assert_eq!(ack_of("MSH|^~\\&|X\r"), None);
    }

    #[test]
    fn throughput_is_messages_per_second() {
        assert_eq!(
            tenths_per_second(1000, Duration::from_secs(10)),
            Some(1_000)
        );
        assert_eq!(
            tenths_per_second(250, Duration::from_millis(500)),
            Some(5_000)
        );
        assert_eq!(tenths_per_second(1, Duration::ZERO), None);
        assert_eq!(per_second(1_234), "123.4 msg/s");
    }

    fn observed(committed_ms: u64, delivered: usize, delivered_ms: Option<u64>) -> Throughput {
        Throughput {
            channels: 1,
            measured: 1000,
            warmup: 100,
            committed_in: Duration::from_millis(committed_ms),
            latencies: vec![2_000; 1000],
            delivered,
            delivered_in: delivered_ms.map(Duration::from_millis),
            channel_tenths: vec![
                tenths_per_second(1000, Duration::from_millis(committed_ms)).unwrap(),
            ],
            channel_delivery_tenths: delivered_ms
                .and_then(|ms| tenths_per_second(1000, Duration::from_millis(ms)))
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn throughput_rows_need_rate_and_every_delivery() {
        let boundary = throughput_rows(&observed(10_000, 1100, Some(10_000)));
        assert_eq!(boundary[0].status, BenchStatus::Met);
        assert_eq!(boundary[0].limit, ">= 100 msg/s");
        assert_eq!(
            throughput_rows(&observed(10_001, 1100, Some(10_001)))[0].status,
            BenchStatus::Missed
        );
        let met = throughput_rows(&observed(5_000, 1100, Some(6_000)));
        assert_eq!(met.len(), 2);
        assert_eq!(met[0].status, BenchStatus::Met);
        assert_eq!(met[0].measured, "200.0 msg/s");
        assert_eq!(
            (met[1].measured.as_str(), met[1].status),
            ("2.000 ms", BenchStatus::Met)
        );
        let slow = throughput_rows(&observed(20_000, 1100, Some(20_000)));
        assert_eq!(slow[0].status, BenchStatus::Missed);
        let slow_delivery = throughput_rows(&observed(5_000, 1100, Some(20_000)));
        assert_eq!(slow_delivery[0].status, BenchStatus::Missed);
        let lost = throughput_rows(&observed(5_000, 1090, None));
        assert_eq!(lost[0].status, BenchStatus::Missed);
        assert!(lost[0].note.contains("1090 of 1100"), "{}", lost[0].note);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_message_is_acknowledged_and_counted_by_the_receiving_system() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let delivered = Arc::new(Received::default());
        tokio::spawn(receiving_system(listener, Arc::clone(&delivered)));
        let mut stream = connect_source(port, Duration::from_secs(5)).await.unwrap();
        let mut buffer = Vec::new();
        let latencies = send_series(&mut stream, &mut buffer, 0, 20).await.unwrap();
        assert_eq!(latencies.len(), 20);
        assert_eq!(delivered.count.load(Ordering::SeqCst), 20);
        assert!(wait_for_count(&delivered.count, 20, Duration::from_millis(10)).await);
        assert!(!wait_for_count(&delivered.count, 21, Duration::from_millis(10)).await);
    }

    #[tokio::test]
    async fn a_duplicate_or_corrupt_delivery_is_not_counted() {
        assert_bad_delivery(hl7_message(0)).await;
        assert_bad_delivery(hl7_message(1).replace("5.4", "6.4")).await;
    }

    async fn assert_bad_delivery(unexpected: String) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Received::default());
        let count = Arc::clone(&received);
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            receiving_connection(stream, count).await;
        });
        let mut stream = connect_source(port, Duration::from_secs(1)).await.unwrap();
        send_message(&mut stream, &mut Vec::new(), 0).await.unwrap();
        stream.write_all(&mllp_frame(&unexpected)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), peer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received.count.load(Ordering::SeqCst), 1);
        assert!(received.invalid.load(Ordering::SeqCst));
    }

    fn run_of(
        base: Instant,
        start_ms: u64,
        commit_ms: u64,
        delivery_ms: Option<u64>,
    ) -> ChannelRun {
        ChannelRun {
            started: base + Duration::from_millis(start_ms),
            committed_at: base + Duration::from_millis(commit_ms),
            latencies: vec![1_000; 100],
            delivered: 110,
            delivered_at: delivery_ms.map(|ms| base + Duration::from_millis(ms)),
        }
    }

    #[test]
    fn channels_aggregate_over_the_common_wall_time() {
        let base = Instant::now();
        let runs = [
            run_of(base, 0, 1_000, Some(1_100)),
            run_of(base, 50, 2_000, Some(2_050)),
        ];
        let t = aggregate(&runs, 100, 10);
        assert_eq!((t.channels, t.measured, t.warmup), (2, 200, 20));
        assert_eq!(t.committed_in, Duration::from_millis(2_000));
        assert_eq!(t.delivered_in, Some(Duration::from_millis(2_050)));
        assert_eq!((t.delivered, t.latencies.len()), (220, 200));
        // 100 messages in 1.0 s and in 1.95 s.
        assert_eq!(t.channel_tenths, vec![1_000, 512]);
        let missing = aggregate(&[run_of(base, 0, 1_000, None), runs[0].clone()], 100, 10);
        assert_eq!(missing.delivered_in, None, "one channel lost a message");
        assert_eq!(aggregate(&[], 100, 10).committed_in, Duration::ZERO);
    }

    #[test]
    fn many_channels_use_the_aggregate_target_and_report_individual_rates() {
        let t = |committed_ms| Throughput {
            channels: 50,
            measured: 5_000,
            warmup: 500,
            committed_in: Duration::from_millis(committed_ms),
            latencies: vec![2_000; 5_000],
            delivered: 5_500,
            delivered_in: Some(Duration::from_millis(committed_ms)),
            channel_tenths: vec![100; 50],
            channel_delivery_tenths: vec![100; 50],
        };
        let met = throughput_rows(&t(10_000));
        assert_eq!(met[0].measured, "500.0 msg/s");
        assert_eq!(met[0].limit, ">= 500 msg/s aggregate");
        assert_eq!(met[0].metric, "message channel throughput, 50 channels");
        assert_eq!(met[0].status, BenchStatus::Met);
        assert!(
            met[0]
                .note
                .contains("slowest 10.0 msg/s average 10.0 msg/s")
        );
        assert_eq!(throughput_rows(&t(10_001))[0].status, BenchStatus::Missed);
        let mut slow_delivery = t(10_000);
        slow_delivery.delivered_in = Some(Duration::from_millis(10_001));
        assert_eq!(
            throughput_rows(&slow_delivery)[0].status,
            BenchStatus::Missed
        );
        let mut missing = t(10_000);
        missing.delivered -= 1;
        assert_eq!(throughput_rows(&missing)[0].status, BenchStatus::Missed);
        // Per-channel rates remain useful diagnostics without imposing separate targets.
        let mut uneven = t(10_000);
        uneven.channel_tenths[0] = 90;
        uneven.channel_delivery_tenths[0] = 80;
        let row = &throughput_rows(&uneven)[0];
        assert_eq!(row.status, BenchStatus::Met);
        assert!(row.note.contains("slowest channel end to end 8.0 msg/s"));
        assert_eq!(prf02_limit(1), ">= 100 msg/s");
        assert_eq!(prf02_limit(2), ">= 500 msg/s aggregate");
        assert_eq!(prf02_metric(1), PRF02_METRIC);
    }

    #[test]
    fn channel_documents_have_distinct_ids_and_ports() {
        let ports: Vec<(u16, u16)> = (0..50).map(|i| (20_000 + 2 * i, 20_001 + 2 * i)).collect();
        for (document, kind) in [
            (throughput_document(&ports), "message"),
            (runtime_document(&ports), "bridge"),
        ] {
            let list = |key: &str, id: &str| -> Vec<String> {
                document[key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v[id].as_str().unwrap().to_owned())
                    .collect()
            };
            let channels = list("channels", "channel_id");
            let endpoints = list("endpoints", "endpoint_id");
            assert_eq!((channels.len(), endpoints.len()), (50, 100), "{kind}");
            let distinct = |v: &[String]| v.iter().collect::<std::collections::HashSet<_>>().len();
            assert_eq!(distinct(&channels), 50);
            assert_eq!(distinct(&endpoints), 100);
            let used: std::collections::HashSet<u64> = document["endpoints"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["settings"]["port"].as_u64().unwrap())
                .collect();
            assert_eq!(used.len(), 100, "every endpoint has its own port");
            assert!(document["channels"][0]["kind"] == kind);
        }
        // One channel keeps the plain ids.
        let single = throughput_document(&[(20_000, 20_001)]);
        assert_eq!(single["channels"][0]["channel_id"], "bench-messages");
        assert_eq!(
            runtime_document(&[(20_000, 20_001)])["channels"][0]["channel_id"],
            "bench-bridge"
        );
    }

    #[test]
    fn the_worst_bridge_sets_the_added_latency() {
        let direct = vec![vec![100; 100], vec![100; 100], vec![]];
        let bridged = vec![vec![300; 100], vec![900; 100], vec![5_000; 100]];
        let (index, d, b, added) = worst_added(&direct, &bridged).unwrap();
        assert_eq!((index, d.p99, b.p99, added), (1, 100, 900, 800));
        assert!(worst_added(&[], &[]).is_none());
    }

    #[test]
    fn cpu_time_is_read_from_proc_stat_and_ps() {
        let stat = "123 (packie (worker) x) S 1 123 123 0 -1 4194560 1000 0 0 0 150 50 0 0 20 0 8 0 100 1000 200";
        assert_eq!(
            parse_proc_stat(stat, 100),
            Some(Duration::from_secs(2)),
            "1.5 s user plus 0.5 s system"
        );
        assert_eq!(parse_proc_stat("garbage", 100), None);
        assert_eq!(parse_cputime("0:01.25"), Some(Duration::from_millis(1_250)));
        assert_eq!(
            parse_cputime("1:02:03.5"),
            Some(Duration::from_millis(3_723_500))
        );
        assert_eq!(
            parse_cputime("2-00:00:01"),
            Some(Duration::from_secs(172_801))
        );
        assert_eq!(parse_cputime(""), None);
        assert_eq!(parse_cputime("18446744073709551615-00:00:00"), None);
        assert_eq!(parse_cputime("18446744073709551615:00:00"), None);
        assert_eq!(cpu_time(std::process::id()).is_some(), !cfg!(windows));
        assert_eq!(
            cpu_percent_tenths(Duration::from_millis(1_500), Duration::from_secs(4)),
            Some(375)
        );
        assert_eq!(
            cpu_percent_tenths(Duration::from_secs(1), Duration::ZERO),
            None
        );
    }

    #[test]
    fn usage_rows_are_informational_or_not_measured() {
        let usage = Usage {
            before_kib: Some(40 * 1024),
            peak_kib: Some(60 * 1024),
            after_kib: Some(55 * 1024),
            cpu: Some(Duration::from_secs(2)),
            wall: Duration::from_secs(4),
        };
        let rows = usage_rows(Target::Prf02, 50, &usage);
        assert_eq!(rows[0].metric, "engine resident memory under load");
        assert_eq!(rows[0].measured, "peak 60.0 MB");
        assert_eq!(rows[1].metric, "engine cpu under load");
        assert_eq!(rows[1].measured, "50.0% of one core");
        assert!(
            rows.iter()
                .all(|r| r.limit == "informational" && r.status == BenchStatus::Met)
        );
        let none = usage_rows(Target::Prf01, 1, &Usage::default());
        assert!(none.iter().all(|r| r.status == BenchStatus::NotMeasured));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_sampler_reads_a_running_process() {
        let sampler = UsageSampler::start(Some(std::process::id())).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let usage = sampler.finish().await;
        if cfg!(windows) {
            assert!(usage.before_kib.is_none() && usage.peak_kib.is_none());
            assert!(usage.after_kib.is_none() && usage.cpu.is_none());
            assert!(
                usage_rows(Target::Prf01, 1, &usage)
                    .iter()
                    .all(|row| row.status == BenchStatus::NotMeasured)
            );
        } else {
            assert!(usage.peak_kib.is_some_and(|k| k > 0) && usage.cpu.is_some());
        }
        assert!(usage.peak_kib >= usage.before_kib && usage.peak_kib >= usage.after_kib);
    }

    async fn task(fail: bool) -> Result<u8, String> {
        if fail {
            return Err("boom".to_owned());
        }
        std::future::pending().await
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn channels_run_together_and_the_first_failure_wins() {
        let ready = Arc::new(Barrier::new(3));
        let mut tasks = Vec::new();
        let mut counters = Vec::new();
        for _ in 0..2 {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let delivered = Arc::new(Received::default());
            tokio::spawn(receiving_system(listener, Arc::clone(&delivered)));
            counters.push(Arc::clone(&delivered));
            tasks.push(drive_channel(5, 20, port, delivered, Arc::clone(&ready)));
        }
        let all = tokio::spawn(join_all_ordered(tasks));
        ready.wait().await;
        assert!(counters.iter().all(|c| c.count.load(Ordering::SeqCst) == 5));
        ready.wait().await;
        let runs = all.await.unwrap().unwrap();
        let t = aggregate(&runs, 20, 5);
        assert_eq!((t.channels, t.measured, t.delivered), (2, 40, 50));
        assert!(t.delivered_in.is_some() && t.channel_tenths.len() == 2);
        assert!(
            counters
                .iter()
                .all(|c| c.count.load(Ordering::SeqCst) == 25)
        );
        let failing = vec![task(false), task(true)];
        assert_eq!(join_all_ordered(failing).await, Err("boom".to_owned()));
    }
}
