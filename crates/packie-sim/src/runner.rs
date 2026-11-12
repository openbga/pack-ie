//! The scenario runner: allocates resources, starts actors and the engine, executes the
//! steps in order, applies the global assertions, tears everything down and reports.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::actors::RtActor;
use crate::actors::folder::FolderActor;
use crate::actors::serial::SerialPeer;
use crate::actors::tcp::{TcpClient, TcpServer};
use crate::engine::{EngineHost, EngineSpec, Needles, bootstrap_document, bootstrap_endpoint};
use crate::error::{ScenarioError, SimError};
use crate::exec;
use crate::ports::PortLease;
use crate::report::{Outcome, ScenarioReport, StepReport, StepStatus};
use crate::resources::{Allocation, Capabilities, allocate};
use crate::scenario::{Actor, Capability, ListenMode, ResolvedScenario, Scenario, StartMode};
use crate::steps::Action;

/// Time allowed for the engine to start: process launch, config load, store open and the
/// first snapshot. About a second on an idle host; the margin covers a debug build, a cold
/// disk cache and several engines starting at once under `--jobs`.
pub(crate) const ENGINE_START: Duration = Duration::from_secs(20);
/// Engine output lines in a failure report, and with `--verbose`.
const OUTPUT_TAIL: usize = 20;
const OUTPUT_TAIL_VERBOSE: usize = 100;
/// Log events in the failure report of a `--verbose` run.
const LOG_TAIL_VERBOSE: usize = 60;

/// Called after every step, for live progress.
pub type StepCallback = Arc<dyn Fn(&StepReport) + Send + Sync>;

/// How the host runs scenarios.
#[derive(Clone)]
pub struct HostOptions {
    /// The `packie` binary.
    pub engine_bin: PathBuf,
    /// Keep the run directory even when the scenario passes.
    pub keep_state: bool,
    /// A failure report carries more engine output and the engine's log tail.
    pub verbose: bool,
    /// What the host provides.
    pub capabilities: Capabilities,
    /// Live progress.
    pub on_step: Option<StepCallback>,
}

impl std::fmt::Debug for HostOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostOptions")
            .field("engine_bin", &self.engine_bin)
            .field("keep_state", &self.keep_state)
            .field("verbose", &self.verbose)
            .finish_non_exhaustive()
    }
}

/// Finds the engine binary, first match wins:
///
/// 1. `explicit` (`--engine`); it must exist, there is no fallback.
/// 2. The `PACKIE_BIN` environment variable; it must exist too.
/// 3. `packie` next to the running executable, then one directory up, which covers both
///    `target/<profile>/packie-sim` and `target/<profile>/deps/<test>`.
///
/// So after `cargo build -p packie -p packie-sim` the engine of the same profile is found
/// without any option.
pub fn locate_engine(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "packie.exe"
    } else {
        "packie"
    };
    if let Some(path) = explicit {
        return check_file(path);
    }
    if let Some(path) = std::env::var_os("PACKIE_BIN") {
        return check_file(Path::new(&path));
    }
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot find the running executable: {e}"))?;
    let mut tried = Vec::new();
    for dir in exe.ancestors().skip(1).take(2) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }
    Err(format!(
        "no engine binary: pass --engine, set PACKIE_BIN, or build `packie` (looked in {})",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn check_file(path: &Path) -> Result<PathBuf, String> {
    if path.is_file() {
        Ok(path.to_owned())
    } else {
        Err(format!("engine binary {} does not exist", path.display()))
    }
}

/// Called with each scenario's report as soon as it completes.
pub type DoneCallback = Arc<dyn Fn(&ScenarioReport) + Send + Sync>;

/// Runs every file, up to `jobs` at a time, and returns the results in file order.
///
/// Ports, run directories and serial pairs are allocated per run, so scenarios do not
/// clash. Scenarios that require `serial_pair` or `test_hooks` still run one at a time,
/// after the others, because they touch process-wide or device-level state.
pub async fn run_many(
    files: &[PathBuf],
    host: &HostOptions,
    jobs: usize,
    on_done: Option<DoneCallback>,
) -> Vec<Result<ScenarioReport, ScenarioError>> {
    let serial = |path: &PathBuf| {
        Scenario::load(path).is_ok_and(|s| {
            s.requires
                .iter()
                .any(|c| matches!(c, Capability::SerialPair | Capability::TestHooks))
        })
    };
    let (serial_files, parallel_files): (Vec<_>, Vec<_>) = files
        .iter()
        .cloned()
        .enumerate()
        .partition(|(_, p)| serial(p));
    let mut results: Vec<Option<Result<ScenarioReport, ScenarioError>>> =
        (0..files.len()).map(|_| None).collect();
    let limit = Arc::new(tokio::sync::Semaphore::new(jobs.max(1)));
    let mut set = tokio::task::JoinSet::new();
    for (index, path) in parallel_files {
        let (host, limit, on_done) = (host.clone(), Arc::clone(&limit), on_done.clone());
        set.spawn(async move {
            let _permit = limit.acquire_owned().await;
            (index, run_and_notify(&path, &host, on_done.as_ref()).await)
        });
    }
    while let Some(joined) = set.join_next().await {
        if let Ok((index, result)) = joined {
            results[index] = Some(result);
        }
    }
    for (index, path) in serial_files {
        results[index] = Some(run_and_notify(&path, host, on_done.as_ref()).await);
    }
    results
        .into_iter()
        .map(|r| {
            r.unwrap_or_else(|| {
                Err(ScenarioError::invalid(
                    Path::new(""),
                    "scenario task failed",
                ))
            })
        })
        .collect()
}

async fn run_and_notify(
    path: &Path,
    host: &HostOptions,
    on_done: Option<&DoneCallback>,
) -> Result<ScenarioReport, ScenarioError> {
    let result = run_file(path, host).await;
    if let (Ok(report), Some(callback)) = (&result, on_done) {
        callback(report);
    }
    result
}

/// Everything a running scenario shares between its steps.
pub(crate) struct Run {
    pub(crate) scenario: ResolvedScenario,
    pub(crate) base_dir: PathBuf,
    pub(crate) alloc: Allocation,
    pub(crate) actors: BTreeMap<String, RtActor>,
    pub(crate) engine: EngineHost,
    pub(crate) ops: HashMap<String, JoinHandle<Result<(), String>>>,
    pub(crate) store_errors_allowed: bool,
}

/// Loads and runs the scenario file at `path`. `Err` only for an invalid file.
pub async fn run_file(path: &Path, host: &HostOptions) -> Result<ScenarioReport, ScenarioError> {
    let scenario = Scenario::load(path)?;
    let missing = host
        .capabilities
        .missing(&scenario.requires.iter().copied().collect());
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(|c| c.name()).collect();
        return Ok(ScenarioReport::skipped(
            &scenario.name,
            format!("missing capability {}", names.join(", ")),
        ));
    }
    let started = Instant::now();
    let alloc = match allocate(&scenario) {
        Ok(a) => a,
        Err(e) => return Ok(setup_failure(&scenario.name, &e, started, None)),
    };
    let resolved = scenario.resolve(&alloc.placeholders, path)?;
    let base_dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_owned);

    let mut report = match Run::prepare(resolved, base_dir, alloc, host) {
        Ok(mut run) => {
            let report = run.execute(host, started).await;
            run.finish(report, host).await
        }
        Err(failed) => setup_failure(&scenario.name, &failed.error, started, Some(failed.kept)),
    };
    report.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(report)
}

fn setup_failure(
    name: &str,
    error: &SimError,
    started: Instant,
    kept_dir: Option<PathBuf>,
) -> ScenarioReport {
    ScenarioReport {
        name: name.to_owned(),
        outcome: Outcome::Failed,
        skip_reason: None,
        steps: Vec::new(),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        run_failure: Some(format!("setup failed: {error}")),
        global_failures: Vec::new(),
        kept_dir,
        engine_output: Vec::new(),
        engine_log: Vec::new(),
    }
}

/// A run that could not be set up: the cause, and the run directory that was kept.
struct SetupFailure {
    error: SimError,
    kept: PathBuf,
}

impl Run {
    /// Builds the actors and the engine host and writes the engine's files. On failure the
    /// run directory is kept, like the directory of any failed run.
    fn prepare(
        scenario: ResolvedScenario,
        base_dir: PathBuf,
        mut alloc: Allocation,
        host: &HostOptions,
    ) -> Result<Self, SetupFailure> {
        let (actors, engine) = match Self::build(&scenario, &base_dir, &mut alloc, host) {
            Ok(built) => built,
            // The files written so far help to see what went wrong.
            Err(error) => {
                return Err(SetupFailure {
                    error,
                    kept: alloc.run_dir.keep(),
                });
            }
        };
        let store_errors_allowed = scenario.steps.iter().any(|s| {
            matches!(
                s.action,
                Action::HoldStoreCommits(_) | Action::CrashEngine(_)
            )
        });
        Ok(Self {
            scenario,
            base_dir,
            alloc,
            actors,
            engine,
            ops: HashMap::new(),
            store_errors_allowed,
        })
    }

    fn build(
        scenario: &ResolvedScenario,
        base_dir: &Path,
        alloc: &mut Allocation,
        host: &HostOptions,
    ) -> Result<(BTreeMap<String, RtActor>, EngineHost), SimError> {
        let mut actors = BTreeMap::new();
        for (name, def) in &scenario.actors {
            actors.insert(name.clone(), build_actor(name, def, alloc)?);
        }
        let root = alloc.run_dir.root();
        let runtime = exec::load_runtime_doc(
            scenario.engine.runtime.as_ref(),
            scenario.engine.runtime_file.as_deref(),
            base_dir,
            &alloc.placeholders,
        )
        .map_err(SimError::other)?;
        let runtime_path = root.join("runtime.json");
        write_json(&runtime_path, &runtime)?;
        let bootstrap = bootstrap_document(
            &scenario.name,
            &alloc.run_dir.state(),
            &alloc.run_dir.logs(),
            &runtime_path,
            scenario.requires.contains(&Capability::TestHooks),
            &scenario.engine.bootstrap,
        );
        let bootstrap_path = root.join("bootstrap.json");
        write_json(&bootstrap_path, &bootstrap)?;
        let drain = bootstrap["hosting"]["shutdown_drain_seconds"]
            .as_u64()
            .unwrap_or(15);
        let endpoint = bootstrap_endpoint(&bootstrap);
        let spec = EngineSpec {
            bin: host.engine_bin.clone(),
            bootstrap_path,
            state_dir: alloc.run_dir.state(),
            logs_dir: alloc.run_dir.logs(),
            endpoint,
            drain: Duration::from_secs(drain),
        };
        let needles = Needles::new(&scenario.no_body_leak);
        Ok((actors, EngineHost::new(spec, needles)))
    }

    /// Starts the engine (when `auto`) and executes the steps.
    async fn execute(&mut self, host: &HostOptions, started: Instant) -> ScenarioReport {
        let deadline = started + Duration::from_millis(self.scenario.timeout_ms);
        let mut report = ScenarioReport {
            name: self.scenario.name.clone(),
            outcome: Outcome::Passed,
            skip_reason: None,
            steps: Vec::new(),
            duration_ms: 0,
            run_failure: None,
            global_failures: Vec::new(),
            kept_dir: None,
            engine_output: Vec::new(),
            engine_log: Vec::new(),
        };
        let mut failed = false;
        // The engine binds these ports itself; until now the runner held them bound.
        self.alloc.release_engine_ports();
        // Startup counts toward the whole-scenario deadline, just like a start_engine
        // step. Teardown has its separate drain budget so a timed-out run still cleans up.
        let startup_budget = ENGINE_START.min(deadline.saturating_duration_since(Instant::now()));
        if self.scenario.engine.start == StartMode::Auto
            && let Err(why) = self.engine.start(startup_budget).await
        {
            report.run_failure = Some(format!("the engine did not start: {why}"));
            failed = true;
        }
        for index in 0..self.scenario.steps.len() {
            let record = self.run_one(index, deadline, failed).await;
            failed |= record.status == StepStatus::Failed;
            if let Some(callback) = &host.on_step {
                callback(&record);
            }
            report.steps.push(record);
        }
        if failed {
            report.outcome = Outcome::Failed;
        }
        report
    }

    /// Runs step `index` unless an earlier one failed (`skip`), within the scenario deadline.
    async fn run_one(&mut self, index: usize, deadline: Instant, skip: bool) -> StepReport {
        let step = self.scenario.steps[index].clone();
        let mut record = StepReport {
            scenario: self.scenario.name.clone(),
            index,
            id: step.id.clone(),
            action: step.action.name().to_owned(),
            actor: step.actor.clone(),
            status: StepStatus::NotRun,
            duration_ms: 0,
            detail: None,
        };
        if skip {
            return record;
        }
        let begun = Instant::now();
        let result = tokio::time::timeout_at(deadline, exec::run_step(self, &step)).await;
        record.duration_ms = u64::try_from(begun.elapsed().as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok(Ok(note)) => {
                record.status = StepStatus::Passed;
                record.detail = note;
            }
            Ok(Err(why)) => {
                record.status = StepStatus::Failed;
                record.detail = Some(why);
            }
            Err(_) => {
                record.status = StepStatus::Failed;
                record.detail = Some(format!(
                    "the scenario timeout of {} ms was exceeded",
                    self.scenario.timeout_ms
                ));
            }
        }
        record
    }

    /// Tears down, applies the end-of-run global assertions and settles the run directory.
    async fn finish(mut self, mut report: ScenarioReport, host: &HostOptions) -> ScenarioReport {
        for (_, op) in self.ops.drain() {
            op.abort();
        }
        for actor in self.actors.values() {
            actor.shutdown().await;
        }
        self.engine.shutdown().await;
        let globals = self.engine.global_failures(self.store_errors_allowed);
        if !globals.is_empty() {
            report.outcome = Outcome::Failed;
            report.global_failures = globals;
        }
        if report.outcome == Outcome::Failed {
            report.engine_output = self.engine.recent_output(if host.verbose {
                OUTPUT_TAIL_VERBOSE
            } else {
                OUTPUT_TAIL
            });
            if host.verbose {
                report.engine_log = self.engine.log_tail(LOG_TAIL_VERBOSE);
            }
        }
        if report.outcome == Outcome::Failed || host.keep_state {
            report.kept_dir = Some(self.alloc.run_dir.keep());
        } else {
            self.alloc.run_dir.remove();
        }
        report
    }
}

/// Creates the actor `name` of definition `def`.
fn build_actor(name: &str, def: &Actor, alloc: &mut Allocation) -> Result<RtActor, SimError> {
    let missing =
        |what: &str, key: &str| SimError::other(format!("{what} `{key}` was not allocated"));
    Ok(match def {
        Actor::TcpServer(s) => {
            let lease = alloc.server_leases.get_mut(name);
            let port = lease.as_ref().map_or(0, |l| l.port());
            let server = TcpServer::new(&s.bind, port, s.backlog, s.recv_buffer_bytes)?;
            // A socket that is bound but not listening makes connects hang on macOS instead
            // of refusing them, so only a server that listens at once gets it. For `listen:
            // manual` it is closed here; the port lock keeps the port reserved.
            let socket = lease.and_then(PortLease::take_socket);
            if s.listen == ListenMode::Auto {
                if let Some(socket) = socket {
                    server.adopt(socket);
                }
                server.listen()?;
            }
            RtActor::Server(server)
        }
        Actor::TcpClient(c) => RtActor::Client(TcpClient::new(
            c.bind_source.as_deref(),
            c.recv_buffer_bytes,
        )?),
        Actor::SerialPeer(s) => {
            let pair = alloc
                .serial_pairs
                .get(&s.pair)
                .ok_or_else(|| missing("serial pair", &s.pair))?;
            RtActor::Serial(SerialPeer::new(Arc::clone(pair)))
        }
        Actor::Folder(f) => {
            let dir = alloc
                .folders
                .get(&f.resource)
                .ok_or_else(|| missing("folder", &f.resource))?;
            RtActor::Folder(FolderActor::new(dir.clone()))
        }
    })
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), SimError> {
    let text = serde_json::to_string_pretty(value).map_err(|e| SimError::other(e.to_string()))?;
    std::fs::write(path, text).map_err(|e| SimError::io(format!("write {}", path.display()), e))
}
