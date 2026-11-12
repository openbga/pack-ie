//! Retention of bridge tap recordings (`docs/spec/bridge-channels.md` §9): the registry's
//! retention run applies each bridge's `tap.retention` to the messages its tap recorded.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use packie_core::{MessageDirection, MessageOrigin, MessageState, ProtocolKind, Timestamp};
use packie_routing::memory::MemoryNetwork;
use packie_store::Store;
use serde_json::{Value, json};

const DAY: Duration = Duration::from_secs(86_400);

/// Records one observed message on `channel`, received `days` ago, as a tap does.
async fn record(store: &Store, channel: &str, days: u32, control: &str) {
    let receipt = packie_store::Receipt {
        channel_id: channel.to_owned(),
        channel_label: format!("Bridge {channel}"),
        source_connector_id: "tap".to_owned(),
        source_endpoint_id: "a".to_owned(),
        source_peer: None,
        source_file: None,
        protocol: ProtocolKind::Hl7,
        direction: MessageDirection::AToB,
        origin: MessageOrigin::Tap,
        initial_state: MessageState::Observed,
        identifiers: packie_store::Identifiers {
            control_id: Some(control.to_owned()),
            ..packie_store::Identifiers::default()
        },
        received_at: Timestamp::now().checked_sub(DAY * days).unwrap(),
        config_version: 1,
        duplicate_detection: false,
        destinations: Vec::new(),
    };
    store.commit_received(receipt, oru(control)).await.unwrap();
}

/// The control ids still stored on `channel`.
fn kept(store: &Store, channel: &str) -> Vec<String> {
    let mut ids: Vec<String> = records(store)
        .into_iter()
        .filter(|r| r.summary.channel_id.as_str() == channel)
        .filter_map(|r| r.summary.control_id)
        .collect();
    ids.sort();
    ids
}

/// A bridge `tap` recording HL7 messages, with `retention` when given.
fn tapped(name: &str, retention: Option<Value>) -> Value {
    let mut tap = json!({"framer": "hl7_mllp", "record_messages": true});
    if let Some(retention) = retention {
        tap["retention"] = retention;
    }
    bridge(
        name,
        &format!("{name}-a"),
        &format!("{name}-b"),
        json!({"tap": tap}),
    )
}

#[tokio::test]
async fn tap_recordings_are_purged_by_the_bridges_tap_retention() {
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let store = open_store(dir.path(), None);
    // `default` keeps 30 days; `short` keeps 5.
    for (channel, days, control) in [
        ("default", 40, "D-40"),
        ("default", 10, "D-10"),
        ("short", 10, "S-10"),
        ("short", 1, "S-1"),
    ] {
        record(&store, channel, days, control).await;
    }
    let net = MemoryNetwork::new();
    let mut h = Harness::in_dir(
        Arc::clone(&dir),
        options_with_store(Arc::new(net.clone()), &store),
    );
    h.apply(document(
        vec![
            listener("default-a", 7101),
            client("default-b", 7102),
            listener("short-a", 7201),
            client("short-b", 7202),
        ],
        vec![
            tapped("default", None),
            tapped("short", Some(json!({"max_age_days": 5}))),
        ],
    ))
    .await;
    // Retention runs once startup reconciliation is done, then every ten minutes.
    until("the tap recordings purged", || {
        (kept(&store, "default") == ["D-10"] && kept(&store, "short") == ["S-1"]).then_some(())
    })
    .await;
}
