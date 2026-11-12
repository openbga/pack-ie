//! The `packie` binary's offline behavior: validation, missing engine, service arguments and
//! the single-instance guard.
#![cfg(test)]

mod common;

use std::process::Output;

/// `packie` run from the repository root, where `config/runtime.json` lives.
fn packie(args: &[&str]) -> Output {
    common::packie()
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn validate_reports_field_paths() {
    let ok = packie(&["config", "validate", "config/runtime.json"]);
    assert!(ok.status.success(), "{}", stderr(&ok));
    assert!(String::from_utf8_lossy(&ok.stdout).contains("is valid: 4 endpoints, 2 channels"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    let mut doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("../../config/runtime.json").unwrap())
            .unwrap();
    doc["channels"][0]["retry"] = serde_json::json!({"multiplier": 0});
    std::fs::write(&path, doc.to_string()).unwrap();
    let bad = packie(&["config", "validate", path.to_str().unwrap()]);
    assert!(!bad.status.success());
    assert!(
        stderr(&bad).contains("channels[0].retry.multiplier: must be at least 1.0"),
        "{}",
        stderr(&bad)
    );
}

#[test]
fn clients_explain_a_missing_engine() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("none.sock");
    let output = packie(&["--ipc", socket.to_str().unwrap(), "status"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("the engine is not running"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn service_install_needs_a_readable_bootstrap() {
    let output = packie(&[
        "--bootstrap",
        "missing/bootstrap.json",
        "service",
        "install",
    ]);
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(
        message.contains("bootstrap file missing/bootstrap.json")
            && message.contains("--bootstrap"),
        "{message}"
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn user_agents_are_macos_only() {
    let output = packie(&["service", "status", "--user"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("macOS only"),
        "{}",
        stderr(&output)
    );
}

#[cfg(unix)]
#[test]
fn second_engine_on_the_same_state_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let bootstrap = common::write_bootstrap(
        dir.path(),
        &serde_json::json!({
            "node": {"node_id": "single", "display_name": "Single"},
            "paths": {"state_directory": "state", "log_directory": "logs"},
            "ipc": {"group": null},
        }),
    );
    let mut first = common::spawn_engine(&bootstrap);
    common::wait_for_engine(&bootstrap);

    let second = packie(&["run", "--bootstrap", bootstrap.to_str().unwrap()]);
    first.kill().unwrap();
    first.wait().unwrap();
    assert!(!second.status.success());
    let message = stderr(&second);
    assert!(
        message.contains("another engine is already running")
            && message.contains(&format!("pid {}", first.id())),
        "{message}"
    );
}

#[test]
fn an_invalid_bootstrap_exits_with_code_2() {
    let dir = tempfile::tempdir().unwrap();
    let bootstrap = common::write_bootstrap(
        dir.path(),
        &serde_json::json!({
            "node": {"node_id": "Not An Id", "display_name": "Bad"},
            "paths": {"state_directory": "state"},
            "ipc": {"group": null},
        }),
    );
    let output = packie(&["run", "--bootstrap", bootstrap.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("node.node_id"),
        "{}",
        stderr(&output)
    );
}

/// How one early stop ended.
#[cfg(unix)]
enum EarlyStop {
    /// Exit code 0: the engine handled the stop.
    Clean,
    /// Killed by the signal before `main` held it (no program can handle a signal there).
    BeforeMain,
    /// Anything else: the property is broken.
    Failed(String),
}

/// Starts `packie run` on a fresh state directory with the startup test seam (`stop.rs`
/// `held_for_test`), sends SIGTERM once `ready` returns, and says how it ended.
#[cfg(unix)]
fn stop_during_startup(pause_ms: Option<u64>, ready: impl FnOnce(&std::path::Path)) -> EarlyStop {
    use std::os::unix::process::ExitStatusExt;

    let dir = tempfile::tempdir().unwrap();
    let bootstrap = dir.path().join("bootstrap.json");
    let document = serde_json::json!({
        "node": {"node_id": "early-stop", "display_name": "Early stop"},
        "paths": {"state_directory": "state", "log_directory": "logs"},
        "ipc": {"group": null},
    });
    std::fs::write(&bootstrap, document.to_string()).unwrap();
    let marker = dir.path().join("held");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_packie"));
    command
        .args(["run", "--bootstrap", bootstrap.to_str().unwrap()])
        .env("PACKIE_TEST_HELD_MARKER", &marker)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    if let Some(ms) = pause_ms {
        command.env("PACKIE_TEST_HELD_PAUSE_MS", ms.to_string());
    }
    let child = command.spawn().unwrap();
    ready(&marker);
    let killed = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let output = child.wait_with_output().unwrap();
    match (output.status.code(), output.status.signal()) {
        (Some(0), _) => EarlyStop::Clean,
        (None, Some(_)) if !marker.exists() => EarlyStop::BeforeMain,
        _ => EarlyStop::Failed(format!("{:?} {}", output.status, stderr(&output))),
    }
}

/// A SIGTERM delivered after `main` held the signals and before the handlers exist (the
/// engine pauses there) is handled: exit 0.
#[cfg(unix)]
#[test]
fn sigterm_between_holding_and_handling_exits_cleanly() {
    let wait_for_hold = |marker: &std::path::Path| {
        common::eventually("main holds the stop signals", || marker.exists());
    };
    match stop_during_startup(Some(1000), wait_for_hold) {
        EarlyStop::Clean => {}
        EarlyStop::BeforeMain => panic!("the marker existed, so main had run"),
        EarlyStop::Failed(how) => panic!("not a clean stop: {how}"),
    }
}

/// A stop request at any point of startup ends the engine cleanly (exit 0), never by the
/// signal's default action. A stop that lands before `main` (no marker, killed by the
/// signal: a busy machine can delay `main` past the earliest offsets) is retried.
#[cfg(unix)]
#[test]
fn sigterm_during_startup_exits_cleanly() {
    const ATTEMPTS: usize = 5;
    let mut reached_main = 0;
    for delay_ms in [5, 20, 40, 60, 90, 130, 200] {
        let sleep = |_: &std::path::Path| {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        };
        let ended = (0..ATTEMPTS)
            .map(|_| stop_during_startup(None, sleep))
            .find(|ended| !matches!(ended, EarlyStop::BeforeMain));
        match ended {
            Some(EarlyStop::Clean) => reached_main += 1,
            Some(EarlyStop::Failed(how)) => panic!("stopped {delay_ms} ms after spawn: {how}"),
            Some(EarlyStop::BeforeMain) | None => {}
        }
    }
    assert!(
        reached_main > 0,
        "no stop reached main; the machine is too busy to test"
    );
}

#[test]
fn config_verify_has_help() {
    let output = packie(&["config", "verify", "--help"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("<VERSION>") && help.contains("--note"),
        "{help}"
    );
}
