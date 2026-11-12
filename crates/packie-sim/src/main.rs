//! `packie-sim`: validates and runs scenario files against a real `packie` engine process.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use packie_sim::bench::{BenchOptions, BenchStatus, Target, table};
use packie_sim::glob::Glob;
use packie_sim::report::{Outcome, ScenarioReport, StepReport, StepStatus, junit};
use packie_sim::resources::Capabilities;
use packie_sim::runner::{DoneCallback, HostOptions, locate_engine, run_file, run_many};
use packie_sim::scenario::{Capability, Scenario};
use packie_sim::soak::SoakOptions;

/// Scripted peers and the scenario runner for Positive ACK.
#[derive(Debug, Parser)]
#[command(name = "packie-sim", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate scenario files without starting anything.
    Check {
        /// A scenario file or a directory of them.
        path: PathBuf,
    },
    /// Run one scenario against the engine.
    Run {
        /// The scenario file.
        file: PathBuf,
        #[command(flatten)]
        options: RunOptions,
    },
    /// Run every scenario in a directory and print a summary.
    ///
    /// Exit code: 0 when every selected scenario passed or was skipped; 1 when one failed
    /// (or one was skipped under --deny-skips); 2 when nothing could run or be reported: a
    /// scenario file is invalid, --filter selects nothing, or the --junit file cannot be
    /// written. The JUnit report is written whenever scenarios ran, also when some failed.
    /// Scenarios run in parallel on ports reserved from a range below the operating
    /// system's ephemeral range, so they cannot collide.
    RunAll {
        /// The directory.
        dir: PathBuf,
        #[command(flatten)]
        options: RunOptions,
        /// Count skipped scenarios as failures.
        #[arg(long)]
        deny_skips: bool,
        /// Scenarios to run at the same time. Scenarios that require `serial_pair` or
        /// `test_hooks` always run alone, after the others.
        #[arg(long, default_value_t = 1)]
        jobs: usize,
        /// Write a JUnit XML report (one test case per scenario) to this file.
        #[arg(long, value_name = "PATH")]
        junit: Option<PathBuf>,
        /// Run only scenarios whose file name (without `.json`) or `name` matches this
        /// pattern: `*` is any text, `?` one character. May be repeated; a scenario
        /// matching any pattern runs.
        #[arg(long, value_name = "GLOB")]
        filter: Vec<String>,
    },
    /// Measure the performance requirements.
    Bench(BenchArgs),
    /// Run the engine for hours under a steady load with injected faults.
    ///
    /// 20 bridges and 10 message channels carry a realistic load while a seeded plan of
    /// faults (cut connections, receivers that stop reading, a destination down for minutes,
    /// config applies, pause and resume, a `kill -9` every hour, a store blocked by a file)
    /// is played. Every 30 seconds the engine's memory, CPU, descriptors, store, logs and
    /// snapshot timing are recorded; after each kill and at the end the messages the
    /// instruments sent are reconciled with what the receiving systems got. The report is
    /// `<out>/report.md`. Exit code: 0 when every assertion passed, 1 when one failed, 2
    /// when the run could not start. Run it with an open files limit of at least 1024.
    Soak(SoakArgs),
}

/// Parameters of `soak`.
#[derive(Debug, Args)]
struct SoakArgs {
    /// Hours the load runs; the drain and the final checks come after.
    #[arg(long, default_value_t = 1.0)]
    hours: f64,
    /// Chaos level: 0 none, 1 rare and short, 2 a fault every minute or two, 3 aggressive.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(0..=3))]
    chaos: u8,
    /// The `packie` binary (default: `PACKIE_BIN`, then next to this executable). It is copied
    /// into the output directory.
    #[arg(long)]
    engine: Option<PathBuf>,
    /// The output directory; it must not hold an earlier run.
    #[arg(long)]
    out: PathBuf,
    /// Seed of the chaos plan and every random choice; random when absent, and printed.
    #[arg(long)]
    seed: Option<u64>,
    /// Mean milliseconds between messages of a message channel (500 is 2 per second).
    #[arg(long, default_value_t = 500, value_parser = clap::value_parser!(u64).range(10..))]
    message_interval_ms: u64,
    /// KiB per second each bridge carries.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    bridge_kib_per_sec: u64,
    /// Seconds between samples.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..))]
    sample_seconds: u64,
    /// Minutes without chaos at the start (default: 15% of the run, at most 10).
    #[arg(long)]
    warmup_minutes: Option<u64>,
    /// Minutes between engine kills.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(5..))]
    restart_minutes: u64,
    /// Messages kept per message channel.
    #[arg(long, default_value_t = 20_000, value_parser = clap::value_parser!(u64).range(1000..))]
    retention_messages: u64,
    /// Seconds the final drain may take.
    #[arg(long, default_value_t = 240)]
    drain_seconds: u64,
    /// Only this kind of chaos episode (may be repeated): source_disconnect, sink_disconnect,
    /// sink_stall, destination_down, config_apply, pause_resume, slow_folder_write,
    /// astm_back_to_back, crash_restart, store_block.
    #[arg(long = "only", value_name = "KIND")]
    only: Vec<String>,
    /// Log level of the engine (`debug` or `trace` to chase a problem; the log grows fast).
    #[arg(long, default_value = "info", value_parser = ["error", "warn", "info", "debug", "trace"])]
    engine_log_level: String,
}

/// Parameters of `bench`.
#[derive(Debug, Args)]
struct BenchArgs {
    /// The `packie` binary (default: `PACKIE_BIN`, then next to this executable).
    #[arg(long)]
    engine: Option<PathBuf>,
    /// What to measure; default: every target that can be measured.
    #[arg(long = "target", value_enum)]
    targets: Vec<BenchTarget>,
    /// Measured round trips per series.
    #[arg(long, default_value_t = 2000)]
    iterations: usize,
    /// Round trips discarded before measuring.
    #[arg(long, default_value_t = 200)]
    warmup: usize,
    /// Payload bytes of one request.
    #[arg(long, default_value_t = 64)]
    payload: usize,
    /// Seconds the engine idles before its memory is read.
    #[arg(long, default_value_t = 3)]
    settle: u64,
    /// Channels driven at the same time by PRF-01 and PRF-02 (PRF-02: 50 per engine).
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    channels: u32,
    /// Exit non-zero when a target is missed, or a requested target cannot be measured.
    #[arg(long)]
    assert: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum BenchTarget {
    /// PRF-01: added bridge latency.
    Prf01,
    /// PRF-02: message channel throughput.
    Prf02,
    /// PRF-03: idle memory.
    Prf03,
}

#[derive(Debug, Args)]
struct RunOptions {
    /// The `packie` binary. Default: the `PACKIE_BIN` environment variable, else `packie` next to this
    /// executable (or one directory up), i.e. the same cargo profile directory. An explicit
    /// path or `PACKIE_BIN` that does not exist is an error, with no further fallback.
    #[arg(long)]
    engine: Option<PathBuf>,
    /// Keep the run directory even when the scenario passes.
    #[arg(long)]
    keep_state: bool,
    /// Print every step as it finishes, and on failure more engine output (100 lines) and
    /// the engine's last 60 log events.
    #[arg(long)]
    verbose: bool,
    /// Print one JSON object per step and a summary object per scenario.
    #[arg(long)]
    json: bool,
}

fn out(line: &str) {
    let _ = writeln!(std::io::stdout().lock(), "{line}");
}

fn err(line: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        err("packie-sim: cannot start the async runtime");
        return ExitCode::FAILURE;
    };
    runtime.block_on(async move {
        match cli.command {
            Command::Check { path } => check(&path),
            Command::Run { file, options } => run_one(&file, &options).await,
            Command::RunAll {
                dir,
                options,
                deny_skips,
                jobs,
                junit,
                filter,
            } => {
                let selection = Selection {
                    deny_skips,
                    jobs,
                    junit,
                    filter,
                };
                run_all(&dir, &options, &selection).await
            }
            Command::Bench(args) => bench(&args).await,
            Command::Soak(args) => soak(&args).await,
        }
    })
}

/// Scenario files in `path`: the file itself, or the `*.json` files of a directory.
fn scenario_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no scenario files in {}", path.display()));
    }
    Ok(files)
}

/// Loads every file; prints the invalid ones. Returns the number of invalid files.
fn validate_all(files: &[PathBuf], show_ok: bool) -> usize {
    let mut invalid = 0;
    for file in files {
        match Scenario::load(file) {
            Ok(scenario) if show_ok => out(&format!(
                "ok       {} ({} steps)",
                scenario.name,
                scenario.steps.len()
            )),
            Ok(_) => {}
            Err(e) => {
                invalid += 1;
                out(&format!("INVALID  {e}"));
            }
        }
    }
    invalid
}

fn check(path: &Path) -> ExitCode {
    let files = match scenario_files(path) {
        Ok(f) => f,
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            return ExitCode::from(2);
        }
    };
    let invalid = validate_all(&files, true);
    out(&format!(
        "{} file(s) checked, {} valid, {invalid} invalid",
        files.len(),
        files.len() - invalid
    ));
    if invalid == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

async fn host(
    options: &RunOptions,
    json: bool,
    probe_engine: bool,
    show_names: bool,
) -> Result<HostOptions, String> {
    let engine_bin = locate_engine(options.engine.as_deref())?;
    let verbose = options.verbose && !json;
    Ok(HostOptions {
        keep_state: options.keep_state,
        verbose: options.verbose,
        capabilities: Capabilities::detect(&engine_bin, probe_engine).await,
        engine_bin,
        on_step: verbose.then(|| {
            Arc::new(move |step: &StepReport| {
                let mark = match step.status {
                    StepStatus::Passed => "ok",
                    StepStatus::Failed => "FAILED",
                    StepStatus::NotRun => return,
                };
                let name = if show_names {
                    format!("{}: ", step.scenario)
                } else {
                    String::new()
                };
                out(&format!(
                    "  {name}step {:>2} {:<22} {:<8} {mark} ({} ms)",
                    step.index, step.action, step.actor, step.duration_ms
                ));
            }) as Arc<dyn Fn(&StepReport) + Send + Sync>
        }),
    })
}

fn print_report(report: &ScenarioReport, options: &RunOptions) {
    if options.json {
        for line in report.json_lines() {
            out(&line);
        }
    } else {
        // Steps were already streamed in verbose mode.
        out(report.human(false).trim_end());
    }
}

async fn run_one(file: &Path, options: &RunOptions) -> ExitCode {
    let needs_hooks =
        Scenario::load(file).is_ok_and(|s| s.requires.contains(&Capability::TestHooks));
    let host = match host(options, options.json, needs_hooks, false).await {
        Ok(h) => h,
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            return ExitCode::FAILURE;
        }
    };
    match run_file(file, &host).await {
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            ExitCode::from(2)
        }
        Ok(report) => {
            print_report(&report, options);
            if report.outcome == Outcome::Failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}

/// What `run-all` runs and how it reports.
struct Selection {
    deny_skips: bool,
    jobs: usize,
    junit: Option<PathBuf>,
    filter: Vec<String>,
}

/// The files whose stem or scenario name matches one of `filters` (all when empty).
fn select(files: Vec<PathBuf>, filters: &[Glob]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|file| {
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let name = Scenario::load(file).ok().map(|s| s.name);
            filters
                .iter()
                .any(|g| g.matches(stem) || name.as_deref().is_some_and(|n| g.matches(n)))
        })
        .collect()
}

fn write_junit(path: &Path, reports: &[ScenarioReport]) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, junit(reports))
        .map_err(|e| format!("cannot write the JUnit report {}: {e}", path.display()))
}

async fn run_all(dir: &Path, options: &RunOptions, selection: &Selection) -> ExitCode {
    let filters: Result<Vec<Glob>, String> =
        selection.filter.iter().map(|p| Glob::new(p)).collect();
    let filters = match filters {
        Ok(f) => f,
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            return ExitCode::from(2);
        }
    };
    let files = match scenario_files(dir) {
        Ok(f) => select(f, &filters),
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            return ExitCode::from(2);
        }
    };
    if files.is_empty() {
        let patterns: Vec<&str> = filters.iter().map(Glob::pattern).collect();
        err(&format!(
            "packie-sim: no scenario in {} matches {}",
            dir.display(),
            patterns.join(", ")
        ));
        return ExitCode::from(2);
    }
    if validate_all(&files, false) > 0 {
        return ExitCode::from(2);
    }
    let jobs = selection.jobs;
    let host = match host(options, options.json, true, jobs > 1).await {
        Ok(h) => h,
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let json = options.json;
    let on_done: DoneCallback = Arc::new(move |report: &ScenarioReport| {
        if json {
            report.json_lines().iter().for_each(|l| out(l));
        } else {
            out(report.human(false).trim_end());
        }
    });
    let mut reports = Vec::new();
    for result in run_many(&files, &host, jobs, Some(on_done)).await {
        match result {
            Ok(report) => reports.push(report),
            Err(e) => {
                err(&format!("packie-sim: {e}"));
                return ExitCode::from(2);
            }
        }
    }
    let outcome = summarize(&reports, selection.deny_skips);
    if let Some(path) = &selection.junit
        && let Err(e) = write_junit(path, &reports)
    {
        err(&format!("packie-sim: {e}"));
        return ExitCode::from(2);
    }
    outcome
}

async fn bench(args: &BenchArgs) -> ExitCode {
    let engine_bin = match locate_engine(args.engine.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let requested: Vec<Target> = args
        .targets
        .iter()
        .map(|t| match t {
            BenchTarget::Prf01 => Target::Prf01,
            BenchTarget::Prf02 => Target::Prf02,
            BenchTarget::Prf03 => Target::Prf03,
        })
        .collect();
    let targets = if requested.is_empty() {
        Target::ALL
            .into_iter()
            .filter(|t| t.implemented())
            .collect()
    } else {
        requested.clone()
    };
    let options = BenchOptions {
        engine_bin,
        iterations: args.iterations.max(1),
        warmup: args.warmup,
        payload: args.payload.max(1),
        settle: std::time::Duration::from_secs(args.settle),
        targets,
        channels: usize::try_from(args.channels).unwrap_or(1).max(1),
    };
    let rows = packie_sim::bench::run(&options).await;
    table(&rows).iter().for_each(|l| out(l));
    let failed = rows.iter().any(|r| match r.status {
        BenchStatus::Missed => true,
        BenchStatus::NotMeasured => !requested.is_empty() || r.metric == "-",
        BenchStatus::Met => false,
    });
    if args.assert && failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn summarize(reports: &[ScenarioReport], deny_skips: bool) -> ExitCode {
    let width = reports
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(8)
        .max(8);
    out("");
    out(&format!(
        "{:<width$}  {:<7}  {:>8}  reason",
        "scenario", "result", "time"
    ));
    for r in reports {
        let result = match r.outcome {
            Outcome::Passed => "passed",
            Outcome::Failed => "failed",
            Outcome::Skipped => "skipped",
        };
        out(&format!(
            "{:<width$}  {result:<7}  {:>6} ms  {}",
            r.name,
            r.duration_ms,
            r.reason()
        ));
    }
    let count = |o: Outcome| reports.iter().filter(|r| r.outcome == o).count();
    let (passed, failed, skipped) = (
        count(Outcome::Passed),
        count(Outcome::Failed),
        count(Outcome::Skipped),
    );
    out(&format!(
        "\n{passed} passed, {failed} failed, {skipped} skipped"
    ));
    if failed > 0 || (deny_skips && skipped > 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `hours` as a duration; the one place the command line's fractional hours are converted.
#[allow(clippy::float_arithmetic)]
fn hours_to_duration(hours: f64) -> Duration {
    Duration::from_secs_f64((hours * 3600.0).max(0.0))
}

async fn soak(args: &SoakArgs) -> ExitCode {
    let engine = match locate_engine(args.engine.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            err(&format!("packie-sim: {e}"));
            return ExitCode::from(2);
        }
    };
    let mut options = SoakOptions::new(
        engine,
        args.out.clone(),
        hours_to_duration(args.hours),
        args.chaos,
    );
    options.seed = args.seed;
    options.message_interval_ms = args.message_interval_ms;
    options.bridge_bytes_per_sec = args.bridge_kib_per_sec * 1024;
    options.sample_interval = Duration::from_secs(args.sample_seconds);
    options.warmup = args.warmup_minutes.map(|m| Duration::from_secs(m * 60));
    options.restart_every = Duration::from_secs(args.restart_minutes * 60);
    options.retention_messages = args.retention_messages;
    options.drain = Duration::from_secs(args.drain_seconds);
    options.engine_log_level.clone_from(&args.engine_log_level);
    options.only.clone_from(&args.only);
    options.command = std::env::args().collect::<Vec<_>>().join(" ");
    options.on_progress = Some(Arc::new(|line: &str| out(&format!("soak: {line}"))));
    match packie_sim::soak::run(&options).await {
        Err(e) => {
            err(&format!("packie-sim: soak: {e}"));
            ExitCode::from(2)
        }
        Ok(result) => {
            out(&format!(
                "soak: {}; report {}",
                if result.passed { "PASS" } else { "FAIL" },
                result.report.display()
            ));
            result
                .failed
                .iter()
                .for_each(|f| out(&format!("soak: failed: {f}")));
            if result.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
