//! One endpoint as a channel instance uses it: its role, state, peer, failure streak,
//! last activity and refused peers.

use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

use packie_config::EndpointConfig;
use packie_core::{
    BridgeSide, ChannelId, ChannelKind, ConnectorId, EndpointId, EndpointKind, EndpointState,
    Timestamp,
};
use packie_transport::Rejections;

use super::{ChannelShared, lock, millis_to_time};

/// The part an endpoint plays in a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// Bridge side A or B.
    Side(BridgeSide),
    /// A message channel's source connector.
    Source,
    /// A message channel's destination.
    Destination(ConnectorId),
}

impl From<BridgeSide> for Role {
    fn from(side: BridgeSide) -> Self {
        Self::Side(side)
    }
}

impl Role {
    /// `endpoint_role` log field: `a`, `b`, `source`, `destination`.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Side(side) => side.as_str(),
            Self::Source => "source",
            Self::Destination(_) => "destination",
        }
    }
}

/// One endpoint as used by this channel (a bridge side, a source or a destination).
#[derive(Debug)]
pub(crate) struct SideCell {
    pub(crate) role: Role,
    pub(crate) endpoint_id: EndpointId,
    pub(crate) label: String,
    pub(crate) kind: EndpointKind,
    status: Mutex<SideStatus>,
    last_activity_ms: AtomicI64,
    refusals: Mutex<Refusals>,
}

/// Peers a listener refused: what the transport counted and what routing refused itself.
#[derive(Debug, Default)]
struct Refusals {
    transport: Rejections,
    local: Rejections,
}

#[derive(Debug, Clone)]
struct SideStatus {
    state: EndpointState,
    since: Timestamp,
    consecutive_failures: u32,
    peer: Option<String>,
}

/// A side's state as the endpoint snapshot needs it.
#[derive(Debug, Clone)]
pub(crate) struct SideView {
    pub(crate) rejections: Option<Rejections>,
    pub(crate) state: EndpointState,
    pub(crate) since: Timestamp,
    pub(crate) consecutive_failures: u32,
    pub(crate) peer: Option<String>,
}

impl SideCell {
    pub(crate) fn new(role: impl Into<Role>, endpoint: &EndpointConfig) -> Self {
        Self {
            role: role.into(),
            endpoint_id: endpoint.id,
            label: endpoint.label.clone(),
            kind: endpoint.kind(),
            status: Mutex::new(SideStatus {
                state: EndpointState::Disconnected,
                since: Timestamp::now(),
                consecutive_failures: 0,
                peer: None,
            }),
            last_activity_ms: AtomicI64::new(0),
            refusals: Mutex::new(Refusals::default()),
        }
    }

    /// The transport listener's refusal counts, sampled while it accepts.
    pub(crate) fn set_transport_rejections(&self, rejections: Rejections) {
        lock(&self.refusals).transport = rejections;
    }

    /// A peer routing itself refused (a second peer while a session is active).
    pub(crate) fn count_refusal(&self, peer: String, reason: &'static str) {
        let mut refusals = lock(&self.refusals);
        refusals.local.count = refusals.local.count.saturating_add(1);
        refusals.local.last = Some((Timestamp::now(), peer, reason));
    }

    /// Refused peers for the endpoint snapshot; `None` unless the endpoint listens.
    pub(crate) fn rejections(&self) -> Option<Rejections> {
        if self.kind != EndpointKind::TcpListener {
            return None;
        }
        let refusals = lock(&self.refusals);
        let last = [&refusals.transport.last, &refusals.local.last]
            .into_iter()
            .flatten()
            .max_by_key(|(at, _, _)| *at)
            .cloned();
        Some(Rejections {
            count: refusals
                .transport
                .count
                .saturating_add(refusals.local.count),
            last,
        })
    }

    /// The role as error summaries name it: `A`, `B`, `source`, `destination lis`.
    pub(crate) fn role_label(&self) -> String {
        match self.role {
            Role::Side(BridgeSide::A) => "A".to_owned(),
            Role::Side(BridgeSide::B) => "B".to_owned(),
            Role::Source => "source".to_owned(),
            Role::Destination(id) => format!("destination {id}"),
        }
    }

    /// `Channel {channel} {role} endpoint "{label}" {operation} failed: {cause}.` for
    /// message channel endpoints (bridges name the side first, see [`crate::Failure`]).
    pub(crate) fn failure_summary(
        &self,
        channel: ChannelId,
        operation: impl std::fmt::Display,
        cause: &str,
    ) -> String {
        format!(
            "Channel {channel} {} endpoint \"{}\" {operation} failed: {cause}.",
            self.role_label(),
            self.label
        )
    }

    /// Successful I/O on this side.
    pub(crate) fn touch(&self) {
        self.last_activity_ms
            .store(Timestamp::now().unix_millis(), Ordering::Relaxed);
    }

    /// Current view.
    pub(crate) fn view(&self) -> SideView {
        let status = lock(&self.status);
        SideView {
            rejections: self.rejections(),
            state: status.state,
            since: status.since,
            consecutive_failures: status.consecutive_failures,
            peer: status.peer.clone(),
        }
    }

    /// Last successful I/O.
    pub(crate) fn last_activity(&self) -> Option<Timestamp> {
        millis_to_time(self.last_activity_ms.load(Ordering::Relaxed))
    }

    /// Sets the endpoint state; `false` when it was already `state`. Leaving
    /// `connected` forgets the peer.
    pub(super) fn set_state(&self, state: EndpointState) -> bool {
        let mut status = lock(&self.status);
        if status.state == state {
            return false;
        }
        status.state = state;
        status.since = Timestamp::now();
        if state != EndpointState::Connected {
            status.peer = None;
        }
        true
    }

    /// Records the attached peer (`None` when it left).
    pub(super) fn set_peer(&self, peer: Option<String>) {
        lock(&self.status).peer = peer;
    }

    /// Whether a peer is attached.
    pub(super) fn has_peer(&self) -> bool {
        lock(&self.status).peer.is_some()
    }

    /// Counts a failure; returns the new streak.
    pub(super) fn count_failure(&self) -> u32 {
        let mut status = lock(&self.status);
        status.consecutive_failures = status.consecutive_failures.saturating_add(1);
        status.consecutive_failures
    }

    /// Resets the failure streak; `true` when there was one.
    pub(super) fn clear_failures(&self) -> bool {
        let mut status = lock(&self.status);
        std::mem::take(&mut status.consecutive_failures) > 0
    }

    pub(super) fn fields(&self) -> (String, &'static str, &'static str) {
        (
            self.endpoint_id.to_string(),
            self.role.as_str(),
            self.kind.as_str(),
        )
    }
}

/// Whether an endpoint in `state` can carry traffic.
pub(super) const fn ready(state: EndpointState) -> bool {
    matches!(state, EndpointState::Connected | EndpointState::Listening)
}

/// Endpoint transitions of a channel instance: each change is logged once inside the
/// channel's span and wakes the snapshot publisher.
impl ChannelShared {
    /// Sets the state of `side`'s endpoint; logs a change.
    pub(crate) fn set_endpoint(&self, role: impl Into<Role>, state: EndpointState) {
        if self.is_stopping() && state != EndpointState::Disconnected {
            return;
        }
        let Some(cell) = self.cell(role.into()) else {
            return;
        };
        if !cell.set_state(state) {
            return;
        }
        let (endpoint_id, role, kind) = cell.fields();
        self.span.in_scope(|| {
            tracing::info!(
                event = "endpoint.state_changed",
                endpoint_id,
                endpoint_role = role,
                endpoint_kind = kind,
                endpoint_state = state.as_str(),
                "Channel {} {role} endpoint {endpoint_id}: {state}",
                self.id
            );
        });
        if self.kind == ChannelKind::Message {
            self.refresh();
        }
        self.notify();
    }

    /// `side` attached a peer.
    pub(crate) fn connected(&self, role: impl Into<Role>, peer: Option<String>) {
        let role = role.into();
        self.set_endpoint(role, EndpointState::Connected);
        let Some(cell) = self.cell(role) else {
            return;
        };
        cell.set_peer(peer.clone());
        let (endpoint_id, role, kind) = cell.fields();
        self.span.in_scope(|| {
            tracing::info!(
                event = "endpoint.connected",
                endpoint_id,
                endpoint_role = role,
                endpoint_kind = kind,
                peer = peer.as_deref().unwrap_or(""),
                "Channel {} {role} endpoint {endpoint_id}: connected",
                self.id
            );
        });
    }

    /// Records the peer of `role` without changing its state (a listener source stays
    /// `listening` while its peer is attached); `None` when the peer leaves.
    pub(crate) fn set_peer(&self, role: Role, peer: Option<String>) {
        let cell = self.cell(role);
        if let Some(cell) = cell {
            cell.set_peer(peer);
        }
        if let Some(messages) = &self.messages {
            messages
                .peer_attached
                .store(cell.is_some_and(SideCell::has_peer), Ordering::Relaxed);
        }
        self.notify();
    }

    /// Counts a failure attributed to `side`; returns the new streak.
    pub(crate) fn count_failure(&self, role: impl Into<Role>) -> u32 {
        self.cell(role.into()).map_or(0, SideCell::count_failure)
    }
}
