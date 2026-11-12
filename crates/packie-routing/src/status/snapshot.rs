//! The published snapshot of a channel instance (`docs/spec/diagnostics.md` §4).

use std::sync::atomic::{AtomicU64, Ordering};

use packie_config::{BridgeChannel, ChannelConfig, MessageChannel};
use packie_core::{BridgeSide, EndpointId, EndpointKind, EndpointState, Timestamp};
use packie_diagnostics::{
    BridgeDetail, ChannelSnapshot, DeliveryQueueSnapshot, DestinationCounters, DestinationSnapshot,
    EndpointRef, ErrorSummary, FlowSnapshot, MessageCounters as SnapshotMessageCounters,
    MessageDetail, SourceSession, TapSnapshot, Unresolved,
};

use super::{ChannelShared, MessageCounters, Role, Status, millis_to_time, side_index};
use crate::buffer::{FlowGauge, FlowLimits};
use crate::message::MessageStore;

impl ChannelShared {
    /// The snapshot of this instance for `channel` (the active definition). Message
    /// destinations carry only what the instance knows; the registry adds store counts.
    pub(crate) fn snapshot(&self, channel: &ChannelConfig) -> ChannelSnapshot {
        let status = self.lock();
        let mut traffic = self.counters.traffic();
        if let Some(tap) = &self.tap {
            traffic.errors = traffic
                .errors
                .saturating_add(tap.framing_errors.load(Ordering::Relaxed));
        }
        let state = status.published;
        ChannelSnapshot {
            id: self.id,
            label: channel.label().to_owned(),
            kind: self.kind,
            enabled: channel.enabled(),
            state,
            stop_reason: status.stop_reason,
            state_since: status.since,
            config_version: self.config_version,
            instance_started_at: self.ran.then_some(self.started_at),
            endpoints: channel.endpoint_ids(),
            traffic,
            retry_count: status.retry_count,
            next_retry_at: status.next_retry_at,
            last_error: self.counters.last_error(),
            recent_errors: status.recent_errors.iter().cloned().collect(),
            allowed_actions: ChannelSnapshot::actions_for(
                channel.enabled(),
                state,
                status.stop_reason,
            ),
            bridge: match channel {
                ChannelConfig::Bridge(bridge) => Some(self.bridge_detail(bridge, &status)),
                ChannelConfig::Message(_) => None,
            },
            message: match channel {
                ChannelConfig::Message(message) => Some(self.message_detail(message)),
                ChannelConfig::Bridge(_) => None,
            },
        }
    }

    fn bridge_detail(&self, bridge: &BridgeChannel, status: &Status) -> BridgeDetail {
        let traffic = self.counters.traffic();
        let flow = |side: BridgeSide, bytes: u64| {
            let (gauge, limits) = self.flow(side).map_or_else(
                || (FlowGauge::default(), FlowLimits::new(&bridge.buffer)),
                |f| (f.gauge(), f.limits()),
            );
            FlowSnapshot {
                bytes,
                messages: self
                    .tap
                    .as_ref()
                    .and_then(|t| t.messages.get(side_index(side)))
                    .map(|m| m.load(Ordering::Relaxed)),
                buffered_segments: gauge.segments,
                buffered_bytes: gauge.bytes,
                max_segments: limits.max_segments,
                max_bytes: limits.max_bytes,
                overflowing: gauge.overflowing,
                overflow_count: gauge.overflow_count,
            }
        };
        let tap = self.tap.as_ref();
        let load = |c: &AtomicU64| c.load(Ordering::Relaxed);
        BridgeDetail {
            session: status.session,
            session_since: status.session_since,
            connect_strategy: bridge.connection.connect_strategy,
            receiver: bridge.connection.receiver,
            endpoint_a: self.endpoint_ref(Role::Side(BridgeSide::A), bridge.endpoint_a),
            endpoint_b: self.endpoint_ref(Role::Side(BridgeSide::B), bridge.endpoint_b),
            a_to_b: flow(BridgeSide::A, traffic.bytes_in),
            b_to_a: flow(BridgeSide::B, traffic.bytes_out),
            tap: TapSnapshot {
                framer: bridge.tap.framer,
                record_messages: bridge.tap.record_messages,
                framing_errors: tap.map(|t| load(&t.framing_errors)),
                tap_dropped: tap.map(|t| load(&t.dropped)),
                transcript_viewers: 0,
            },
        }
    }

    fn message_detail(&self, message: &MessageChannel) -> MessageDetail {
        let counters = self.messages.as_ref();
        let load = |pick: fn(&MessageCounters) -> &AtomicU64| {
            counters.map_or(0, |c| pick(c).load(Ordering::Relaxed))
        };
        let source = self.endpoint_ref(Role::Source, message.source.endpoint);
        let receiving = counters.is_some_and(|c| c.receiving.load(Ordering::Relaxed));
        let attached = counters.is_some_and(|c| c.peer_attached.load(Ordering::Relaxed));
        let session = match source.state {
            EndpointState::Listening | EndpointState::Connected if receiving => {
                SourceSession::Receiving
            }
            EndpointState::Listening if attached => SourceSession::Idle,
            EndpointState::Listening => SourceSession::WaitingForPeer,
            EndpointState::Connected => SourceSession::Idle,
            _ => SourceSession::Closed,
        };
        let last = counters.map_or(0, |c| c.last_received_ms.load(Ordering::Relaxed));
        MessageDetail {
            source,
            protocol: message.source.protocol.kind(),
            session,
            counters: SnapshotMessageCounters {
                received: load(|c| &c.received),
                duplicates: load(|c| &c.duplicates),
                invalid: load(|c| &c.invalid),
                filtered: load(|c| &c.filtered),
                aborted: load(|c| &c.aborted),
                bytes_in: load(|c| &c.bytes_in),
            },
            last_received_at: millis_to_time(last),
            destinations: message
                .destinations
                .iter()
                .map(|d| DestinationSnapshot {
                    destination_id: d.id,
                    label: d.label.clone(),
                    enabled: d.enabled,
                    endpoint: self.endpoint_ref(Role::Destination(d.id), d.endpoint),
                    protocol: d.protocol.kind(),
                    queue: DeliveryQueueSnapshot::default(),
                    unresolved: Unresolved::default(),
                    counters: DestinationCounters::default(),
                    last_attempt_at: None,
                    last_ack_at: None,
                    last_error: None,
                })
                .collect(),
            unresolved: Unresolved::default(),
        }
    }

    /// The endpoint reference of `role` (idle when this instance has no such cell).
    fn endpoint_ref(&self, role: Role, configured: EndpointId) -> EndpointRef {
        let role_text = match role {
            Role::Destination(id) => id.as_str().to_owned(),
            other => other.as_str().to_owned(),
        };
        let Some(cell) = self.cell(role) else {
            return EndpointRef {
                endpoint_id: configured,
                label: String::new(),
                kind: EndpointKind::TcpClient,
                role: role_text,
                state: EndpointState::Disconnected,
                consecutive_failures: 0,
                active_peer: None,
                last_activity: None,
            };
        };
        let view = cell.view();
        EndpointRef {
            endpoint_id: cell.endpoint_id,
            label: cell.label.clone(),
            kind: cell.kind,
            role: role_text,
            state: view.state,
            consecutive_failures: view.consecutive_failures,
            active_peer: view.peer,
            last_activity: cell.last_activity(),
        }
    }
}

/// Adds the store's current queue counts to a message channel's destinations.
pub(crate) fn add_store_counts(
    detail: &mut MessageDetail,
    channel: &MessageChannel,
    store: &dyn MessageStore,
) {
    let mut total = Unresolved::default();
    for destination in &mut detail.destinations {
        let Ok(counts) = store.queue_counts(channel.id, destination.destination_id) else {
            continue;
        };
        destination.queue = DeliveryQueueSnapshot {
            queued: counts.queued,
            sending: counts.sending,
            oldest_queued_at: counts.oldest_queued_at,
            next_attempt_at: counts.next_attempt_at,
        };
        destination.unresolved = Unresolved {
            rejected: counts.rejected,
            timed_out: counts.timed_out,
            outcome_unknown: counts.outcome_unknown,
            failed: counts.failed,
            transform_errors: counts.transform_errors,
        };
        destination.counters = DestinationCounters {
            attempts: counts.attempts,
            acknowledged: counts.acknowledged,
            rejected: counts.rejected,
            timed_out: counts.timed_out,
            outcome_unknown: counts.outcome_unknown,
            failed: counts.failed,
            bytes_out: counts.bytes_out,
        };
        destination.last_attempt_at = counts.last_attempt_at;
        destination.last_ack_at = counts.last_ack_at;
        destination.last_error = counts.last_error.map(|summary| ErrorSummary {
            at: counts.last_attempt_at.unwrap_or_else(Timestamp::now),
            summary,
            next_action: None,
        });
        total.rejected += destination.unresolved.rejected;
        total.timed_out += destination.unresolved.timed_out;
        total.outcome_unknown += destination.unresolved.outcome_unknown;
        total.failed += destination.unresolved.failed;
        total.transform_errors += destination.unresolved.transform_errors;
    }
    detail.unresolved = total;
}
