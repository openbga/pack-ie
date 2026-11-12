//! What the integration tests share: the `packie` binary, bootstrap files and a running engine.
// The engine-spawning helpers serve the Unix-only tests; on Windows they are compiled but unused.
#![cfg_attr(windows, allow(dead_code))]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// How long a test waits for something that happens in another process.
pub(crate) const WAIT: Duration = Duration::from_secs(20);

/// The `packie` binary, with nothing in the environment that selects an engine.
pub(crate) fn packie() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_packie"));
    command
        .env_remove("PACKIE_BOOTSTRAP")
        .env_remove("PACKIE_IPC");
    command
}

/// Writes `document` as `bootstrap.json` in `dir`.
pub(crate) fn write_bootstrap(dir: &Path, document: &Value) -> PathBuf {
    let path = dir.join("bootstrap.json");
    std::fs::write(&path, document.to_string()).unwrap();
    path
}

/// Starts `packie run` on `bootstrap`, with its output discarded.
pub(crate) fn spawn_engine(bootstrap: &Path) -> Child {
    packie()
        .args(["run", "--bootstrap", bootstrap.to_str().unwrap()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Waits until the engine of `bootstrap` answers `packie status`.
pub(crate) fn wait_for_engine(bootstrap: &Path) {
    eventually("the engine answers", || {
        packie()
            .args(["--bootstrap", bootstrap.to_str().unwrap(), "status"])
            .output()
            .unwrap()
            .status
            .success()
    });
}

/// Polls `check` until it holds, for at most [`WAIT`].
pub(crate) fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for: {what}");
}
