//! ASTM E1381/E1394 message channels (`docs/spec/message-channels.md` §2.2, §3, §8) with
//! scripted senders and receivers over the in-memory network.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::*;
use packie_astm::{ACK, ENQ, EOT, LF, NAK, STX, encode_records};
use packie_core::{AckLevel, DeliveryState, MessageState};
use packie_routing::memory::MemoryNetwork;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const ANALYZER: &str = "127.0.0.1:9611";
const LIS: &str = "127.0.0.1:9711";

fn records(sample: &str) -> Vec<Vec<u8>> {
    [
        "H|\\^&|CTRL-A||SIMANALYZER^1.0|||||||P|1|20260101120000".to_owned(),
        "P|1|PID-TEST-A".to_owned(),
        format!("O|1|{sample}||^^^GLU"),
        "R|1|^^^GLU|5.4|mmol/L".to_owned(),
        "L|1|N".to_owned(),
    ]
    .into_iter()
    .map(String::into_bytes)
    .collect()
}

fn original(records: &[Vec<u8>]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|r| r.iter().copied().chain(*b"\r"))
        .collect()
}

async fn rig(destination: Value) -> MessageRig {
    let channel = json!({
        "kind": "message",
        "channel_id": "results",
        "label": "Results",
        "source": {"endpoint": "analyzer", "protocol": "astm", "settings": {}},
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-astm",
            "protocol": "astm", "settings": destination,
        }],
        "retry": quick_retry(),
        "delivery": {
            "max_attempts": 2, "send_timeout_seconds": 3, "ack_timeout_seconds": 1,
            "retry": quick_retry(),
        },
    });
    MessageRig::start(document(
        vec![listener("analyzer", 9611), client("lis-astm", 9711)],
        vec![channel],
    ))
    .await
}

async fn analyzer(rig: &MessageRig) -> Line {
    Line::new(dial(&rig.net, ANALYZER).await)
}

/// A scripted ASTM line end.
struct Line {
    stream: DuplexStream,
    buffer: Vec<u8>,
}

impl Line {
    fn new(stream: DuplexStream) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
        }
    }

    async fn send(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
    }

    async fn fill(&mut self, ms: u64) -> bool {
        let mut buf = vec![0_u8; 4096];
        match tokio::time::timeout(Duration::from_millis(ms), self.stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                self.buffer.extend_from_slice(&buf[..n]);
                true
            }
            _ => false,
        }
    }

    /// The next control byte, or `None` within `ms`.
    async fn control_within(&mut self, ms: u64) -> Option<u8> {
        loop {
            if let Some(first) = self.buffer.first().copied() {
                self.buffer.remove(0);
                return Some(first);
            }
            if !self.fill(ms).await {
                return None;
            }
        }
    }

    async fn expect_control(&mut self, expected: u8) {
        assert_eq!(self.control_within(5000).await, Some(expected));
    }

    /// The next item: a control byte or a whole frame (STX … LF).
    async fn next_item(&mut self) -> Vec<u8> {
        loop {
            if let Some(item) = self.take_item() {
                return item;
            }
            assert!(self.fill(5000).await, "line went silent");
        }
    }

    fn take_item(&mut self) -> Option<Vec<u8>> {
        let first = *self.buffer.first()?;
        if first != STX {
            self.buffer.remove(0);
            return Some(vec![first]);
        }
        let end = self.buffer.iter().position(|b| *b == LF)?;
        Some(self.buffer.drain(..=end).collect())
    }

    /// Sends a whole transfer as an instrument: ENQ, frames, EOT, checking each ACK.
    async fn transfer(&mut self, records: &[Vec<u8>]) {
        self.send(&[ENQ]).await;
        self.expect_control(ACK).await;
        for frame in encode_records(records).unwrap() {
            self.send(&frame).await;
            self.expect_control(ACK).await;
        }
        self.send(&[EOT]).await;
    }

    /// Receives a whole transfer as a LIS, ACKing everything; returns the frame texts.
    async fn receive(&mut self) -> Vec<Vec<u8>> {
        self.expect_control(ENQ).await;
        self.send(&[ACK]).await;
        let mut frames = Vec::new();
        loop {
            let item = self.next_item().await;
            if item == [EOT] {
                return frames;
            }
            frames.push(item);
            self.send(&[ACK]).await;
        }
    }
}

async fn lis(net: &MemoryNetwork) -> (packie_routing::memory::MemoryListener, Line) {
    let (listener, stream) = accept_on(net, LIS).await;
    (listener, Line::new(stream))
}

#[tokio::test]
async fn frames_commit_before_their_ack_and_the_message_is_delivered() {
    let rig = rig(json!({})).await;
    let (_listener, mut lis) = lis(&rig.net).await;
    let mut analyzer = analyzer(&rig).await;
    let sent = records("SID-A1");
    analyzer.transfer(&sent).await;

    let messages = rig
        .messages_until("routed", |m| {
            m.first()
                .is_some_and(|m| m.summary.state == MessageState::Routed)
        })
        .await;
    // The records are what the store keeps; its frames are replaced by them when the
    // transfer completes (frame durability is proven in `packie-store` and by the
    // startup-recovery test in `message_sqlite.rs`).
    let summary = &messages[0].summary;
    assert_eq!(rig.body(summary.message_id), original(&sent));
    assert_eq!(summary.sample_id.as_deref(), Some("SID-A1"));
    assert_eq!(summary.control_id.as_deref(), Some("CTRL-A"));

    let frames = lis.receive().await;
    assert_eq!(
        frames,
        encode_records(&sent).unwrap(),
        "pass-through sends the same records"
    );
    let deliveries = rig
        .deliveries_until("acknowledged", |d| {
            d.first()
                .is_some_and(|d| d.state == DeliveryState::Acknowledged)
        })
        .await;
    assert_eq!(deliveries[0].ack_level, AckLevel::Received);
}

#[tokio::test]
async fn a_frame_is_not_acked_while_the_store_cannot_commit() {
    let rig = rig(json!({})).await;
    let mut analyzer = analyzer(&rig).await;
    analyzer.send(&[ENQ]).await;
    analyzer.expect_control(ACK).await;
    rig.set_unavailable(true);
    let frame = encode_records(&records("SID-A2")).unwrap().remove(0);
    analyzer.send(&frame).await;
    assert_eq!(
        analyzer.control_within(500).await,
        None,
        "no ACK without a commit"
    );
    assert!(rig.messages().is_empty());
}

#[tokio::test]
async fn an_interrupted_transfer_is_aborted() {
    let rig = rig(json!({})).await;
    let mut analyzer = analyzer(&rig).await;
    analyzer.send(&[ENQ]).await;
    analyzer.expect_control(ACK).await;
    let frames = encode_records(&records("SID-A3")).unwrap();
    analyzer.send(&frames[0]).await;
    analyzer.expect_control(ACK).await;
    let receiving = rig.messages();
    assert_eq!(receiving[0].summary.state, MessageState::Receiving);
    drop(analyzer);
    let messages = rig
        .messages_until("aborted", |m| {
            m.first()
                .is_some_and(|m| m.summary.state == MessageState::Aborted)
        })
        .await;
    assert_eq!(messages[0].abort_reason.as_deref(), Some("connection_lost"));
    assert!(rig.deliveries().is_empty());
}

#[tokio::test]
async fn a_receiver_that_refuses_every_frame_fails_the_delivery() {
    let rig = rig(json!({"max_frame_retries": 0, "ack_timeout_ms": 1000})).await;
    let (_listener, mut lis) = lis(&rig.net).await;
    let mut analyzer = analyzer(&rig).await;
    analyzer.transfer(&records("SID-A4")).await;
    // Each attempt: ENQ accepted, the first frame NAKed: a write failure, retried once.
    for _ in 0..2 {
        lis.expect_control(ENQ).await;
        lis.send(&[ACK]).await;
        let item = lis.next_item().await;
        assert_eq!(item.first(), Some(&STX));
        lis.send(&[NAK]).await;
        // The sender ends the transfer after its retries are spent.
        assert_eq!(lis.next_item().await, [EOT]);
    }
    let deliveries = rig
        .deliveries_until("failed", |d| {
            d.first().is_some_and(|d| d.state == DeliveryState::Failed)
        })
        .await;
    assert_eq!(deliveries[0].attempts, 2);
    assert!(
        deliveries[0]
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("rejected too often"))
    );
}
