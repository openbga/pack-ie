//! Bridge behaviors from `tests/scenarios/bridge-*.json` over the in-memory transport.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use packie_config::{ApplyOutcome, Change};
use packie_core::{BridgeSide, ChannelState, EndpointState, SessionState, StopReason};
use packie_engine::{ChannelRegistry, RegistryError};
use packie_ipc::ChannelCommand;
use packie_ipc::messages::RejectionCode;
use packie_routing::memory::MemoryNetwork;
use packie_routing::{RegistryOptions, TapObservation, TapSink};
use serde_json::json;
use tokio::io::AsyncReadExt;

/// The endpoint's refused peers once the latest refusal has `reason`.
async fn rejections_until(
    h: &Harness,
    endpoint: &str,
    reason: &str,
) -> packie_diagnostics::PeerRejections {
    let deadline = tokio::time::Instant::now() + WITHIN;
    loop {
        let snapshot = h.endpoint(endpoint_id(endpoint));
        if let Some(rejections) = snapshot
            .rejections
            .filter(|r| r.last_reason.as_deref() == Some(reason))
        {
            return rejections;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no {reason} refusal reported"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn harness() -> (Harness, MemoryNetwork) {
    let net = MemoryNetwork::new();
    (Harness::new(Arc::new(net.clone())), net)
}

/// `inst` listens on :9001 (A), `sys` is a client of :9101 (B).
fn standard(channel: &str, extra: serde_json::Value) -> serde_json::Value {
    document(
        vec![listener("inst", 9001), client("sys", 9101)],
        vec![bridge(channel, "inst", "sys", extra)],
    )
}

#[tokio::test]
async fn full_duplex_passes_bytes_unchanged_and_counts_them() {
    let (mut h, net) = harness();
    let mut sys = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("duplex");
    h.apply(standard("duplex", json!({}))).await;
    let waiting = h
        .until(id, "waiting for peer", |s| {
            s.session_state == SessionState::WaitingForPeer
                && s.endpoint_a.state == EndpointState::Listening
        })
        .await;
    assert_eq!(waiting.state, ChannelState::Starting);

    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys.accept().await.unwrap().stream;
    h.running(id).await;
    let status = h.status(id);
    assert_eq!(status.endpoint_a.state, EndpointState::Connected);
    assert_eq!(status.endpoint_b.state, EndpointState::Connected);

    send(&mut inst, b"hello from instrument\n").await;
    expect_bytes(&mut sys, b"hello from instrument\n").await;
    send(&mut sys, b"hello from system\n").await;
    expect_bytes(&mut inst, b"hello from system\n").await;

    let up = counter32(65536, 1);
    let down = counter32(65536, 2);
    let writer = tokio::spawn(async move {
        send(&mut inst, &up).await;
        inst
    });
    expect_bytes(&mut sys, &counter32(65536, 1)).await;
    let mut inst = writer.await.unwrap();
    let writer = tokio::spawn(async move {
        send(&mut sys, &down).await;
        sys
    });
    expect_bytes(&mut inst, &counter32(65536, 2)).await;
    let _sys = writer.await.unwrap();

    let status = h
        .until(id, "byte counters", |s| s.counters.bytes_b_to_a == 65554)
        .await;
    assert_eq!(status.counters.bytes_a_to_b, 65558);
    assert_eq!(status.counters.messages_a_to_b, None);
    assert_eq!(status.retry_count, 0);
    assert_eq!(status.endpoint_a.consecutive_failure_count, 0);
    assert_eq!(status.endpoint_b.consecutive_failure_count, 0);
    assert!(status.endpoint_a.last_activity_at.is_some());
    let snapshot = h.channel(id);
    assert_eq!(snapshot.traffic.bytes_in, 65558);
    assert_eq!(snapshot.traffic.bytes_out, 65554);
    let endpoint = h.endpoint(endpoint_id("inst"));
    assert_eq!(endpoint.state, EndpointState::Connected);
    assert_eq!(endpoint.traffic.bytes_in, 65558);
    assert_eq!(endpoint.traffic.bytes_out, 65554);
    assert_eq!(endpoint.held_by, Some(id));
    assert_eq!(endpoint.settings_summary, "127.0.0.1:9001");
}

#[tokio::test]
async fn close_on_one_side_closes_the_other_and_recovers() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("close");
    h.apply(standard("close", json!({}))).await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;

    send(&mut sys, b"bye\n").await;
    drop(sys);
    expect_bytes(&mut inst, b"bye\n").await;
    expect_closed(&mut inst).await;
    let status = h
        .until(id, "retrying after close", |s| {
            s.state == ChannelState::Retrying
        })
        .await;
    assert_eq!(status.endpoint_a.state, EndpointState::Listening);
    assert!(status.retry_count >= 1);
    let summary = status.last_error_summary.unwrap();
    assert!(summary.contains("endpoint B"), "{summary}");
    assert!(
        summary.contains("read failed: peer closed the connection"),
        "{summary}"
    );
    assert!(summary.contains("Retrying in"), "{summary}");

    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    send(&mut inst, b"again\n").await;
    expect_bytes(&mut sys, b"again\n").await;
    let status = h.until(id, "stable session", |s| s.retry_count == 0).await;
    assert_eq!(status.endpoint_b.consecutive_failure_count, 0);
    assert_eq!(status.next_retry_at, None);
}

#[tokio::test]
async fn bytes_sent_just_before_a_close_are_delivered() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("tail");
    h.apply(standard("tail", json!({}))).await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    let tail = counter32(32 * 1024, 9);
    send(&mut inst, &tail).await;
    drop(inst);
    expect_bytes(&mut sys, &tail).await;
    expect_closed(&mut sys).await;
}

#[tokio::test]
async fn listener_rejects_peers_outside_the_allowlist_without_changing_state() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("allow");
    let doc = |allowed: &[&str]| {
        document(
            vec![
                listener_allowing("inst", 9001, allowed),
                client("sys", 9101),
            ],
            vec![bridge("allow", "inst", "sys", json!({}))],
        )
    };
    h.apply(doc(&["10.9.9.9"])).await;
    let mut first_sys = sys_listener.accept().await.unwrap().stream;
    h.until(id, "listening", |s| {
        s.endpoint_a.state == EndpointState::Listening
    })
    .await;
    let mut bad = dial(&net, "127.0.0.1:9001").await;
    expect_closed(&mut bad).await;
    let rejections = rejections_until(&h, "inst", "not_allowed").await;
    assert_eq!(rejections.count, 1);
    let status = h.status(id);
    assert_eq!(status.session_state, SessionState::WaitingForPeer);
    assert_eq!(status.retry_count, 0);
    assert_eq!(status.state, ChannelState::Starting);

    let diff = h.apply(doc(&["10.0.0.0/8"])).await;
    assert_eq!(diff.channels[0].change, Change::Changed);
    expect_closed(&mut first_sys).await;
    let version = h.config.config_version;
    let mut good = dial_from(
        &net,
        "127.0.0.1:9001",
        IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)),
    )
    .await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    assert_eq!(h.status(id).config_version, version);
    send(&mut good, b"authorized\n").await;
    expect_bytes(&mut sys, b"authorized\n").await;
}

#[tokio::test]
async fn a_second_peer_is_refused_and_the_first_session_survives() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("single");
    h.apply(standard("single", json!({}))).await;
    let mut first = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;

    let mut second = dial(&net, "127.0.0.1:9001").await;
    expect_closed(&mut second).await;
    rejections_until(&h, "inst", "session_active").await;
    expect_open(&mut first, 200).await;
    let status = h.status(id);
    assert_eq!(status.state, ChannelState::Running);
    assert_eq!(status.retry_count, 0);
    send(&mut first, b"first-client\n").await;
    expect_bytes(&mut sys, b"first-client\n").await;

    drop(first);
    expect_closed(&mut sys).await;
    h.until(id, "retrying with the listener bound", |s| {
        s.state == ChannelState::Retrying && s.endpoint_a.state == EndpointState::Listening
    })
    .await;
    // A peer that connects during backoff waits in the backlog.
    let mut later = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    send(&mut later, b"second-client\n").await;
    expect_bytes(&mut sys, b"second-client\n").await;
}

#[tokio::test]
async fn reconnects_with_backoff_and_resets_once_stable() {
    let (mut h, net) = harness();
    let id = channel_id("reconnect");
    h.apply(standard("reconnect", json!({}))).await;
    let status = h
        .until(id, "two failed attempts", |s| {
            s.retry_count >= 2 && s.endpoint_b.consecutive_failure_count >= 2
        })
        .await;
    assert_eq!(status.state, ChannelState::Retrying);
    assert!(status.next_retry_at.is_some());
    assert_eq!(status.endpoint_a.state, EndpointState::Listening);
    assert_eq!(status.endpoint_b.state, EndpointState::Retrying);
    assert!(
        status
            .last_error_summary
            .unwrap()
            .contains("connect failed: connection refused")
    );
    let endpoint = h.endpoint(endpoint_id("sys"));
    assert_eq!(endpoint.state, EndpointState::Retrying);
    assert!(endpoint.last_error.is_some());

    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    send(&mut inst, b"first-session\n").await;
    expect_bytes(&mut sys, b"first-session\n").await;
    let status = h.until(id, "stable", |s| s.retry_count == 0).await;
    assert_eq!(status.next_retry_at, None);
    assert_eq!(status.endpoint_b.consecutive_failure_count, 0);
    assert!(h.endpoint(endpoint_id("sys")).last_error.is_none());
}

#[tokio::test]
async fn overflow_degrades_holds_the_sender_and_loses_nothing() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("overflow");
    let buffer = json!({"buffer": {"max_buffered_segments": 8, "max_bytes": 65536, "block_timeout_ms": 100}});
    h.apply(standard("overflow", buffer)).await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;

    const TOTAL: usize = 2 * 1024 * 1024;
    let payload = counter32(TOTAL, 0);
    let sender = tokio::spawn(async move {
        send(&mut inst, &payload).await;
        inst
    });
    let status = h
        .until(id, "degraded", |s| s.state == ChannelState::Degraded)
        .await;
    assert!(status.buffer_a_to_b.overflowing);
    assert_eq!(status.buffer_a_to_b.max_bytes, 65536);
    assert_eq!(status.buffer_a_to_b.max_segments, 8);
    assert!(status.counters.overflow_count >= 1);
    assert_eq!(status.session_state, SessionState::Bridged);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!sender.is_finished(), "the sender is held back");

    let mut received = vec![0_u8; TOTAL];
    tokio::time::timeout(Duration::from_secs(20), sys.read_exact(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert!(
        received == counter32(TOTAL, 0),
        "every byte arrives in order"
    );
    let _inst = sender.await.unwrap();
    let status = h
        .until(id, "running again", |s| s.state == ChannelState::Running)
        .await;
    assert_eq!(status.counters.bytes_a_to_b, TOTAL as u64);
    assert!(!status.buffer_a_to_b.overflowing);
    assert_eq!(
        h.channel(id).bridge.unwrap().a_to_b.overflow_count,
        status.counters.overflow_count
    );
}

#[tokio::test]
async fn receiver_then_sender_waits_unbounded_then_bounds_the_second_leg() {
    let (mut h, net) = harness();
    let id = channel_id("rts");
    h.apply(document(
        vec![listener("recv", 9201), listener("sender", 9202)],
        vec![bridge(
            "rts",
            "recv",
            "sender",
            json!({"connection": {"connect_strategy": "receiver_then_sender", "receiver": "a", "pairing_timeout_ms": 300}}),
        )],
    ))
    .await;
    let status = h
        .until(id, "waiting for the receiver", |s| {
            s.endpoint_a.state == EndpointState::Listening
        })
        .await;
    assert_eq!(status.state, ChannelState::Starting);
    assert_eq!(status.endpoint_b.state, EndpointState::Disconnected);
    assert_eq!(status.receiver, Some(BridgeSide::A));
    assert!(
        net.connect("127.0.0.1:9202").is_err(),
        "sender listener not bound yet"
    );

    tokio::time::sleep(Duration::from_millis(600)).await;
    let status = h.status(id);
    assert_eq!(
        status.state,
        ChannelState::Starting,
        "the receiver wait is unbounded"
    );
    assert_eq!(status.retry_count, 0);
    assert_eq!(status.last_error_summary, None);

    let mut inst = dial(&net, "127.0.0.1:9201").await;
    h.until(id, "sender listening", |s| {
        s.endpoint_b.state == EndpointState::Listening
    })
    .await;
    let mut sys = dial(&net, "127.0.0.1:9202").await;
    h.running(id).await;
    send(&mut inst, b"from-instrument\n").await;
    expect_bytes(&mut sys, b"from-instrument\n").await;
    send(&mut sys, b"from-system\n").await;
    expect_bytes(&mut inst, b"from-system\n").await;

    drop(sys);
    expect_closed(&mut inst).await;
    h.until(id, "retrying, sender unbound", |s| {
        s.state == ChannelState::Retrying
            && s.endpoint_a.state == EndpointState::Listening
            && s.endpoint_b.state == EndpointState::Disconnected
    })
    .await;

    // The receiver attaches again; the sender never does.
    let mut inst = dial(&net, "127.0.0.1:9201").await;
    expect_closed(&mut inst).await;
    let status = h
        .until(id, "pairing timeout", |s| {
            s.last_error_summary
                .as_deref()
                .is_some_and(|e| e.contains("pairing timeout"))
        })
        .await;
    assert_eq!(status.state, ChannelState::Retrying);
    assert!(status.retry_count >= 1);
    assert!(status.endpoint_b.consecutive_failure_count >= 1);
    assert_eq!(status.endpoint_a.consecutive_failure_count, 0);
    assert!(status.last_error_summary.unwrap().contains("endpoint B"));
}

#[tokio::test]
async fn pause_and_resume_follow_the_operator_rules_and_survive_a_restart() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("pause");
    h.apply(standard("pause", json!({}))).await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;

    let rejected = h.registry.command(id, ChannelCommand::Resume).await;
    assert!(matches!(
        rejected,
        Err(RegistryError::Rejected {
            code: RejectionCode::NotPaused,
            ..
        })
    ));
    h.registry.command(id, ChannelCommand::Pause).await.unwrap();
    let status = h.status(id);
    assert_eq!(status.state, ChannelState::Stopped);
    assert_eq!(status.stop_reason, Some(StopReason::OperatorPaused));
    assert_eq!(status.next_retry_at, None);
    assert_eq!(status.endpoint_a.state, EndpointState::Disconnected);
    assert_eq!(status.endpoint_b.state, EndpointState::Disconnected);
    expect_closed(&mut inst).await;
    expect_closed(&mut sys).await;
    assert!(
        net.connect("127.0.0.1:9001").is_err(),
        "a paused listener is unbound"
    );

    let again = h.registry.command(id, ChannelCommand::Pause).await;
    assert!(matches!(
        again,
        Err(RegistryError::Rejected {
            code: RejectionCode::AlreadyStopped,
            ..
        })
    ));
    let restart = h.registry.command(id, ChannelCommand::Restart).await;
    assert!(matches!(
        restart,
        Err(RegistryError::Rejected {
            code: RejectionCode::OperatorPaused,
            ..
        })
    ));
    let missing = h
        .registry
        .command(channel_id("missing"), ChannelCommand::Pause)
        .await;
    assert!(matches!(missing, Err(RegistryError::NotFound(_))));

    // The flag survives an engine restart.
    let doc = standard("pause", json!({}));
    let mut h = h.restart_engine().await;
    h.apply(doc).await;
    let status = h.status(id);
    assert_eq!(status.stop_reason, Some(StopReason::OperatorPaused));

    h.registry
        .command(id, ChannelCommand::Resume)
        .await
        .unwrap();
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    assert_eq!(h.status(id).stop_reason, None);
    send(&mut sys, b"after\n").await;
    expect_bytes(&mut inst, b"after\n").await;
}

#[tokio::test]
async fn restart_starts_a_new_instance_with_fresh_counters() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("restart");
    h.apply(standard("restart", json!({}))).await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    send(&mut inst, b"x").await;
    expect_bytes(&mut sys, b"x").await;
    let before = h.status(id).instance_started_at;

    h.registry
        .command(id, ChannelCommand::Restart)
        .await
        .unwrap();
    expect_closed(&mut inst).await;
    let status = h.status(id);
    assert!(status.instance_started_at > before);
    assert_eq!(status.counters.bytes_a_to_b, 0);
    assert_ne!(status.state, ChannelState::Stopped);
}

#[tokio::test]
async fn apply_restarts_only_changed_channels_and_relabels_in_place() {
    let (mut h, net) = harness();
    let mut sys1 = net.listen("127.0.0.1:9111", 4).unwrap();
    let mut sys2 = net.listen("127.0.0.1:9112", 4).unwrap();
    let (x, y) = (channel_id("x"), channel_id("y"));
    let doc = |y_bytes: u64, x_label: &str| {
        let mut x_channel = bridge("x", "inst1", "sys1", json!({}));
        x_channel["label"] = json!(x_label);
        document(
            vec![
                listener("inst1", 9011),
                client("sys1", 9111),
                listener("inst2", 9012),
                client("sys2", 9112),
            ],
            vec![
                x_channel,
                bridge(
                    "y",
                    "inst2",
                    "sys2",
                    json!({"buffer": {"max_bytes": y_bytes}}),
                ),
            ],
        )
    };
    h.apply(doc(1_048_576, "Bridge X")).await;
    let mut inst1 = dial(&net, "127.0.0.1:9011").await;
    let mut inst2 = dial(&net, "127.0.0.1:9012").await;
    let mut s1 = sys1.accept().await.unwrap().stream;
    let mut s2 = sys2.accept().await.unwrap().stream;
    h.running(x).await;
    h.running(y).await;
    assert_eq!(h.status(x).config_version, 1);

    let diff = h.apply(doc(2_097_152, "Bridge X")).await;
    assert_eq!(diff.channels[0].change, Change::Unchanged);
    assert_eq!(h.outcome("x"), ApplyOutcome::Unchanged);
    assert_eq!(h.outcome("y"), ApplyOutcome::Restarted);
    expect_open(&mut inst1, 200).await;
    send(&mut s1, b"x2\n").await;
    expect_bytes(&mut inst1, b"x2\n").await;
    assert_eq!(h.status(x).config_version, 1);
    expect_closed(&mut inst2).await;
    expect_closed(&mut s2).await;
    let mut inst2 = dial(&net, "127.0.0.1:9012").await;
    let mut s2 = sys2.accept().await.unwrap().stream;
    h.running(y).await;
    let status = h.status(y);
    assert_eq!(status.config_version, 2);
    assert_eq!(status.buffer_a_to_b.max_bytes, 2_097_152);
    send(&mut inst2, b"y\n").await;
    expect_bytes(&mut s2, b"y\n").await;

    // A label-only change is applied in place.
    h.apply(doc(2_097_152, "Bridge X renamed")).await;
    assert_eq!(h.outcome("x"), ApplyOutcome::Updated);
    assert_eq!(h.channel(x).label, "Bridge X renamed");
    expect_open(&mut inst1, 200).await;
    send(&mut inst1, b"x3\n").await;
    expect_bytes(&mut s1, b"x3\n").await;
    assert_eq!(h.status(x).config_version, 1);

    // Removal stops only the removed channel.
    h.apply(document(
        vec![listener("inst1", 9011), client("sys1", 9111)],
        vec![bridge(
            "x",
            "inst1",
            "sys1",
            json!({"label": "Bridge X renamed"}),
        )],
    ))
    .await;
    expect_closed(&mut inst2).await;
    assert!(h.registry.snapshot().channels.iter().all(|c| c.id != y));
    assert_eq!(h.registry.snapshot().channels.len(), 1);
    send(&mut inst1, b"x4\n").await;
    expect_bytes(&mut s1, b"x4\n").await;
}

#[tokio::test]
async fn disabled_and_message_channels_report_why_they_are_stopped() {
    let (mut h, _net) = harness();
    let mut disabled = bridge("off", "inst", "sys", json!({}));
    disabled["enabled"] = json!(false);
    let message = json!({
        "kind": "message",
        "channel_id": "msg",
        "label": "Message channel",
        "source": {"endpoint": "inst2", "protocol": "hl7_mllp", "settings": {}},
        "destinations": [{
            "destination_id": "out", "label": "Out", "endpoint": "sys2",
            "protocol": "hl7_mllp", "settings": {},
        }],
    });
    h.apply(document(
        vec![
            listener("inst", 9001),
            client("sys", 9101),
            listener("inst2", 9002),
            client("sys2", 9102),
        ],
        vec![disabled, message],
    ))
    .await;
    assert_eq!(h.outcome("off"), ApplyOutcome::Stopped);
    let off = h.channel(channel_id("off"));
    assert_eq!(off.state, ChannelState::Stopped);
    assert_eq!(off.stop_reason, Some(StopReason::DisabledByConfig));
    let msg = h.channel(channel_id("msg"));
    assert_eq!(msg.stop_reason, Some(StopReason::NotImplemented));
    assert!(msg.message.is_some());
    assert_eq!(
        h.endpoint(endpoint_id("inst")).state,
        EndpointState::Disconnected
    );
    let restart = h
        .registry
        .command(channel_id("msg"), ChannelCommand::Restart)
        .await;
    assert!(matches!(restart, Err(RegistryError::NotImplemented(_))));
    let pause = h
        .registry
        .command(channel_id("off"), ChannelCommand::Pause)
        .await;
    assert!(matches!(
        pause,
        Err(RegistryError::Rejected {
            code: RejectionCode::DisabledInConfig,
            ..
        })
    ));
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(BridgeSide, Vec<u8>)>>);

impl TapSink for Recorder {
    fn observed(&self, observation: TapObservation) {
        self.0
            .lock()
            .unwrap()
            .push((observation.from, observation.message));
    }
}

fn mllp(payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x0b];
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&[0x1c, 0x0d]);
    frame
}

#[tokio::test]
async fn hl7_tap_counts_and_records_without_altering_the_stream() {
    let net = MemoryNetwork::new();
    let recorder = Arc::new(Recorder::default());
    let mut options = RegistryOptions::new(Arc::new(net.clone()));
    options.tap_sink = Some(recorder.clone());
    let mut h = Harness::with_options(options);
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("tap");
    h.apply(standard(
        "tap",
        json!({"tap": {"framer": "hl7_mllp", "record_messages": true}}),
    ))
    .await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    assert_eq!(h.status(id).counters.messages_a_to_b, Some(0));

    let oru = mllp(b"MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|CTRL-TAP-1|P|2.5.1\rPID|1||PID-TEST-011||TEST^PATIENT\r");
    let ack = mllp(
        b"MSH|^~\\&|PACKIE|PACKIEFAC|SIMLAB|SIMFAC|20260101120001||ACK|A1|P|2.5.1\rMSA|AA|CTRL-TAP-1\r",
    );
    for _ in 0..2 {
        // A byte-identical repeat is real traffic: counted again, never deduplicated.
        send(&mut inst, &oru[..10]).await;
        send(&mut inst, &oru[10..]).await;
        expect_bytes(&mut sys, &oru).await;
        send(&mut sys, &ack).await;
        expect_bytes(&mut inst, &ack).await;
    }
    let status = h
        .until(id, "tap counts", |s| {
            s.counters.messages_a_to_b == Some(2) && s.counters.messages_b_to_a == Some(2)
        })
        .await;
    assert_eq!(status.counters.framing_errors, Some(0));
    assert_eq!(status.counters.tap_dropped, Some(0));
    assert_eq!(status.tap_framer, packie_config::TapFramer::Hl7Mllp);
    let recorded = recorder.0.lock().unwrap().clone();
    assert_eq!(recorded.len(), 4);
    assert_eq!(
        recorded
            .iter()
            .filter(|(side, _)| *side == BridgeSide::A)
            .count(),
        2
    );
    assert!(
        recorded
            .iter()
            .any(|(_, m)| m.starts_with(b"MSH|^~\\&|SIMLAB"))
    );
}

#[tokio::test]
async fn transcript_viewer_sees_segments_only_while_watching() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("transcript");
    h.apply(standard("transcript", json!({}))).await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    let mut viewer = h.context.tap.subscribe(id).unwrap();
    send(&mut inst, b"\x02seen\x03").await;
    expect_bytes(&mut sys, b"\x02seen\x03").await;
    let segment = tokio::time::timeout(WITHIN, viewer.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(segment.bytes, b"\x02seen\x03");
    assert_eq!(segment.endpoint_id, endpoint_id("inst"));
}

#[tokio::test]
async fn shutdown_drains_buffered_bytes_then_stops() {
    let (mut h, net) = harness();
    let mut sys_listener = net.listen("127.0.0.1:9101", 4).unwrap();
    let id = channel_id("drain");
    h.apply(standard("drain", json!({}))).await;
    let mut inst = dial(&net, "127.0.0.1:9001").await;
    let mut sys = sys_listener.accept().await.unwrap().stream;
    h.running(id).await;
    // Fill the pipe to sys and the A→B buffer while sys is not reading.
    let payload = counter32(256 * 1024, 5);
    let sender = tokio::spawn(async move {
        send(&mut inst, &payload).await;
        inst
    });
    let _inst = sender.await.unwrap();
    h.until(id, "bytes buffered", |s| s.buffer_a_to_b.bytes > 0)
        .await;
    let reader = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut all = Vec::new();
        sys.read_to_end(&mut all).await.unwrap();
        all
    });
    h.registry.shutdown(Duration::from_secs(5)).await;
    let received = reader.await.unwrap();
    assert!(
        received == counter32(256 * 1024, 5),
        "the drain delivered everything"
    );
    let status = h.status(id);
    assert_eq!(status.state, ChannelState::Stopped);
    assert_eq!(status.stop_reason, Some(StopReason::Shutdown));
    let applied = h
        .apply(standard("drain", json!({"label": "after shutdown"})))
        .await;
    assert_eq!(applied.channels.len(), 1);
    assert_eq!(
        h.channel(id).state,
        ChannelState::Stopped,
        "nothing starts after shutdown"
    );
}

/// `diagnostics.retain_recent_errors` of the document bounds each channel's recent errors
/// (`configuration.md` §3); applying a lower value later trims the list in place.
#[tokio::test]
async fn the_document_bounds_the_recent_errors_kept() {
    let net = MemoryNetwork::new();
    let mut h = Harness::new(Arc::new(net.clone()));
    let fast = json!({"retry": {
        "initial_delay_ms": 10, "max_delay_ms": 10, "multiplier": 1, "jitter_percent": 0,
        "max_consecutive_failures_before_alert": 3,
    }});
    // B dials an address nobody listens on: every attempt fails and is recorded.
    let mut document = document(
        vec![listener("rre-a", 7301), client("rre-b", 7302)],
        vec![bridge("rre", "rre-a", "rre-b", fast)],
    );
    document["diagnostics"] = json!({"retain_recent_errors": 12});
    h.apply(document.clone()).await;
    let id = channel_id("rre");
    h.until(id, "twenty failures recorded", |s| s.retry_count >= 20)
        .await;
    assert_eq!(h.status(id).recent_errors, 12);

    document["diagnostics"] = json!({"retain_recent_errors": 10});
    h.apply(document).await;
    assert_eq!(h.status(id).recent_errors, 10);
}
