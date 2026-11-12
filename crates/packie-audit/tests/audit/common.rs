//! Shared fixtures: actors, entries, a movable clock and a tampered log.

use std::path::Path;
use std::sync::{Arc, Mutex};

use packie_audit::{Actor, ActorOrigin, Audit, Entry, Options, verify_dir};
use packie_core::Timestamp;
use packie_ipc::audit::{AuditAction, AuditObjectType, AuditProblemKind};
use rusqlite::Connection;

pub(crate) fn actor(name: &str, uid: &str, origin: ActorOrigin) -> Actor {
    Actor {
        name: name.into(),
        uid_or_sid: uid.into(),
        origin,
        pid: Some(4242),
    }
}

pub(crate) fn jdoe() -> Actor {
    actor("jdoe", "501", ActorOrigin::Cli)
}

pub(crate) fn pause(channel: &str) -> Entry {
    Entry::new(jdoe(), AuditAction::ChannelPause).object(AuditObjectType::Channel, channel)
}

/// A clock the test moves.
#[derive(Clone)]
pub(crate) struct TestClock(Arc<Mutex<Timestamp>>);

impl TestClock {
    pub(crate) fn at(text: &str) -> Self {
        Self(Arc::new(Mutex::new(Timestamp::parse(text).unwrap())))
    }

    pub(crate) fn set(&self, text: &str) {
        *self.0.lock().unwrap() = Timestamp::parse(text).unwrap();
    }

    pub(crate) fn options(&self) -> Options {
        let clock = Arc::clone(&self.0);
        Options {
            background: false,
            clock: Arc::new(move || *clock.lock().unwrap()),
            ..Options::default()
        }
    }
}

/// Options without the maintenance thread.
pub(crate) fn quiet() -> Options {
    Options {
        background: false,
        ..Options::default()
    }
}

/// Opens a log in a fresh directory.
pub(crate) fn open_log() -> (tempfile::TempDir, Audit) {
    let dir = tempfile::tempdir().unwrap();
    let audit = Audit::open(dir.path(), quiet()).unwrap();
    (dir, audit)
}

pub(crate) fn record_n(audit: &Audit, n: usize) {
    for i in 0..n {
        audit.record(&pause(&format!("ch-{i}"))).unwrap();
    }
}

/// A closed log of `events` events, then edited as an attacker with `SQLite` access would
/// (without the triggers). The log is closed first, so its write-ahead log is folded into
/// the file and nothing else touches the file while it is edited and verified.
pub(crate) fn tampered(events: usize, sql: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    {
        let audit = Audit::open(dir.path(), quiet()).unwrap();
        record_n(&audit, events);
    }
    assert!(
        !dir.path().join("audit.sqlite3-wal").exists(),
        "the log is closed"
    );
    let connection = Connection::open(dir.path().join("audit.sqlite3")).unwrap();
    connection
        .execute_batch(&format!(
            "DROP TRIGGER events_no_update; DROP TRIGGER events_no_delete; {sql}"
        ))
        .unwrap();
    assert!(
        connection.changes() > 0,
        "the tampering changed nothing: {sql}"
    );
    connection.close().unwrap();
    dir
}

/// The problem kinds verification reports for `dir`, in order.
pub(crate) fn kinds(dir: &Path) -> Vec<AuditProblemKind> {
    verify_dir(dir)
        .unwrap()
        .problems
        .iter()
        .map(|p| p.kind)
        .collect()
}
