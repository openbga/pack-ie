//! Verification: an intact chain passes and every kind of tampering is reported.

use packie_audit::verify_dir;
use packie_ipc::audit::AuditProblemKind::{BrokenLink, HashMismatch, SequenceGap};
use rusqlite::Connection;

use crate::common::{kinds, open_log, record_n, tampered};

#[test]
fn an_intact_chain_verifies() {
    let (_dir, audit) = open_log();
    record_n(&audit, 20);
    let report = audit.verify().unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.events, 20);
    assert_eq!((report.first_seq, report.last_seq), (Some(1), Some(20)));
    assert_eq!(
        report.head_hash.as_deref(),
        Some(audit.status().head_hash.as_str())
    );
}

#[test]
fn the_database_refuses_updates_and_deletes() {
    let (dir, audit) = open_log();
    record_n(&audit, 2);
    let connection = Connection::open(dir.path().join("audit.sqlite3")).unwrap();
    for sql in [
        "UPDATE events SET outcome = 'failed'",
        "DELETE FROM events WHERE seq = 1",
    ] {
        let error = connection.execute(sql, []).unwrap_err();
        assert!(error.to_string().contains("append-only"), "{error}");
    }
}

#[test]
fn verification_reports_each_kind_of_tampering() {
    let swap = "UPDATE events SET seq = 1000 WHERE seq = 3;
                UPDATE events SET seq = 3 WHERE seq = 7;
                UPDATE events SET seq = 7 WHERE seq = 1000;";
    // An attacker who recomputes the edited event's hash still breaks the next link.
    let rehash = "UPDATE events SET hash = (CASE substr(hash, 1, 1) WHEN '0' THEN '1' ELSE '0' END) \
                  || substr(hash, 2) WHERE seq = 2;";
    // (what, events, tampering, problems it must raise, whether it raises nothing else)
    let cases = [
        (
            "an edited event",
            10,
            "UPDATE events SET actor_name = 'someone-else' WHERE seq = 4;",
            vec![HashMismatch],
            true,
        ),
        (
            "a deleted event",
            10,
            "DELETE FROM events WHERE seq = 6;",
            vec![SequenceGap, BrokenLink],
            true,
        ),
        (
            "reordered events",
            10,
            swap,
            vec![BrokenLink, HashMismatch],
            false,
        ),
        ("a rehashed edit", 5, rehash, vec![BrokenLink], false),
    ];
    for (what, events, sql, expected, only) in cases {
        let dir = tampered(events, sql);
        let found = kinds(dir.path());
        for kind in &expected {
            assert!(found.contains(kind), "{what}: {found:?}");
        }
        assert!(!only || found == expected, "{what}: {found:?}");
        assert!(!verify_dir(dir.path()).unwrap().ok, "{what}");
    }
}

#[test]
fn an_edited_event_is_reported_once_at_its_seq() {
    let dir = tampered(
        10,
        "UPDATE events SET actor_name = 'someone-else' WHERE seq = 4;",
    );
    let report = verify_dir(dir.path()).unwrap();
    assert_eq!(report.problems.len(), 1, "{report:?}");
    assert_eq!(report.problems[0].seq, Some(4));
}
