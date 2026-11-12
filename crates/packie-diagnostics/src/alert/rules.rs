//! The rules: what each catalog alert looks at, its summary and its next action
//! (`docs/spec/diagnostics.md` §6). A rule yields [`Finding`]s; [`AlertBook`] turns them
//! into raised, refreshed and cleared alerts.

use std::collections::HashSet;
use std::time::Duration;

use packie_config::{ChannelConfig, RuntimeConfig};
use packie_core::{
    ChannelId, ChannelState, ConnectorId, EndpointId, EndpointState, SessionState, Severity,
    Timestamp,
};
use serde_json::json;

use super::actions::{
    AUDIT_UNAVAILABLE, BACKLOG, BACKUP_STALE, BUFFER_PRESSURE, CONFIG_APPLY_FAILED,
    CONFIG_REJECTED, DISK_LOW, ENDPOINT_DOWN, ENDPOINT_ERROR, INVALID_MESSAGES, NEEDS_ATTENTION,
    NO_PEER, NOTIFICATIONS_FAILING, STORE_UNAVAILABLE, TRANSFORM_FAILED,
};
use super::{AlertBook, AlertKind, NodeConditions};
use crate::snapshot::{
    BridgeDetail, ChannelSnapshot, DestinationSnapshot, EndpointRef, EndpointSnapshot,
    FlowSnapshot, MessageDetail, SourceSession,
};

/// Identifies one alert: its code and what it concerns.
pub(super) type AlertKey = (
    AlertKind,
    Option<ChannelId>,
    Option<EndpointId>,
    Option<ConnectorId>,
);

/// A condition found by one evaluation.
pub(super) struct Finding {
    pub(super) key: AlertKey,
    pub(super) severity: Severity,
    pub(super) summary: String,
    pub(super) next_action: String,
    pub(super) details: serde_json::Value,
}

impl Finding {
    fn new(
        key: AlertKey,
        severity: Severity,
        summary: impl Into<String>,
        next_action: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            key,
            severity,
            summary: summary.into(),
            next_action: next_action.into(),
            details,
        }
    }
}

/// The key of an alert about the node as a whole.
const fn node_key(kind: AlertKind) -> AlertKey {
    (kind, None, None, None)
}

/// The key of an alert about one channel.
const fn channel_key(kind: AlertKind, channel: ChannelId) -> AlertKey {
    (kind, Some(channel), None, None)
}

/// One node alert while any notification target fails, naming each with its error.
fn notifications_failing(failing: &[super::FailingTarget], findings: &mut Vec<Finding>) {
    let Some(first) = failing.iter().map(|f| f.since).min() else {
        return;
    };
    let causes = failing
        .iter()
        .map(|f| format!("{}: {}", f.target, f.error.trim_end_matches('.')))
        .collect::<Vec<_>>()
        .join("; ");
    findings.push(Finding::new(
        node_key(AlertKind::NotificationsFailing),
        Severity::Warning,
        format!("Alert notifications cannot be delivered ({causes})"),
        NOTIFICATIONS_FAILING,
        json!({
            "targets": failing.iter().map(|f| f.target.as_str()).collect::<Vec<_>>(),
            "failing_since": first,
        }),
    ));
}

const fn severity(critical: bool) -> Severity {
    if critical {
        Severity::Critical
    } else {
        Severity::Warning
    }
}

fn secs(seconds: u32) -> Duration {
    Duration::from_secs(seconds.into())
}

/// A rejection keeps `peer_rejected` active this long.
const REJECTION_WINDOW: Duration = Duration::from_mins(10);

impl AlertBook {
    /// Runs every rule over the snapshots and the node conditions, in a fixed order, and
    /// forgets the state kept for channels that no longer exist.
    pub(super) fn findings(
        &mut self,
        config: &RuntimeConfig,
        channels: &[ChannelSnapshot],
        endpoints: &[EndpointSnapshot],
        node: &NodeConditions,
        now: Timestamp,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        let rules = Rules {
            config,
            endpoints,
            now,
        };
        for channel in channels
            .iter()
            .filter(|c| c.enabled && c.state != ChannelState::Stopped)
        {
            rules.channel(channel, &mut findings);
            if let Some(bridge) = &channel.bridge {
                self.buffer_pressure(&rules, channel, bridge, &mut findings);
            }
            if let Some(message) = &channel.message {
                self.invalid_messages(&rules, channel, message, &mut findings);
            }
        }
        rules.endpoints(&mut findings);
        rules.node(node, &mut findings);
        let live: HashSet<ChannelId> = channels.iter().map(|c| c.id).collect();
        self.pressure_since.retain(|id, _| live.contains(id));
        self.invalid_history.retain(|id, _| live.contains(id));
        findings
    }

    fn invalid_messages(
        &mut self,
        rules: &Rules<'_>,
        channel: &ChannelSnapshot,
        message: &MessageDetail,
        findings: &mut Vec<Finding>,
    ) {
        let alerts = &rules.config.alerts;
        let limit = u64::from(alerts.invalid_messages_count);
        let window = secs(alerts.invalid_messages_window_seconds);
        let total = message
            .counters
            .invalid
            .saturating_add(message.counters.aborted);
        let history = self.invalid_history.entry(channel.id).or_default();
        // Counters restart with the instance.
        let restarted = history.instance != channel.instance_started_at
            || history
                .samples
                .back()
                .is_some_and(|&(_, last)| total < last);
        if restarted {
            history.instance = channel.instance_started_at;
            history.samples.clear();
        }
        let samples = &mut history.samples;
        if samples.is_empty() {
            samples.push_back((channel.instance_started_at.unwrap_or(rules.now), 0));
        }
        if samples.back().is_some_and(|&(_, last)| last != total) {
            samples.push_back((rules.now, total));
        }
        if let Some(start) = rules.now.checked_sub(window) {
            while samples.get(1).is_some_and(|&(at, _)| at <= start) {
                samples.pop_front();
            }
        }
        let count = total.saturating_sub(samples.front().map_or(0, |&(_, value)| value));
        if limit == 0 || count < limit {
            return;
        }
        findings.push(Finding::new(
            channel_key(AlertKind::InvalidMessages, channel.id),
            Severity::Warning,
            format!(
                "{}: {count} invalid or aborted messages in the last {}s",
                channel.label,
                window.as_secs()
            ),
            INVALID_MESSAGES,
            json!({
                "count": count,
                "window_seconds": window.as_secs(),
                "invalid": message.counters.invalid,
                "aborted": message.counters.aborted,
            }),
        ));
    }

    fn buffer_pressure(
        &mut self,
        rules: &Rules<'_>,
        channel: &ChannelSnapshot,
        bridge: &BridgeDetail,
        findings: &mut Vec<Finding>,
    ) {
        let percent = u128::from(rules.config.alerts.buffer_high_percent);
        let high = |f: &FlowSnapshot| {
            u128::from(f.buffered_bytes) * 100 >= u128::from(f.max_bytes.max(1)) * percent
                || u128::from(f.buffered_segments) * 100
                    >= u128::from(f.max_segments.max(1)) * percent
        };
        let overflowing = bridge.a_to_b.overflowing || bridge.b_to_a.overflowing;
        if !(overflowing || high(&bridge.a_to_b) || high(&bridge.b_to_a)) {
            self.pressure_since.remove(&channel.id);
            return;
        }
        let since = *self.pressure_since.entry(channel.id).or_insert(rules.now);
        let held = secs(rules.config.alerts.buffer_high_seconds);
        if !overflowing && rules.now.saturating_duration_since(since) < held {
            return;
        }
        findings.push(Finding::new(
            channel_key(AlertKind::BridgeBufferPressure, channel.id),
            severity(overflowing),
            format!(
                "{} buffer is at or above {percent}% of its limit",
                channel.label
            ),
            BUFFER_PRESSURE,
            json!({
                "a_to_b_bytes": bridge.a_to_b.buffered_bytes,
                "b_to_a_bytes": bridge.b_to_a.buffered_bytes,
                "overflowing": overflowing,
            }),
        ));
    }
}

/// The inputs every stateless rule reads.
struct Rules<'a> {
    config: &'a RuntimeConfig,
    endpoints: &'a [EndpointSnapshot],
    now: Timestamp,
}

impl Rules<'_> {
    fn channel(&self, channel: &ChannelSnapshot, findings: &mut Vec<Finding>) {
        let streak = match self.config.channel(channel.id) {
            Some(ChannelConfig::Bridge(b)) => b.retry.max_consecutive_failures_before_alert,
            Some(ChannelConfig::Message(m)) => m.retry.max_consecutive_failures_before_alert,
            None => u32::MAX,
        };
        let mut refs: Vec<&EndpointRef> = Vec::new();
        if let Some(bridge) = &channel.bridge {
            refs.extend([&bridge.endpoint_a, &bridge.endpoint_b]);
            self.no_peer(
                channel,
                bridge.session == SessionState::WaitingForPeer,
                bridge.session_since,
                findings,
            );
        }
        if let Some(message) = &channel.message {
            refs.push(&message.source);
            refs.extend(message.destinations.iter().map(|d| &d.endpoint));
            let waiting = message.session == SourceSession::WaitingForPeer;
            self.no_peer(channel, waiting, channel.state_since, findings);
            for destination in &message.destinations {
                self.destination(channel, destination, findings);
            }
        }
        for endpoint in refs {
            self.endpoint_down(channel, endpoint, streak, findings);
        }
    }

    fn endpoint_down(
        &self,
        channel: &ChannelSnapshot,
        e: &EndpointRef,
        streak: u32,
        findings: &mut Vec<Finding>,
    ) {
        let down = matches!(
            e.state,
            EndpointState::Retrying | EndpointState::Error | EndpointState::Disconnected
        );
        if !down {
            return;
        }
        let since = self
            .endpoints
            .iter()
            .find(|s| s.id == e.endpoint_id)
            .map_or(self.now, |s| s.state_since);
        let age = self.now.saturating_duration_since(since);
        let alerts = &self.config.alerts;
        if age < secs(alerts.endpoint_down_seconds) && e.consecutive_failures < streak {
            return;
        }
        let critical =
            e.state == EndpointState::Error || age >= secs(alerts.endpoint_down_critical_seconds);
        let next_action = if e.state == EndpointState::Error {
            ENDPOINT_ERROR
        } else {
            ENDPOINT_DOWN
        };
        findings.push(Finding::new(
            (
                AlertKind::EndpointDown,
                Some(channel.id),
                Some(e.endpoint_id),
                None,
            ),
            severity(critical),
            format!("{} is {} ({} needs it)", e.label, e.state, channel.label),
            next_action,
            json!({
                "state": e.state, "seconds": age.as_secs(), "failures": e.consecutive_failures,
            }),
        ));
    }

    fn no_peer(
        &self,
        channel: &ChannelSnapshot,
        waiting: bool,
        since: Timestamp,
        findings: &mut Vec<Finding>,
    ) {
        let threshold = self.config.alerts.no_peer_seconds;
        let age = self.now.saturating_duration_since(since);
        if threshold == 0 || !waiting || age < secs(threshold) {
            return;
        }
        findings.push(Finding::new(
            channel_key(AlertKind::NoPeerConnected, channel.id),
            Severity::Warning,
            format!("{} has waited {}s for a peer", channel.label, age.as_secs()),
            NO_PEER,
            json!({ "seconds": age.as_secs() }),
        ));
    }

    fn destination(
        &self,
        channel: &ChannelSnapshot,
        d: &DestinationSnapshot,
        findings: &mut Vec<Finding>,
    ) {
        let key = |kind| (kind, Some(channel.id), None, Some(d.destination_id));
        let alerts = &self.config.alerts;
        let count = d.unresolved.total();
        if count > 0 {
            let critical = count >= u64::from(alerts.delivery_attention_critical_count);
            findings.push(Finding::new(
                key(AlertKind::DeliveryNeedsAttention),
                severity(critical),
                format!(
                    "{} → {}: {count} deliveries need a decision",
                    channel.label, d.label
                ),
                NEEDS_ATTENTION,
                json!({
                    "count": count, "rejected": d.unresolved.rejected, "failed": d.unresolved.failed,
                    "timed_out": d.unresolved.timed_out, "outcome_unknown": d.unresolved.outcome_unknown,
                }),
            ));
        }
        let failed_mappings = d.unresolved.transform_errors;
        if failed_mappings > 0 {
            findings.push(Finding::new(
                key(AlertKind::TransformFailed),
                Severity::Warning,
                format!(
                    "{} → {}: {failed_mappings} deliveries failed in the mapping",
                    channel.label, d.label
                ),
                TRANSFORM_FAILED,
                json!({ "count": failed_mappings }),
            ));
        }
        let queued = d.queue.queued;
        let age = d
            .queue
            .oldest_queued_at
            .map_or(0, |t| self.now.saturating_duration_since(t).as_secs());
        let (count_limit, age_limit) = (
            u64::from(alerts.delivery_backlog_count),
            u64::from(alerts.delivery_backlog_age_seconds),
        );
        let over = |value: u64, limit: u64, factor: u64| {
            limit > 0 && value >= limit.saturating_mul(factor)
        };
        if over(queued, count_limit, 1) || over(age, age_limit, 1) {
            let critical = over(queued, count_limit, 3) || over(age, age_limit, 3);
            findings.push(Finding::new(
                key(AlertKind::DeliveryBacklog),
                severity(critical),
                format!(
                    "{} → {}: {queued} deliveries queued, oldest {age}s",
                    channel.label, d.label
                ),
                BACKLOG,
                json!({ "queued": queued, "oldest_seconds": age }),
            ));
        }
    }

    fn endpoints(&self, findings: &mut Vec<Finding>) {
        for endpoint in self.endpoints {
            let Some(rejections) = &endpoint.rejections else {
                continue;
            };
            let recent = rejections
                .last_at
                .is_some_and(|at| self.now.saturating_duration_since(at) < REJECTION_WINDOW);
            if !recent {
                continue;
            }
            // Allowlists hold addresses; the sender's port changes on every connection.
            let peer = rejections
                .last_peer
                .as_deref()
                .map_or_else(String::new, |p| {
                    p.parse::<std::net::SocketAddr>()
                        .map_or_else(|_| p.to_owned(), |addr| addr.ip().to_string())
                });
            let reason = rejections.last_reason.as_deref();
            let next_action = if reason == Some("session_active") {
                format!(
                    "Only one peer may be connected at a time. Find out why {peer} connects while another peer is connected."
                )
            } else {
                format!(
                    "If {peer} should connect, add it to the endpoint's allowed peers and apply. If not, find out which device it is."
                )
            };
            findings.push(Finding::new(
                (AlertKind::PeerRejected, None, Some(endpoint.id), None),
                Severity::Warning,
                format!(
                    "{} refused {peer} ({})",
                    endpoint.label,
                    reason.unwrap_or("")
                ),
                next_action,
                json!({ "count": rejections.count, "peer": peer, "reason": rejections.last_reason }),
            ));
        }
    }

    /// The node-wide conditions: store, configuration, disk, audit log and backups.
    fn node(&self, node: &NodeConditions, findings: &mut Vec<Finding>) {
        if node.store_unavailable {
            findings.push(Finding::new(
                node_key(AlertKind::MessageStoreUnavailable),
                Severity::Critical,
                "The message store cannot be used",
                STORE_UNAVAILABLE,
                json!({}),
            ));
        }
        if node.config_rejected {
            findings.push(Finding::new(
                node_key(AlertKind::ConfigRejected),
                Severity::Warning,
                "The last configuration failed validation",
                CONFIG_REJECTED,
                json!({}),
            ));
        }
        for (channel, reason) in &node.apply_failures {
            findings.push(Finding::new(
                channel_key(AlertKind::ConfigApplyFailed, *channel),
                Severity::Warning,
                format!("Applying the new definition of {channel} failed: {reason}"),
                CONFIG_APPLY_FAILED,
                json!({}),
            ));
        }
        if let Some(free) = node.free_disk_bytes {
            let mib = 1024 * 1024;
            let warning = u64::from(self.config.alerts.disk_low_megabytes) * mib;
            if free < warning {
                let critical = free < u64::from(self.config.alerts.disk_critical_megabytes) * mib;
                findings.push(Finding::new(
                    node_key(AlertKind::DiskLow),
                    severity(critical),
                    format!(
                        "Free space on the state volume is {}",
                        packie_core::ByteSize(free)
                    ),
                    DISK_LOW,
                    json!({ "free_bytes": free }),
                ));
            }
        }
        if node.audit_unavailable {
            findings.push(Finding::new(
                node_key(AlertKind::AuditUnavailable),
                Severity::Critical,
                "The audit log cannot be written",
                AUDIT_UNAVAILABLE,
                json!({}),
            ));
        }
        if let Some(stale) = &node.backup_stale {
            let summary = match stale.newest_success_at {
                Some(at) => format!(
                    "The newest successful backup is from {at}, more than {} hours ago",
                    stale.limit_hours
                ),
                None => format!(
                    "No backup has succeeded in the {} hours since the engine started",
                    stale.limit_hours
                ),
            };
            findings.push(Finding::new(
                node_key(AlertKind::BackupStale),
                Severity::Warning,
                summary,
                BACKUP_STALE,
                json!({
                    "newest_success_at": stale.newest_success_at,
                    "limit_hours": stale.limit_hours,
                }),
            ));
        }
        notifications_failing(&node.notifications_failing, findings);
    }
}
