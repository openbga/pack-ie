//! HL7 sources over loopback TCP through the host transports: what a real socket does
//! when the sender goes away (`docs/spec/message-channels.md` §2.2).

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use packie_hl7::mllp;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A port that was free a moment ago.
async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// The soak instrument's message: an ORU of about 700 bytes, framed.
fn message(control: &str) -> Vec<u8> {
    let mut body = oru(control);
    body.extend_from_slice(format!("NTE|1||{}\r", "x".repeat(520)).as_bytes());
    mllp::encode(&body, 1 << 20).unwrap()
}

/// Sends `frame` on `stream` and waits up to `within` for its acknowledgement.
async fn exchange(stream: &mut TcpStream, frame: &[u8], within: Duration) -> bool {
    let attempt = async {
        stream.write_all(frame).await.ok()?;
        let mut reply = vec![0_u8; 4096];
        let n = stream.read(&mut reply).await.ok()?;
        (n > 0 && reply[..n].ends_with(&[0x1C, 0x0D])).then_some(())
    };
    tokio::time::timeout(within, attempt)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// A sender served within `limit` on a new connection, and how long that took.
async fn served(port: u16, frame: &[u8], limit: Duration) -> (Option<TcpStream>, Duration) {
    let started = Instant::now();
    while started.elapsed() < limit {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)).await
            && exchange(&mut stream, frame, Duration::from_millis(500)).await
        {
            return (Some(stream), started.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    (None, started.elapsed())
}

/// Soak defect `session_held_after_cut`, the soak's exact sequence: an instrument that
/// has sent a few dozen messages on its connection sends half of the next and resets the
/// connection, waits 400 ms and reconnects. macOS ignores that reset (it arrives behind
/// bytes not yet acknowledged), so without the source's keep-alive probe the session held
/// the listener's single slot until `frame_timeout_ms` (here 30 s) ended the frame.
#[tokio::test]
async fn a_reset_behind_half_a_message_on_a_busy_connection_frees_the_session_at_once() {
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let store = open_store(dir.path(), None);
    let mut h = Harness::in_dir(
        Arc::clone(&dir),
        options_with_store(system_transports(), &store),
    );
    let lis = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lis_port = lis.local_addr().unwrap().port();
    let port = free_port().await;
    let channel = hl7_channel(json!({"frame_timeout_ms": 30_000}), json!({}), json!({}));
    h.apply(document(
        vec![listener("lab-in", port), client("lis-out", lis_port)],
        vec![channel],
    ))
    .await;
    let (mut sender, _) = served(port, &message("FIRST"), WITHIN).await;
    for round in 0..3 {
        let mut instrument = sender.take().expect("a sender the engine serves");
        for seq in 0..40 {
            let frame = message(&format!("R{round}-{seq}"));
            assert!(
                exchange(&mut instrument, &frame, Duration::from_secs(5)).await,
                "round {round}: message {seq} was not acknowledged"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let next = message(&format!("CUT-{round}"));
        instrument.write_all(&next[..next.len() / 2]).await.unwrap();
        instrument.flush().await.unwrap();
        instrument.set_zero_linger().unwrap();
        drop(instrument);
        let cut = Instant::now();
        tokio::time::sleep(Duration::from_millis(400)).await;
        let (next_sender, _) = served(port, &next, Duration::from_secs(10)).await;
        let waited = cut.elapsed();
        assert!(
            next_sender.is_some(),
            "round {round}: no sender was served within 10 s of the reset"
        );
        assert!(
            waited < Duration::from_secs(4),
            "round {round}: the session was held {waited:?} after the reset"
        );
        sender = next_sender;
    }
    drop(lis);
}
