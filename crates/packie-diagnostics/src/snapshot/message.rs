//! A message channel's source, counters and per-destination delivery queues.

use packie_core::{ConnectorId, ProtocolKind, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{EndpointRef, ErrorSummary};

/// Session of a message channel's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SourceSession {
    /// Ready, nothing in progress.
    Idle,
    /// A transfer is in progress.
    Receiving,
    /// A listener waits for its peer.
    WaitingForPeer,
    /// Not open.
    Closed,
}

/// A message channel's source, counters and destinations (§4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageDetail {
    /// The source endpoint (role `source`).
    pub source: EndpointRef,
    /// The source protocol.
    pub protocol: ProtocolKind,
    /// The source session.
    pub session: SourceSession,
    /// Since the instance started.
    pub counters: MessageCounters,
    /// The last committed message.
    pub last_received_at: Option<Timestamp>,
    /// One entry per destination, in configuration order.
    pub destinations: Vec<DestinationSnapshot>,
    /// Current store counts across destinations.
    pub unresolved: Unresolved,
}

/// Message counters of a channel since its instance started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageCounters {
    /// Every message committed, including duplicates and invalid ones.
    #[ts(type = "number")]
    pub received: u64,
    /// Duplicates recorded and not processed.
    #[ts(type = "number")]
    pub duplicates: u64,
    /// Messages that did not parse or validate.
    #[ts(type = "number")]
    pub invalid: u64,
    /// Messages excluded by filters.
    #[ts(type = "number")]
    pub filtered: u64,
    /// Transfers that ended incomplete.
    #[ts(type = "number")]
    pub aborted: u64,
    /// Bytes received from the source.
    #[ts(type = "number")]
    pub bytes_in: u64,
}

/// Deliveries waiting for an operator decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Unresolved {
    /// Negatively acknowledged.
    #[ts(type = "number")]
    pub rejected: u64,
    /// Sent, no acknowledgement in time.
    #[ts(type = "number")]
    pub timed_out: u64,
    /// May or may not have arrived.
    #[ts(type = "number")]
    pub outcome_unknown: u64,
    /// Not delivered, attempts exhausted.
    #[ts(type = "number")]
    pub failed: u64,
    /// Of `failed`, the deliveries whose mapping failed (reason `transform_error`).
    #[serde(default)]
    #[ts(type = "number")]
    pub transform_errors: u64,
}

impl Unresolved {
    /// All of them.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.rejected
            .saturating_add(self.timed_out)
            .saturating_add(self.outcome_unknown)
            .saturating_add(self.failed)
    }
}

/// One destination's delivery queue (§4.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DestinationSnapshot {
    /// The destination.
    pub destination_id: ConnectorId,
    /// Its label.
    pub label: String,
    /// `enabled` in configuration.
    pub enabled: bool,
    /// Its endpoint (role = the destination id).
    pub endpoint: EndpointRef,
    /// Its protocol.
    pub protocol: ProtocolKind,
    /// Current store counts.
    pub queue: DeliveryQueueSnapshot,
    /// Current store counts of deliveries needing attention.
    pub unresolved: Unresolved,
    /// Since the instance started.
    pub counters: DestinationCounters,
    /// Last attempt start.
    pub last_attempt_at: Option<Timestamp>,
    /// Last acknowledgement.
    pub last_ack_at: Option<Timestamp>,
    /// Last failure, until cleared.
    pub last_error: Option<ErrorSummary>,
}

/// Queued and in-flight deliveries of one destination.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeliveryQueueSnapshot {
    /// Waiting for an attempt.
    #[ts(type = "number")]
    pub queued: u64,
    /// In flight.
    #[ts(type = "number")]
    pub sending: u64,
    /// Receipt time of the oldest queued delivery.
    pub oldest_queued_at: Option<Timestamp>,
    /// Earliest scheduled attempt.
    pub next_attempt_at: Option<Timestamp>,
}

/// Delivery counters of a destination since its instance started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DestinationCounters {
    /// Attempts started.
    #[ts(type = "number")]
    pub attempts: u64,
    /// Deliveries acknowledged.
    #[ts(type = "number")]
    pub acknowledged: u64,
    /// Deliveries rejected.
    #[ts(type = "number")]
    pub rejected: u64,
    /// Deliveries timed out.
    #[ts(type = "number")]
    pub timed_out: u64,
    /// Deliveries with unknown outcome.
    #[ts(type = "number")]
    pub outcome_unknown: u64,
    /// Deliveries failed.
    #[ts(type = "number")]
    pub failed: u64,
    /// Bytes sent.
    #[ts(type = "number")]
    pub bytes_out: u64,
}
