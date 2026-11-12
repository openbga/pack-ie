//! `ByteStream::peer_probe` on an accepted TCP connection: a reset the system ignored is
//! drawn again, so the pending read fails.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::time::Duration;

use packie_transport::PROBE_AFTER;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use common::{binder, endpoint};

/// The peer sends a 700 byte message and `ours` answers, `times` times, a few
/// milliseconds apart like a paced instrument.
async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    ours: &mut S,
    peer: &mut TcpStream,
    times: usize,
) {
    let message = [b'M'; 700];
    let mut buf = [0_u8; 700];
    let mut reply = [0_u8; 3];
    for _ in 0..times {
        peer.write_all(&message).await.unwrap();
        ours.read_exact(&mut buf).await.unwrap();
        ours.write_all(b"ACK").await.unwrap();
        peer.read_exact(&mut reply).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// The instrument of the soak's `session_held_after_cut`: after a few dozen exchanges it
/// sends half a message and resets. macOS ignores that reset (the connection stays
/// established with nothing to read); with the probe on, the read fails within seconds.
/// Linux and Windows end the connection on the reset themselves.
#[tokio::test]
async fn a_probe_ends_a_connection_whose_reset_behind_half_a_message_was_ignored() {
    let port = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config =
        endpoint(json!({"kind": "tcp_listener", "bind_address": "127.0.0.1", "port": port}));
    let mut acceptor = binder(&config).bind().await.unwrap();
    let (peer, incoming) = tokio::join!(TcpStream::connect(("127.0.0.1", port)), acceptor.accept());
    let (mut peer, mut ours) = (peer.unwrap(), incoming.unwrap().link.stream);
    let probe = ours.peer_probe().expect("TCP links have a probe");
    exchange(&mut ours, &mut peer, 60).await;
    peer.write_all(&[b'M'; 350]).await.unwrap();
    peer.set_zero_linger().unwrap();
    drop(peer);
    let mut buf = [0_u8; 1024];
    match ours.read(&mut buf).await {
        Ok(n) => assert_eq!(n, 350, "the half message is read first"),
        // Windows discards unread data when the reset arrives and reports the reset at
        // once: the connection already ended without the probe.
        Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::ConnectionReset => return,
        Err(e) => panic!("the half message was not read: {e}"),
    }
    probe.waiting(true);
    let ended = tokio::time::timeout(PROBE_AFTER * 5, ours.read(&mut buf)).await;
    assert!(
        matches!(ended, Ok(Ok(0) | Err(_))),
        "the connection did not end: {ended:?}"
    );
}
