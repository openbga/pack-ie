//! Live state of one channel instance: what supervisors write and snapshots read.
//!
//! Hot-path values are atomics ([`Counters`], buffer gauges, last activity); the rest
//! sits in small mutexes that are never held across an `.await`. Every state transition
//! is logged once, inside the channel's span, and wakes the snapshot publisher.
//! Endpoint cells live in `side`, snapshot building in `snapshot`.

mod counters;
mod side;
mod snapshot;

pub(crate) use counters::{MessageCounters, TapCounters};
pub(crate) use side::{Role, SideCell, SideView};
pub(crate) use snapshot::add_store_counts;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use packie_config::ChannelConfig;
use packie_core::{
    BridgeSide, ChannelId, ChannelKind, ChannelState, EndpointState, SessionState, StopReason,
    Timestamp,
};
use packie_diagnostics::{Counters, ErrorSummary};
use tokio::sync::Notify;
use tracing::Span;

use crate::buffer::FlowBuffer;

/// Recent error summaries kept per channel instance until the configuration says
/// otherwise (`diagnostics.retain_recent_errors`, default 100).
const RECENT_ERRORS: usize = 100;

/// What the supervisor is doing; the published state also folds in buffer health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// First attempt of the instance.
    Starting,
    /// A session is bridged.
    Running,
    /// Waiting for or making a later attempt.
    Retrying,
    /// Not running.
    Stopped,
}

/// Why an instance stopped, including the internal restart reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopCause {
    /// A published stop reason.
    Reason(StopReason),
    /// Operator restart; the instance is replaced and never published as stopped.
    OperatorRestart,
}

impl StopCause {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reason(reason) => reason.as_str(),
            Self::OperatorRestart => "operator_restart",
        }
    }
}

#[derive(Debug)]
struct Status {
    phase: Phase,
    stop_reason: Option<StopReason>,
    published: ChannelState,
    since: Timestamp,
    session: SessionState,
    session_since: Timestamp,
    retry_count: u32,
    next_retry_at: Option<Timestamp>,
    recent_errors: VecDeque<ErrorSummary>,
}

/// Shared state of one channel instance.
#[derive(Debug)]
pub(crate) struct ChannelShared {
    pub(crate) id: ChannelId,
    pub(crate) kind: ChannelKind,
    pub(crate) config_version: u64,
    pub(crate) started_at: Timestamp,
    /// Whether this instance runs (an instance created stopped never started).
    ran: bool,
    /// The definition and the configuration version it came from, updated in place for
    /// changes that need no restart.
    definition: std::sync::RwLock<(Arc<ChannelConfig>, u64)>,
    pub(crate) span: Span,
    /// Bridge sides A and B; empty for message channels.
    pub(crate) sides: Vec<SideCell>,
    /// Bridge buffers A→B and B→A; empty for message channels.
    pub(crate) flows: Vec<Arc<FlowBuffer>>,
    /// Channel traffic (bridges: in = A→B, out = B→A).
    pub(crate) counters: Counters,
    /// Present when a tap framer is configured.
    pub(crate) tap: Option<TapCounters>,
    /// Present for message channels.
    pub(crate) messages: Option<MessageCounters>,
    stopping: AtomicBool,
    stop_cause: Mutex<Option<StopCause>>,
    /// How many recent error summaries are kept (`diagnostics.retain_recent_errors`).
    errors_kept: AtomicUsize,
    status: Mutex<Status>,
    notify: Arc<Notify>,
}

impl ChannelShared {
    /// State for a new instance of `channel`.
    pub(crate) fn new(
        channel: &ChannelConfig,
        sides: Vec<SideCell>,
        flows: Vec<Arc<FlowBuffer>>,
        tap: bool,
        config_version: u64,
        notify: Arc<Notify>,
    ) -> Self {
        let id = channel.id();
        let now = Timestamp::now();
        Self {
            id,
            kind: channel.kind(),
            config_version,
            started_at: now,
            ran: true,
            definition: std::sync::RwLock::new((Arc::new(channel.clone()), config_version)),
            span: tracing::info_span!(
                "channel",
                channel_id = %id,
                channel_kind = channel.kind().as_str()
            ),
            sides,
            flows,
            counters: Counters::new(),
            tap: tap.then(TapCounters::default),
            messages: (channel.kind() == ChannelKind::Message).then(MessageCounters::default),
            stopping: AtomicBool::new(false),
            stop_cause: Mutex::new(None),
            errors_kept: AtomicUsize::new(RECENT_ERRORS),
            status: Mutex::new(Status {
                phase: Phase::Starting,
                stop_reason: None,
                published: ChannelState::Starting,
                since: now,
                session: SessionState::Closed,
                session_since: now,
                retry_count: 0,
                next_retry_at: None,
                recent_errors: VecDeque::new(),
            }),
            notify,
        }
    }

    /// A message channel committed a received message: its counters and the channel's
    /// traffic (`traffic.bytes_in`, `messages_in`, `last_activity`).
    pub(crate) fn message_received(&self, bytes: usize) {
        if let Some(messages) = &self.messages {
            messages.committed(bytes);
        }
        self.counters.bytes_in(bytes);
        self.counters.message_in();
    }

    /// A message channel destination wrote a whole message (`traffic.bytes_out`,
    /// `messages_out`, `last_activity`).
    pub(crate) fn message_sent(&self, bytes: usize) {
        self.counters.bytes_out(bytes);
        self.counters.message_out();
    }

    /// An instance that never runs: stopped with `reason` from the start.
    pub(crate) fn stopped(
        channel: &ChannelConfig,
        sides: Vec<SideCell>,
        reason: StopReason,
        config_version: u64,
        notify: Arc<Notify>,
    ) -> Self {
        let mut shared = Self::new(channel, sides, Vec::new(), false, config_version, notify);
        shared.ran = false;
        {
            let mut status = shared.lock();
            status.phase = Phase::Stopped;
            status.stop_reason = Some(reason);
            status.published = ChannelState::Stopped;
        }
        shared.stopping.store(true, Ordering::Relaxed);
        shared
    }

    /// The current definition.
    pub(crate) fn definition(&self) -> Arc<ChannelConfig> {
        self.versioned_definition().0
    }

    /// The current definition with the configuration version it came from.
    pub(crate) fn versioned_definition(&self) -> (Arc<ChannelConfig>, u64) {
        self.definition
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Applies an in-place change (label, tags, filters, mappings, delivery policy).
    pub(crate) fn update_definition(&self, channel: ChannelConfig, version: u64) {
        *self
            .definition
            .write()
            .unwrap_or_else(PoisonError::into_inner) = (Arc::new(channel), version);
        self.span.in_scope(|| {
            tracing::info!(
                event = "channel.updated",
                "Channel {} updated in place",
                self.id
            );
        });
        self.notify();
    }

    /// The cell of `side`.
    pub(crate) fn side(&self, side: BridgeSide) -> Option<&SideCell> {
        self.cell(Role::Side(side))
    }

    /// The cell of `role`.
    pub(crate) fn cell(&self, role: Role) -> Option<&SideCell> {
        self.sides.iter().find(|c| c.role == role)
    }

    /// The buffer of the direction that starts at `from`.
    pub(crate) fn flow(&self, from: BridgeSide) -> Option<&Arc<FlowBuffer>> {
        self.flows.get(side_index(from))
    }

    /// Whether a stop has begun; failures after that are part of the stop.
    pub(crate) fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
    }

    /// Marks the start of a stop.
    pub(crate) fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
    }

    /// Marks the start of a stop and records why (the first cause wins).
    pub(crate) fn begin_stop_for(&self, cause: StopCause) {
        lock(&self.stop_cause).get_or_insert(cause);
        self.begin_stop();
    }

    /// The `reason_code` a delivery interrupted by this stop records.
    pub(crate) fn stop_reason_code(&self) -> &'static str {
        match *lock(&self.stop_cause) {
            Some(StopCause::Reason(StopReason::OperatorPaused)) => "channel_paused",
            Some(StopCause::Reason(StopReason::Shutdown)) => "engine_stopped",
            _ => "channel_stopped",
        }
    }

    /// The published channel state.
    pub(crate) fn state(&self) -> ChannelState {
        self.lock().published
    }

    /// The stop reason, when stopped.
    pub(crate) fn stop_reason(&self) -> Option<StopReason> {
        self.lock().stop_reason
    }

    /// Sets the supervisor phase.
    pub(crate) fn set_phase(&self, phase: Phase) {
        if self.is_stopping() && phase != Phase::Stopped {
            return;
        }
        self.lock().phase = phase;
        self.refresh();
    }

    /// Sets the session state.
    pub(crate) fn set_session(&self, session: SessionState) {
        let changed = {
            let mut status = self.lock();
            let changed = std::mem::replace(&mut status.session, session) != session;
            if changed {
                status.session_since = Timestamp::now();
            }
            changed
        };
        if changed {
            self.notify();
        }
    }

    /// Recomputes the published state from phase and buffer health; logs and notifies
    /// when it changed.
    pub(crate) fn refresh(&self) {
        let unhealthy = match self.kind {
            ChannelKind::Bridge => self.flows.iter().any(|f| f.is_overflowing()),
            ChannelKind::Message => !self.message_ready(),
        };
        let source_ready = self.kind == ChannelKind::Bridge || self.source_ready();
        let (state, session) = {
            let mut status = self.lock();
            let state = match status.phase {
                Phase::Stopped => ChannelState::Stopped,
                // A message channel runs while its source is ready (`diagnostics.md` §4.3).
                _ if self.kind == ChannelKind::Message && source_ready && unhealthy => {
                    ChannelState::Degraded
                }
                _ if self.kind == ChannelKind::Message && source_ready => ChannelState::Running,
                Phase::Starting => ChannelState::Starting,
                Phase::Running if unhealthy => ChannelState::Degraded,
                Phase::Running => ChannelState::Running,
                Phase::Retrying => ChannelState::Retrying,
            };
            if status.published == state {
                return;
            }
            status.published = state;
            status.since = Timestamp::now();
            (state, status.session)
        };
        self.span.in_scope(|| {
            tracing::info!(
                event = "channel.state_changed",
                channel_state = state.as_str(),
                session_state = session.as_str(),
                "Channel {} is {state}",
                self.id
            );
        });
        self.notify();
    }

    /// The source endpoint is connected or listening.
    fn source_ready(&self) -> bool {
        self.cell(Role::Source)
            .is_some_and(|c| side::ready(c.view().state))
    }

    /// Every destination endpoint is ready and the store accepts commits.
    fn message_ready(&self) -> bool {
        let destinations_ready = self
            .sides
            .iter()
            .filter(|c| matches!(c.role, Role::Destination(_)))
            .all(|c| side::ready(c.view().state));
        destinations_ready
            && self
                .messages
                .as_ref()
                .is_none_or(|m| !m.store_unavailable.load(Ordering::Relaxed))
    }

    /// Records an operator-facing failure on the channel.
    pub(crate) fn record_error(&self, summary: &ErrorSummary) {
        self.counters.error(summary.clone());
        let kept = self.errors_kept.load(Ordering::Relaxed);
        let mut status = self.lock();
        status.recent_errors.push_back(summary.clone());
        while status.recent_errors.len() > kept {
            status.recent_errors.pop_front();
        }
    }

    /// Keeps the newest `kept` error summaries from now on (`diagnostics.retain_recent_errors`
    /// of the active document); a lower value drops the oldest at once.
    pub(crate) fn keep_recent_errors(&self, kept: usize) {
        self.errors_kept.store(kept, Ordering::Relaxed);
        let mut status = self.lock();
        while status.recent_errors.len() > kept {
            status.recent_errors.pop_front();
        }
    }

    /// Records an endpoint failure with its next action on the channel and on the
    /// endpoint's engine-lifetime `counters`; returns the recorded summary.
    pub(crate) fn record_endpoint_error(
        &self,
        counters: &Counters,
        summary: String,
        next_action: &str,
    ) -> ErrorSummary {
        let error = ErrorSummary::new(summary, Some(next_action));
        self.record_error(&error);
        counters.error(error.clone());
        error
    }

    /// Increments the retry counter and sets the next retry time; returns the count.
    pub(crate) fn schedule_retry(&self, next_retry_at: Option<Timestamp>) -> u32 {
        let mut status = self.lock();
        status.retry_count = status.retry_count.saturating_add(1);
        status.next_retry_at = next_retry_at;
        status.retry_count
    }

    /// Scheduled recovery cycles since the session was last stable.
    pub(crate) fn retry_count(&self) -> u32 {
        self.lock().retry_count
    }

    /// When the next retry starts, while one waits.
    pub(crate) fn next_retry_at(&self) -> Option<Timestamp> {
        self.lock().next_retry_at
    }

    /// The next retry time no longer applies (an attempt starts).
    pub(crate) fn clear_next_retry(&self) {
        self.lock().next_retry_at = None;
    }

    /// The session is stable: the retry counter and the failure streaks reset. Returns
    /// the sides whose streak was cleared.
    pub(crate) fn session_stable(&self) -> Vec<Role> {
        {
            let mut status = self.lock();
            status.retry_count = 0;
            status.next_retry_at = None;
        }
        let cleared = self
            .sides
            .iter()
            .filter(|cell| cell.clear_failures())
            .map(|cell| cell.role)
            .collect();
        self.notify();
        cleared
    }

    /// Final state after a stop: endpoints `disconnected`, session closed, retry state
    /// cleared. Logs `channel.stopped`.
    pub(crate) fn mark_stopped(&self, cause: StopCause) {
        self.begin_stop();
        for cell in &self.sides {
            self.set_endpoint(cell.role, EndpointState::Disconnected);
        }
        {
            let mut status = self.lock();
            status.phase = Phase::Stopped;
            status.stop_reason = match cause {
                StopCause::Reason(reason) => Some(reason),
                StopCause::OperatorRestart => None,
            };
            status.session = SessionState::Closed;
            status.retry_count = 0;
            status.next_retry_at = None;
        }
        for flow in &self.flows {
            flow.reset();
        }
        self.refresh();
        self.span.in_scope(|| {
            tracing::info!(
                event = "channel.stopped",
                stop_reason = cause.as_str(),
                "Channel {} stopped ({})",
                self.id,
                cause.as_str()
            );
        });
        self.notify();
    }

    /// Wakes the snapshot publisher.
    pub(crate) fn notify(&self) {
        self.notify.notify_one();
    }

    fn lock(&self) -> MutexGuard<'_, Status> {
        lock(&self.status)
    }
}

/// Index of the direction that starts at `side` (0 = A→B, 1 = B→A).
pub(crate) const fn side_index(side: BridgeSide) -> usize {
    match side {
        BridgeSide::A => 0,
        BridgeSide::B => 1,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The time of Unix milliseconds stored in an atomic; `None` for 0 (never).
fn millis_to_time(ms: i64) -> Option<Timestamp> {
    (ms != 0).then(|| Timestamp::from_unix_millis(ms)).flatten()
}
