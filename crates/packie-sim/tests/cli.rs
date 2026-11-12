//! The `packie-sim` command line: exit codes and the shape of the output.
#![cfg(test)]

use std::path::Path;
use std::process::{Command, Output};

fn sim(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_packie-sim"))
        .args(args)
        .output()
        .expect("run packie-sim")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn scenarios() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/scenarios")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn check_accepts_the_suite_and_rejects_a_broken_file_with_exit_code_2() {
    let ok = sim(&["check", &scenarios()]);
    assert_eq!(ok.status.code(), Some(0));
    assert!(
        text(&ok.stdout).contains("30 file(s) checked, 30 valid, 0 invalid"),
        "{}",
        text(&ok.stdout)
    );

    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("broken.json");
    std::fs::write(&bad, r#"{"format": 1, "name": "broken"}"#).unwrap();
    let out = sim(&["check", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let shown = text(&out.stdout);
    assert!(
        shown.contains("INVALID") && shown.contains("broken.json") && shown.contains("description"),
        "{shown}"
    );
}

#[test]
fn bench_says_when_a_target_cannot_be_measured_and_asserts_only_on_request() {
    // A target that cannot run (the "engine" here is the simulator itself) is reported, and
    // asking for it explicitly with --assert must not pass.
    let engine = env!("CARGO_BIN_EXE_packie-sim");
    let run = |extra: &[&str]| {
        Command::new(engine)
            .args(["bench", "--target", "prf02", "--engine", engine])
            .args(extra)
            .output()
            .expect("run packie-sim")
    };
    let plain = run(&[]);
    assert_eq!(plain.status.code(), Some(0));
    assert!(
        text(&plain.stdout).contains("not measured"),
        "{}",
        text(&plain.stdout)
    );
    let asserted = run(&["--assert"]);
    assert_eq!(asserted.status.code(), Some(1));
}

#[test]
fn a_missing_engine_binary_is_reported_before_anything_runs() {
    let file = format!("{}/bridge-pause-resume.json", scenarios());
    let out = sim(&["run", &file, "--engine", "/nonexistent/packie"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("does not exist"),
        "{}",
        text(&out.stderr)
    );
}

/// Runs `run-all` over the scenario suite with `extra` options.
fn run_all(extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_packie-sim"))
        .arg("run-all")
        .arg(scenarios())
        .args(extra)
        .env("PACKIE_BIN", engine_binary())
        .output()
        .expect("run packie-sim")
}

/// The engine: `PACKIE_BIN`, next to the test binaries, or built by `cargo build -p packie`.
fn engine_binary() -> std::path::PathBuf {
    packie_sim::runner::locate_engine(None).unwrap_or_else(|_| {
        let status = Command::new("cargo")
            .args(["build", "-p", "packie"])
            .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .status()
            .expect("run cargo build -p packie");
        assert!(status.success(), "cargo build -p packie failed");
        let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            std::path::PathBuf::from,
        );
        target
            .join("debug")
            .join(format!("packie{}", std::env::consts::EXE_SUFFIX))
    })
}

#[test]
fn run_all_filters_by_glob_and_writes_a_junit_report() {
    let work = tempfile::tempdir().unwrap();
    let report = work.path().join("out/results.xml");
    let out = run_all(&[
        "--filter",
        "bridge-tcp-*",
        "--filter",
        "msg-hl7-in-ack",
        "--junit",
        report.to_str().unwrap(),
    ]);
    let shown = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{shown}{}", text(&out.stderr));
    assert!(shown.contains("2 passed, 0 failed, 0 skipped"), "{shown}");
    let xml = std::fs::read_to_string(&report).unwrap();
    assert!(
        xml.starts_with("<?xml")
            && xml.contains(r#"<testsuites name="packie-sim" tests="2" failures="0""#),
        "{xml}"
    );
    assert!(xml.contains(r#"<testcase classname="scenarios" name="bridge-tcp-full-duplex""#));
    assert!(!xml.contains("<failure"));
}

#[test]
fn run_all_with_a_failing_scenario_exits_1_and_still_writes_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let failing = r#"{"format": 1, "name": "always-fails", "description": "d", "requirements": [],
        "timeout_ms": 30000, "engine": {"runtime": {"schema_version": 1}},
        "steps": [{"do": "assert_snapshot", "actor": "engine", "target": {"node": true},
                   "within_ms": 0, "expect": {"node_state": "failed & <broken>"}}]}"#;
    std::fs::write(dir.path().join("always-fails.json"), failing).unwrap();
    let report = dir.path().join("junit.xml");
    let out = Command::new(env!("CARGO_BIN_EXE_packie-sim"))
        .arg("run-all")
        .arg(dir.path())
        .args(["--junit", report.to_str().unwrap()])
        .env("PACKIE_BIN", engine_binary())
        .output()
        .expect("run packie-sim");
    let shown = text(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{shown}{}", text(&out.stderr));
    assert!(
        shown.contains("state kept:"),
        "the kept path is printed: {shown}"
    );
    let xml = std::fs::read_to_string(&report).unwrap();
    assert!(
        xml.contains(r#"failures="1""#) && xml.contains("<failure message="),
        "{xml}"
    );
    assert!(
        xml.contains("failed &amp; &lt;broken&gt;"),
        "text is escaped: {xml}"
    );
    // Clean up the kept state directory named in the output.
    if let Some(path) = shown
        .lines()
        .find_map(|l| l.trim().strip_prefix("state kept: "))
    {
        let _ = std::fs::remove_dir_all(path);
    }
}

#[test]
fn run_all_exits_2_when_the_filter_selects_nothing_or_a_file_is_invalid() {
    let none = run_all(&["--filter", "no-such-scenario-*"]);
    assert_eq!(none.status.code(), Some(2));
    assert!(
        text(&none.stderr).contains("matches no-such-scenario-*"),
        "{}",
        text(&none.stderr)
    );

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("broken.json"),
        r#"{"format": 1, "name": "broken"}"#,
    )
    .unwrap();
    let report = dir.path().join("junit.xml");
    let out = Command::new(env!("CARGO_BIN_EXE_packie-sim"))
        .arg("run-all")
        .arg(dir.path())
        .args(["--junit", report.to_str().unwrap()])
        .output()
        .expect("run packie-sim");
    assert_eq!(out.status.code(), Some(2));
    assert!(
        !report.exists(),
        "nothing ran, so there is nothing to report"
    );
}

#[test]
fn soak_refuses_what_it_cannot_run_with_exit_code_2() {
    let engine = env!("CARGO_BIN_EXE_packie-sim");
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().join("run");
    let soak = |extra: &[&str]| {
        Command::new(engine)
            .args([
                "soak",
                "--engine",
                engine,
                "--out",
                out_dir.to_str().unwrap(),
            ])
            .args(extra)
            .output()
            .expect("run packie-sim")
    };
    // Under two minutes there is nothing to measure.
    let short = soak(&["--hours", "0.01"]);
    assert_eq!(short.status.code(), Some(2));
    assert!(
        text(&short.stderr).contains("at least two minutes"),
        "{}",
        text(&short.stderr)
    );
    // An unknown kind of chaos lists the known ones.
    let unknown = soak(&["--hours", "0.1", "--only", "meteor_strike"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(
        text(&unknown.stderr).contains("store_block"),
        "{}",
        text(&unknown.stderr)
    );
    // A directory that holds a run is never reused.
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("report.md"), "# earlier run\n").unwrap();
    let reused = soak(&["--hours", "0.1"]);
    assert_eq!(reused.status.code(), Some(2));
    assert!(
        text(&reused.stderr).contains("already holds a soak run"),
        "{}",
        text(&reused.stderr)
    );
}

#[test]
fn soak_help_describes_the_run() {
    let out = sim(&["soak", "--help"]);
    assert_eq!(out.status.code(), Some(0));
    let help = text(&out.stdout);
    for needle in [
        "--hours", "--chaos", "--engine", "--out", "--seed", "--only",
    ] {
        assert!(help.contains(needle), "{needle} is missing from:\n{help}");
    }
}
