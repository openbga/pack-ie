//! Bridge behaviors over loopback TCP through the host transports (`packie-transport`).

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use common::*;
use packie_core::{ChannelId, ChannelState, EndpointState};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};

/// A port that was free a moment ago.
async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

struct Rig {
    h: Harness,
    id: ChannelId,
    inst_port: u16,
    sys: TcpListener,
}

/// `inst` (A) is an engine listener; `sys` (B) is a peer server the engine dials.
async fn rig(name: &str) -> Rig {
    let mut h = Harness::new(system_transports());
    let sys = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sys_port = sys.local_addr().unwrap().port();
    let inst_port = free_port().await;
    h.apply(document(
        vec![
            listener(&format!("{name}-inst"), inst_port),
            client(&format!("{name}-sys"), sys_port),
        ],
        vec![bridge(
            name,
            &format!("{name}-inst"),
            &format!("{name}-sys"),
            json!({}),
        )],
    ))
    .await;
    let id = channel_id(name);
    h.until(id, "listening", |s| {
        s.endpoint_a.state == EndpointState::Listening
    })
    .await;
    Rig {
        h,
        id,
        inst_port,
        sys,
    }
}

impl Rig {
    async fn connect(&self) -> TcpStream {
        TcpStream::connect(("127.0.0.1", self.inst_port))
            .await
            .unwrap()
    }

    async fn accept(&self) -> TcpStream {
        tokio::time::timeout(WITHIN, self.sys.accept())
            .await
            .unwrap()
            .unwrap()
            .0
    }
}

#[tokio::test]
async fn tcp_full_duplex_64k_both_directions() {
    let rig = rig("tcp-duplex").await;
    let mut inst = rig.connect().await;
    let mut sys = rig.accept().await;
    rig.h.running(rig.id).await;

    let (mut inst_read, mut inst_write) = inst.split();
    let (mut sys_read, mut sys_write) = sys.split();
    let up = counter32(65536, 1);
    let down = counter32(65536, 2);
    tokio::join!(
        send(&mut inst_write, &up),
        send(&mut sys_write, &down),
        expect_bytes(&mut sys_read, &up),
        expect_bytes(&mut inst_read, &down),
    );
    let status = rig
        .h
        .until(rig.id, "counters", |s| {
            s.counters.bytes_a_to_b == 65536 && s.counters.bytes_b_to_a == 65536
        })
        .await;
    assert_eq!(status.retry_count, 0);
    assert!(
        status
            .endpoint_a
            .active_peer
            .as_deref()
            .unwrap()
            .starts_with("127.0.0.1:")
    );
}

#[tokio::test]
async fn tcp_close_propagates_and_the_bridge_recovers() {
    let rig = rig("tcp-close").await;
    let mut inst = rig.connect().await;
    let sys = rig.accept().await;
    rig.h.running(rig.id).await;
    drop(sys);
    expect_closed(&mut inst).await;
    let status = rig
        .h
        .until(rig.id, "retrying", |s| s.state == ChannelState::Retrying)
        .await;
    assert_eq!(status.endpoint_a.state, EndpointState::Listening);
    assert!(status.last_error_summary.unwrap().contains("read failed"));

    let mut inst = rig.connect().await;
    let mut sys = rig.accept().await;
    rig.h.running(rig.id).await;
    send(&mut inst, b"again\n").await;
    expect_bytes(&mut sys, b"again\n").await;
    rig.h.until(rig.id, "stable", |s| s.retry_count == 0).await;
}

#[tokio::test]
async fn tcp_second_peer_is_reset_while_a_session_is_active() {
    let rig = rig("tcp-single").await;
    let mut first = rig.connect().await;
    let mut sys = rig.accept().await;
    rig.h.running(rig.id).await;
    let mut second = rig.connect().await;
    expect_closed(&mut second).await;
    expect_open(&mut first, 200).await;
    send(&mut first, b"first\n").await;
    expect_bytes(&mut sys, b"first\n").await;
    assert_eq!(rig.h.status(rig.id).state, ChannelState::Running);
}

#[tokio::test]
async fn tcp_client_refused_backs_off_then_connects() {
    let mut h = Harness::new(system_transports());
    let (inst_port, sys_port) = (free_port().await, free_port().await);
    // Windows retries a refused SYN for about two seconds before reporting the refusal; a
    // shorter connect timeout would report a timeout there instead.
    let mut sys_client = client("tcp-rc-sys", sys_port);
    sys_client["settings"]["connect_timeout_ms"] = json!(5000);
    h.apply(document(
        vec![listener("tcp-rc-inst", inst_port), sys_client],
        vec![bridge("tcp-rc", "tcp-rc-inst", "tcp-rc-sys", json!({}))],
    ))
    .await;
    let id = channel_id("tcp-rc");
    let status = h.until(id, "backing off", |s| s.retry_count >= 2).await;
    assert!(
        status
            .last_error_summary
            .unwrap()
            .contains("connect failed: connection refused")
    );
    let sys = TcpListener::bind(("127.0.0.1", sys_port)).await.unwrap();
    let mut inst = TcpStream::connect(("127.0.0.1", inst_port)).await.unwrap();
    let (mut sys, _) = tokio::time::timeout(WITHIN, sys.accept())
        .await
        .unwrap()
        .unwrap();
    h.running(id).await;
    send(&mut inst, b"hello\n").await;
    expect_bytes(&mut sys, b"hello\n").await;
    h.until(id, "stable", |s| s.retry_count == 0).await;
}
