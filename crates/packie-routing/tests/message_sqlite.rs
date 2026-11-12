//! Message channels over the durable SQLite store (`packie_store::Store` through
//! `SqliteStore`): commit before acknowledgement with the commit gate, delivery, transfer
//! recovery at startup and the node snapshot's store figures.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::*;
use packie_core::{AckLevel, DeliveryState, MessageState, ProtocolKind, Timestamp};
use packie_engine::ChannelRegistry;
use packie_routing::message::store::{Receipt, RetentionPolicy};
use packie_routing::message::{MessageStore, SqliteStore};
use serde_json::{Value, json};

/// An HL7 channel `orders` with default settings.
fn orders() -> Value {
    hl7_document(hl7_channel(json!({}), json!({}), json!({})))
}

#[tokio::test]
async fn the_ack_waits_for_the_durable_commit_and_the_delivery_is_tracked() {
    let (hold, held) = tokio::sync::watch::channel(false);
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let rig = MessageRig::start_in(dir, Some(held), orders()).await;
    let (_listener, mut lis) = Peer::accept(&rig.net, "127.0.0.1:9701").await;
    let mut instrument = Peer::dial(&rig.net, "127.0.0.1:9601").await;

    hold.send(true).unwrap();
    instrument.send(&oru("CTRL-S1")).await;
    assert!(
        instrument.frame_within(300).await.is_none(),
        "no ACK while the commit is held"
    );
    hold.send(false).unwrap();
    assert_eq!(instrument.msa().await.0, "AA", "ACK after the commit");

    assert_eq!(lis.frame().await, oru("CTRL-S1"));
    lis.send(&ack("AA", "CTRL-S1")).await;
    let record = until("acknowledged", || {
        let record = rig.messages().into_iter().next()?;
        (record.deliveries.first()?.state == DeliveryState::Acknowledged).then_some(record)
    })
    .await;
    assert_eq!(record.summary.state, MessageState::Routed);
    assert_eq!(record.response_code.as_deref(), Some("AA"));
    assert_eq!(record.deliveries[0].ack_level, AckLevel::Accepted);
    assert_eq!(rig.body(record.summary.message_id), oru("CTRL-S1"));

    let figures = rig.h.registry.store_status().expect("store figures");
    assert_eq!(figures.messages_stored, Some(1));
    assert!(figures.size_bytes.unwrap() > 0);
    let detail = rig.h.channel(channel_id("orders")).message.unwrap();
    assert_eq!(detail.destinations[0].counters.acknowledged, 1);
    assert!(detail.destinations[0].last_attempt_at.is_some());
}

#[tokio::test]
async fn an_interrupted_astm_transfer_with_every_frame_is_recovered_at_startup() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path(), None);
    let adapter = SqliteStore::new(store.clone());
    let records: Vec<Vec<u8>> = [
        "H|\\^&|CTRL-R||SIM^1|||||||P|1|20260101",
        "P|1|PID-TEST-R",
        "O|1|SID-R||^^^GLU",
        "L|1|N",
    ]
    .iter()
    .map(|r| r.as_bytes().to_vec())
    .collect();
    let frames = packie_astm::encode_records(&records).unwrap();
    let receipt = Receipt {
        channel_id: channel_id("results"),
        channel_label: "Results".into(),
        source_endpoint_id: endpoint_id("analyzer"),
        source_peer: None,
        protocol: ProtocolKind::Astm,
        received_at: Timestamp::now(),
        config_version: 1,
        duplicate_detection: true,
        identifiers: Default::default(),
        source_file: None,
    };
    // Complete: every frame committed, then the engine stopped before EOT.
    let complete = adapter
        .begin_transfer(receipt.clone(), frames[0].clone())
        .await
        .unwrap();
    for frame in &frames[1..] {
        adapter.append_frame(complete, frame.clone()).await.unwrap();
    }
    // Partial: the terminator never arrived.
    let partial = adapter
        .begin_transfer(receipt, frames[0].clone())
        .await
        .unwrap();

    let report = adapter.reconcile_at_startup().await.unwrap();
    assert_eq!((report.recovered, report.aborted), (1, 1));
    assert!(report.requeued.contains(&complete));
    let recovered = store.message(complete).unwrap();
    assert_eq!(recovered.summary.state, MessageState::Received);
    assert_eq!(recovered.summary.sample_id.as_deref(), Some("SID-R"));
    let original = store.body(complete, None).unwrap();
    assert!(
        original.starts_with(b"H|") && original.ends_with(b"L|1|N\r"),
        "records, not frames"
    );
    let aborted = store.message(partial).unwrap();
    assert_eq!(aborted.abort_reason.as_deref(), Some("startup_interrupted"));
}

#[tokio::test]
async fn a_recovered_transfer_takes_its_specimen_id_from_the_configured_position() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path(), None);
    let adapter = SqliteStore::new(store.clone());
    let records: Vec<Vec<u8>> = [
        "H|\\^&|CTRL-R||SIM^1|||||||P|1|20260101",
        "O|1||^^SID-O4^B|^^^^WBC",
        "L|1|N",
    ]
    .iter()
    .map(|r| r.as_bytes().to_vec())
    .collect();
    let frames = packie_astm::encode_records(&records).unwrap();
    let receipt = Receipt {
        channel_id: channel_id("results"),
        channel_label: "Results".into(),
        source_endpoint_id: endpoint_id("analyzer"),
        source_peer: None,
        protocol: ProtocolKind::Astm,
        received_at: Timestamp::now(),
        config_version: 1,
        duplicate_detection: true,
        identifiers: Default::default(),
        source_file: None,
    };
    let message = adapter
        .begin_transfer(receipt, frames[0].clone())
        .await
        .unwrap();
    for frame in &frames[1..] {
        adapter.append_frame(message, frame.clone()).await.unwrap();
    }
    adapter.set_astm_sample_positions([(channel_id("results"), (4, Some(3)))].into());
    adapter.reconcile_at_startup().await.unwrap();
    let recovered = store.message(message).unwrap();
    assert_eq!(recovered.summary.sample_id.as_deref(), Some("SID-O4"));
}

#[tokio::test]
async fn retention_runs_and_resolved_history_can_be_purged() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = SqliteStore::new(open_store(dir.path(), None));
    let purged = adapter
        .purge(vec![RetentionPolicy {
            channel_id: channel_id("orders"),
            max_age_days: Some(1),
            max_messages: None,
        }])
        .await
        .unwrap();
    assert_eq!(purged, 0, "nothing old enough");
}
