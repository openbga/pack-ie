//! Per-channel difference between two runtime documents, which drives apply.
//!
//! A channel is `changed` (restart) when its definition differs or when any endpoint it
//! uses differs, and `updated` (apply in place, no restart) when only its label, tags or,
//! for message channels, filters, mappings, delivery policy or retention differ
//! (`docs/spec/configuration.md` §10). A channel the new document disables is `stopped`
//! where it would otherwise be added or changed.
//! Definitions are compared after parsing, so formatting, key order and spelled-out
//! defaults never count as changes.

use std::collections::HashMap;

use packie_core::{ChannelId, EndpointId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::channel::ChannelConfig;
use crate::runtime::RuntimeConfig;

/// What happened to one item between two documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// Only in the new document.
    Added,
    /// In both, with a different definition: the channel restarts.
    Changed,
    /// In both, differing only in what applies without a restart.
    Updated,
    /// Only in the old document.
    Removed,
    /// In both, identical.
    Unchanged,
    /// A channel the new document disables, added or changed: it is stopped, not started
    /// (channels only).
    Stopped,
}

/// One channel's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChannelChange {
    /// The channel.
    #[serde(rename = "channel_id")]
    pub id: ChannelId,
    /// Its label in the new document, or the old one when removed.
    pub label: String,
    /// What happened.
    pub change: Change,
}

/// One endpoint's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EndpointChange {
    /// The endpoint.
    #[serde(rename = "endpoint_id")]
    pub id: EndpointId,
    /// Its label in the new document, or the old one when removed.
    pub label: String,
    /// What happened.
    pub change: Change,
}

/// The difference between two documents: channels and endpoints in new-document order,
/// then removed ones in old-document order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConfigDiff {
    /// Channel changes.
    pub channels: Vec<ChannelChange>,
    /// Endpoint changes.
    pub endpoints: Vec<EndpointChange>,
    /// Whether the alert thresholds differ.
    pub alerts_changed: bool,
}

impl ConfigDiff {
    /// Compares `old` with `new`.
    #[must_use]
    pub fn between(old: &RuntimeConfig, new: &RuntimeConfig) -> Self {
        let endpoints = endpoint_changes(old, new);
        let endpoint_change: HashMap<EndpointId, Change> =
            endpoints.iter().map(|e| (e.id, e.change)).collect();

        let mut channels: Vec<ChannelChange> = new
            .channels
            .iter()
            .map(|channel| ChannelChange {
                id: channel.id(),
                label: channel.label().to_owned(),
                change: channel_change(old, channel, &endpoint_change),
            })
            .collect();
        for channel in old
            .channels
            .iter()
            .filter(|c| new.channel(c.id()).is_none())
        {
            channels.push(ChannelChange {
                id: channel.id(),
                label: channel.label().to_owned(),
                change: Change::Removed,
            });
        }
        Self {
            channels,
            endpoints,
            alerts_changed: old.alerts != new.alerts,
        }
    }

    /// Channels with the given change.
    pub fn channels_with(&self, change: Change) -> impl Iterator<Item = ChannelId> + '_ {
        self.channels
            .iter()
            .filter(move |c| c.change == change)
            .map(|c| c.id)
    }

    /// Whether nothing differs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.alerts_changed
            && self.channels.iter().all(|c| c.change == Change::Unchanged)
            && self.endpoints.iter().all(|e| e.change == Change::Unchanged)
    }
}

fn channel_change(
    old: &RuntimeConfig,
    channel: &ChannelConfig,
    endpoint_change: &HashMap<EndpointId, Change>,
) -> Change {
    let Some(before) = old.channel(channel.id()) else {
        return if channel.enabled() {
            Change::Added
        } else {
            Change::Stopped
        };
    };
    let endpoint_differs = channel
        .endpoint_ids()
        .iter()
        .any(|id| endpoint_change.get(id) != Some(&Change::Unchanged));
    if endpoint_differs || in_place_part(before) != in_place_part(channel) {
        if channel.enabled() {
            Change::Changed
        } else {
            Change::Stopped
        }
    } else if before != channel {
        Change::Updated
    } else {
        Change::Unchanged
    }
}

/// The channel without the fields that apply in place.
fn in_place_part(channel: &ChannelConfig) -> ChannelConfig {
    let mut rest = channel.clone();
    match &mut rest {
        ChannelConfig::Bridge(bridge) => {
            bridge.label.clear();
            bridge.tags.clear();
        }
        ChannelConfig::Message(message) => {
            message.label.clear();
            message.tags.clear();
            message.filters.clear();
            message.delivery = crate::channel::DeliveryPolicy::default();
            message.retention = crate::channel::Retention::default();
            for destination in &mut message.destinations {
                destination.filters.clear();
                destination.mapping = None;
                destination.delivery = crate::channel::DeliveryOverrides::default();
            }
        }
    }
    rest
}

fn endpoint_changes(old: &RuntimeConfig, new: &RuntimeConfig) -> Vec<EndpointChange> {
    let mut changes: Vec<EndpointChange> = new
        .endpoints
        .iter()
        .map(|endpoint| EndpointChange {
            id: endpoint.id,
            label: endpoint.label.clone(),
            change: match old.endpoint(endpoint.id) {
                None => Change::Added,
                Some(before) if before != endpoint => Change::Changed,
                Some(_) => Change::Unchanged,
            },
        })
        .collect();
    changes.extend(
        old.endpoints
            .iter()
            .filter(|e| new.endpoint(e.id).is_none())
            .map(|e| EndpointChange {
                id: e.id,
                label: e.label.clone(),
                change: Change::Removed,
            }),
    );
    changes
}

/// What an apply did to one channel (`docs/spec/configuration.md` §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ApplyOutcome {
    /// Not touched.
    Unchanged,
    /// Applied in place, without a restart.
    Updated,
    /// A new channel started.
    Started,
    /// Replaced by a new instance.
    Restarted,
    /// Stopped (disabled by the new definition).
    Stopped,
    /// Removed from the configuration.
    Removed,
    /// The change failed; the channel keeps its previous definition and instance when it
    /// had one, and the next apply retries it.
    Failed {
        /// Operator-facing cause.
        reason: String,
    },
}

/// One channel's apply outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChannelOutcome {
    /// The channel.
    pub channel_id: ChannelId,
    /// What happened.
    #[serde(flatten)]
    pub outcome: ApplyOutcome,
}

/// The per-channel outcomes of one apply, in new-document order then removed channels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ApplyReport {
    /// One entry per channel of either document.
    pub channels: Vec<ChannelOutcome>,
}

impl ApplyReport {
    /// The outcomes a registry reports when every change took effect as planned.
    #[must_use]
    pub fn planned(diff: &ConfigDiff) -> Self {
        let channels = diff
            .channels
            .iter()
            .map(|c| {
                let outcome = match c.change {
                    Change::Unchanged => ApplyOutcome::Unchanged,
                    Change::Updated => ApplyOutcome::Updated,
                    Change::Removed => ApplyOutcome::Removed,
                    Change::Stopped => ApplyOutcome::Stopped,
                    Change::Added => ApplyOutcome::Started,
                    Change::Changed => ApplyOutcome::Restarted,
                };
                ChannelOutcome {
                    channel_id: c.id,
                    outcome,
                }
            })
            .collect();
        Self { channels }
    }

    /// Whether any channel failed.
    #[must_use]
    pub fn partial(&self) -> bool {
        self.channels
            .iter()
            .any(|c| matches!(c.outcome, ApplyOutcome::Failed { .. }))
    }
}
