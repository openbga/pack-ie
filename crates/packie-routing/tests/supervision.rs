//! Supervision: a panicking channel stays isolated, and a channel without exclusive
//! resources is replaced make-before-break.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use packie_config::EndpointConfig;
use packie_core::ChannelId;
use packie_core::{ChannelState, SessionState};
use packie_routing::memory::MemoryNetwork;
use packie_transport::{BoxFuture, Dialer, Link, LinkError, Opener, Transports};
use serde_json::json;

/// The memory network, except that dialing an endpoint labelled `boom client` panics.
struct Exploding(MemoryNetwork);

struct Boom;

impl Dialer for Boom {
    fn dial(&self) -> BoxFuture<'_, Result<Link, LinkError>> {
        Box::pin(async { panic!("injected fault") })
    }
}

impl Transports for Exploding {
    fn opener(&self, endpoint: &EndpointConfig, holder: ChannelId) -> Result<Opener, LinkError> {
        if endpoint.label == "boom client" {
            return Ok(Opener::Dial(Box::new(Boom)));
        }
        self.0.opener(endpoint, holder)
    }
}

#[tokio::test]
async fn a_panicking_channel_is_reported_and_others_keep_running() {
    let net = MemoryNetwork::new();
    let mut h = Harness::new(Arc::new(Exploding(net.clone())));
    let mut sys = net.listen("127.0.0.1:9401", 4).unwrap();
    h.apply(document(
        vec![
            listener("ok-inst", 9400),
            client("ok-sys", 9401),
            listener("boom-inst", 9402),
            client("boom", 9403),
        ],
        vec![
            bridge("ok", "ok-inst", "ok-sys", json!({})),
            bridge("boom", "boom-inst", "boom", json!({})),
        ],
    ))
    .await;
    let boom = channel_id("boom");
    let status = h
        .until(boom, "the fault is reported", |s| {
            s.last_error_summary.is_some()
        })
        .await;
    assert_eq!(status.state, ChannelState::Retrying);
    let summary = status.last_error_summary.unwrap();
    assert!(summary.contains("failed unexpectedly"), "{summary}");
    assert!(summary.contains("Restarting the channel in"), "{summary}");

    let ok = channel_id("ok");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut inst = dial(&net, "127.0.0.1:9400").await;
    let mut sys = sys.accept().await.unwrap().stream;
    h.until(ok, "the healthy channel runs", |s| {
        s.state == ChannelState::Running && s.session_state == SessionState::Bridged
    })
    .await;
    send(&mut inst, b"unaffected\n").await;
    expect_bytes(&mut sys, b"unaffected\n").await;
}

#[tokio::test]
async fn client_only_channel_is_replaced_make_before_break() {
    let net = MemoryNetwork::new();
    let mut h = Harness::new(Arc::new(net.clone()));
    let mut left = net.listen("127.0.0.1:9501", 8).unwrap();
    let mut right = net.listen("127.0.0.1:9502", 8).unwrap();
    let doc = |max_bytes: u64| {
        document(
            vec![client("mbb-left", 9501), client("mbb-right", 9502)],
            vec![bridge(
                "mbb",
                "mbb-left",
                "mbb-right",
                json!({"buffer": {"max_bytes": max_bytes}}),
            )],
        )
    };
    let id = channel_id("mbb");
    h.apply(doc(1_048_576)).await;
    let mut old_left = left.accept().await.unwrap().stream;
    let _old_right = right.accept().await.unwrap().stream;
    h.until(id, "running", |s| s.state == ChannelState::Running)
        .await;

    h.apply(doc(2_097_152)).await;
    // The replacement connected while the old instance still ran, then took over.
    let mut new_left = left.accept().await.unwrap().stream;
    let mut new_right = right.accept().await.unwrap().stream;
    expect_closed(&mut old_left).await;
    let status = h
        .until(id, "replacement running", |s| {
            s.state == ChannelState::Running
        })
        .await;
    assert_eq!(status.config_version, 2);
    assert_eq!(status.buffer_a_to_b.max_bytes, 2_097_152);
    send(&mut new_left, b"through the replacement\n").await;
    expect_bytes(&mut new_right, b"through the replacement\n").await;
}
