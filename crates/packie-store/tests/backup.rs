#![cfg(test)]
//! Store snapshots for backups: consistent under concurrent commits, with every spilled
//! body the snapshot references.

use packie_core::{
    ChannelId, ConnectorId, EndpointId, MessageDirection, MessageOrigin, MessageState,
    ProtocolKind, Timestamp,
};
use packie_ipc::messages::MessageQuery;
use packie_store::{Identifiers, OpenOptions, Receipt, Store, StoreCopy, backup_closed};

fn spilling() -> OpenOptions {
    OpenOptions {
        inline_body_cap: 16,
        ..OpenOptions::default()
    }
}

fn receipt(channel: &str) -> Receipt {
    Receipt {
        channel_id: channel.to_owned(),
        channel_label: "Analyzer results".into(),
        source_connector_id: ConnectorId::generate().to_string(),
        source_endpoint_id: EndpointId::generate().to_string(),
        source_peer: None,
        source_file: None,
        protocol: ProtocolKind::Hl7,
        direction: MessageDirection::Inbound,
        origin: MessageOrigin::Received,
        initial_state: MessageState::Received,
        identifiers: Identifiers::default(),
        received_at: Timestamp::now(),
        config_version: 1,
        duplicate_detection: false,
        destinations: Vec::new(),
    }
}

fn body(i: usize) -> Vec<u8> {
    format!("MSH|^~\\&|ANALYZER|LAB|||{i:08}||ORU^R01|{i}|P|2.5.1\r").into_bytes()
}

/// Opens the copy as a store and reads every message's original body back (which checks
/// each spilled file against its content hash).
async fn readable_messages(copy: &StoreCopy) -> u64 {
    let directory = copy.database.parent().unwrap();
    let store = Store::open(directory, spilling()).unwrap();
    let list = store
        .query_messages(&MessageQuery {
            limit: 1000,
            ..MessageQuery::default()
        })
        .unwrap();
    for message in &list.messages {
        store.body(message.message_id, None).unwrap();
    }
    list.total
}

async fn commit_many(store: Store, channel: String, first: usize) {
    for n in first..first + 100 {
        store
            .commit_received(receipt(&channel), body(n))
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_snapshot_under_write_load_is_consistent_and_complete() {
    let state = tempfile::tempdir().unwrap();
    let store = Store::open(state.path(), spilling()).unwrap();
    let channel = ChannelId::generate().to_string();
    for i in 0..50 {
        store
            .commit_received(receipt(&channel), body(i))
            .await
            .unwrap();
    }
    let writers: Vec<_> = (0..4)
        .map(|w| tokio::spawn(commit_many(store.clone(), channel.clone(), 1000 + w * 1000)))
        .collect();
    let dest = state.path().join("backup/store");
    let snapshot = store.clone();
    let target = dest.clone();
    let copy = tokio::task::spawn_blocking(move || snapshot.backup_into(&target))
        .await
        .unwrap()
        .unwrap();
    for writer in writers {
        writer.await.unwrap();
    }
    assert!(copy.messages >= 50, "{copy:?}");
    assert_eq!(
        copy.bodies.len() as u64,
        copy.messages,
        "one spilled file per body"
    );
    assert!(copy.schema_version >= 8);
    assert!(!dest.join("messages.sqlite3-wal").exists());
    assert_eq!(readable_messages(&copy).await, copy.messages);

    // A second backup into the same directory is refused rather than mixed.
    assert!(store.backup_into(&dest).is_err());
}

#[tokio::test]
async fn a_closed_store_is_copied_without_opening_it_for_writing() {
    let state = tempfile::tempdir().unwrap();
    {
        let store = Store::open(state.path(), spilling()).unwrap();
        let channel = ChannelId::generate().to_string();
        for i in 0..10 {
            store
                .commit_received(receipt(&channel), body(i))
                .await
                .unwrap();
        }
    }
    let copy = backup_closed(state.path(), &state.path().join("copy")).unwrap();
    assert_eq!(copy.messages, 10);
    assert_eq!(readable_messages(&copy).await, 10);
    assert!(matches!(
        backup_closed(&state.path().join("missing"), &state.path().join("other")),
        Err(packie_store::Error::NotFound)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_only_snapshot_works_while_another_handle_writes() {
    let state = tempfile::tempdir().unwrap();
    let store = Store::open(state.path(), spilling()).unwrap();
    let channel = ChannelId::generate().to_string();
    for i in 0..20 {
        store
            .commit_received(receipt(&channel), body(i))
            .await
            .unwrap();
    }
    let writer = tokio::spawn(commit_many(store.clone(), channel, 5000));
    let (source, dest) = (state.path().to_owned(), state.path().join("copy"));
    let copy = tokio::task::spawn_blocking(move || backup_closed(&source, &dest))
        .await
        .unwrap()
        .unwrap();
    writer.await.unwrap();
    assert!(copy.messages >= 20);
    assert_eq!(readable_messages(&copy).await, copy.messages);
}
