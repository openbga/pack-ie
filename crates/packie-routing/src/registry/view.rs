//! The read model snapshots are built from: swapped in after every registry operation,
//! read without waiting for one.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use packie_config::{ChannelConfig, EndpointConfig, Parity, RuntimeConfig, StopBits, Transport};
use packie_core::{ChannelId, ChannelState, EndpointId, EndpointKind, EndpointState, Timestamp};
use packie_diagnostics::{EndpointSnapshot, PeerRejections, TrafficTap};
use packie_engine::RegistrySnapshot;

use super::slots::is_exclusive;
use crate::message::MessageStore;
use crate::runner::EndpointCounters;
use crate::status::{ChannelShared, SideView, add_store_counts};

/// What snapshots read.
pub(super) struct View {
    pub(super) config: Arc<RuntimeConfig>,
    pub(super) channels: HashMap<ChannelId, Arc<ChannelShared>>,
    pub(super) endpoint_counters: EndpointCounters,
    pub(super) endpoint_since: HashMap<EndpointId, Timestamp>,
    pub(super) store: Option<Arc<dyn MessageStore>>,
    pub(super) tap: TrafficTap,
}

impl View {
    pub(super) fn snapshot(&self) -> RegistrySnapshot {
        let channels = self
            .config
            .channels
            .iter()
            .filter_map(|c| {
                let shared = self.channels.get(&c.id())?;
                let mut snapshot = shared.snapshot(c);
                if let (Some(detail), ChannelConfig::Message(channel), Some(store)) =
                    (snapshot.message.as_mut(), c, &self.store)
                {
                    add_store_counts(detail, channel, store.as_ref());
                }
                if let Some(bridge) = snapshot.bridge.as_mut() {
                    bridge.tap.transcript_viewers = u32::from(self.tap.is_watched(c.id()));
                }
                Some(snapshot)
            })
            .collect();
        // Which channels use each endpoint, in configuration order: one pass, not one
        // scan of every channel per endpoint.
        let mut users: HashMap<EndpointId, Vec<ChannelId>> = HashMap::new();
        for channel in &self.config.channels {
            for endpoint in channel.endpoint_ids() {
                users.entry(endpoint).or_default().push(channel.id());
            }
        }
        let endpoints = self
            .config
            .endpoints
            .iter()
            .map(|e| self.endpoint_snapshot(e, users.remove(&e.id).unwrap_or_default()))
            .collect();
        RegistrySnapshot {
            channels,
            endpoints,
        }
    }

    /// The snapshot of `endpoint`, used by the channels `users`.
    fn endpoint_snapshot(
        &self,
        endpoint: &EndpointConfig,
        users: Vec<ChannelId>,
    ) -> EndpointSnapshot {
        // Uses by channel instances that run; intentionally stopped ones leave it idle.
        let uses: Vec<(ChannelId, SideView)> = users
            .iter()
            .filter_map(|id| self.channels.get(id))
            .filter(|s| s.state() != ChannelState::Stopped)
            .flat_map(|s| {
                s.sides
                    .iter()
                    .filter(|c| c.endpoint_id == endpoint.id)
                    .map(|c| (s.id, c.view()))
            })
            .collect();
        let chosen = uses
            .iter()
            .map(|(_, u)| u)
            .min_by_key(|u| health_rank(u.state));
        let exclusive = is_exclusive(&endpoint.transport);
        let held_by = uses
            .iter()
            .find(|(_, u)| {
                exclusive && matches!(u.state, EndpointState::Connected | EndpointState::Listening)
            })
            .map(|(id, _)| *id);
        // Retry state as seen by the holding channel, else the first that uses it.
        let holder = held_by
            .or_else(|| uses.first().map(|(id, _)| *id))
            .and_then(|id| self.channels.get(&id));
        let counters = self.endpoint_counters.get(&endpoint.id);
        let since = self
            .endpoint_since
            .get(&endpoint.id)
            .copied()
            .unwrap_or_else(Timestamp::now);
        EndpointSnapshot {
            id: endpoint.id,
            label: endpoint.label.clone(),
            kind: endpoint.kind(),
            enabled: endpoint.enabled,
            state: if endpoint.enabled {
                chosen.map_or(EndpointState::Disconnected, |u| u.state)
            } else {
                EndpointState::Disabled
            },
            state_since: chosen.map_or(since, |u| u.since),
            peer: uses.iter().find_map(|(_, u)| u.peer.clone()),
            held_by,
            settings_summary: settings_summary(&endpoint.transport),
            consecutive_failures: uses
                .iter()
                .map(|(_, u)| u.consecutive_failures)
                .max()
                .unwrap_or(0),
            retry_count: holder.map_or(0, |s| s.retry_count()),
            next_retry_at: holder.and_then(|s| s.next_retry_at()),
            rejections: (endpoint.kind() == EndpointKind::TcpListener)
                .then(|| peer_rejections(uses.iter().filter_map(|(_, u)| u.rejections.as_ref()))),
            traffic: counters.map(|c| c.traffic()).unwrap_or_default(),
            last_error: counters.and_then(|c| c.last_error()),
            channels: users,
        }
    }
}

/// Non-secret one-line description of an endpoint's transport.
fn settings_summary(transport: &Transport) -> String {
    match transport {
        Transport::Serial(s) => {
            let parity = match s.parity {
                Parity::None => 'N',
                Parity::Even => 'E',
                Parity::Odd => 'O',
            };
            let stop = match s.stop_bits {
                StopBits::One => 1,
                StopBits::Two => 2,
            };
            format!(
                "{} {} {}{parity}{stop}",
                s.port_name, s.baud_rate, s.data_bits
            )
        }
        Transport::TcpClient(s) => format!("{}:{}", s.host, s.port),
        Transport::TcpListener(s) => SocketAddr::new(s.bind_address, s.port).to_string(),
        Transport::Folder(s) => s.path.display().to_string(),
        Transport::Sftp(s) => format!("sftp://{}:{}{}", s.host, s.port, s.remote_directory),
    }
}

/// Refusals of every channel use of a listener, merged: summed counts, latest refusal.
fn peer_rejections<'a>(
    uses: impl Iterator<Item = &'a packie_transport::Rejections>,
) -> PeerRejections {
    let mut merged = PeerRejections {
        count: 0,
        last_at: None,
        last_peer: None,
        last_reason: None,
    };
    for rejections in uses {
        merged.count = merged.count.saturating_add(rejections.count);
        if let Some((at, peer, reason)) = &rejections.last
            && merged.last_at.is_none_or(|last| *at > last)
        {
            merged.last_at = Some(*at);
            merged.last_peer = Some(peer.clone());
            merged.last_reason = Some((*reason).to_owned());
        }
    }
    merged
}

/// Lower is less healthy (`docs/spec/diagnostics.md` §5).
const fn health_rank(state: EndpointState) -> u8 {
    match state {
        EndpointState::Error => 0,
        EndpointState::Retrying => 1,
        EndpointState::Disconnected | EndpointState::Disabled => 2,
        EndpointState::Connecting => 3,
        EndpointState::Listening => 4,
        EndpointState::Connected => 5,
    }
}
