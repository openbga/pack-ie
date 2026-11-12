//! A message store that cannot be opened at start (`docs/spec/message-channels.md` §13):
//! bridges run, message channels report `stopped` (`store_unavailable`), the registry
//! retries opening the store and then starts the message channels without touching the
//! bridges.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use packie_core::{ChannelState, StopReason};
use packie_diagnostics::StoreState;
use packie_engine::ChannelRegistry;
use packie_routing::memory::MemoryNetwork;
use packie_routing::{RegistryOptions, StoreOpener, StoreRetry, sqlite_store_opener};
use serde_json::{Value, json};

const FAST: StoreRetry = StoreRetry {
    initial: Duration::from_millis(50),
    max: Duration::from_millis(200),
};

/// A bridge `bridge-1` (:9811 to :9812) and an HL7 message channel `orders` (:9813 to
/// a LIS on :9814).
fn doc() -> Value {
    let orders = json!({
        "kind": "message", "channel_id": "orders", "label": "Orders",
        "source": {"endpoint": "lab-in", "protocol": "hl7_mllp", "settings": {}},
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-out",
            "protocol": "hl7_mllp", "settings": {"ack_mode": "none"},
        }],
    });
    document(
        vec![
            listener("in-a", 9811),
            client("out-b", 9812),
            listener("lab-in", 9813),
            client("lis-out", 9814),
        ],
        vec![bridge("bridge-1", "in-a", "out-b", json!({})), orders],
    )
}

/// Fails ("database is locked") until `available` is set, then opens the SQLite store.
fn gated_opener(
    available: &Arc<AtomicBool>,
    attempts: &Arc<AtomicUsize>,
    open: StoreOpener,
) -> StoreOpener {
    let (available, attempts) = (Arc::clone(available), Arc::clone(attempts));
    Arc::new(move || {
        attempts.fetch_add(1, Ordering::SeqCst);
        if !available.load(Ordering::SeqCst) {
            return Err("database is locked".into());
        }
        open()
    })
}

/// An instrument sends one message to the message channel and gets it acknowledged.
async fn commit_one(net: &MemoryNetwork, control: &str) {
    let mut instrument = Peer::dial(net, "127.0.0.1:9813").await;
    instrument.send(&oru(control)).await;
    assert_eq!(instrument.msa().await.0, "AA");
}

fn store_state(h: &Harness) -> Option<StoreState> {
    h.registry.store_status().map(|s| s.state)
}

#[tokio::test]
async fn message_channels_wait_for_the_store_while_bridges_run_then_start_when_it_opens() {
    let net = MemoryNetwork::new();
    let available = Arc::new(AtomicBool::new(false));
    let attempts = Arc::new(AtomicUsize::new(0));
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let opener = gated_opener(&available, &attempts, sqlite_store_opener(&context(&dir)));
    let mut options = RegistryOptions::new(Arc::new(net.clone()));
    options.store_opener = Some(opener);
    options.store_retry = FAST;
    let mut h = Harness::in_dir(dir, options);
    let mut sys_listener = net.listen("127.0.0.1:9812", 4).unwrap();
    h.apply(doc()).await;

    // The bridge runs and carries bytes.
    let mut inst = dial(&net, "127.0.0.1:9811").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    let bridged = h
        .until(channel_id("bridge-1"), "bridged", |s| {
            s.state == ChannelState::Running
        })
        .await;
    send(&mut inst, b"before").await;
    expect_bytes(&mut sys, b"before").await;

    // The message channel waits; the node reports the store unavailable.
    let orders = h.channel(channel_id("orders"));
    assert_eq!(orders.state, ChannelState::Stopped);
    assert_eq!(orders.stop_reason, Some(StopReason::StoreUnavailable));
    assert!(orders.allowed_actions.is_empty());
    assert_eq!(store_state(&h), Some(StoreState::Unavailable));
    until_holds("a few attempts", || attempts.load(Ordering::SeqCst) >= 3).await;

    // The store opens: the message channel starts and commits; the bridge was not touched.
    let _lis = net.listen("127.0.0.1:9814", 4).unwrap();
    available.store(true, Ordering::SeqCst);
    until_holds("orders running", || {
        h.channel(channel_id("orders")).state == ChannelState::Running
    })
    .await;
    assert_eq!(store_state(&h), Some(StoreState::Ok));
    commit_one(&net, "CTRL-R1").await;
    assert_eq!(h.registry.store_status().unwrap().messages_stored, Some(1));
    let after = h.status(channel_id("bridge-1"));
    assert_eq!(after.instance_started_at, bridged.instance_started_at);
    send(&mut inst, b"after").await;
    expect_bytes(&mut sys, b"after").await;
}

#[tokio::test]
async fn a_store_path_that_cannot_be_opened_is_retried_until_it_can() {
    let dir = Arc::new(tempfile::tempdir().unwrap());
    // A file where the store's directory belongs: opening fails until it is removed.
    let store_dir = dir.path().join("store");
    std::fs::write(&store_dir, "in the way\n").unwrap();
    let opener = sqlite_store_opener(&context(&dir));
    assert!(opener().is_err(), "the store must not open over a file");

    let net = MemoryNetwork::new();
    let mut options = RegistryOptions::new(Arc::new(net.clone()));
    options.store_opener = Some(opener);
    options.store_retry = FAST;
    let mut h = Harness::in_dir(Arc::clone(&dir), options);
    h.apply(doc()).await;
    assert_eq!(
        h.channel(channel_id("orders")).stop_reason,
        Some(StopReason::StoreUnavailable)
    );

    let _lis = net.listen("127.0.0.1:9814", 4).unwrap();
    std::fs::remove_file(&store_dir).unwrap();
    until_holds("orders running", || {
        h.channel(channel_id("orders")).state == ChannelState::Running
    })
    .await;
    assert_eq!(store_state(&h), Some(StoreState::Ok));
    commit_one(&net, "CTRL-R2").await;
    assert_eq!(h.registry.store_status().unwrap().messages_stored, Some(1));
}
