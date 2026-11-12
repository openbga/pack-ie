//! `packie-sim soak`: runs a real engine for hours under a steady, realistic load while
//! random faults are injected, and checks that it neither loses data nor degrades.
//!
//! ```text
//! packie-sim soak --hours 8 --chaos 2 --engine target/release/packie --out soak/run-8h
//! ```
//!
//! # What runs
//!
//! | Part | What it is |
//! | --- | --- |
//! | Engine | The given `packie` binary, copied into the output directory so a rebuild does not change a run, started with `packie run` like the other simulator runs. |
//! | Topology | 20 bridges (two with HL7 taps), 7 HL7 channels, 2 ASTM channels mapped to HL7, 1 delimited folder channel (`topology`). |
//! | Peers | An instrument per channel sending at a steady rate (2 messages per second, 10 KiB per second per bridge by default) and a system receiving, acknowledging and recording (`peers`, `ledger`). |
//! | Chaos | Seeded episodes: disconnects in the middle of a message, receivers that stop reading, a destination down for minutes, config applies, pause and resume, a slow file, a `kill -9` every hour, a store blocked by a file (`chaos`). |
//! | Monitor | Every 30 seconds: process memory footprint (and RSS), CPU, descriptors, store, logs, snapshot timing, per channel counters, alerts (`monitor`). |
//! | Judgement | Reconciliation by control id after every kill and at the end (`reconcile`), growth bounds, counter agreement (`analysis`, `checks`). |
//!
//! # Output
//!
//! `report.md` (assertions, episodes, reconciliations, graphs, anomalies with the engine log
//! around them), `samples.csv`, `channels.csv`, `chaos.jsonl`, `reconcile.jsonl`,
//! `alerts.jsonl` and the engine's own state and log directories under `engine/`. The exit
//! code of the command is 0 when every assertion passed, 1 when one failed and 2 when the
//! run could not start. A run ended early with Ctrl-C still drains and writes its report.
//!
//! # Load shape and runtime
//!
//! `--hours` is the time the load runs; the run then stops the instruments, waits for the
//! engine to drain (up to `--drain-seconds`) and reconciles. No episode starts in the last
//! tenth of the run (between 3 and 15 minutes), and none before the warm-up (the first 15%,
//! at most 10 minutes). Descriptors: the soak process holds about 250 and the engine about
//! 150; a shell limit of 1024 is plenty, 256 (the macOS default) is not.

mod analysis;
mod census;
mod chan;
mod chaos;
mod checks;
mod graphs;
mod ledger;
mod logscan;
mod monitor;
mod peers;
mod probe;
mod reconcile;
mod render;
mod rig;
mod samples;
mod snap;
mod topology;
mod util;
mod wire;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use packie_ipc::{Request, Response};
use serde_json::{Value, json};
use tokio::task::JoinSet;

use self::chan::{ChannelRt, Env};
use self::chaos::plan::{PlanInput, Planned, schedule};
use self::checks::{Facts, FinalState};
use self::render::{EngineInfo, Inputs};
use self::rig::{Journal, Owner, Paths, Rig};
use self::topology::{DocOptions, Topology};
use self::util::{Stop, hms, unix_ms};
use crate::engine::{EngineHost, EngineSpec, Needles, bootstrap_document};

/// A callback for progress lines.
pub type ProgressFn = Arc<dyn Fn(&str) + Send + Sync>;

/// Parameters of a soak run.
#[derive(Clone)]
pub struct SoakOptions {
    /// The `packie` binary under test.
    pub engine: PathBuf,
    /// The output directory; it must not hold an earlier run.
    pub out: PathBuf,
    /// How long the load runs.
    pub duration: Duration,
    /// Chaos level 0 to 3.
    pub chaos: u8,
    /// Seed of the chaos plan and of every random choice; random when absent.
    pub seed: Option<u64>,
    /// Mean time between messages of a message channel, milliseconds.
    pub message_interval_ms: u64,
    /// Bytes per second each bridge carries.
    pub bridge_bytes_per_sec: u64,
    /// Size of the HL7 messages of message channels.
    pub message_bytes: usize,
    /// Size of the records of plain bridges.
    pub bridge_record_bytes: usize,
    /// Size of the HL7 messages of tapped bridges.
    pub tap_message_bytes: usize,
    /// Time between samples.
    pub sample_interval: Duration,
    /// No chaos before this time; by default 15% of the run, at most 10 minutes.
    pub warmup: Option<Duration>,
    /// Period of the engine kill.
    pub restart_every: Duration,
    /// Messages kept per message channel.
    pub retention_messages: u64,
    /// How long the final drain may take.
    pub drain: Duration,
    /// Only these kinds of chaos episode (by name); all of the level's kinds when empty.
    pub only: Vec<String>,
    /// Log level of the engine: `info` normally; `debug` or `trace` to chase a problem.
    pub engine_log_level: String,
    /// The command line, for the report.
    pub command: String,
    /// Receives one line at a time about what the run is doing.
    pub on_progress: Option<ProgressFn>,
}

impl SoakOptions {
    /// Options with the defaults of the specification: 2 messages per second per message
    /// channel, 10 KiB per second per bridge, a kill every hour.
    #[must_use]
    pub fn new(engine: PathBuf, out: PathBuf, duration: Duration, chaos: u8) -> Self {
        Self {
            engine,
            out,
            duration,
            chaos,
            seed: None,
            message_interval_ms: 500,
            bridge_bytes_per_sec: 10 * 1024,
            message_bytes: 700,
            bridge_record_bytes: 1024,
            tap_message_bytes: 2048,
            sample_interval: Duration::from_secs(30),
            warmup: None,
            restart_every: Duration::from_secs(3600),
            retention_messages: 20_000,
            drain: Duration::from_secs(240),
            only: Vec::new(),
            engine_log_level: "info".to_owned(),
            command: String::new(),
            on_progress: None,
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone)]
pub struct SoakResult {
    /// Whether every assertion passed.
    pub passed: bool,
    /// `report.md`.
    pub report: PathBuf,
    /// The assertions that failed.
    pub failed: Vec<String>,
}

/// What stays the same during a run.
struct Context {
    options: SoakOptions,
    started_unix_ms: u64,
    seed: u64,
    engine: EngineInfo,
    plan: Vec<Planned>,
    warmup: Duration,
    host: String,
    load: String,
}

impl Context {
    fn progress(&self, line: &str) {
        if let Some(f) = &self.options.on_progress {
            f(line);
        }
    }
}

fn command_text(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn host_text() -> String {
    format!(
        "{}; open files limit {}",
        command_text("uname", &["-srm"]),
        command_text("sh", &["-c", "ulimit -n"])
    )
}

/// The default warm-up: 15% of the run, at most 10 minutes.
fn default_warmup(duration: Duration) -> Duration {
    (duration * 15 / 100).min(Duration::from_secs(600))
}

fn load_text(o: &SoakOptions) -> String {
    let tenths = 10_000 / o.message_interval_ms.max(1);
    format!(
        "20 bridges ({} KiB/s each, two with HL7 taps), 7 HL7 + 2 ASTM + 1 folder message channels ({}.{} messages/s each)",
        o.bridge_bytes_per_sec / 1024,
        tenths / 10,
        tenths % 10
    )
}

/// Copies the engine into the run directory.
fn copy_engine(opts: &SoakOptions, engine_dir: &Path) -> Result<EngineInfo, String> {
    let src = std::fs::canonicalize(&opts.engine)
        .map_err(|e| format!("cannot find the engine {}: {e}", opts.engine.display()))?;
    let copy = engine_dir.join(format!("packie{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(&src, &copy).map_err(|e| format!("cannot copy the engine: {e}"))?;
    let version = command_text(&copy.to_string_lossy(), &["--version"]);
    let bytes = std::fs::metadata(&copy).map_or(0, |m| m.len());
    Ok(EngineInfo {
        path: src,
        copy,
        version,
        bytes,
    })
}

/// Writes the runtime document and the bootstrap, and builds the engine host.
fn engine_host(
    paths: &Paths,
    engine: &EngineInfo,
    document: &Value,
    log_level: &str,
) -> Result<EngineHost, String> {
    let overrides = json!({
        "ipc": { "path": paths.ipc },
        "hosting": { "shutdown_drain_seconds": 5 },
        "logging": { "level": log_level, "retention_days": 3, "ring_buffer_events": checks::RING_EVENTS },
    });
    let bootstrap = bootstrap_document(
        "soak",
        &paths.state,
        &paths.logs,
        &paths.runtime,
        false,
        overrides.as_object().unwrap_or(&serde_json::Map::new()),
    );
    let bootstrap_path = paths.out.join("engine").join("bootstrap.json");
    for (path, value) in [(&paths.runtime, document), (&bootstrap_path, &bootstrap)] {
        let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    let spec = EngineSpec {
        bin: engine.copy.clone(),
        bootstrap_path,
        state_dir: paths.state.clone(),
        logs_dir: paths.logs.clone(),
        endpoint: paths.ipc.clone(),
        drain: Duration::from_secs(5),
    };
    let mut host = EngineHost::new(spec, Needles::default());
    host.set_lean();
    Ok(host)
}

fn prepare_dirs(out: &Path) -> Result<Paths, String> {
    let out = &std::path::absolute(out).map_err(|e| format!("bad output directory: {e}"))?;
    if out.join("report.md").exists() {
        return Err(format!(
            "{} already holds a soak run; use a new directory",
            out.display()
        ));
    }
    let engine_dir = out.join("engine");
    let (state, logs) = (engine_dir.join("state"), engine_dir.join("logs"));
    for dir in [&state, &logs, &out.join("folders")] {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let ipc = std::env::temp_dir().join(format!("packie-soak-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&ipc);
    let ipc = if cfg!(windows) {
        crate::engine::run_ipc_endpoint(&state)
    } else {
        ipc.to_string_lossy().into_owned()
    };
    Ok(Paths {
        out: out.to_owned(),
        runtime: engine_dir.join("runtime.json"),
        state,
        logs,
        ipc,
    })
}

fn progress_line(rig: &Rig) -> String {
    let (rss, up, fds, queued) = rig
        .monitor
        .lock()
        .ok()
        .and_then(|d| {
            d.samples
                .last()
                .map(|s| (s.memory_kib() >> 10, s.up, s.fds.total, s.deliveries_queued))
        })
        .unwrap_or_default();
    let episodes = rig.episodes.lock().map_or(0, |e| e.len());
    let failures = rig.journal.anomalies().iter().filter(|a| a.fatal).count();
    format!(
        "t={} engine {} memory {rss} MiB fds {fds} queued {queued}; {episodes} episodes done, {failures} failure(s)",
        hms(rig.now_ms() / 1000),
        if up { "up" } else { "down" }
    )
}

fn final_state(snapshot: Option<&Value>) -> FinalState {
    let Some(s) = snapshot else {
        return FinalState::default();
    };
    let states = snap::channel_states(s);
    FinalState {
        channels: states.len(),
        running: states
            .iter()
            .filter(|(_, state)| state == "running")
            .count(),
        store_ok: snap::text(&s["node"], &["store", "state"]) == "ok",
        queued: snap::number(&s["node"], &["totals", "deliveries_queued"]),
        node_state: snap::text(&s["node"], &["node_state"]).to_owned(),
    }
}

/// Builds the report from what the rig holds now; returns it with the assertions.
fn build_report(
    rig: &Rig,
    ctx: &Context,
    finished: bool,
    engine_failures: &[String],
    state: &FinalState,
) -> (String, Vec<checks::Check>) {
    let data = rig.monitor.lock().map(|d| d.clone()).unwrap_or_default();
    let episodes = rig.episodes.lock().map(|e| e.clone()).unwrap_or_default();
    let recons = rig.recons.lock().map(|r| r.clone()).unwrap_or_default();
    let anomalies = rig.journal.anomalies();
    let census = rig.census.lock().ok().and_then(|c| c.clone());
    let warmup_s = ctx.warmup.as_secs();
    let facts = Facts {
        data: &data,
        episodes: &episodes,
        recons: &recons,
        anomalies: &anomalies,
        engine_failures,
        final_state: state,
        chaos_level: ctx.options.chaos,
        warmup_s,
    };
    let checks = checks::evaluate(&facts);
    let text = render::render(&Inputs {
        finished,
        started_unix_ms: ctx.started_unix_ms,
        ended_unix_ms: unix_ms(),
        elapsed_s: rig.now_ms() / 1000,
        planned_s: ctx.options.duration.as_secs(),
        chaos_level: ctx.options.chaos,
        seed: ctx.seed,
        command: ctx.options.command.clone(),
        host: ctx.host.clone(),
        engine: &ctx.engine,
        plan: &ctx.plan,
        facts: &facts,
        census: census.as_ref(),
        checks: &checks,
        out: &rig.paths.out,
        logs: &rig.paths.logs,
        load: ctx.load.clone(),
    });
    (text, checks)
}

fn write_report(rig: &Rig, text: &str) -> PathBuf {
    let path = rig.paths.out.join("report.md");
    let _ = std::fs::write(&path, text);
    path
}

/// Writes an interim report every quarter of an hour and a progress line every five minutes.
async fn housekeeping(rig: Arc<Rig>, ctx: Arc<Context>) {
    let mut ticks = 0_u64;
    while rig.env.halt.sleep(Duration::from_secs(300)).await {
        ticks += 1;
        ctx.progress(&progress_line(&rig));
        if ticks.is_multiple_of(3) {
            let state = rig
                .monitor
                .lock()
                .ok()
                .and_then(|d| d.samples.last().cloned())
                .map(|s| FinalState {
                    channels: 30,
                    running: usize::try_from(s.running).unwrap_or(0),
                    store_ok: s.store_state == "ok",
                    queued: s.deliveries_queued,
                    node_state: s.node_state,
                })
                .unwrap_or_default();
            let (text, _) = build_report(&rig, &ctx, false, &[], &state);
            write_report(&rig, &text);
        }
    }
}

/// Checks that the engine took the document: every channel must exist.
async fn verify_started(rig: &Rig, document: &Value) -> Result<(), String> {
    let problems = match rig
        .call(Request::ValidateConfig {
            document: document.clone(),
        })
        .await
    {
        Ok(Response::Validation { problems, .. }) => {
            serde_json::to_value(&problems).unwrap_or_default()
        }
        _ => Value::Null,
    };
    let snapshot = rig.snapshot().await.ok_or("no snapshot after the start")?;
    let have = snapshot["channels"].as_array().map_or(0, Vec::len);
    if have != rig.channels.len() {
        return Err(format!(
            "the engine runs {have} of {} channels; validation says {problems}",
            rig.channels.len()
        ));
    }
    Ok(())
}

/// Starts the engine, the peers, the monitor and the chaos; returns the handles.
struct Running {
    peers: JoinSet<()>,
    monitor: tokio::task::JoinHandle<()>,
    watch: tokio::task::JoinHandle<()>,
    chaos: tokio::task::JoinHandle<()>,
    housekeeping: tokio::task::JoinHandle<()>,
}

fn launch(
    rig: &Arc<Rig>,
    ctx: &Arc<Context>,
    listeners: BTreeMap<String, tokio::net::TcpListener>,
) -> Running {
    let mut peers = JoinSet::new();
    peers::spawn_all(&rig.env, &rig.channels, listeners, &mut peers);
    let watch =
        tokio::spawn(Arc::clone(&rig.watch).follow(rig.paths.ipc.clone(), rig.env.halt.clone()));
    let monitor = tokio::spawn(monitor::run(Arc::clone(rig), ctx.options.sample_interval));
    let chaos = tokio::spawn(chaos::run(Arc::clone(rig), ctx.plan.clone()));
    let housekeeping = tokio::spawn(housekeeping(Arc::clone(rig), Arc::clone(ctx)));
    Running {
        peers,
        monitor,
        watch,
        chaos,
        housekeeping,
    }
}

/// The kinds of chaos episode with these names.
fn kinds_named(names: &[String]) -> Result<Vec<chaos::plan::Kind>, String> {
    names
        .iter()
        .map(|n| {
            chaos::plan::Kind::from_name(n).ok_or_else(|| {
                let all: Vec<&str> = chaos::plan::Kind::ALL.iter().map(|k| k.name()).collect();
                format!("unknown chaos kind `{n}`; the kinds are {}", all.join(", "))
            })
        })
        .collect()
}

/// What a run is made of before the load starts.
struct Prepared {
    rig: Arc<Rig>,
    ctx: Arc<Context>,
    listeners: BTreeMap<String, tokio::net::TcpListener>,
    /// Holds the ports for the length of the run.
    _topology: Topology,
}

/// What the peers share.
fn environment(opts: &SoakOptions, seed: u64, journal: &Arc<Journal>) -> Env {
    Env {
        journal: Arc::clone(journal),
        trace: std::env::var_os("PACKIE_SOAK_TRACE").is_some(),
        started: Instant::now(),
        stop: Stop::new(),
        halt: Stop::new(),
        seed,
        message_interval_ms: opts.message_interval_ms,
        bridge_bytes_per_sec: opts.bridge_bytes_per_sec,
        message_size: opts.message_bytes,
        bridge_record_size: opts.bridge_record_bytes,
        tap_message_size: opts.tap_message_bytes,
    }
}

/// Everything before the load: directories, the engine copy, ports, the rig.
fn prepare(opts: &SoakOptions) -> Result<Prepared, String> {
    if opts.duration < Duration::from_secs(120) {
        return Err("a soak run needs at least two minutes".to_owned());
    }
    let paths = prepare_dirs(&opts.out)?;
    let engine = copy_engine(opts, &paths.out.join("engine"))?;
    let seed = opts
        .seed
        .unwrap_or_else(|| unix_ms() ^ u64::from(std::process::id()) << 32);
    let mut topology = Topology::allocate(&paths.out.join("folders")).map_err(|e| e.to_string())?;
    let listeners = std::mem::take(&mut topology.listeners);
    let doc_options = DocOptions {
        retention_messages: opts.retention_messages,
    };
    let document = topology::runtime_document(&topology.plans, &BTreeMap::new(), &doc_options);
    let host = engine_host(&paths, &engine, &document, &opts.engine_log_level)?;
    let warmup = opts.warmup.unwrap_or_else(|| default_warmup(opts.duration));
    let only = kinds_named(&opts.only)?;
    let plan = schedule(&PlanInput {
        only: &only,
        level: opts.chaos,
        seed,
        duration: opts.duration,
        warmup,
        restart_every: opts.restart_every,
    });
    let journal = Arc::new(Journal::create(&paths.out)?);
    let env = Arc::new(environment(opts, seed, &journal));
    let channels: Vec<Arc<ChannelRt>> = topology
        .plans
        .iter()
        .cloned()
        .map(|p| Arc::new(ChannelRt::new(p)))
        .collect();
    let rig = Arc::new(Rig {
        env,
        journal,
        paths,
        channels,
        engine: tokio::sync::RwLock::new(host),
        knobs: Mutex::new(BTreeMap::new()),
        doc_options,
        watch: Arc::default(),
        incarnation: AtomicU32::new(0),
        down_on_purpose: AtomicBool::new(true),
        active_episodes: AtomicU32::new(0),
        last_episode_end_ms: AtomicU64::new(0),
        monitor: Mutex::new(monitor::MonitorData::default()),
        episodes: Mutex::new(Vec::new()),
        verdicts: Mutex::new(std::collections::HashMap::new()),
        recons: Mutex::new(Vec::new()),
        census: Mutex::new(None),
        targets: Mutex::new(chaos::Targets::default()),
    });
    let ctx = Arc::new(Context {
        started_unix_ms: unix_ms(),
        seed,
        plan,
        warmup,
        host: host_text(),
        load: load_text(opts),
        options: opts.clone(),
        engine,
    });
    rig.monitor
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .scan = logscan::LogScan::new(rig.paths.logs.clone());
    Ok(Prepared {
        rig,
        ctx,
        listeners,
        _topology: topology,
    })
}

/// Writes the chaos plan to `plan.jsonl`.
fn write_plan(rig: &Rig, ctx: &Context) {
    let lines: Vec<String> = ctx
        .plan
        .iter()
        .map(|p| {
            json!({ "at_s": p.at_ms / 1000, "kind": p.kind.name(), "seed": p.seed }).to_string()
        })
        .collect();
    let _ = std::fs::write(rig.paths.out.join("plan.jsonl"), lines.join("\n") + "\n");
}

/// Runs a soak. See the module documentation.
pub async fn run(opts: &SoakOptions) -> Result<SoakResult, String> {
    let Prepared {
        rig,
        ctx,
        listeners,
        _topology,
    } = prepare(opts)?;
    write_plan(&rig, &ctx);
    ctx.progress(&format!(
        "engine {} ({}); seed {}; {} chaos episodes planned over {}",
        ctx.engine.path.display(),
        ctx.engine.version,
        ctx.seed,
        ctx.plan.len(),
        hms(opts.duration.as_secs())
    ));
    let took = rig
        .start_engine()
        .await
        .map_err(|e| format!("the engine did not start: {e}"))?;
    verify_started(&rig, &rig.document()).await?;
    ctx.progress(&format!(
        "engine ready in {:.1} s; starting the load",
        took.as_secs_f64()
    ));
    let mut running = launch(&rig, &ctx, listeners);
    let interrupt = interrupt_on_ctrl_c(&rig);
    rig.env.stop.sleep(opts.duration).await;
    interrupt.abort();
    ctx.progress("load ends; checking the engine's health while the instruments still send");
    let health = end_health(&rig).await;
    rig.env.stop.set();
    ctx.progress("instruments stopped; waiting for the chaos episodes in progress, then the drain");
    let _ = (&mut running.chaos).await;
    let result = finish(&rig, &ctx, running, health).await;
    let _ = std::fs::remove_file(&rig.paths.ipc);
    result
}

/// The engine's state at the end of the load: waits for the last chaos episode to finish
/// (they stop starting before the end) and for every channel to run.
async fn end_health(rig: &Rig) -> FinalState {
    let began = tokio::time::Instant::now();
    while rig
        .active_episodes
        .load(std::sync::atomic::Ordering::SeqCst)
        > 0
        && began.elapsed() < Duration::from_secs(600)
        && !rig.env.halt.is_set()
    {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    loop {
        let state = final_state(rig.snapshot().await.as_deref());
        if (state.channels > 0 && state.running == state.channels)
            || began.elapsed() > Duration::from_secs(780)
        {
            return state;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn interrupt_on_ctrl_c(rig: &Arc<Rig>) -> tokio::task::JoinHandle<()> {
    let stop = rig.env.stop.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            stop.set();
        }
    })
}

/// Drains, reconciles a last time, stops everything and writes the report.
async fn finish(
    rig: &Arc<Rig>,
    ctx: &Arc<Context>,
    mut running: Running,
    health: FinalState,
) -> Result<SoakResult, String> {
    let drained = reconcile::wait_drained(rig, ctx.options.drain).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let last = reconcile::run(rig, "final", true, 0).await;
    ctx.progress(&format!(
        "final reconciliation: {}; {}",
        if last.ok {
            "no message lost or duplicated"
        } else {
            "FAILED"
        },
        if drained {
            "drained"
        } else {
            "queues had not emptied"
        }
    ));
    take_census(rig, ctx).await;
    let drained_state = final_state(rig.snapshot().await.as_deref());
    let state = FinalState {
        queued: drained_state.queued,
        ..health
    };
    rig.env.halt.set();
    let _ = (&mut running.monitor).await;
    let _ = (&mut running.watch).await;
    let _ = (&mut running.housekeeping).await;
    running.peers.abort_all();
    while running.peers.join_next().await.is_some() {}
    let engine_failures = stop_engine(rig).await;
    let (text, checks) = build_report(rig, ctx, true, &engine_failures, &state);
    let report = write_report(rig, &text);
    Ok(SoakResult {
        passed: render::passed(&checks),
        report,
        failed: checks
            .iter()
            .filter(|c| !c.pass)
            .map(|c| c.name.to_owned())
            .collect(),
    })
}

/// Counts what the store holds and notes where retention did not keep it in bounds.
async fn take_census(rig: &Arc<Rig>, ctx: &Arc<Context>) {
    let mut scan = rig
        .monitor
        .lock()
        .map(|d| d.scan.clone())
        .unwrap_or_default();
    scan.refresh();
    let census = census::take(rig, &scan).await;
    census::judge(rig, &census, ctx.options.retention_messages);
    if let Ok(mut slot) = rig.census.lock() {
        *slot = Some(census);
    }
}

/// Stops the engine with SIGTERM and returns what was wrong with its end: an exit that
/// needed a kill, an abnormal status, a panic on its console.
async fn stop_engine(rig: &Rig) -> Vec<String> {
    let mut failures = Vec::new();
    if let Err(why) = rig.stop_engine().await {
        failures.push(why);
    }
    {
        let engine = rig.engine.read().await;
        failures.extend(engine.problems().iter().cloned());
        if engine.panic_seen() {
            failures.push("the engine panicked (a `panicked at` line was printed)".to_owned());
        }
    }
    if let Ok(mut data) = rig.monitor.lock() {
        data.scan.refresh();
    }
    for message in &failures {
        rig.warn("engine_stop", Owner::Core, message.clone());
    }
    failures
}
