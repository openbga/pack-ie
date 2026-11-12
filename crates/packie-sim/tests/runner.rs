//! The runner against the real `packie` engine process.
//!
//! Covers routed channels, isolated actor exchanges, reporting and parallel execution.
#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use packie_sim::report::{Outcome, ScenarioReport, StepStatus};
use packie_sim::resources::Capabilities;
use packie_sim::runner::{HostOptions, locate_engine, run_file};
use packie_sim::scenario::Capability;

/// The one line to change when the engine routes bridge channels.
const ENGINE_ROUTES_CHANNELS: bool = true;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `packie` binary: found next to the test executable, or `PACKIE_BIN`, or built with
/// `cargo build -p packie` into the usual target directory (`CARGO_TARGET_DIR` respected).
fn engine_binary() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Ok(path) = locate_engine(None) {
            return path;
        }
        let status = Command::new("cargo")
            .args(["build", "-p", "packie"])
            .current_dir(workspace_root())
            .status()
            .expect("run cargo build -p packie");
        assert!(status.success(), "cargo build -p packie failed");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map_or_else(|| workspace_root().join("target"), PathBuf::from);
        let name = if cfg!(windows) {
            "packie.exe"
        } else {
            "packie"
        };
        let bin = target.join("debug").join(name);
        assert!(bin.is_file(), "built engine not found at {}", bin.display());
        bin
    })
    .clone()
}

fn host(capabilities: Capabilities) -> HostOptions {
    HostOptions {
        engine_bin: engine_binary(),
        keep_state: false,
        verbose: false,
        capabilities,
        on_step: None,
    }
}

async fn real_host() -> HostOptions {
    host(Capabilities::detect(&engine_binary(), false).await)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Writes `text` as `<name>.json` in a fresh directory.
fn scenario_file(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(format!("{name}.json"));
    std::fs::write(&path, text).unwrap();
    path
}

/// A scenario with no channels: the engine starts empty.
fn empty_engine_scenario(name: &str, requires: &str, timeout_ms: u64, steps: &str) -> String {
    format!(
        r#"{{"format": 1, "name": "{name}", "description": "d", "requirements": [],
             "requires": [{requires}], "timeout_ms": {timeout_ms},
             "engine": {{"runtime": {{"schema_version": 1}}}},
             "actors": {{"srv": {{"kind": "tcp_server"}}, "cli": {{"kind": "tcp_client"}}}},
             "steps": [{steps}]}}"#
    )
}

fn cleanup(report: &ScenarioReport) {
    if let Some(dir) = &report.kept_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bridge_tcp_full_duplex_against_the_current_engine() {
    let path = workspace_root().join("tests/scenarios/bridge-tcp-full-duplex.json");
    let started = std::time::Instant::now();
    let report = run_file(&path, &real_host().await).await.unwrap();
    assert!(started.elapsed().as_secs() < 30, "the run must not hang");
    if ENGINE_ROUTES_CHANNELS {
        assert_eq!(report.outcome, Outcome::Passed, "{}", report.human(true));
        assert!(
            report.kept_dir.is_none(),
            "a passing run removes its state directory"
        );
        return;
    }
    assert_eq!(report.outcome, Outcome::Failed, "{}", report.human(true));
    match report.failed_step() {
        // The engine runs but has no such channel: the first assert_snapshot fails.
        Some(step) => {
            assert_eq!(
                (step.index, step.action.as_str()),
                (0, "assert_snapshot"),
                "{}",
                report.human(true)
            );
            let detail = step.detail.clone().unwrap_or_default();
            assert!(
                detail.contains("session_state") || detail.contains("bridge-1"),
                "{detail}"
            );
            assert!(
                detail.contains("expected") || detail.contains("not in the snapshot"),
                "{detail}"
            );
        }
        // The engine does not yet accept the specified runtime document: it never starts.
        None => {
            let why = report.run_failure.clone().unwrap_or_default();
            assert!(why.contains("the engine did not start"), "{why}");
            assert!(
                !report.engine_output.is_empty(),
                "the engine's own error is reported"
            );
            assert!(report.steps.iter().all(|s| s.status == StepStatus::NotRun));
        }
    }
    // A failed run keeps its state directory (scenarios.md §4); the engine is gone.
    let kept = report
        .kept_dir
        .clone()
        .expect("failed runs keep the state directory");
    assert!(kept.join("state").is_dir());
    cleanup(&report);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_engine_without_channels_runs_a_scenario_with_restart() {
    let report = run_file(&fixture("engine-idle.json"), &real_host().await)
        .await
        .unwrap();
    assert_eq!(report.outcome, Outcome::Passed, "{}", report.human(true));
    assert!(report.steps.iter().all(|s| s.status == StepStatus::Passed));
    assert!(
        report.kept_dir.is_none(),
        "a passing run removes its state directory"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn async_sends_backpressure_and_protocol_exchanges_work_through_steps() {
    let report = run_file(&fixture("actors-flow.json"), &real_host().await)
        .await
        .unwrap();
    assert_eq!(report.outcome, Outcome::Passed, "{}", report.human(true));
    assert!(report.steps.iter().all(|s| s.status == StepStatus::Passed));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_assertion_is_reported_with_expected_and_observed_and_later_steps_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let steps = r#"
        {"do": "assert_snapshot", "actor": "engine", "target": {"node": true}, "within_ms": 0,
         "expect": {"node_state": "healthy", "channel_counts.running": 0}},
        {"do": "assert_snapshot", "actor": "engine", "target": {"node": true}, "within_ms": 200,
         "expect": {"node_state": "degraded", "channel_counts.running": 5, "missing.path": {"exists": true}}},
        {"do": "wait", "actor": "engine", "ms": 1}"#;
    let path = scenario_file(
        &dir,
        "failing-assert",
        &empty_engine_scenario("failing-assert", "", 30_000, steps),
    );
    let report = run_file(&path, &real_host().await).await.unwrap();

    assert_eq!(report.outcome, Outcome::Failed);
    let statuses: Vec<StepStatus> = report.steps.iter().map(|s| s.status).collect();
    assert_eq!(
        statuses,
        [StepStatus::Passed, StepStatus::Failed, StepStatus::NotRun]
    );
    let failed = report.failed_step().unwrap();
    assert_eq!(failed.index, 1);
    let detail = failed.detail.clone().unwrap();
    assert!(
        detail.contains(r#"node_state: expected "degraded", observed "healthy""#),
        "{detail}"
    );
    assert!(
        detail.contains("channel_counts.running: expected 5, observed 0"),
        "{detail}"
    );
    assert!(
        detail.contains("missing.path: expected a value, observed <absent>"),
        "{detail}"
    );
    assert!(detail.contains("polled for 200 ms"), "{detail}");
    assert!(
        !report.engine_output.is_empty(),
        "recent engine output accompanies a failure"
    );
    let human = report.human(false);
    assert!(
        human.contains("step 1 assert_snapshot (engine): FAILED"),
        "{human}"
    );
    assert!(human.contains("state kept:"), "{human}");
    let json = report.json_lines();
    assert_eq!(
        json.len(),
        4,
        "one line per step plus the summary: {json:?}"
    );
    assert!(
        json[1].contains(r#""status":"failed""#) && json[2].contains(r#""status":"not_run""#),
        "{json:?}"
    );

    let kept = report.kept_dir.clone().unwrap();
    assert!(kept.join("bootstrap.json").is_file());
    cleanup(&report);
    assert!(!kept.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_capabilities_skip_the_scenario_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let steps = r#"{"do": "crash_engine", "actor": "engine"}"#;
    let path = scenario_file(
        &dir,
        "needs-hooks",
        &empty_engine_scenario("needs-hooks", r#""test_hooks""#, 30_000, steps),
    );
    let report = run_file(&path, &host(Capabilities::of([]))).await.unwrap();
    assert_eq!(report.outcome, Outcome::Skipped);
    assert_eq!(
        report.skip_reason.as_deref(),
        Some("missing capability test_hooks")
    );
    assert!(report.steps.is_empty());
    let with = host(Capabilities::of([Capability::TestHooks]));
    // Offered, the scenario runs (and crashes the engine on purpose).
    let report = run_file(&path, &with).await.unwrap();
    assert_ne!(report.outcome, Outcome::Skipped, "{}", report.human(true));
    cleanup(&report);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_scenario_timeout_fails_the_running_step_and_stops_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let steps = r#"{"do": "wait", "actor": "engine", "ms": 20000}, {"do": "wait", "actor": "engine", "ms": 1}"#;
    let path = scenario_file(
        &dir,
        "too-slow",
        &empty_engine_scenario("too-slow", "", 1_500, steps),
    );
    let started = std::time::Instant::now();
    let report = run_file(&path, &real_host().await).await.unwrap();
    assert!(
        started.elapsed().as_secs() < 15,
        "the timeout ends the run promptly"
    );
    assert_eq!(report.outcome, Outcome::Failed);
    let failed = report.failed_step().unwrap();
    assert_eq!(failed.index, 0);
    assert!(
        failed
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("scenario timeout of 1500 ms"),
        "{:?}",
        failed.detail
    );
    assert!(
        report.global_failures.is_empty(),
        "the engine still stopped cleanly: {:?}",
        report.global_failures
    );
    cleanup(&report);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_scenario_file_is_an_error_not_a_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = scenario_file(&dir, "broken", r#"{"format": 1, "name": "broken"}"#);
    let error = run_file(&path, &real_host().await)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("broken.json"), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn body_leaks_are_caught_in_the_global_assertions() {
    let dir = tempfile::tempdir().unwrap();
    // The sentinel appears in the engine's own log output through a log filter text: the
    // runner plants it in the bootstrap's node name, which the engine reports in snapshots.
    let text = empty_engine_scenario(
        "leaky",
        "",
        30_000,
        r#"{"do": "wait", "actor": "engine", "ms": 1}"#,
    )
    .replace(
        r#""engine": {"runtime""#,
        r#""engine": {"bootstrap": {"node": {"display_name": "PID-TEST-LEAK"}}, "runtime""#,
    )
    .replace(
        r#""steps""#,
        r#""no_body_leak": ["PID-TEST-LEAK"], "steps""#,
    );
    let path = scenario_file(&dir, "leaky", &text);
    let report = run_file(&path, &real_host().await).await.unwrap();
    assert_eq!(report.outcome, Outcome::Failed, "{}", report.human(true));
    assert!(
        report
            .global_failures
            .iter()
            .any(|f| f.contains("body leak") && f.contains("PID-TEST-LEAK")),
        "{:?}",
        report.global_failures
    );
    cleanup(&report);
}

#[tokio::test(flavor = "multi_thread")]
async fn run_many_runs_scenarios_side_by_side_and_keeps_file_order() {
    use packie_sim::runner::run_many;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let files = [
        fixture("engine-idle.json"),
        fixture("actors-flow.json"),
        fixture("engine-idle.json"),
    ];
    let done = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&done);
    let callback: packie_sim::runner::DoneCallback = Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let results = run_many(&files, &real_host().await, 3, Some(callback)).await;
    let names: Vec<String> = results
        .iter()
        .map(|r| r.as_ref().unwrap().name.clone())
        .collect();
    assert_eq!(names, ["engine-idle", "actors-flow", "engine-idle"]);
    for result in &results {
        let report = result.as_ref().unwrap();
        assert_eq!(report.outcome, Outcome::Passed, "{}", report.human(true));
    }
    assert_eq!(done.load(Ordering::SeqCst), 3);
}

/// Scenarios that bind engine ports and `tcp_server` ports, fast and without capabilities.
const COLLISION_SUITE: [&str; 8] = [
    "bridge-close-propagation",
    "bridge-tap-hl7",
    "bridge-tcp-full-duplex",
    "bridge-pause-resume",
    "config-apply-per-channel",
    "msg-duplicate",
    "msg-hl7-in-ack",
    "msg-resend",
];

/// The regression test of the parallel-run flake. Scenarios used to probe a free port, release
/// it and bind it later, and a neighbour's outgoing connection could take it in between
/// ("bind 127.0.0.1:<port>: Address already in use", about one full run in two). Eight
/// scenarios now run at once, twenty times over, which is 160 runs and over a thousand
/// reserved ports; any collision fails the test and names the scenario.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn eight_scenarios_at_once_twenty_times_never_collide_on_ports() {
    let files: Vec<PathBuf> = COLLISION_SUITE
        .iter()
        .map(|name| workspace_root().join(format!("tests/scenarios/{name}.json")))
        .collect();
    assert!(
        files.iter().all(|f| f.is_file()),
        "the collision suite exists"
    );
    let host = real_host().await;
    for round in 0..20 {
        let results = packie_sim::runner::run_many(&files, &host, 8, None).await;
        for result in results {
            let report = result.unwrap();
            let text = report.human(true);
            assert!(
                !text.contains("Address already in use"),
                "round {round}: port collision\n{text}"
            );
            assert_eq!(
                report.outcome,
                Outcome::Passed,
                "round {round}: {}",
                report.human(true)
            );
            assert!(report.kept_dir.is_none(), "round {round}: {text}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_run_keeps_its_state_and_a_passed_one_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let ok = scenario_file(
        &dir,
        "passes",
        &empty_engine_scenario(
            "passes",
            "",
            30_000,
            r#"{"do": "wait", "actor": "engine", "ms": 1}"#,
        ),
    );
    let passed = run_file(&ok, &real_host().await).await.unwrap();
    assert_eq!(passed.outcome, Outcome::Passed, "{}", passed.human(true));
    assert!(passed.kept_dir.is_none());
    assert!(!passed.human(true).contains("state kept"));

    let bad = scenario_file(
        &dir,
        "fails",
        &empty_engine_scenario(
            "fails",
            "",
            30_000,
            r#"{"do": "assert_snapshot", "actor": "engine", "target": {"node": true},
                "within_ms": 0, "expect": {"node_state": "failed"}}"#,
        ),
    );
    let failed = run_file(&bad, &real_host().await).await.unwrap();
    assert_eq!(failed.outcome, Outcome::Failed);
    let kept = failed
        .kept_dir
        .clone()
        .expect("a failed run keeps its directory");
    assert!(kept.join("state").is_dir() && kept.join("logs").is_dir());
    assert!(
        failed
            .human(false)
            .contains(&format!("state kept: {}", kept.display())),
        "the path is printed"
    );
    cleanup(&failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn verbose_failures_carry_the_engine_log_tail_and_quiet_ones_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let steps = r#"{"do": "assert_snapshot", "actor": "engine", "target": {"node": true},
                    "within_ms": 0, "expect": {"node_state": "failed"}}"#;
    let path = scenario_file(
        &dir,
        "verbose-fail",
        &empty_engine_scenario("verbose-fail", "", 30_000, steps),
    );
    let quiet = run_file(&path, &real_host().await).await.unwrap();
    assert!(quiet.engine_log.is_empty(), "{:?}", quiet.engine_log);
    cleanup(&quiet);

    let mut verbose_host = real_host().await;
    verbose_host.verbose = true;
    let verbose = run_file(&path, &verbose_host).await.unwrap();
    assert_eq!(verbose.outcome, Outcome::Failed);
    assert!(
        verbose
            .engine_log
            .iter()
            .any(|l| l.contains("node.started")),
        "{:?}",
        verbose.engine_log
    );
    let text = verbose.human(false);
    assert!(text.contains("engine log (last"), "{text}");
    let summary: serde_json::Value =
        serde_json::from_str(verbose.json_lines().last().unwrap()).unwrap();
    assert!(
        summary["engine_log"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().contains("node.started")),
        "verbose JSON also carries the log tail: {summary}"
    );
    cleanup(&verbose);
}

/// A listening IPC socket is not enough: a stalled subscription must still respect the
/// startup budget and leave no engine process behind.
#[cfg(unix)]
#[tokio::test]
async fn engine_startup_budget_includes_unresponsive_ipc() {
    use packie_sim::engine::{EngineHost, EngineSpec, Needles};
    use packie_sim::resources::RunDir;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let dir = RunDir::create().unwrap();
    let bin = dir.root().join("stalled-engine");
    // `exec` ensures killing the child also ends the sleeper; no grandchild survives.
    std::fs::write(&bin, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.state().join("packie.sock");
    let _listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let mut engine = EngineHost::new(
        EngineSpec {
            bin,
            bootstrap_path: dir.root().join("bootstrap.json"),
            state_dir: dir.state(),
            logs_dir: dir.logs(),
            endpoint: socket.to_string_lossy().into_owned(),
            drain: Duration::ZERO,
        },
        Needles::new(&[]),
    );
    let started = Instant::now();
    let error = engine.start(Duration::from_millis(200)).await.unwrap_err();
    assert!(error.contains("not ready within 200 ms"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!engine.is_running());
}
