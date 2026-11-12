//! Anchors, yearly sealing, retention and backup copies.

use std::path::Path;

use packie_audit::{Audit, Options, verify_dir};
use packie_ipc::audit::{AuditAction, AuditProblemKind, AuditQuery};

use crate::common::{TestClock, kinds, pause, record_n};

fn sqlite_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".sqlite3"))
        .collect();
    names.sort();
    names
}

#[test]
fn anchors_are_written_daily_while_events_arrive() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2026-03-01T08:00:00Z");
    let audit = Audit::open(dir.path(), clock.options()).unwrap();
    audit.run_maintenance();
    assert_eq!(audit.status().head_seq, 0, "no anchor for an empty log");
    record_n(&audit, 2);
    audit.run_maintenance();
    assert_eq!(audit.status().head_seq, 3, "first anchor after events");
    clock.set("2026-03-01T20:00:00Z");
    record_n(&audit, 1);
    audit.run_maintenance();
    assert_eq!(audit.status().head_seq, 4, "not yet a day since the anchor");
    clock.set("2026-03-02T08:00:01Z");
    audit.run_maintenance();
    assert_eq!(audit.status().head_seq, 5);
    audit.run_maintenance();
    assert_eq!(audit.status().head_seq, 5, "nothing new to anchor");
}

#[test]
fn the_turn_of_the_year_seals_the_file_and_the_chain_continues() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2025-12-31T23:59:00Z");
    let audit = Audit::open(dir.path(), clock.options()).unwrap();
    record_n(&audit, 3);
    clock.set("2026-01-01T00:00:01Z");
    assert_eq!(
        audit.record(&pause("lis")).unwrap(),
        5,
        "the seal event is 4"
    );
    let sealed = dir.path().join("audit-2025-000000000001.sqlite3");
    assert!(sealed.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&sealed).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o400);
    }
    let page = audit.query(&AuditQuery::default()).unwrap();
    assert_eq!(
        page.events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [5, 4, 3, 2, 1]
    );
    assert_eq!(page.events[1].action, AuditAction::AuditSeal);
    let report = audit.verify().unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.files, 2);
    assert_eq!(report.events, 5);
}

/// A log with two events in each of 2018, 2019 and 2020 (a sealed file per year, the 2020
/// one current), at `2020-06-01`.
fn three_years() -> (tempfile::TempDir, TestClock, Audit) {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2018-06-01T00:00:00Z");
    let options = Options {
        retention_days: 2190,
        ..clock.options()
    };
    let audit = Audit::open(dir.path(), options).unwrap();
    for year in ["2018", "2019", "2020"] {
        clock.set(&format!("{year}-06-01T00:00:00Z"));
        record_n(&audit, 2);
    }
    (dir, clock, audit)
}

#[test]
fn retention_removes_whole_sealed_files_and_records_it() {
    let (dir, clock, audit) = three_years();
    assert_eq!(
        sqlite_files(dir.path()),
        [
            "audit-2018-000000000001.sqlite3",
            "audit-2019-000000000003.sqlite3",
            "audit.sqlite3"
        ]
    );
    // Six years after mid-2018 only the 2018 file has expired. Recording the removal in
    // 2024 seals the 2020 file first.
    clock.set("2024-07-01T00:00:00Z");
    assert_eq!(audit.prune_expired().unwrap(), 1);
    assert_eq!(
        sqlite_files(dir.path()),
        [
            "audit-2019-000000000003.sqlite3",
            "audit-2020-000000000006.sqlite3",
            "audit.sqlite3"
        ]
    );
    assert_eq!(audit.prune_expired().unwrap(), 0);
    let prune = audit.query(&AuditQuery::default()).unwrap().events[0].clone();
    assert_eq!(prune.action, AuditAction::AuditPrune);
    assert_eq!(prune.details["last_seq"], 2);
    let report = audit.verify().unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.first_seq, Some(3));
}

#[test]
fn removing_a_sealed_file_without_a_recorded_prune_is_detected() {
    let (dir, _clock, audit) = three_years();
    drop(audit);
    let sealed = dir.path().join("audit-2018-000000000001.sqlite3");
    let mut permissions = std::fs::metadata(&sealed).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&sealed, permissions).unwrap();
    std::fs::remove_file(&sealed).unwrap();
    assert_eq!(kinds(dir.path()), [AuditProblemKind::UnexplainedStart]);
}

#[test]
fn a_copy_of_the_log_verifies_on_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let clock = TestClock::at("2025-12-31T23:00:00Z");
    let audit = Audit::open(dir.path().join("audit"), clock.options()).unwrap();
    record_n(&audit, 3);
    clock.set("2026-01-02T00:00:00Z");
    record_n(&audit, 3);
    let copy = dir.path().join("backup/audit");
    let copied = audit.backup_into(&copy).unwrap();
    assert_eq!(copied.files.len(), 2);
    assert_eq!(copied.head, Some((7, audit.status().head_hash)));
    let report = verify_dir(&copy).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.last_seq, Some(7));
    // The copy is a standalone file: no write-ahead log beside it.
    assert!(!copy.join("audit.sqlite3-wal").exists());
}
