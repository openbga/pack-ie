//! Serial ports through a virtual (pseudo-terminal) link.
#![cfg(all(test, unix))]

mod common;

use std::io::{Read, Write};
use std::task::Poll;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use common::{dialer, endpoint, holder, transports};

#[tokio::test(flavor = "multi_thread")]
async fn virtual_link_round_trip_and_single_holder() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("ttyPACKIE0");
    let config = endpoint(json!({
        "kind": "serial", "port_name": "virtual", "virtual_link": link,
        "baud_rate": 9600, "read_timeout_ms": 50,
    }));
    let transports = transports();
    let dial = dialer(&transports, &config, holder());
    let mut engine_side = dial.dial().await.unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // Software that only speaks COM opens the other end.
    // The pair is configured raw already, so the peer opens it as a plain file (macOS
    // pseudo-terminals reject the baud-rate call a serial library would make).
    let mut peer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&link)
        .unwrap();
    peer.write_all(b"\x02R|1|^^^GLU|5.4\x03").unwrap();
    let mut received = vec![0_u8; 16];
    tokio::time::timeout(
        Duration::from_secs(5),
        engine_side.stream.read_exact(&mut received),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(received, b"\x02R|1|^^^GLU|5.4\x03");

    engine_side.stream.write_all(b"\x06").await.unwrap();
    let mut ack = [0_u8; 1];
    peer.read_exact(&mut ack).unwrap();
    assert_eq!(ack, [6]);

    // A second channel instance cannot take the port while the first holds it.
    let other = dialer(&transports, &config, holder());
    let error = other.dial().await.unwrap_err();
    assert!(error.cause.contains("in use by channel"), "{error}");

    // After release the next holder gets the same open port and the bytes sent meanwhile.
    drop(engine_side);
    peer.write_all(b"later").unwrap();
    let mut next = other.dial().await.unwrap();
    let mut later = vec![0_u8; 5];
    tokio::time::timeout(Duration::from_secs(5), next.stream.read_exact(&mut later))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(later, b"later");
}

#[tokio::test]
async fn missing_ports_fail_the_attempt() {
    let config = endpoint(
        json!({"kind": "serial", "port_name": "/dev/packie-no-such-port", "baud_rate": 9600}),
    );
    let error = dialer(&transports(), &config, holder())
        .dial()
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("open failed: /dev/packie-no-such-port"),
        "{error}"
    );
}

/// A bridge stop or pairing timeout drops the dial while the port opens; the claim must
/// not outlive it (bridge-channels §9).
#[tokio::test(flavor = "multi_thread")]
async fn cancelled_open_releases_the_port() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("ttyPACKIE1");
    let config = endpoint(json!({
        "kind": "serial", "port_name": "virtual", "virtual_link": link,
        "baud_rate": 9600, "read_timeout_ms": 50,
    }));
    let transports = transports();
    let cancelled = dialer(&transports, &config, holder());
    {
        // One poll claims the port and starts the open on a blocking thread.
        let mut dial = cancelled.dial();
        let first = std::future::poll_fn(|cx| Poll::Ready(dial.as_mut().poll(cx))).await;
        assert!(first.is_pending(), "the open finished within one poll");
    }

    let next = dialer(&transports, &config, holder());
    let error_text = match next.dial().await {
        Ok(_link) => String::new(),
        Err(error) => error.to_string(),
    };
    assert_eq!(error_text, "", "the port stayed claimed after the cancel");
}
