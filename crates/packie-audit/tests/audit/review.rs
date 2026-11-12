//! Recording, paging, filtering, export and input limits.

use packie_audit::{ActorOrigin, Audit, Entry, Error};
use packie_ipc::audit::{
    AuditAction, AuditEvent, AuditExportFormat, AuditExportRequest, AuditFilter, AuditObjectType,
    AuditOrigin, AuditQuery,
};
use serde_json::json;

use crate::common::{actor, open_log, pause, quiet, record_n};

fn seqs(events: &[AuditEvent]) -> Vec<u64> {
    events.iter().map(|e| e.seq).collect()
}

fn filtered(audit: &Audit, filter: AuditFilter) -> Vec<AuditEvent> {
    audit
        .query(&AuditQuery {
            filter,
            ..AuditQuery::default()
        })
        .unwrap()
        .events
}

#[test]
fn pages_newest_first_and_stops_at_the_end() {
    let (_dir, audit) = open_log();
    record_n(&audit, 6);
    let page = audit
        .query(&AuditQuery {
            limit: 2,
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(seqs(&page.events), [6, 5]);
    assert_eq!(page.next_before_seq, Some(5));
    let next = audit
        .query(&AuditQuery {
            before_seq: page.next_before_seq,
            limit: 10,
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(seqs(&next.events), [4, 3, 2, 1]);
    assert_eq!(next.next_before_seq, None);
    let refused = audit.query(&AuditQuery {
        limit: 0,
        ..AuditQuery::default()
    });
    assert!(matches!(refused, Err(Error::Invalid(_))));
}

#[test]
fn filters_by_actor_action_and_object() {
    let (_dir, audit) = open_log();
    record_n(&audit, 5);
    let body = Entry::new(
        actor("asmith", "502", ActorOrigin::Console),
        AuditAction::MessageBodyView,
    )
    .object(
        AuditObjectType::Message,
        "0190f3c2-0000-7000-8000-000000000001",
    )
    .details(json!({ "body": "original" }));
    assert_eq!(audit.record(&body).unwrap(), 6);

    let views = filtered(
        &audit,
        AuditFilter {
            actor: Some("502".into()),
            action: Some(AuditAction::MessageBodyView),
            ..AuditFilter::default()
        },
    );
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].actor_name, "asmith");
    assert_eq!(views[0].origin, AuditOrigin::Console);
    assert_eq!(views[0].details, json!({ "body": "original" }));

    let by_object = filtered(
        &audit,
        AuditFilter {
            object_id: Some("ch-3".into()),
            ..AuditFilter::default()
        },
    );
    assert_eq!(seqs(&by_object), [4]);
}

#[test]
fn exports_csv_and_json_lines_in_chunks_oldest_first() {
    let (_dir, audit) = open_log();
    record_n(&audit, 1500);
    let first = audit
        .export(&AuditExportRequest {
            format: AuditExportFormat::Csv,
            ..AuditExportRequest::default()
        })
        .unwrap();
    assert_eq!(first.rows, 1000);
    assert_eq!(first.next_after_seq, Some(1000));
    let mut lines = first.content.lines();
    assert_eq!(
        lines.next().unwrap(),
        "seq,at,actor_uid,actor_name,origin,action,object_type,object_id,outcome,details,prev_hash,hash"
    );
    assert!(lines.next().unwrap().starts_with("1,"));
    let rest = audit
        .export(&AuditExportRequest {
            format: AuditExportFormat::JsonLines,
            after_seq: first.next_after_seq,
            ..AuditExportRequest::default()
        })
        .unwrap();
    assert_eq!(rest.rows, 500);
    assert_eq!(rest.next_after_seq, None);
    let line: serde_json::Value =
        serde_json::from_str(rest.content.lines().next().unwrap()).unwrap();
    assert_eq!(line["seq"], 1001);
    assert_eq!(line["action"], "channel_pause");
}

#[test]
fn malformed_entries_are_refused_without_marking_the_log_unavailable() {
    let (_dir, audit) = open_log();
    for entry in [
        pause("lis").details(json!(["not", "an", "object"])),
        pause("lis").details(json!({ "note": "x".repeat(5000) })),
        Entry::new(actor("", "1", ActorOrigin::Cli), AuditAction::ChannelPause),
        pause(&"c".repeat(257)),
    ] {
        assert!(matches!(audit.record(&entry), Err(Error::Invalid(_))));
    }
    assert!(audit.status().available);
}

#[test]
fn the_chain_survives_reopening_with_a_shutdown_anchor() {
    let dir = tempfile::tempdir().unwrap();
    {
        let audit = Audit::open(dir.path(), quiet()).unwrap();
        record_n(&audit, 3);
    }
    let audit = Audit::open(dir.path(), quiet()).unwrap();
    // Closing wrote a shutdown anchor after the three events.
    assert_eq!(audit.status().head_seq, 4);
    assert_eq!(audit.record(&pause("lis")).unwrap(), 5);
    let anchors = filtered(
        &audit,
        AuditFilter {
            action: Some(AuditAction::AuditAnchor),
            ..AuditFilter::default()
        },
    );
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].details, json!({ "reason": "shutdown" }));
    assert!(audit.verify().unwrap().ok);
}
