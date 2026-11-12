//! HL7 v2 over MLLP message channels (`docs/spec/message-channels.md` §2–§9) over the
//! in-memory network with scripted peers and the durable store.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use packie_core::{AckLevel, ChannelState, DeliveryState, MessageState};
use packie_engine::ChannelRegistry;
use packie_hl7::{Message, mllp};
use packie_ipc::ChannelCommand;
use packie_ipc::messages::{CloseRequest, ResendMode, ResendRequest, ResendTargets};
use packie_routing::message::{MessageStore, SqliteStore};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const SOURCE: &str = "127.0.0.1:9601";
const LIS: &str = "127.0.0.1:9701";

/// The channel [`hl7_channel`] builds, running over the durable store.
async fn rig(source: Value, destination: Value, extra: Value) -> MessageRig {
    MessageRig::start(hl7_document(hl7_channel(source, destination, extra))).await
}

/// The destination of the default channel, to give a mapping or filters.
fn default_destination() -> Value {
    hl7_channel(json!({}), json!({}), json!({}))["destinations"][0].clone()
}

/// The LIS: accepts the engine's destination connection.
async fn lis(rig: &MessageRig) -> (packie_routing::memory::MemoryListener, Peer) {
    Peer::accept(&rig.net, LIS).await
}

async fn source(rig: &MessageRig) -> Peer {
    Peer::dial(&rig.net, SOURCE).await
}

fn channel_snapshot(h: &Harness) -> packie_diagnostics::ChannelSnapshot {
    h.channel(channel_id("orders"))
}

#[tokio::test]
async fn ack_follows_the_commit_and_the_delivery_is_acknowledged() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;

    instrument.send(&oru("CTRL-1")).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "CTRL-1".into()));
    assert_eq!(rig.messages().len(), 1, "committed before the ACK went out");
    // The response is recorded once it has been sent.
    let messages = rig
        .messages_until("the response is recorded", |m| m[0].response_code.is_some())
        .await;
    assert_eq!(messages[0].response_code.as_deref(), Some("AA"));
    assert_eq!(messages[0].summary.control_id.as_deref(), Some("CTRL-1"));
    assert_eq!(messages[0].summary.sample_id.as_deref(), Some("SID-1"));

    assert_eq!(
        lis.frame().await,
        oru("CTRL-1"),
        "pass-through keeps the original bytes"
    );
    lis.send(&ack("AA", "CTRL-1")).await;
    let deliveries = rig
        .deliveries_until("acknowledged", |d| {
            d.first()
                .is_some_and(|d| d.state == DeliveryState::Acknowledged)
        })
        .await;
    assert_eq!(deliveries[0].ack_level, AckLevel::Accepted);
    assert_eq!(deliveries[0].attempts, 1);
    assert_eq!(rig.messages()[0].summary.state, MessageState::Routed);

    let snapshot = channel_snapshot(&rig.h);
    assert_eq!(snapshot.state, ChannelState::Running);
    let detail = snapshot.message.unwrap();
    assert_eq!(detail.counters.received, 1);
    assert_eq!(detail.destinations[0].counters.acknowledged, 1);
    assert_eq!(detail.destinations[0].queue.queued, 0);
}

#[tokio::test]
async fn no_positive_ack_without_a_commit() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let mut instrument = source(&rig).await;
    rig.set_unavailable(true);
    instrument.send(&oru("CTRL-2")).await;
    assert_eq!(instrument.msa().await, ("AE".into(), "CTRL-2".into()));
    assert!(rig.messages().is_empty());
    let snapshot = channel_snapshot(&rig.h);
    assert_eq!(
        snapshot.state,
        ChannelState::Degraded,
        "the store is unavailable"
    );

    rig.set_unavailable(false);
    instrument.send(&oru("CTRL-2")).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "CTRL-2".into()));
    assert_eq!(rig.messages().len(), 1);
}

#[tokio::test]
async fn an_invalid_message_is_kept_and_rejected() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let mut instrument = source(&rig).await;
    instrument
        .send(b"MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101||ORU^R01|CTRL-3|P\rPID|1\r")
        .await;
    assert_eq!(instrument.msa().await.0, "AR");
    let messages = rig
        .messages_until("invalid", |m| {
            m.first()
                .is_some_and(|m| m.summary.state == MessageState::Invalid)
        })
        .await;
    assert!(messages[0].reason_code.is_some());
    assert!(rig.deliveries().is_empty());
}

#[tokio::test]
async fn a_duplicate_is_acknowledged_and_recorded_not_processed() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    for _ in 0..2 {
        instrument.send(&oru("CTRL-4")).await;
        assert_eq!(instrument.msa().await.0, "AA");
    }
    let messages = rig.messages();
    assert_eq!(messages[1].summary.state, MessageState::Duplicate);
    assert_eq!(
        messages[1].duplicate_of,
        Some(messages[0].summary.message_id)
    );
    let _ = lis.frame().await;
    lis.send(&ack("AA", "CTRL-4")).await;
    rig.deliveries_until("one delivery acknowledged", |d| {
        d.len() == 1 && d[0].state == DeliveryState::Acknowledged
    })
    .await;
    assert_eq!(
        channel_snapshot(&rig.h)
            .message
            .unwrap()
            .counters
            .duplicates,
        1
    );
}

#[tokio::test]
async fn a_rejection_is_final_does_not_block_and_can_be_resent() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-5")).await;
    instrument.msa().await;
    instrument.send(&oru("CTRL-6")).await;
    instrument.msa().await;

    assert_eq!(
        Message::parse(&lis.frame().await).unwrap().get("MSH-10"),
        Some("CTRL-5")
    );
    lis.send(&ack("AR", "CTRL-5")).await;
    assert_eq!(
        Message::parse(&lis.frame().await).unwrap().get("MSH-10"),
        Some("CTRL-6")
    );
    lis.send(&ack("AA", "CTRL-6")).await;
    let deliveries = rig
        .deliveries_until("both final", |d| {
            d.len() == 2
                && d.iter()
                    .all(|d| d.state != DeliveryState::Queued && d.state != DeliveryState::Sending)
        })
        .await;
    assert_eq!(deliveries[0].state, DeliveryState::Rejected);
    assert_eq!(
        deliveries[0].attempts, 1,
        "a rejection is never retried automatically"
    );
    assert_eq!(deliveries[1].state, DeliveryState::Acknowledged);
    let unresolved = channel_snapshot(&rig.h).message.unwrap().unresolved;
    assert_eq!(unresolved.rejected, 1);

    let result = rig
        .h
        .registry
        .resend(ResendRequest {
            message_id: deliveries[0].message_id,
            destinations: ResendTargets::All,
            mode: ResendMode::SameMessage,
            confirm_duplicate_risk: false,
            reason: None,
            confirm_superseded: false,
        })
        .await
        .unwrap();
    assert!(result.destinations[0].delivery_id.is_some());
    assert_eq!(
        Message::parse(&lis.frame().await).unwrap().get("MSH-10"),
        Some("CTRL-5")
    );
    lis.send(&ack("AA", "CTRL-5")).await;
    rig.deliveries_until("resent and acknowledged", |d| {
        d[0].state == DeliveryState::Acknowledged
    })
    .await;
    assert_eq!(rig.deliveries()[0].total_attempts, 2);
}

#[tokio::test]
async fn no_ack_in_time_is_timed_out_and_can_be_closed() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-7")).await;
    instrument.msa().await;
    let _ = lis.frame().await;
    let deliveries = rig
        .deliveries_until("timed out", |d| {
            d.first()
                .is_some_and(|d| d.state == DeliveryState::TimedOut)
        })
        .await;
    assert_eq!(
        deliveries[0].attempts, 1,
        "unacknowledged sends are not retried by default"
    );
    // A late acknowledgement matches nothing in flight and changes nothing.
    lis.send(&ack("AA", "CTRL-7")).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(rig.deliveries()[0].state, DeliveryState::TimedOut);

    let closed = rig
        .h
        .registry
        .close_delivery(CloseRequest {
            delivery_id: deliveries[0].delivery_id,
            reason: "checked at the LIS".into(),
        })
        .await
        .unwrap();
    assert_eq!(closed.state, DeliveryState::Cancelled);
}

#[tokio::test]
async fn a_connection_lost_after_sending_is_outcome_unknown() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let (listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-8")).await;
    instrument.msa().await;
    let _ = lis.frame().await;
    drop(lis);
    drop(listener);
    let deliveries = rig
        .deliveries_until("outcome unknown", |d| {
            d.first()
                .is_some_and(|d| d.state == DeliveryState::OutcomeUnknown)
        })
        .await;
    assert_eq!(deliveries[0].ack_level, AckLevel::None);
}

#[tokio::test]
async fn enhanced_mode_sends_commit_then_application_acks() {
    let rig = rig(
        json!({"ack_mode": "enhanced", "send_application_ack": true}),
        json!({"ack_mode": "enhanced"}),
        json!({}),
    )
    .await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    let enhanced = b"MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|CTRL-9|P|2.5.1|||AL|AL\rPID|1||PID-TEST-9\rOBR|1||SID-9\r";
    instrument.send(enhanced).await;
    assert_eq!(instrument.msa().await, ("CA".into(), "CTRL-9".into()));
    assert_eq!(instrument.msa().await, ("AA".into(), "CTRL-9".into()));

    let _ = lis.frame().await;
    lis.send(&ack("CA", "CTRL-9")).await;
    rig.deliveries_until("commit proven", |d| {
        d.first().is_some_and(|d| d.ack_level == AckLevel::Received)
    })
    .await;
    assert_eq!(
        rig.deliveries()[0].state,
        DeliveryState::Sending,
        "received is below accepted"
    );
    lis.send(&ack("AA", "CTRL-9")).await;
    rig.deliveries_until("accepted", |d| d[0].state == DeliveryState::Acknowledged)
        .await;
}

#[tokio::test]
async fn a_failing_mapping_creates_a_failed_delivery_without_attempts() {
    // MSH-10 is not in the table and a miss is an error.
    let mapping = json!({
        "tables": {"codes": {"KNOWN": "K"}},
        "rules": [{"op": "lookup", "from": "MSH.10", "table": "codes", "to": "MSH.10", "on_missing": "error"}],
    });
    let mut destination = default_destination();
    destination["mapping"] = mapping;
    let rig = rig(json!({}), json!({}), json!({"destinations": [destination]})).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-10")).await;
    assert_eq!(
        instrument.msa().await.0,
        "AA",
        "a mapping never decides the acknowledgement"
    );
    let deliveries = rig
        .deliveries_until("failed", |d| {
            d.first().is_some_and(|d| d.state == DeliveryState::Failed)
        })
        .await;
    assert_eq!(
        deliveries[0].reason_code.as_deref(),
        Some("transform_error")
    );
    assert_eq!(deliveries[0].total_attempts, 0);
    assert_eq!(rig.messages()[0].summary.state, MessageState::Routed);
    let detail = channel_snapshot(&rig.h).message.unwrap();
    assert_eq!(detail.destinations[0].unresolved.transform_errors, 1);
    assert_eq!(detail.unresolved.transform_errors, 1);
    assert_eq!(detail.unresolved.failed, 1);
}

#[tokio::test]
async fn a_down_destination_queues_without_spending_attempts() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-11")).await;
    assert_eq!(instrument.msa().await.0, "AA", "the source keeps receiving");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let deliveries = rig.deliveries();
    assert_eq!(deliveries[0].state, DeliveryState::Queued);
    assert_eq!(deliveries[0].attempts, 0, "waiting is not failing");
    assert_eq!(channel_snapshot(&rig.h).state, ChannelState::Degraded);

    let (_listener, mut lis) = lis(&rig).await;
    let _ = lis.frame().await;
    lis.send(&ack("AA", "CTRL-11")).await;
    rig.deliveries_until("delivered once the LIS is up", |d| {
        d[0].state == DeliveryState::Acknowledged
    })
    .await;
}

#[tokio::test]
async fn pause_closes_the_source_and_resume_continues_the_queue() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let id = channel_id("orders");
    let mut instrument = source(&rig).await;
    rig.h
        .registry
        .command(id, ChannelCommand::Pause)
        .await
        .unwrap();
    let snapshot = channel_snapshot(&rig.h);
    assert_eq!(snapshot.state, ChannelState::Stopped);
    assert!(
        rig.net.connect(SOURCE).is_err(),
        "the sender is refused while paused"
    );
    assert!(instrument.frame_within(200).await.is_none());

    rig.h
        .registry
        .command(id, ChannelCommand::Resume)
        .await
        .unwrap();
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-12")).await;
    assert_eq!(instrument.msa().await.0, "AA");
    let _ = lis.frame().await;
    lis.send(&ack("AA", "CTRL-12")).await;
    rig.deliveries_until("delivered after resume", |d| {
        d[0].state == DeliveryState::Acknowledged
    })
    .await;
}

#[tokio::test]
async fn startup_reconciliation_marks_sending_deliveries_outcome_unknown() {
    // A store left by a crash: one delivery in `sending`.
    let (dir, store) = {
        let rig = rig(json!({}), json!({}), json!({})).await;
        let (_listener, mut lis) = lis(&rig).await;
        let mut instrument = source(&rig).await;
        instrument.send(&oru("CTRL-13")).await;
        instrument.msa().await;
        let _ = lis.frame().await;
        rig.deliveries_until("sending", |d| {
            d.first().is_some_and(|d| d.state == DeliveryState::Sending)
        })
        .await;
        // Crash: the registry is dropped without shutdown.
        (std::sync::Arc::clone(&rig.h.dir), rig.store.clone())
    };
    let report = SqliteStore::new(store.clone())
        .reconcile_at_startup()
        .await
        .unwrap();
    assert_eq!(report.unknown, 1);
    let delivery = &deliveries_of(records(&store))[0];
    assert_eq!(delivery.state, DeliveryState::OutcomeUnknown);
    assert_eq!(delivery.reason_code.as_deref(), Some("engine_restarted"));
    drop(dir);
}

#[tokio::test]
async fn new_message_resend_runs_the_current_pipeline() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-14")).await;
    instrument.msa().await;
    let _ = lis.frame().await;
    lis.send(&ack("AR", "CTRL-14")).await;
    let deliveries = rig
        .deliveries_until("rejected", |d| {
            d.first()
                .is_some_and(|d| d.state == DeliveryState::Rejected)
        })
        .await;
    let result = rig
        .h
        .registry
        .resend(ResendRequest {
            message_id: deliveries[0].message_id,
            destinations: ResendTargets::All,
            mode: ResendMode::NewMessage,
            confirm_duplicate_risk: false,
            reason: None,
            confirm_superseded: false,
        })
        .await
        .unwrap();
    let new_message = result.new_message_id.expect("a new message");
    assert_eq!(
        Message::parse(&lis.frame().await).unwrap().get("MSH-10"),
        Some("CTRL-14")
    );
    lis.send(&ack("AA", "CTRL-14")).await;
    rig.deliveries_until("new message delivered", |d| {
        d.iter()
            .any(|d| d.message_id == new_message && d.state == DeliveryState::Acknowledged)
    })
    .await;
    assert_eq!(
        rig.deliveries()[0].state,
        DeliveryState::Rejected,
        "the original is untouched"
    );
}

#[tokio::test]
async fn filters_and_a_mapping_shape_what_the_destination_receives() {
    let mut destination = default_destination();
    destination["mapping"] =
        json!({"rules": [{"op": "constant", "value": "MAPPED", "to": "OBR.4"}]});
    destination["filters"] = json!([{"path": "OBX.3", "op": "equals", "value": "GLU"}]);
    let rig = rig(
        json!({}),
        json!({}),
        json!({
            "filters": [{"path": "MSH.9.1", "op": "equals", "value": "ORU"}],
            "destinations": [destination],
        }),
    )
    .await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-M1")).await;
    assert_eq!(instrument.msa().await.0, "AA");
    let sent = Message::parse(&lis.frame().await).unwrap();
    assert_eq!(sent.get("OBR-4"), Some("MAPPED"));
    assert_eq!(
        sent.get("MSH-10"),
        Some("CTRL-M1"),
        "a target of the source's protocol starts as a copy of the source"
    );
    lis.send(&ack("AA", "CTRL-M1")).await;

    let adt = b"MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ADT^A01|CTRL-M2|P|2.5.1\rPID|1||PID-TEST-1\r";
    instrument.send(adt).await;
    assert_eq!(
        instrument.msa().await.0,
        "AA",
        "a filtered message is accepted"
    );
    let messages = rig
        .messages_until("filtered", |m| {
            m.iter().any(|m| m.summary.state == MessageState::Filtered)
        })
        .await;
    let filtered = messages
        .iter()
        .find(|m| m.summary.state == MessageState::Filtered)
        .unwrap();
    assert_eq!(filtered.reason_code.as_deref(), Some("source_filter"));
}

#[tokio::test]
async fn filters_that_do_not_compile_fail_the_change_and_keep_the_running_definition() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    // Validation rejects such a document; the registry checks again at apply.
    let mut document = hl7_document(hl7_channel(json!({}), json!({}), json!({})));
    document["config_version"] = json!(rig.h.config.config_version + 1);
    document["generated_at"] = json!("2026-11-06T00:00:00.000000000Z");
    let mut config = packie_config::RuntimeConfig::from_value(document).unwrap();
    if let Some(packie_config::ChannelConfig::Message(channel)) = config.channels.first_mut() {
        channel.filters = vec![json!({"path": "MSH.9.1", "op": "resembles", "value": "ORU"})];
    }
    let diff = packie_config::ConfigDiff::between(&rig.h.config, &config);
    let report = rig.h.registry.apply(&config, &diff).await;
    let Some(packie_config::ApplyOutcome::Failed { reason }) =
        report.channels.first().map(|c| c.outcome.clone())
    else {
        panic!("expected failed, got {report:?}");
    };
    assert!(reason.contains("channels[0].filters[0]"), "{reason}");
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-C1")).await;
    assert_eq!(instrument.msa().await.0, "AA");
    rig.deliveries_until("routed with the previous filters", |d| d.len() == 1)
        .await;
}

#[tokio::test]
async fn a_corrected_mapping_applies_in_place_and_a_resend_runs_it() {
    let mapping = |table: Value| {
        json!({
            "tables": {"codes": table},
            "rules": [{"op": "lookup", "from": "MSH.10", "table": "codes", "to": "MSH.10", "on_missing": "error"}],
        })
    };
    let with = |mapping: Value| {
        let mut destination = default_destination();
        destination["mapping"] = mapping;
        json!({"destinations": [destination]})
    };
    let mut rig = rig(json!({}), json!({}), with(mapping(json!({"KNOWN": "K"})))).await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("CTRL-F1")).await;
    assert_eq!(instrument.msa().await.0, "AA");
    let failed = rig
        .deliveries_until("failed", |d| {
            d.first().is_some_and(|d| d.state == DeliveryState::Failed)
        })
        .await;

    let fixed = mapping(json!({"KNOWN": "K", "CTRL-F1": "FIXED-1"}));
    rig.h
        .apply(hl7_document(hl7_channel(json!({}), json!({}), with(fixed))))
        .await;
    assert_eq!(
        rig.h.outcome("orders"),
        packie_config::ApplyOutcome::Updated
    );
    rig.h
        .registry
        .resend(ResendRequest {
            message_id: failed[0].message_id,
            destinations: ResendTargets::All,
            mode: ResendMode::SameMessage,
            confirm_duplicate_risk: false,
            reason: None,
            confirm_superseded: false,
        })
        .await
        .unwrap();
    let sent = Message::parse(&lis.frame().await).unwrap();
    assert_eq!(sent.get("MSH-10"), Some("FIXED-1"));
    lis.send(&ack("AA", "FIXED-1")).await;
    let delivered = rig
        .deliveries_until("acknowledged", |d| {
            d.first()
                .is_some_and(|d| d.state == DeliveryState::Acknowledged)
        })
        .await;
    assert_eq!(delivered[0].mapping_version, Some(2));
    let detail = channel_snapshot(&rig.h).message.unwrap();
    assert_eq!(detail.unresolved.transform_errors, 0);
}

#[tokio::test]
async fn channel_traffic_counts_committed_and_delivered_messages() {
    let rig = rig(json!({}), json!({}), json!({})).await;
    let (_listener, mut lis) = lis(&rig).await;
    let mut instrument = source(&rig).await;
    assert!(channel_snapshot(&rig.h).traffic.last_activity.is_none());
    instrument.send(&oru("CTRL-T1")).await;
    assert_eq!(instrument.msa().await.0, "AA");
    let _ = lis.frame().await;
    lis.send(&ack("AA", "CTRL-T1")).await;
    rig.deliveries_until("acknowledged", |d| {
        d.first()
            .is_some_and(|d| d.state == DeliveryState::Acknowledged)
    })
    .await;
    let traffic = channel_snapshot(&rig.h).traffic;
    let size = oru("CTRL-T1").len() as u64;
    assert_eq!((traffic.messages_in, traffic.bytes_in), (1, size));
    assert_eq!((traffic.messages_out, traffic.bytes_out), (1, size));
    assert!(traffic.last_activity.is_some());
}

#[tokio::test]
async fn a_peer_that_never_reads_its_acknowledgements_loses_the_connection() {
    // No destination connects: its refused connection attempts would fill the channel's
    // short list of recent errors before the source session's error is read.
    let mut destination = default_destination();
    destination["enabled"] = json!(false);
    let rig = rig(
        json!({"frame_timeout_ms": 1000}),
        json!({}),
        json!({"destinations": [destination]}),
    )
    .await;
    let stalled = dial(&rig.net, SOURCE).await;
    // Messages without ever reading an ACK, until the engine's replies fill the pipe.
    let (mut reader, mut writer) = tokio::io::split(stalled);
    let flood = tokio::spawn(async move { flood(&mut writer).await });
    // Nothing is read: the session gives up on a reply within `frame_timeout_ms` and
    // reports it (before the next session's first commit clears the channel's errors).
    until("the stalled response is reported", || {
        channel_errors(&rig.h)
            .iter()
            .any(|e| e.contains("did not read the response"))
            .then_some(())
    })
    .await;
    // It closed, and the listener takes the next peer.
    let mut next = source(&rig).await;
    next.send(&oru("CTRL-AFTER")).await;
    assert_eq!(next.msa().await.0, "AA");
    flood.abort();
    // The stalled connection was closed: what was buffered drains, then end of stream.
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        let mut buf = vec![0_u8; 64 * 1024];
        while matches!(reader.read(&mut buf).await, Ok(n) if n > 0) {}
    })
    .await;
    assert!(closed.is_ok(), "the stalled connection is still open");
}

/// The summaries of the channel's recent and last errors.
fn channel_errors(h: &Harness) -> Vec<String> {
    let snapshot = channel_snapshot(h);
    snapshot
        .recent_errors
        .iter()
        .map(|e| e.summary.clone())
        .chain(snapshot.last_error.iter().map(|e| e.summary.clone()))
        .collect()
}

/// Sends messages with long control ids until the connection refuses more. The
/// acknowledgements echo the id, so the pipe fills after a few dozen: a long run of
/// commits queued behind one read would end the session on its frame deadline (a partial
/// frame waiting behind them) before the unread acknowledgements stall it.
async fn flood(writer: &mut tokio::io::WriteHalf<DuplexStream>) {
    let padding = "X".repeat(1000);
    for n in 0..3000 {
        let frame = mllp::encode(&oru(&format!("CTRL-S{n}-{padding}")), 1 << 20).unwrap();
        if writer.write_all(&frame).await.is_err() {
            return;
        }
    }
}

/// A burst of complete frames and the start of the next one in one write, on a store
/// that commits slowly: the partial frame's `frame_timeout_ms` runs only while the
/// engine waits for its bytes, not while the burst is committed, so the connection stays
/// up and the frame completes (`message-channels.md` §2.2).
#[tokio::test]
async fn a_burst_with_a_partial_frame_survives_slow_commits() {
    let rig = rig(json!({"frame_timeout_ms": 1000}), json!({}), json!({})).await;
    rig.set_commit_latency(Duration::from_millis(40));
    let mut instrument = source(&rig).await;
    let mut burst = Vec::new();
    for n in 0..50 {
        burst.extend(mllp::encode(&oru(&format!("BURST-{n}")), 1 << 20).unwrap());
    }
    let last = mllp::encode(&oru("BURST-50"), 1 << 20).unwrap();
    let (head, tail) = last.split_at(last.len() / 2);
    burst.extend_from_slice(head);
    instrument.write_raw(&burst).await;

    // 50 commits at 40 ms each take about 2 s, twice the frame timeout.
    for n in 0..50 {
        assert_eq!(instrument.msa().await, ("AA".into(), format!("BURST-{n}")));
    }
    instrument.write_raw(tail).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "BURST-50".into()));
    let messages = rig
        .messages_until("all 51 committed", |m| m.len() == 51)
        .await;
    assert_eq!(
        messages
            .last()
            .and_then(|m| m.summary.control_id.as_deref()),
        Some("BURST-50")
    );
}

/// `frame_timeout_ms` measures the wait for the bytes of an open frame: a frame whose
/// bytes keep coming completes even when it takes longer in total; one whose bytes stop
/// is discarded and the connection closed (`message-channels.md` §2.2).
#[tokio::test]
async fn an_open_frame_times_out_only_when_its_bytes_stop() {
    let rig = rig(json!({"frame_timeout_ms": 1000}), json!({}), json!({})).await;
    let mut instrument = source(&rig).await;
    let frame = mllp::encode(&oru("SLOW-1"), 1 << 20).unwrap();
    let third = frame.len() / 3;
    for piece in [&frame[..third], &frame[third..2 * third]] {
        instrument.write_raw(piece).await;
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    instrument.write_raw(&frame[2 * third..]).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "SLOW-1".into()));

    let abandoned = mllp::encode(&oru("SLOW-2"), 1 << 20).unwrap();
    instrument.write_raw(&abandoned[..third]).await;
    instrument.expect_closed().await;
    assert_eq!(rig.messages().len(), 1, "the abandoned frame is not stored");
}

/// A failed startup reconciliation (`message-channels.md` §13) is not latched
/// complete. Message channels wait (`stopped`, `store_unavailable`) and the
/// store reports unavailable; the reconciliation is tried again with the store-reopen
/// backoff, and once it completes the channels start.
#[tokio::test]
async fn a_failed_startup_reconciliation_is_retried_before_channels_start() {
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let store = open_store(dir.path(), None);
    let net = packie_routing::memory::MemoryNetwork::new();
    let (mut options, faults) = options_with_latency(Arc::new(net.clone()), &store);
    options.store_retry = packie_routing::StoreRetry {
        initial: Duration::from_millis(100),
        max: Duration::from_millis(200),
    };
    faults.unavailable(true);
    let mut h = Harness::in_dir(Arc::clone(&dir), options);
    h.apply(hl7_document(hl7_channel(json!({}), json!({}), json!({}))))
        .await;
    let orders = channel_id("orders");
    assert_eq!(h.channel(orders).state, ChannelState::Stopped);
    assert_eq!(
        h.channel(orders).stop_reason,
        Some(packie_core::StopReason::StoreUnavailable)
    );
    assert_eq!(
        h.registry.store_status().map(|s| s.state),
        Some(packie_diagnostics::StoreState::Unavailable)
    );
    // Still waiting after several retry periods while the store keeps failing.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(h.channel(orders).state, ChannelState::Stopped);

    faults.unavailable(false);
    let (_listener, _lis) = Peer::accept(&net, LIS).await;
    until("the channel started after reconciliation", || {
        (h.channel(orders).state != ChannelState::Stopped).then_some(())
    })
    .await;
    assert_eq!(
        h.registry.store_status().map(|s| s.state),
        Some(packie_diagnostics::StoreState::Ok)
    );
    let mut instrument = Peer::dial(&net, SOURCE).await;
    instrument.send(&oru("AFTER-RETRY")).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "AFTER-RETRY".into()));
}

/// A queued delivery (`message-channels.md` §12) keeps the acknowledgement
/// level frozen when it was created. After `ack_required` is lowered in place, its attempt
/// still waits for that level (the store completes it only there, so finishing below it
/// left the delivery `sending`); deliveries created afterwards use the new level.
#[tokio::test]
async fn a_queued_delivery_keeps_its_frozen_ack_requirement_after_an_in_place_change() {
    // Enhanced mode: a commit accept (CA) proves `received`, the application accept (AA)
    // `accepted`, the level frozen with the delivery by default.
    let enhanced = json!({"ack_mode": "enhanced"});
    let mut rig = rig(json!({}), enhanced.clone(), json!({})).await;
    let mut instrument = source(&rig).await;
    instrument.send(&oru("FROZEN")).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "FROZEN".into()));
    rig.deliveries_until("queued while the LIS is down", |d| {
        d.first().is_some_and(|d| d.state == DeliveryState::Queued)
    })
    .await;
    let lowered = json!({"delivery": {
        "max_attempts": 3, "send_timeout_seconds": 1, "ack_timeout_seconds": 2,
        "retry": quick_retry(), "ack_required": "received",
    }});
    rig.h
        .apply(hl7_document(hl7_channel(json!({}), enhanced, lowered)))
        .await;
    assert_eq!(
        rig.h.outcome("orders"),
        packie_config::ApplyOutcome::Updated
    );

    let (_listener, mut lis) = lis(&rig).await;
    assert_eq!(lis.frame().await, oru("FROZEN"));
    // The commit accept alone does not complete the delivery: it waits for the AA.
    lis.send(&ack("CA", "FROZEN")).await;
    lis.send(&ack("AA", "FROZEN")).await;
    let deliveries = rig
        .deliveries_until("acknowledged at the frozen level", |d| {
            d.first()
                .is_some_and(|d| d.state == DeliveryState::Acknowledged)
        })
        .await;
    assert_eq!(deliveries[0].ack_level, AckLevel::Accepted);
}
