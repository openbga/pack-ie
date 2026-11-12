//! Alerts (`docs/spec/diagnostics.md` §6): operator-relevant conditions derived from
//! snapshots by threshold rules.
//!
//! [`AlertBook::evaluate`] runs after each snapshot. A condition that holds raises one
//! alert, kept with `last_seen_at` updated (and its severity raised or lowered) while it
//! holds; when it stops holding the alert is cleared and kept in the history for 30 days.
//! Conditions caused by an operator pause or a disabled channel raise nothing.
//!
//! Evaluated here: `endpoint_down`, `no_peer_connected`, `peer_rejected`,
//! `bridge_buffer_pressure`, `message_store_unavailable`, `disk_low`,
//! `delivery_needs_attention`, `delivery_backlog`, `config_rejected`,
//! `config_apply_failed`, `audit_unavailable`, `backup_stale`, `notifications_failing`,
//! `invalid_messages` (from the
//! channel's message counters, which count since the instance started: the book keeps
//! their recent values to count within the window) and `transform_failed` (from the
//! store's count of unresolved deliveries whose mapping failed).
//!
//! The rules and their texts live in `rules` and `actions`; this module keeps the alert
//! types and the book that raises, refreshes and clears alerts from their findings.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use packie_config::RuntimeConfig;
use packie_core::{AlertId, ChannelId, ConnectorId, EndpointId, Severity, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::snapshot::{ChannelSnapshot, EndpointSnapshot};
use rules::{AlertKey, Finding};

mod actions;
mod rules;

/// Alert codes of the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// An endpoint a running channel needs is down.
    EndpointDown,
    /// A channel waits for a peer longer than `no_peer_seconds`.
    NoPeerConnected,
    /// A listener rejected a peer in the last 10 minutes.
    PeerRejected,
    /// A bridge buffer is nearly full or overflowing.
    BridgeBufferPressure,
    /// The message store cannot be opened or a commit failed.
    MessageStoreUnavailable,
    /// Free space on the state volume is low.
    DiskLow,
    /// Deliveries wait for an operator decision.
    DeliveryNeedsAttention,
    /// A delivery failed in its mapping.
    TransformFailed,
    /// Deliveries pile up or wait too long.
    DeliveryBacklog,
    /// A sender produces invalid messages.
    InvalidMessages,
    /// The last configuration failed validation.
    ConfigRejected,
    /// One channel's change failed during an apply.
    ConfigApplyFailed,
    /// The audit log cannot be written; audited commands are refused.
    AuditUnavailable,
    /// The newest successful backup is older than `backup.stale_after_hours`.
    BackupStale,
    /// An alert notification target (email, webhook) could not be reached.
    NotificationsFailing,
}

/// Whether an alert still holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    /// The condition holds.
    Active,
    /// The condition cleared.
    Cleared,
}

/// An operator-relevant condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Alert {
    /// Stable while the condition holds.
    #[serde(rename = "alert_id")]
    pub id: AlertId,
    /// Catalog code.
    #[serde(rename = "code")]
    pub kind: AlertKind,
    /// `warning` or `critical`.
    pub severity: Severity,
    /// Active or cleared.
    pub state: AlertState,
    /// The channel concerned.
    pub channel_id: Option<ChannelId>,
    /// The endpoint concerned.
    pub endpoint_id: Option<EndpointId>,
    /// The destination concerned.
    pub destination_id: Option<ConnectorId>,
    /// What is wrong, one sentence, no payload.
    pub summary: String,
    /// What the operator can do, or what the engine does next.
    pub next_action: String,
    /// When the condition was first seen.
    #[serde(rename = "first_seen_at")]
    pub first_seen: Timestamp,
    /// When it was last seen.
    #[serde(rename = "last_seen_at")]
    pub last_seen: Timestamp,
    /// When it cleared.
    pub cleared_at: Option<Timestamp>,
    /// Times the condition was re-raised after clearing.
    pub occurrences: u32,
    /// Numbers and codes only (counts, ages, peer address, state).
    #[ts(type = "Record<string, unknown>")]
    pub details: serde_json::Value,
}

/// Node-level conditions the engine reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeConditions {
    /// Free space on the state volume, when known.
    pub free_disk_bytes: Option<u64>,
    /// The message store cannot be opened or a commit failed.
    pub store_unavailable: bool,
    /// The last apply or startup configuration failed validation.
    pub config_rejected: bool,
    /// Channels whose change failed in the last apply, with the reason.
    pub apply_failures: Vec<(ChannelId, String)>,
    /// The audit log cannot be written.
    pub audit_unavailable: bool,
    /// Backups are stale (`backup.stale_after_hours` is set and exceeded).
    pub backup_stale: Option<StaleBackup>,
    /// Notification targets whose last delivery failed.
    pub notifications_failing: Vec<FailingTarget>,
}

/// A notification target whose last delivery failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailingTarget {
    /// `email` or `webhook`.
    pub target: String,
    /// The first failure after the last success.
    pub since: Timestamp,
    /// The newest error, operator text without secrets.
    pub error: String,
}

/// Why backups count as stale.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaleBackup {
    /// When the newest successful backup was taken; `None` when there is none.
    pub newest_success_at: Option<Timestamp>,
    /// The configured limit.
    pub limit_hours: u32,
}

/// How long cleared alerts stay in the history.
const HISTORY: Duration = Duration::from_hours(30 * 24);
/// At most this many cleared alerts are kept.
const HISTORY_LIMIT: usize = 1000;

/// Active and recently cleared alerts.
#[derive(Debug, Default)]
pub struct AlertBook {
    active: HashMap<AlertKey, Alert>,
    cleared: VecDeque<Alert>,
    /// Times each key was raised, for `occurrences`.
    raised: HashMap<AlertKey, u32>,
    /// Since when each bridge buffer has been above the threshold.
    pressure_since: HashMap<ChannelId, Timestamp>,
    /// Each message channel's recent invalid plus aborted counts.
    invalid_history: HashMap<ChannelId, CounterHistory>,
}

/// A since-start counter's values when it changed, oldest first; the first entry is the
/// value at or before the window start.
#[derive(Debug, Default)]
struct CounterHistory {
    /// The instance the counter belongs to.
    instance: Option<Timestamp>,
    samples: VecDeque<(Timestamp, u64)>,
}

impl AlertBook {
    /// An empty book.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-evaluates every rule; returns the alerts raised, changed in severity, or cleared
    /// by this call.
    pub fn evaluate(
        &mut self,
        config: &RuntimeConfig,
        channels: &[ChannelSnapshot],
        endpoints: &[EndpointSnapshot],
        node: &NodeConditions,
        now: Timestamp,
    ) -> Vec<Alert> {
        let findings = self.findings(config, channels, endpoints, node, now);
        self.apply(findings, now)
    }

    fn apply(&mut self, findings: Vec<Finding>, now: Timestamp) -> Vec<Alert> {
        let mut changes = Vec::new();
        let mut seen = HashSet::new();
        for finding in findings {
            seen.insert(finding.key);
            if let Some(alert) = self.active.get_mut(&finding.key) {
                changes.extend(refresh(alert, finding, now));
                continue;
            }
            let occurrences = self.raised.entry(finding.key).or_default();
            *occurrences = occurrences.saturating_add(1);
            let (kind, channel_id, endpoint_id, destination_id) = finding.key;
            let alert = Alert {
                id: AlertId::generate(),
                kind,
                severity: finding.severity,
                state: AlertState::Active,
                channel_id,
                endpoint_id,
                destination_id,
                summary: finding.summary,
                next_action: finding.next_action,
                first_seen: now,
                last_seen: now,
                cleared_at: None,
                occurrences: *occurrences,
                details: finding.details,
            };
            changes.push(alert.clone());
            self.active.insert(finding.key, alert);
        }
        let gone: Vec<AlertKey> = self
            .active
            .keys()
            .filter(|k| !seen.contains(*k))
            .copied()
            .collect();
        for mut alert in gone.iter().filter_map(|key| self.active.remove(key)) {
            alert.cleared_at = Some(now);
            alert.state = AlertState::Cleared;
            changes.push(alert.clone());
            self.cleared.push_front(alert);
        }
        let horizon = now.checked_sub(HISTORY);
        self.cleared
            .retain(|a| horizon.is_none_or(|h| a.cleared_at.is_none_or(|at| at >= h)));
        self.cleared.truncate(HISTORY_LIMIT);
        changes
    }

    /// Active alerts, newest first.
    #[must_use]
    pub fn active(&self) -> Vec<Alert> {
        let mut alerts: Vec<Alert> = self.active.values().cloned().collect();
        alerts.sort_by_key(|a| std::cmp::Reverse(a.first_seen));
        alerts
    }

    /// Active alerts, then (with `include_cleared`) cleared ones, newest first; only alerts
    /// seen at or after `since` when given.
    #[must_use]
    pub fn history(&self, include_cleared: bool, since: Option<Timestamp>) -> Vec<Alert> {
        let recent = |a: &&Alert| since.is_none_or(|s| a.last_seen >= s);
        let mut alerts: Vec<Alert> = self.active().into_iter().filter(|a| recent(&a)).collect();
        if include_cleared {
            alerts.extend(self.cleared.iter().filter(recent).cloned());
        }
        alerts
    }
}

/// Updates an active alert from a new finding; returns it when its severity changed.
fn refresh(alert: &mut Alert, finding: Finding, now: Timestamp) -> Option<Alert> {
    let escalated = alert.severity != finding.severity;
    alert.last_seen = now;
    alert.summary = finding.summary;
    // Summary, next action and details describe the same latest occurrence.
    alert.next_action = finding.next_action;
    alert.details = finding.details;
    alert.severity = finding.severity;
    escalated.then(|| alert.clone())
}

/// Free bytes on the volume holding `path`, or `None` when the OS cannot tell.
#[must_use]
pub fn free_space(path: &std::path::Path) -> Option<u64> {
    fs4::available_space(path).ok()
}

/// Total bytes of the volume holding `path`, or `None` when the OS cannot tell.
#[must_use]
pub fn total_space(path: &std::path::Path) -> Option<u64> {
    fs4::total_space(path).ok()
}
