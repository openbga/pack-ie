//! Alert rules raise, escalate, keep and clear alerts from snapshots.
#![cfg(test)]

use std::path::Path;
use std::time::Duration;

use packie_config::{ConnectStrategy, RuntimeConfig, TapFramer, load_runtime_file};
use packie_core::{
    BridgeSide, ChannelKind, ChannelState, EndpointKind, EndpointState, SessionState, Severity,
    Timestamp,
};
use packie_diagnostics::{
    Alert, AlertBook, AlertKind, AlertState, BridgeDetail, ChannelSnapshot, EndpointRef,
    EndpointSnapshot, FlowSnapshot, NodeConditions, PeerRejections, TapSnapshot, TrafficSnapshot,
};

fn config() -> RuntimeConfig {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/runtime.json"
    ));
    load_runtime_file(path).unwrap()
}

fn side(
    config: &RuntimeConfig,
    index: usize,
    role: BridgeSide,
    state: EndpointState,
) -> EndpointRef {
    let e = &config.endpoints[index];
    EndpointRef {
        endpoint_id: e.id,
        label: e.label.clone(),
        kind: e.kind(),
        role: role.to_string(),
        state,
        consecutive_failures: 0,
        active_peer: None,
        last_activity: None,
    }
}

fn bridge(config: &RuntimeConfig, a: EndpointState) -> ChannelSnapshot {
    let channel = &config.channels[0];
    ChannelSnapshot {
        id: channel.id(),
        label: channel.label().to_owned(),
        kind: ChannelKind::Bridge,
        enabled: true,
        state: ChannelState::Retrying,
        stop_reason: None,
        state_since: Timestamp::now(),
        config_version: 1,
        instance_started_at: None,
        endpoints: channel.endpoint_ids(),
        traffic: TrafficSnapshot::default(),
        retry_count: 0,
        next_retry_at: None,
        last_error: None,
        recent_errors: Vec::new(),
        allowed_actions: Vec::new(),
        bridge: Some(BridgeDetail {
            session: SessionState::WaitingForPeer,
            session_since: Timestamp::now(),
            connect_strategy: ConnectStrategy::Parallel,
            receiver: None,
            endpoint_a: side(config, 0, BridgeSide::A, a),
            endpoint_b: side(config, 1, BridgeSide::B, EndpointState::Listening),
            a_to_b: FlowSnapshot {
                max_segments: 1000,
                max_bytes: 1000,
                ..FlowSnapshot::default()
            },
            b_to_a: FlowSnapshot {
                max_segments: 1000,
                max_bytes: 1000,
                ..FlowSnapshot::default()
            },
            tap: TapSnapshot {
                framer: TapFramer::Astm,
                record_messages: false,
                framing_errors: Some(0),
                tap_dropped: Some(0),
                transcript_viewers: 0,
            },
        }),
        message: None,
    }
}

fn endpoint(
    config: &RuntimeConfig,
    index: usize,
    state: EndpointState,
    since: Timestamp,
) -> EndpointSnapshot {
    let e = &config.endpoints[index];
    EndpointSnapshot {
        id: e.id,
        label: e.label.clone(),
        kind: EndpointKind::Serial,
        enabled: true,
        state,
        state_since: since,
        peer: None,
        held_by: None,
        settings_summary: e.settings_summary(),
        consecutive_failures: 0,
        retry_count: 0,
        next_retry_at: None,
        traffic: TrafficSnapshot::default(),
        last_error: None,
        rejections: None,
        channels: Vec::new(),
    }
}

fn ago(seconds: u64) -> Timestamp {
    Timestamp::now()
        .checked_sub(Duration::from_secs(seconds))
        .unwrap()
}

/// Evaluates the snapshots with no node-wide condition.
fn evaluate(
    book: &mut AlertBook,
    config: &RuntimeConfig,
    channels: &[ChannelSnapshot],
    endpoints: &[EndpointSnapshot],
    now: Timestamp,
) -> Vec<Alert> {
    book.evaluate(config, channels, endpoints, &NodeConditions::default(), now)
}

#[test]
fn endpoint_down_escalates_and_clears() {
    let config = config();
    let channels = vec![bridge(&config, EndpointState::Retrying)];
    let mut book = AlertBook::new();
    let now = Timestamp::now();

    let recent = vec![endpoint(&config, 0, EndpointState::Retrying, now)];
    assert!(evaluate(&mut book, &config, &channels, &recent, now).is_empty());

    let down = vec![endpoint(&config, 0, EndpointState::Retrying, ago(120))];
    let raised = evaluate(&mut book, &config, &channels, &down, now);
    assert_eq!(
        (raised[0].kind, raised[0].severity),
        (AlertKind::EndpointDown, Severity::Warning)
    );
    assert_eq!(raised[0].endpoint_id, Some(config.endpoints[0].id));
    assert!(evaluate(&mut book, &config, &channels, &down, now).is_empty());

    let long = vec![endpoint(&config, 0, EndpointState::Retrying, ago(1000))];
    let escalated = evaluate(&mut book, &config, &channels, &long, now);
    assert_eq!(
        (escalated[0].severity, escalated[0].id),
        (Severity::Critical, raised[0].id)
    );

    let up = vec![bridge(&config, EndpointState::Connected)];
    let cleared = evaluate(&mut book, &config, &up, &long, now);
    assert_eq!(cleared[0].state, AlertState::Cleared);
    assert!(book.active().is_empty());
    assert_eq!(book.history(true, None).len(), 1);
    assert!(book.history(false, None).is_empty());
}

#[test]
fn paused_channels_raise_nothing() {
    let config = config();
    let mut channel = bridge(&config, EndpointState::Disconnected);
    channel.state = ChannelState::Stopped;
    let endpoints = vec![endpoint(&config, 0, EndpointState::Disconnected, ago(3600))];
    let found = evaluate(
        &mut AlertBook::new(),
        &config,
        &[channel],
        &endpoints,
        Timestamp::now(),
    );
    assert!(found.is_empty());
}

#[test]
fn buffer_pressure_waits_for_the_hold_time_unless_overflowing() {
    let config = config();
    let mut channel = bridge(&config, EndpointState::Connected);
    if let Some(detail) = &mut channel.bridge {
        detail.a_to_b.buffered_bytes = 900;
    }
    let mut book = AlertBook::new();
    assert!(evaluate(&mut book, &config, &[channel.clone()], &[], ago(20)).is_empty());
    let raised = evaluate(
        &mut book,
        &config,
        &[channel.clone()],
        &[],
        Timestamp::now(),
    );
    assert_eq!(
        (raised[0].kind, raised[0].severity),
        (AlertKind::BridgeBufferPressure, Severity::Warning)
    );
    if let Some(detail) = &mut channel.bridge {
        detail.b_to_a.overflowing = true;
    }
    let escalated = evaluate(&mut book, &config, &[channel], &[], Timestamp::now());
    assert_eq!(escalated[0].severity, Severity::Critical);
}

#[test]
fn node_conditions_and_rejections() {
    let config = config();
    let mib = 1024 * 1024;
    let mut listener = endpoint(&config, 1, EndpointState::Listening, ago(10));
    listener.rejections = Some(PeerRejections {
        count: 2,
        last_at: Some(ago(5)),
        last_peer: Some("10.0.0.9:51000".into()),
        last_reason: Some("not_allowed".into()),
    });
    let node = NodeConditions {
        free_disk_bytes: Some(100 * mib),
        store_unavailable: true,
        config_rejected: true,
        apply_failures: vec![(config.channels[0].id(), "port in use".into())],
        audit_unavailable: true,
        backup_stale: Some(packie_diagnostics::StaleBackup {
            newest_success_at: None,
            limit_hours: 26,
        }),
        notifications_failing: vec![packie_diagnostics::FailingTarget {
            target: "webhook".into(),
            since: Timestamp::now(),
            error: "connection refused".into(),
        }],
    };
    let mut found: Vec<(AlertKind, Severity)> = AlertBook::new()
        .evaluate(&config, &[], &[listener], &node, Timestamp::now())
        .into_iter()
        .map(|a| (a.kind, a.severity))
        .collect();
    found.sort_by_key(|(k, _)| format!("{k:?}"));
    assert_eq!(
        found,
        [
            (AlertKind::AuditUnavailable, Severity::Critical),
            (AlertKind::BackupStale, Severity::Warning),
            (AlertKind::ConfigApplyFailed, Severity::Warning),
            (AlertKind::ConfigRejected, Severity::Warning),
            (AlertKind::DiskLow, Severity::Critical),
            (AlertKind::MessageStoreUnavailable, Severity::Critical),
            (AlertKind::NotificationsFailing, Severity::Warning),
            (AlertKind::PeerRejected, Severity::Warning),
        ]
    );
    assert!(packie_diagnostics::free_space(Path::new(".")).is_some());
}

#[test]
fn invalid_messages_count_within_the_window() {
    use packie_core::ProtocolKind;
    use packie_diagnostics::{MessageCounters, MessageDetail, SourceSession, Unresolved};

    let config = config();
    let limit = u64::from(config.alerts.invalid_messages_count);
    let window = u64::from(config.alerts.invalid_messages_window_seconds);
    let channel = |invalid: u64, aborted: u64, started: Timestamp| ChannelSnapshot {
        kind: ChannelKind::Message,
        state: ChannelState::Running,
        instance_started_at: Some(started),
        bridge: None,
        message: Some(MessageDetail {
            source: side(&config, 1, BridgeSide::A, EndpointState::Listening),
            protocol: ProtocolKind::Hl7,
            session: SourceSession::Idle,
            counters: MessageCounters {
                invalid,
                aborted,
                ..MessageCounters::default()
            },
            last_received_at: None,
            destinations: Vec::new(),
            unresolved: Unresolved::default(),
        }),
        ..bridge(&config, EndpointState::Connected)
    };
    let started = ago(3 * window);
    let mut book = AlertBook::new();
    let at = |book: &mut AlertBook, snapshot: ChannelSnapshot, seconds_ago: u64| {
        evaluate(book, &config, &[snapshot], &[], ago(seconds_ago))
    };

    assert!(at(&mut book, channel(0, 0, started), 2 * window).is_empty());
    let below = at(&mut book, channel(limit - 2, 1, started), 2 * window - 10);
    assert!(below.is_empty(), "{below:?}");
    let raised = at(&mut book, channel(limit - 1, 1, started), 2 * window - 20);
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].kind, AlertKind::InvalidMessages);
    assert_eq!(raised[0].details["count"], limit);

    // A window later nothing new arrived: the burst has aged out.
    let cleared = at(&mut book, channel(limit - 1, 1, started), window - 30);
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0].state, AlertState::Cleared);

    // A restarted instance counts from zero again.
    let restarted = ago(window / 2);
    let again = at(&mut book, channel(limit, 0, restarted), 10);
    assert_eq!(again.len(), 1, "{again:?}");
    assert_eq!(again[0].state, AlertState::Active);
}

#[test]
fn transform_failed_follows_the_unresolved_mapping_failures() {
    use packie_core::{ConnectorId, ProtocolKind};
    use packie_diagnostics::{
        DeliveryQueueSnapshot, DestinationCounters, DestinationSnapshot, MessageCounters,
        MessageDetail, SourceSession, Unresolved,
    };

    let config = config();
    let channel = |transform_errors: u64| {
        let unresolved = Unresolved {
            failed: transform_errors,
            transform_errors,
            ..Unresolved::default()
        };
        ChannelSnapshot {
            kind: ChannelKind::Message,
            state: ChannelState::Running,
            bridge: None,
            message: Some(MessageDetail {
                source: side(&config, 1, BridgeSide::A, EndpointState::Listening),
                protocol: ProtocolKind::Astm,
                session: SourceSession::Idle,
                counters: MessageCounters::default(),
                last_received_at: None,
                destinations: vec![DestinationSnapshot {
                    destination_id: ConnectorId::new("lis").unwrap(),
                    label: "LIS".into(),
                    enabled: true,
                    endpoint: side(&config, 0, BridgeSide::B, EndpointState::Connected),
                    protocol: ProtocolKind::Hl7,
                    queue: DeliveryQueueSnapshot::default(),
                    unresolved,
                    counters: DestinationCounters::default(),
                    last_attempt_at: None,
                    last_ack_at: None,
                    last_error: None,
                }],
                unresolved,
            }),
            ..bridge(&config, EndpointState::Connected)
        }
    };
    let mut book = AlertBook::new();
    let mut at_now = |snapshot| evaluate(&mut book, &config, &[snapshot], &[], Timestamp::now());

    let raised = at_now(channel(2));
    let failed = raised
        .iter()
        .find(|a| a.kind == AlertKind::TransformFailed)
        .unwrap();
    assert_eq!(
        failed.destination_id,
        Some(ConnectorId::new("lis").unwrap())
    );
    assert_eq!(failed.details["count"], 2);
    // The same deliveries also need a decision.
    assert!(
        raised
            .iter()
            .any(|a| a.kind == AlertKind::DeliveryNeedsAttention)
    );

    let cleared = at_now(channel(0));
    assert!(
        cleared
            .iter()
            .any(|a| a.kind == AlertKind::TransformFailed && a.state == AlertState::Cleared)
    );
}

#[test]
fn peer_rejected_names_one_peer_and_follows_the_latest_refusal() {
    let config = config();
    let listener = |peer: &str, reason: &str| {
        let mut e = endpoint(&config, 1, EndpointState::Listening, ago(10));
        e.rejections = Some(PeerRejections {
            count: 1,
            last_at: Some(ago(1)),
            last_peer: Some(peer.into()),
            last_reason: Some(reason.into()),
        });
        e
    };
    let mut book = AlertBook::new();
    let first = evaluate(
        &mut book,
        &config,
        &[],
        &[listener("10.0.0.9:51000", "not_allowed")],
        Timestamp::now(),
    );
    assert!(first[0].summary.contains("10.0.0.9 ") && !first[0].summary.contains("51000"));
    assert!(
        first[0]
            .next_action
            .starts_with("If 10.0.0.9 should connect")
    );

    // A later refusal of another peer updates summary and next action together.
    evaluate(
        &mut book,
        &config,
        &[],
        &[listener("10.0.0.7:40000", "session_active")],
        Timestamp::now(),
    );
    let alert = &book.active()[0];
    assert!(alert.summary.contains("10.0.0.7"), "{}", alert.summary);
    assert!(
        alert.next_action.contains("10.0.0.7"),
        "{}",
        alert.next_action
    );
    assert!(
        !alert.next_action.contains("10.0.0.9"),
        "{}",
        alert.next_action
    );
}
