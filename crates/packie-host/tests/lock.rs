#![cfg(test)]
//! The single-instance guard across processes.

use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};

use packie_host::{HostError, InstanceLock};

/// The lock is an `flock` on the open file: a child forked by this test binary shares every
/// descriptor until it execs (they are close-on-exec), so while one test spawns its child,
/// another test's lock would stay held past its drop. Tests that take a lock or spawn run
/// one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Run by the test below in a child process: holds the lock until stdin closes.
#[test]
fn hold_lock_child() {
    let Ok(dir) = std::env::var("PACKIE_LOCK_TEST_DIR") else {
        return;
    };
    let _lock = InstanceLock::acquire(dir.as_ref(), "child.json".as_ref()).unwrap();
    println!("locked");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

#[test]
fn second_engine_on_the_same_state_dir_is_refused() {
    let _serial = serial();
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(InstanceLock::holder(dir.path()), None);

    let first = InstanceLock::acquire(dir.path(), "first.json".as_ref()).unwrap();
    let info = InstanceLock::holder(dir.path()).unwrap();
    assert_eq!(info.pid, std::process::id());
    assert_eq!(info.bootstrap.to_str(), Some("first.json"));

    match InstanceLock::acquire(dir.path(), "second.json".as_ref()) {
        Err(HostError::AlreadyRunning { pid, .. }) => assert_eq!(pid, std::process::id()),
        other => panic!("expected AlreadyRunning, got {other:?}"),
    }

    // The lock is owner-only; the instance file (no secrets) is readable by the group, so
    // operators see who runs the engine.
    #[cfg(unix)]
    for (name, expected) in [("packie.lock", 0o600), ("packie.instance.json", 0o640)] {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, expected, "{name}");
    }

    drop(first);
    assert_eq!(InstanceLock::holder(dir.path()), None);
    // The last holder is still on record, unchecked.
    assert_eq!(
        InstanceLock::last_info(dir.path()).map(|i| i.pid),
        Some(std::process::id())
    );
    // An info file left readable by others gets the group-only mode again.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let info = dir.path().join("packie.instance.json");
        std::fs::set_permissions(&info, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _again = InstanceLock::acquire(dir.path(), "second.json".as_ref()).unwrap();
        let mode = std::fs::metadata(&info).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
    #[cfg(not(unix))]
    let _again = InstanceLock::acquire(dir.path(), "second.json".as_ref()).unwrap();
}

#[test]
fn lock_held_by_another_process_is_refused_until_it_exits() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    let _serial = serial();
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "hold_lock_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("PACKIE_LOCK_TEST_DIR", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains("locked") {
        line.clear();
        assert_ne!(out.read_line(&mut line).unwrap(), 0, "child ended early");
    }

    let holder = InstanceLock::holder(dir.path()).unwrap();
    assert_eq!(holder.pid, child.id());
    assert!(matches!(
        InstanceLock::acquire(dir.path(), "parent.json".as_ref()),
        Err(HostError::AlreadyRunning { pid, .. }) if pid == child.id()
    ));

    child.stdin.take().unwrap().write_all(b"\n").unwrap();
    assert!(child.wait().unwrap().success());
    let _lock = InstanceLock::acquire(dir.path(), "parent.json".as_ref()).unwrap();
}
