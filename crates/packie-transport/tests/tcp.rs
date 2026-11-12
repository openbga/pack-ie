//! TCP client and listener over loopback.
#![cfg(test)]

mod common;

use std::time::Duration;

use packie_transport::Acceptor;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use common::{binder, dialer, endpoint, holder, transports};

async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn bind(settings: serde_json::Value) -> Box<dyn Acceptor> {
    binder(&endpoint(settings)).bind().await.unwrap()
}

/// Reads until end of stream or reset; `true` when the peer closed it without data.
async fn closed_without_data(stream: &mut TcpStream) -> bool {
    let mut buf = [0_u8; 16];
    matches!(
        tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await,
        Ok(Ok(0) | Err(_))
    )
}

#[tokio::test]
async fn client_connects_and_reports_refusal() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = server.local_addr().unwrap().port();
    let config = endpoint(json!({"kind": "tcp_client", "host": "127.0.0.1", "port": port}));
    let dialer = dialer(&transports(), &config, holder());
    let mut link = dialer.dial().await.unwrap();
    assert_eq!(
        link.peer.as_deref(),
        Some(format!("127.0.0.1:{port}").as_str())
    );
    let (mut accepted, _) = server.accept().await.unwrap();
    link.stream.write_all(b"\x05").await.unwrap();
    let mut byte = [0_u8; 1];
    accepted.read_exact(&mut byte).await.unwrap();
    assert_eq!(byte, [5]);

    drop(server);
    let error = dialer.dial().await.unwrap_err();
    assert_eq!(error.to_string(), "connect failed: connection refused");
    assert!(!error.persistent);
}

#[tokio::test]
async fn listener_enforces_allowlist_and_single_peer() {
    let port = free_port().await;
    let mut acceptor = bind(json!({
        "kind": "tcp_listener", "bind_address": "127.0.0.1", "port": port,
        "allowed_peer_cidrs": ["127.0.0.1"],
    }))
    .await;
    let mut first = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut incoming = acceptor.accept().await.unwrap();
    assert_eq!(incoming.ip, Some("127.0.0.1".parse().unwrap()));

    // A second peer while the first is connected is reset and counted.
    let mut second = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert!(closed_without_data(&mut second).await);
    let rejections = acceptor.rejections();
    assert_eq!(rejections.count, 1);
    assert_eq!(rejections.last.unwrap().2, "session_active");

    first.write_all(b"hi").await.unwrap();
    let mut buf = [0_u8; 2];
    incoming.link.stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hi");

    // Once the first link is gone, the next peer is accepted on the same socket.
    drop(incoming);
    drop(first);
    let _third = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert!(acceptor.accept().await.is_ok());
}

#[tokio::test]
async fn a_peer_reconnecting_at_once_waits_for_the_old_session_to_release() {
    let port = free_port().await;
    let mut acceptor = bind(json!({
        "kind": "tcp_listener", "bind_address": "127.0.0.1", "port": port,
        "allowed_peer_cidrs": ["127.0.0.1"],
    }))
    .await;
    let first = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let incoming = acceptor.accept().await.unwrap();

    // The sender reconnects immediately; the old session releases its link 50 ms later.
    drop(first);
    let mut second = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(incoming);
    let mut accepted = tokio::time::timeout(Duration::from_secs(2), acceptor.accept())
        .await
        .unwrap()
        .unwrap();
    second.write_all(b"ok").await.unwrap();
    let mut buf = [0_u8; 2];
    accepted.link.stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ok");
    assert_eq!(acceptor.rejections().count, 0);
}

#[tokio::test]
async fn peers_outside_the_allowlist_are_reset() {
    let port = free_port().await;
    let mut acceptor = bind(json!({
        "kind": "tcp_listener", "bind_address": "127.0.0.1", "port": port,
        "allowed_peer_cidrs": ["10.0.0.0/8"],
    }))
    .await;
    let mut stranger = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert!(closed_without_data(&mut stranger).await);
    let rejections = acceptor.rejections();
    assert_eq!(
        (rejections.count, rejections.last.unwrap().2),
        (1, "not_allowed")
    );
    let nothing = tokio::time::timeout(Duration::from_millis(200), acceptor.accept()).await;
    assert!(nothing.is_err(), "a refused peer must not be handed out");
}

#[tokio::test]
async fn idle_links_time_out() {
    let port = free_port().await;
    let mut acceptor = bind(json!({
        "kind": "tcp_listener", "bind_address": "127.0.0.1", "port": port,
        "idle_timeout_seconds": 1,
    }))
    .await;
    let _peer = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut incoming = acceptor.accept().await.unwrap();
    let mut buf = [0_u8; 8];
    let started = std::time::Instant::now();
    let error = incoming.link.stream.read(&mut buf).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() >= Duration::from_millis(900));
}

#[tokio::test]
async fn bind_conflicts_are_reported_with_the_address() {
    let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = taken.local_addr().unwrap().port();
    let config =
        endpoint(json!({"kind": "tcp_listener", "bind_address": "127.0.0.1", "port": port}));
    let Err(error) = binder(&config).bind().await else {
        panic!("bound a taken port");
    };
    assert_eq!(
        error.to_string(),
        format!("listen failed: address in use on 127.0.0.1:{port}")
    );
    assert!(!error.persistent);
}
