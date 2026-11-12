//! Fail-closed writes: a locked or unopenable log refuses commands and recovers.

use std::time::Duration;

use packie_audit::{Audit, Error, Options};
use rusqlite::Connection;

use crate::common::{pause, quiet, record_n};

#[test]
fn writes_fail_closed_while_the_log_is_locked_and_recover() {
    let dir = tempfile::tempdir().unwrap();
    let audit = Audit::open(
        dir.path(),
        Options {
            busy_timeout: Duration::from_millis(50),
            ..quiet()
        },
    )
    .unwrap();
    record_n(&audit, 1);
    let blocker = Connection::open(dir.path().join("audit.sqlite3")).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();

    let error = audit.record(&pause("lis")).unwrap_err();
    assert!(matches!(error, Error::Unavailable(_)), "{error}");
    let status = audit.status();
    assert!(!status.available);
    assert!(status.last_error.is_some());

    blocker.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(
        audit.record(&pause("lis")).unwrap(),
        2,
        "nothing was written meanwhile"
    );
    assert!(audit.status().available);
    assert!(audit.verify().unwrap().ok);
}

#[test]
fn an_unopenable_log_starts_unavailable_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("audit");
    std::fs::write(&blocked, b"not a directory").unwrap();
    assert!(Audit::open(&blocked, quiet()).is_err());
    let audit = Audit::start(&blocked, quiet());
    assert!(!audit.status().available);
    assert!(matches!(
        audit.record(&pause("lis")),
        Err(Error::Unavailable(_))
    ));
    std::fs::remove_file(&blocked).unwrap();
    assert_eq!(audit.record(&pause("lis")).unwrap(), 1);
    assert!(audit.status().available);
}

/// An outage is found by checking, without waiting for the next recorded event (export
/// continuations rely on it).
#[cfg(unix)]
#[test]
fn a_write_check_finds_an_unwritable_directory_without_recording() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, audit) = crate::common::open_log();
    record_n(&audit, 1);
    audit.check_writable().unwrap();
    assert!(!dir.path().join(".write-check").exists());

    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let checked = audit.check_writable();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    if nix::unistd::geteuid().is_root() {
        return; // root writes anyway
    }
    assert!(matches!(checked, Err(Error::Unavailable(_))), "{checked:?}");
    assert!(!audit.status().available);
    assert_eq!(
        audit.record(&pause("lis")).unwrap(),
        2,
        "recovers with the next write"
    );
}
