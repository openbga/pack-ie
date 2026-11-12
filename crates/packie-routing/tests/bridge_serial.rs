//! `bridge-serial-tcp`: a serial endpoint bridged to TCP through the host transports.
//!
//! - Every Unix host: a virtual serial endpoint (`virtual_link`). The engine serves one
//!   pseudo-terminal end and exposes the other at a link path, which the test opens as a
//!   plain file the way COM-only software would. Covers opening during the attempt,
//!   8-bit clean traffic both ways and idle reads that are not disconnects. The transport
//!   keeps the far end open itself, so a virtual link never sees a hang-up: unplug cannot
//!   be simulated through it.
//! - Linux only: a physical-style endpoint on a raw pseudo-terminal opened by path
//!   (macOS refuses the baud-rate call on a pseudo-terminal). Closing the far end is an
//!   unplug: the engine's reads fail, the session ends and the channel retries. A pty
//!   path disappears with its pair, so replug onto the same path is not simulated.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use packie_core::{ChannelState, EndpointKind, EndpointState, SessionState};
use serde_json::json;
use tokio::fs::File;
use tokio::net::TcpListener;

async fn open_peer(link: &Path) -> File {
    let deadline = tokio::time::Instant::now() + WITHIN;
    loop {
        match tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(link)
            .await
        {
            Ok(file) => return file,
            Err(error) if tokio::time::Instant::now() >= deadline => {
                panic!("{}: {error}", link.display())
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn virtual_serial_to_tcp_is_8_bit_clean_and_idle_reads_are_not_disconnects() {
    let mut h = Harness::new(system_transports());
    let link = h.dir.path().join("ttyPACKIE-inst");
    let sys = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sys_port = sys.local_addr().unwrap().port();
    let serial = json!({
        "endpoint_id": "serial-inst",
        "label": "Instrument port",
        "kind": "serial",
        "settings": {
            "port_name": "virtual", "virtual_link": link, "baud_rate": 9600, "read_timeout_ms": 50,
        },
    });
    h.apply(document(
        vec![serial, client("serial-sys", sys_port)],
        vec![bridge("serial", "serial-inst", "serial-sys", json!({}))],
    ))
    .await;
    let id = channel_id("serial");

    let (mut tcp, _) = tokio::time::timeout(WITHIN, sys.accept())
        .await
        .unwrap()
        .unwrap();
    let mut port = open_peer(&link).await;
    let status = h
        .until(id, "bridged", |s| {
            s.state == ChannelState::Running && s.session_state == SessionState::Bridged
        })
        .await;
    assert_eq!(status.endpoint_a.kind, EndpointKind::Serial);
    assert_eq!(status.endpoint_a.state, EndpointState::Connected);

    send(&mut port, &[0x05]).await;
    expect_bytes(&mut tcp, &[0x05]).await;
    send(&mut tcp, &[0x06]).await;
    expect_bytes(&mut port, &[0x06]).await;
    // Every byte value, including control characters, passes unchanged.
    let up = counter32(4096, 3);
    send(&mut port, &up).await;
    expect_bytes(&mut tcp, &up).await;
    let down = counter32(4096, 7);
    send(&mut tcp, &down).await;
    expect_bytes(&mut port, &down).await;

    // Several read timeouts pass with no data: not a disconnect.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let status = h.status(id);
    assert_eq!(status.state, ChannelState::Running);
    assert_eq!(status.endpoint_a.state, EndpointState::Connected);
    assert_eq!(status.endpoint_a.consecutive_failure_count, 0);
    assert_eq!(status.retry_count, 0);
}

#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "macOS refuses the baud-rate call on a pseudo-terminal opened by path"
)]
async fn serial_unplug_ends_the_session_and_retries() {
    use std::io::Write as _;

    let (master, slave) = serialport::TTYPort::pair().unwrap();
    let port_name = serialport::SerialPort::name(&slave).unwrap();
    drop(slave);
    let mut h = Harness::new(system_transports());
    let sys = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sys_port = sys.local_addr().unwrap().port();
    let serial = json!({
        "endpoint_id": "pty-inst",
        "label": "Instrument pty",
        "kind": "serial",
        "settings": {"port_name": port_name, "baud_rate": 9600, "read_timeout_ms": 50},
    });
    h.apply(document(
        vec![serial, client("pty-sys", sys_port)],
        vec![bridge("pty", "pty-inst", "pty-sys", json!({}))],
    ))
    .await;
    let id = channel_id("pty");
    let (mut tcp, _) = tokio::time::timeout(WITHIN, sys.accept())
        .await
        .unwrap()
        .unwrap();
    h.until(id, "bridged", |s| s.session_state == SessionState::Bridged)
        .await;
    let mut master = tokio::task::spawn_blocking(move || {
        let mut master = master;
        master.write_all(&[0x05]).unwrap();
        master
    })
    .await
    .unwrap();
    expect_bytes(&mut tcp, &[0x05]).await;

    // Unplug: the far end goes away and the engine's reads fail.
    master.flush().unwrap();
    drop(master);
    let status = h
        .until(id, "retrying after unplug", |s| {
            s.state == ChannelState::Retrying
        })
        .await;
    let summary = status.last_error_summary.unwrap();
    assert!(
        summary.contains("endpoint A")
            && (summary.contains("read failed") || summary.contains("write failed")),
        "{summary}"
    );
    expect_closed(&mut tcp).await;
    h.until(id, "the vanished port keeps failing", |s| {
        s.endpoint_a.consecutive_failure_count >= 2
    })
    .await;
}
