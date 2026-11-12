//! The bridge channel supervisor (`docs/spec/bridge-channels.md` §2–§5, §7, §8, §10).
//!
//! One lifecycle loop per instance: attach both sides in strategy order, bridge them
//! until the session ends, schedule the next attempt with the channel's backoff, repeat
//! until stopped. Listeners stay bound across attempts (except the sender-side listener
//! under `receiver_then_sender`, bound only while the receiver is attached), so a peer
//! that connects during backoff waits in the accept backlog.

use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use packie_config::{BridgeChannel, ChannelConfig, ConnectStrategy, EndpointConfig, Transport};
use packie_core::{BridgeSide, EndpointState, SessionState, Timestamp};
use packie_diagnostics::Counters;
use packie_engine::BoxFuture;

use crate::failure::Failure;
use crate::listen;
use crate::retry::{Backoff, describe_delay};
use crate::runner::{ChannelRunner, RunContext};
use crate::session::{self, SessionContext, SessionEnd};
use crate::status::{ChannelShared, Phase, side_index};
use crate::stop::{StopRx, is_requested, sleep_unless_stopped, stopped};
use packie_transport::{Acceptor, Link, LinkError, Opener, Operation};

/// Runs bridge channels.
#[derive(Debug, Default)]
pub(crate) struct BridgeRunner;

impl ChannelRunner for BridgeRunner {
    fn run(&self, cx: RunContext) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let mut stop = cx.stop.clone();
            let Some(mut supervisor) = Supervisor::new(&cx) else {
                return;
            };
            supervisor.run(&mut stop).await;
        })
    }
}

/// One side's endpoint and how to bring it up.
struct SideSetup {
    side: BridgeSide,
    opener: Result<Opener, LinkError>,
    counters: Arc<Counters>,
}

impl SideSetup {
    fn listens(&self) -> bool {
        matches!(self.opener, Ok(Opener::Listen(_)))
    }

    /// The operation a pairing timeout on this side is reported as.
    fn pairing_operation(&self, endpoint: &EndpointConfig) -> Operation {
        match (&self.opener, &endpoint.transport) {
            (Ok(Opener::Listen(_)), _) => Operation::Accept,
            (_, Transport::Serial(_)) => Operation::Open,
            _ => Operation::Connect,
        }
    }
}

struct Supervisor {
    shared: Arc<ChannelShared>,
    channel: BridgeChannel,
    endpoints: [EndpointConfig; 2],
    sides: [SideSetup; 2],
    listeners: [Option<Box<dyn Acceptor>>; 2],
    backoff: Backoff,
    session: Arc<SessionContext>,
}

impl Supervisor {
    fn new(cx: &RunContext) -> Option<Self> {
        let ChannelConfig::Bridge(channel) = &cx.channel else {
            return None;
        };
        let endpoint_a = cx.config.endpoint(channel.endpoint_a)?.clone();
        let endpoint_b = cx.config.endpoint(channel.endpoint_b)?.clone();
        let side = |side: BridgeSide, endpoint: &EndpointConfig| SideSetup {
            side,
            opener: cx.transports.opener(endpoint, cx.shared.id),
            counters: cx.counters_of(endpoint.id),
        };
        let sides = [
            side(BridgeSide::A, &endpoint_a),
            side(BridgeSide::B, &endpoint_b),
        ];
        let session = Arc::new(SessionContext {
            shared: Arc::clone(&cx.shared),
            endpoint_counters: [
                Arc::clone(&sides[0].counters),
                Arc::clone(&sides[1].counters),
            ],
            traffic_tap: cx.traffic_tap.clone(),
            tap: session::TapSettings::of(&channel.tap),
            tap_sink: cx.tap_sink.clone(),
            stable_after: Duration::from_millis(u64::from(channel.retry.max_delay_ms)),
            close_drain: Duration::from_millis(u64::from(channel.buffer.block_timeout_ms)),
        });
        Some(Self {
            shared: Arc::clone(&cx.shared),
            backoff: Backoff::new(&channel.retry),
            channel: channel.clone(),
            endpoints: [endpoint_a, endpoint_b],
            sides,
            listeners: [None, None],
            session,
        })
    }

    async fn run(&mut self, stop: &mut StopRx) {
        loop {
            self.shared.clear_next_retry();
            self.shared.set_session(SessionState::WaitingForPeer);
            for flow in &self.shared.flows {
                flow.reset();
            }
            self.shared.refresh();
            let attempt = tokio::select! {
                biased;
                _ = stopped(stop) => return,
                attempt = self.attach() => attempt,
            };
            let (failure, propagated) = match attempt {
                Err(failure) => (failure, false),
                Ok(links) => match self.bridge(links, stop).await {
                    SessionEnd::Stopped => return,
                    SessionEnd::Ended(failure) => (failure, true),
                },
            };
            if is_requested(stop) || self.shared.is_stopping() {
                return;
            }
            let delay = self.schedule_retry(&failure, propagated);
            if !sleep_unless_stopped(stop, delay).await {
                return;
            }
        }
    }

    fn receiver(&self) -> Option<BridgeSide> {
        (self.channel.connection.connect_strategy == ConnectStrategy::ReceiverThenSender)
            .then(|| self.channel.connection.receiver.unwrap_or(BridgeSide::A))
    }

    async fn attach(&mut self) -> Result<[Link; 2], Failure> {
        let pairing =
            Duration::from_millis(u64::from(self.channel.connection.pairing_timeout_ms.max(1)));
        match self.receiver() {
            Some(receiver) => self.attach_in_order(receiver, pairing).await,
            None => self.attach_parallel(pairing).await,
        }
    }

    /// Both sides at once; the wait for the first is unbounded, the second must follow
    /// within the pairing timeout. Any transport failure fails the attempt at once.
    async fn attach_parallel(&mut self, pairing: Duration) -> Result<[Link; 2], Failure> {
        let timeouts = [
            self.pairing_timeout(BridgeSide::A),
            self.pairing_timeout(BridgeSide::B),
        ];
        let shared = &*self.shared;
        let [setup_a, setup_b] = &self.sides;
        let [listener_a, listener_b] = &mut self.listeners;
        // Listeners are bound before any outbound attempt, so a peer that dials early
        // waits in the backlog even when the other side's connect fails.
        ensure_bound(shared, setup_a, listener_a).await?;
        ensure_bound(shared, setup_b, listener_b).await?;
        let mut a = pin!(attach_side(shared, setup_a, listener_a));
        let mut b = pin!(attach_side(shared, setup_b, listener_b));
        let (first, link) = tokio::select! {
            link = &mut a => (BridgeSide::A, link?),
            link = &mut b => (BridgeSide::B, link?),
        };
        let second = match first {
            BridgeSide::A => b,
            BridgeSide::B => a,
        };
        let other = first.opposite();
        let other_link = match tokio::time::timeout(pairing, second).await {
            Ok(result) => result?,
            Err(_) => return Err(timeouts[side_index(other)].clone()),
        };
        Ok(match first {
            BridgeSide::A => [link, other_link],
            BridgeSide::B => [other_link, link],
        })
    }

    /// The receiver first, unbounded; then the sender within the pairing timeout.
    async fn attach_in_order(
        &mut self,
        receiver: BridgeSide,
        pairing: Duration,
    ) -> Result<[Link; 2], Failure> {
        let sender = receiver.opposite();
        let timeout = self.pairing_timeout(sender);
        let shared = &*self.shared;
        let [listener_a, listener_b] = &mut self.listeners;
        let (receiver_listener, sender_listener) = match receiver {
            BridgeSide::A => (listener_a, listener_b),
            BridgeSide::B => (listener_b, listener_a),
        };
        let receiver_setup = &self.sides[side_index(receiver)];
        let sender_setup = &self.sides[side_index(sender)];
        let receiver_link = attach_side(shared, receiver_setup, receiver_listener).await?;
        let sender_link =
            match tokio::time::timeout(pairing, attach_side(shared, sender_setup, sender_listener))
                .await
            {
                Ok(result) => result?,
                Err(_) => return Err(timeout),
            };
        Ok(match receiver {
            BridgeSide::A => [receiver_link, sender_link],
            BridgeSide::B => [sender_link, receiver_link],
        })
    }

    fn pairing_timeout(&self, side: BridgeSide) -> Failure {
        let index = side_index(side);
        let operation = self.sides[index].pairing_operation(&self.endpoints[index]);
        Failure::new(side, operation, "pairing timeout")
    }

    async fn bridge(&mut self, links: [Link; 2], stop: &mut StopRx) -> SessionEnd {
        let shared = Arc::clone(&self.shared);
        let peers = [links[0].peer.clone(), links[1].peer.clone()];
        shared.set_session(SessionState::Bridged);
        shared.set_phase(Phase::Running);
        shared.span.in_scope(|| {
            tracing::info!(
                event = "bridge.session_opened",
                session_state = SessionState::Bridged.as_str(),
                peer_a = peers[0].as_deref().unwrap_or(""),
                peer_b = peers[1].as_deref().unwrap_or(""),
                "Channel {} bridged",
                shared.id
            );
        });
        let end = session::run(Arc::clone(&self.session), links, &mut self.listeners, stop).await;
        if !shared.is_stopping() {
            shared.set_session(SessionState::Closing);
        }
        shared.span.in_scope(|| {
            tracing::info!(
                event = "bridge.session_closed",
                session_state = SessionState::Closed.as_str(),
                "Channel {} session closed",
                shared.id
            );
        });
        end
    }

    /// Records `failure`, schedules the next attempt and returns its delay.
    fn schedule_retry(&mut self, failure: &Failure, propagated: bool) -> Duration {
        let receiver = self.receiver();
        if let Some(receiver) = receiver {
            // The sender-side listener is bound only while the receiver is attached.
            self.listeners[side_index(receiver.opposite())] = None;
        }
        let shared = Arc::clone(&self.shared);
        let streak = shared.count_failure(failure.side);
        let delay = self
            .backoff
            .after_failure(failure.persistent, shared.retry_count().saturating_add(1));
        let retry_count = shared.schedule_retry(Timestamp::now().checked_add(delay));
        for setup in &self.sides {
            let state = self.waiting_state(setup, failure, receiver);
            shared.set_endpoint(setup.side, state);
        }
        shared.set_session(SessionState::Closed);
        shared.set_phase(Phase::Retrying);

        let Some(cell) = shared.side(failure.side) else {
            return delay;
        };
        let summary = failure.summary(shared.id, cell);
        let next_action = format!("Retrying in {}.", describe_delay(delay));
        shared.record_endpoint_error(
            &self.sides[side_index(failure.side)].counters,
            summary.clone(),
            &next_action,
        );
        let endpoint_id = cell.endpoint_id.to_string();
        let role = failure.side.as_str();
        let text = format!("{summary} {next_action}");
        let next_retry_at = Timestamp::now()
            .checked_add(delay)
            .map(|t| t.to_string())
            .unwrap_or_default();
        let alerting = streak >= self.channel.retry.max_consecutive_failures_before_alert;
        shared.span.in_scope(|| {
            if propagated {
                tracing::info!(
                    event = "bridge.close_propagated",
                    endpoint_id,
                    endpoint_role = role,
                    endpoint_kind = cell.kind.as_str(),
                    error_summary = text,
                    "Channel {} {role} endpoint {endpoint_id}: session ended; closed the other side",
                    shared.id
                );
            }
            if alerting {
                tracing::error!(
                    event = "endpoint.retry_scheduled",
                    endpoint_id,
                    endpoint_role = role,
                    endpoint_kind = cell.kind.as_str(),
                    retry_count,
                    next_retry_at,
                    error_summary = text,
                    "Channel {} {role} endpoint {endpoint_id}: {streak} consecutive failures",
                    shared.id
                );
            } else {
                tracing::warn!(
                    event = "endpoint.retry_scheduled",
                    endpoint_id,
                    endpoint_role = role,
                    endpoint_kind = cell.kind.as_str(),
                    retry_count,
                    next_retry_at,
                    error_summary = text,
                    "Channel {} {role} endpoint {endpoint_id}: retry scheduled",
                    shared.id
                );
            }
        });
        delay
    }

    /// The state `setup`'s endpoint reports while the retry waits.
    fn waiting_state(
        &self,
        setup: &SideSetup,
        failure: &Failure,
        receiver: Option<BridgeSide>,
    ) -> EndpointState {
        if setup.side == failure.side && failure.persistent {
            return EndpointState::Error;
        }
        if setup.listens() {
            if self.listeners[side_index(setup.side)].is_some() {
                return EndpointState::Listening;
            }
            if receiver.is_some_and(|r| r != setup.side) {
                return EndpointState::Disconnected;
            }
        }
        EndpointState::Retrying
    }
}

/// Brings up one side: dials, or binds (if needed) and accepts one allowed peer.
async fn attach_side(
    shared: &ChannelShared,
    setup: &SideSetup,
    listener: &mut Option<Box<dyn Acceptor>>,
) -> Result<Link, Failure> {
    let side = setup.side;
    let opener = setup
        .opener
        .as_ref()
        .map_err(|error| Failure::link(side, error.clone()))?;
    let link = listen::attach(shared, side.into(), opener, listener)
        .await
        .map_err(|e| Failure::link(side, e))?;
    shared.connected(side, link.peer.clone());
    Ok(link)
}

/// Binds `setup`'s listener unless it is bound already; a no-op for dialing sides.
async fn ensure_bound(
    shared: &ChannelShared,
    setup: &SideSetup,
    listener: &mut Option<Box<dyn Acceptor>>,
) -> Result<(), Failure> {
    let Ok(Opener::Listen(bind)) = &setup.opener else {
        return Ok(());
    };
    listen::ensure_bound(shared, setup.side.into(), bind.as_ref(), listener)
        .await
        .map_err(|e| Failure::link(setup.side, e))
}
