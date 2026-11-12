//! Delimited-file message channels over real folders and the SQLite store
//! (`docs/spec/message-channels.md` §2.4, §8): one message per row committed together,
//! the file moved after the commit, a mapped file written per delivery.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use common::*;
use packie_core::{AckLevel, DeliveryState, MessageState, ProtocolKind};
use packie_store::Store;
use serde_json::{Value, json};

struct Rig {
    _h: Harness,
    store: Store,
    inbox: std::path::PathBuf,
    outbox: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn rig(source_settings: Value) -> Rig {
    rig_with(source_settings, json!({})).await
}

/// `rig` with `destination_extra` added to the destination's settings.
async fn rig_with(source_settings: Value, destination_extra: Value) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("in");
    let outbox = dir.path().join("out");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&outbox).unwrap();
    let store = open_store(dir.path(), None);
    let mut h = Harness::with_options(options_with_store(system_transports(), &store));
    let mut destination_settings = json!({
        "file_name": "r-{sample_id}.csv", "line_ending": "lf",
        "columns": [{"name": "sample", "position": 1}, {"name": "result", "position": 2}],
    });
    if let (Some(settings), Some(extra)) = (
        destination_settings.as_object_mut(),
        destination_extra.as_object(),
    ) {
        settings.extend(extra.clone());
    }
    let channel = json!({
        "kind": "message", "channel_id": "files", "label": "Files",
        "source": {"endpoint": "in-dir", "protocol": "delimited_file", "settings": source_settings},
        "destinations": [{
            "destination_id": "out", "label": "Out", "endpoint": "out-dir",
            "protocol": "delimited_file",
            "settings": destination_settings,
            "mapping": {"rules": [
                {"op": "copy", "from": "col.sample_id", "to": "col.sample"},
                {"op": "copy", "from": "col.value", "to": "col.result"},
            ]},
        }],
    });
    h.apply(document(
        vec![folder("in-dir", &inbox), folder("out-dir", &outbox)],
        vec![channel],
    ))
    .await;
    Rig {
        _h: h,
        store,
        inbox,
        outbox,
        _dir: dir,
    }
}

fn columns() -> Value {
    json!([{"name": "sample_id"}, {"name": "value", "type": "decimal"}])
}

#[tokio::test]
async fn each_row_becomes_a_message_and_each_delivery_a_mapped_file() {
    let rig = rig(json!({"columns": columns()})).await;
    std::fs::write(
        rig.inbox.join("batch.csv"),
        "sample_id,value\r\nSID-F1,4.5\r\nSID-F2,6\r\n",
    )
    .unwrap();
    let first = rig.outbox.join("r-SID-F1.csv");
    let second = rig.outbox.join("r-SID-F2.csv");
    until("both files written", || {
        (first.exists() && second.exists()).then_some(())
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(&first).unwrap(),
        "sample,result\nSID-F1,4.5\n"
    );
    assert!(rig.inbox.join("processed").join("batch.csv").exists());
    assert!(!rig.inbox.join("batch.csv").exists());

    let records = until("acknowledged", || {
        let records = records(&rig.store);
        records
            .iter()
            .all(|r| {
                r.deliveries
                    .first()
                    .is_some_and(|d| d.state == DeliveryState::Acknowledged)
            })
            .then_some(records)
    })
    .await;
    assert_eq!(records.len(), 2);
    let mut rows = Vec::new();
    for record in &records {
        assert_eq!(record.summary.state, MessageState::Routed);
        assert_eq!(record.summary.protocol, ProtocolKind::Delimited);
        assert_eq!(record.summary.message_type.as_deref(), Some("delimited"));
        assert_eq!(record.deliveries[0].ack_level, AckLevel::HandedOff);
        let file = record.source_file.clone().expect("source file");
        assert_eq!(file.name, "batch.csv");
        assert_eq!(file.file_hash.len(), 64);
        rows.push((
            record.summary.sample_id.clone().unwrap(),
            file.first_row,
            file.last_row,
        ));
    }
    rows.sort();
    assert_eq!(
        rows,
        vec![("SID-F1".to_owned(), 1, 1), ("SID-F2".to_owned(), 2, 2)]
    );
}

#[tokio::test]
async fn an_oversized_file_is_recorded_aborted_and_a_malformed_one_invalid() {
    let rig = rig(json!({"columns": columns(), "max_file_bytes": 1024})).await;
    let big = format!("sample_id,value\r\n{}", "SID-BIG,1\r\n".repeat(200));
    std::fs::write(rig.inbox.join("big.csv"), big).unwrap();
    std::fs::write(
        rig.inbox.join("ragged.csv"),
        "sample_id,value\r\nSID-R,1,extra\r\n",
    )
    .unwrap();
    let records = until("both files recorded", || {
        let records = records(&rig.store);
        (records.len() == 2
            && records
                .iter()
                .all(|r| r.summary.state != MessageState::Received))
        .then_some(records)
    })
    .await;
    let aborted = records
        .iter()
        .find(|r| r.summary.state == MessageState::Aborted)
        .expect("aborted");
    assert_eq!(aborted.abort_reason.as_deref(), Some("file_too_large"));
    // Not read: no bytes stored and no file hash.
    assert_eq!(aborted.summary.byte_length, 0);
    let file = aborted.source_file.clone().expect("source file");
    assert_eq!(
        (file.name.as_str(), file.file_hash.as_str()),
        ("big.csv", "")
    );
    let invalid = records
        .iter()
        .find(|r| r.summary.state == MessageState::Invalid)
        .expect("invalid");
    assert_eq!(invalid.reason_code.as_deref(), Some("malformed_file"));
    assert!(invalid.deliveries.is_empty());
    until("both files consumed", || {
        let processed = rig.inbox.join("processed");
        (processed.join("big.csv").exists() && processed.join("ragged.csv").exists()).then_some(())
    })
    .await;
}

/// A mapped file larger than the destination's `max_file_bytes` is never written: the
/// delivery fails with `transform_error`, so a reader with the same template accepts
/// every file the engine writes (`message-channels.md` §8).
#[tokio::test]
async fn a_mapped_file_over_the_destination_limit_is_not_written() {
    let rig = rig_with(
        json!({"columns": columns()}),
        json!({"max_file_bytes": 1024}),
    )
    .await;
    let long = format!("SID-{}", "L".repeat(1100));
    std::fs::write(
        rig.inbox.join("big.csv"),
        format!("sample_id,value\r\n{long},4.5\r\nSID-OK,6\r\n"),
    )
    .unwrap();
    let settled = |r: &packie_ipc::messages::MessageRecord| {
        let state = r.deliveries.first().map(|d| d.state);
        matches!(
            state,
            Some(DeliveryState::Acknowledged | DeliveryState::Failed)
        )
    };
    let records = until("both deliveries settled", || {
        let records = records(&rig.store);
        (records.len() == 2 && records.iter().all(settled)).then_some(records)
    })
    .await;
    let delivery = |sample: &str| {
        records
            .iter()
            .find(|r| r.summary.sample_id.as_deref() == Some(sample))
            .and_then(|r| r.deliveries.first().cloned())
            .expect("delivery")
    };
    let failed = delivery(&long);
    assert_eq!(failed.state, DeliveryState::Failed);
    assert_eq!(failed.reason_code.as_deref(), Some("transform_error"));
    assert_eq!(failed.attempts, 0);
    assert_eq!(delivery("SID-OK").state, DeliveryState::Acknowledged);
    let written: Vec<_> = std::fs::read_dir(&rig.outbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(written, vec!["r-SID-OK.csv".to_owned()]);
}
