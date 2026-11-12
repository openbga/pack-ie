//! AU400/AU640 message channel source (`docs/spec/message-channels.md` §2.2): class B
//! blocks are acknowledged only after their commit; a completed sample is routed.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use packie_au::{AuConfig, FormatConfig, FrameEnd, FramingConfig, TransmissionClass, encode_frame};
use packie_core::{DeliveryState, MessageState};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const ANALYZER: &str = "127.0.0.1:9621";

fn au_settings() -> serde_json::Value {
    json!({
        "profile": "au640",
        "framing": {"start": [2], "end": [3], "bcc": true, "use_etb": true, "max_wire_len": 256, "class": "b"},
        "format": {
            "unit_no": true, "rack_digits": 4, "sample_id_digits": 8, "sex": false, "age": false,
            "month": false, "patient_info_digits": [0, 0, 0, 0, 0, 0], "result_digits": 6,
        },
        "frame_timeout_ms": 5000,
    })
}

fn config() -> AuConfig {
    AuConfig {
        framing: FramingConfig {
            start: vec![2],
            end: vec![3],
            bcc: true,
            use_etb: true,
            max_wire_len: 256,
            class: TransmissionClass::B,
        },
        format: FormatConfig {
            unit_no: true,
            rack_digits: 4,
            sample_id_digits: 8,
            sex: false,
            age: false,
            month: false,
            patient_info_digits: [0; 6],
            result_digits: 6,
            test_code_digits: 2,
        },
    }
}

/// One final result block (synthetic, as in `packie-au`'s driver tests).
fn final_block(sample_id: &str) -> Vec<u8> {
    let text = format!("D 01000101U0001{sample_id}    E01012.34  ").into_bytes();
    encode_frame(&config(), &text, FrameEnd::Text).unwrap()
}

async fn rig() -> MessageRig {
    let channel = json!({
        "kind": "message",
        "channel_id": "au-results",
        "label": "AU results",
        "source": {"endpoint": "au", "protocol": "au", "settings": au_settings()},
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-out",
            "protocol": "au", "settings": au_settings(),
        }],
    });
    MessageRig::start(document(
        vec![listener("au", 9621), client("lis-out", 9721)],
        vec![channel],
    ))
    .await
}

async fn response_within(stream: &mut DuplexStream, ms: u64) -> Option<u8> {
    let mut byte = [0_u8; 1];
    match tokio::time::timeout(Duration::from_millis(ms), stream.read(&mut byte)).await {
        Ok(Ok(1)) => Some(byte[0]),
        _ => None,
    }
}

#[tokio::test]
async fn a_block_is_acknowledged_after_its_commit_and_the_sample_is_routed() {
    let rig = rig().await;
    let mut instrument = dial(&rig.net, ANALYZER).await;
    let sent = Instant::now();
    instrument
        .write_all(&final_block("SAMPLE01"))
        .await
        .unwrap();
    assert_eq!(response_within(&mut instrument, 3000).await, Some(0x06));
    assert!(
        sent.elapsed() >= Duration::from_millis(450),
        "class B waits the turnaround"
    );

    let messages = rig
        .messages_until("the sample is routed", |m| {
            m.iter().any(|m| m.summary.state == MessageState::Routed)
        })
        .await;
    let message = &messages[0];
    assert_eq!(message.summary.sample_id.as_deref(), Some("SAMPLE01"));
    // AU destinations are not built: the delivery waits in the queue.
    assert_eq!(message.deliveries[0].state, DeliveryState::Queued);
}

#[tokio::test]
async fn no_acknowledgement_without_a_commit() {
    let rig = rig().await;
    let mut instrument = dial(&rig.net, ANALYZER).await;
    rig.set_unavailable(true);
    instrument
        .write_all(&final_block("SAMPLE02"))
        .await
        .unwrap();
    assert_eq!(response_within(&mut instrument, 900).await, None);
    assert!(rig.messages().is_empty());
}
