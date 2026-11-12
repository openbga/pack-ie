//! Channel slots: starting, stopping and replacing instances, operator commands and
//! shutdown (`docs/spec/bridge-channels.md` §12–§14, `message-channels.md` §11–§12).

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use packie_config::{ChannelConfig, RuntimeConfig, TapFramer, Transport};
use packie_core::{
    BridgeSide, ChannelId, ChannelKind, ChannelState, EndpointId, StopReason, Timestamp,
};
use packie_engine::RegistryError;
use packie_ipc::ChannelCommand;
use packie_ipc::messages::RejectionCode;

use super::{Control, Inner, Running, STOP_GRACE, Slot, TAKEOVER_LIMIT, rejected};
use crate::buffer::{FlowBuffer, FlowLimits};
use crate::runner::{self, ChannelRunner, RunContext};
use crate::status::{ChannelShared, Role, SideCell, StopCause};
use crate::stop::{self, StopMode};

impl Inner {
    /// A change confined to label, tags and (message channels) filters, mappings,
    /// delivery policy and retention: the running instance picks it up between messages.
    pub(super) async fn update_in_place(
        &self,
        control: &mut Control,
        old: &RuntimeConfig,
        id: ChannelId,
    ) {
        let Some(channel) = control.config.channel(id).cloned() else {
            return;
        };
        self.cancel_removed_destinations(old, &channel).await;
        match control.slots.get(&id) {
            Some(slot) => slot
                .shared
                .update_definition(channel, control.config.config_version),
            None => self.add(control, id),
        }
    }

    /// Cancels the unresolved deliveries of destinations `channel` no longer has.
    pub(super) async fn cancel_removed_destinations(
        &self,
        old: &RuntimeConfig,
        channel: &ChannelConfig,
    ) {
        let id = channel.id();
        let (Some(ChannelConfig::Message(before)), ChannelConfig::Message(after)) =
            (old.channel(id), channel)
        else {
            return;
        };
        for removed in before
            .destinations
            .iter()
            .filter(|d| !after.destinations.iter().any(|a| a.id == d.id))
        {
            self.cancel_unresolved(id, Some(removed.id), "destination_removed")
                .await;
        }
    }

    /// Stops `slot`: bridges at once, message channels draining in-flight work within
    /// the drain budget (`message-channels.md` §11).
    pub(super) async fn stop(&self, slot: Slot, cause: StopCause) {
        let mode = if slot.shared.kind == ChannelKind::Message {
            StopMode::Drain(tokio::time::Instant::now() + self.drain())
        } else {
            StopMode::Now
        };
        stop_slot(slot, cause, mode).await;
    }

    pub(super) fn drain(&self) -> Duration {
        Duration::from_secs(u64::from(
            self.context.bootstrap.hosting.shutdown_drain_seconds,
        ))
    }

    pub(super) fn add(&self, control: &mut Control, id: ChannelId) {
        let slot = self.start(control, id);
        if let Some(slot) = slot {
            control.slots.insert(id, slot);
        }
    }

    pub(super) async fn remove(&self, control: &mut Control, id: ChannelId) {
        if let Some(slot) = control.slots.remove(&id) {
            let message = slot.shared.kind == ChannelKind::Message;
            self.stop(slot, StopCause::Reason(StopReason::RemovedByConfig))
                .await;
            if message {
                self.cancel_unresolved(id, None, "channel_removed").await;
            }
        }
        self.context.tap.remove(id);
    }

    pub(super) async fn change(&self, control: &mut Control, old: &RuntimeConfig, id: ChannelId) {
        let new = Arc::clone(&control.config);
        let Some(channel) = new.channel(id) else {
            return;
        };
        self.cancel_removed_destinations(old, channel).await;
        let Some(slot) = control.slots.remove(&id) else {
            self.add(control, id);
            return;
        };
        let exclusive = old.channel(id).is_some_and(|c| uses_exclusive(old, c))
            || uses_exclusive(&new, channel);
        let working = matches!(
            slot.shared.state(),
            ChannelState::Running | ChannelState::Degraded
        );
        // Message channels are always replaced break-before-make (`message-channels.md` §12).
        let message = channel.kind() == ChannelKind::Message;
        if slot.running.is_none() || exclusive || message || !working || !channel.enabled() {
            // Break before make: release the port or socket, then start the new one.
            self.stop(slot, StopCause::Reason(StopReason::ReplacedByConfig))
                .await;
            self.add(control, id);
            return;
        }
        // Make before break: the replacement takes over once paired.
        let Some(replacement) = self.start(control, id) else {
            self.stop(slot, StopCause::Reason(StopReason::ReplacedByConfig))
                .await;
            return;
        };
        let limit = takeover_limit(&new, channel);
        if !wait_until_working(&replacement.shared, limit).await {
            tracing::warn!(
                event = "config.replacement_not_ready",
                channel_id = %id,
                "Channel {id}: the replacement did not pair within {}s; it takes over anyway",
                limit.as_secs()
            );
        }
        self.stop(slot, StopCause::Reason(StopReason::ReplacedByConfig))
            .await;
        control.slots.insert(id, replacement);
    }

    /// A new slot for channel `id` of the active document: running when it can run,
    /// otherwise stopped with the reason.
    pub(super) fn start(&self, control: &Control, id: ChannelId) -> Option<Slot> {
        let config = Arc::clone(&control.config);
        let channel = config.channel(id)?;
        let sides = sides_of(&config, channel);
        let version = config.config_version;
        let notify = Arc::clone(&self.context.snapshot_now);
        let stopped_reason = if !channel.enabled() {
            Some(StopReason::DisabledByConfig)
        } else if control.paused.contains(id) {
            Some(StopReason::OperatorPaused)
        } else if self.shutting_down.load(Ordering::Relaxed) {
            Some(StopReason::Shutdown)
        } else if channel.kind() == ChannelKind::Message
            && self.store_handle().is_some()
            && !self.reconciled.load(Ordering::Relaxed)
        {
            // Reconciliation runs before any source opens (`message-channels.md` §13).
            Some(StopReason::StoreUnavailable)
        } else if self.runner_for(channel.kind()).is_none() {
            Some(if self.store_unavailable.load(Ordering::Relaxed) {
                StopReason::StoreUnavailable
            } else {
                StopReason::NotImplemented
            })
        } else {
            None
        };
        if let Some(reason) = stopped_reason {
            let shared = ChannelShared::stopped(channel, sides, reason, version, notify);
            shared.keep_recent_errors(config.retain_recent_errors(&self.context.bootstrap));
            return Some(Slot {
                shared: Arc::new(shared),
                running: None,
            });
        }
        let runner = self.runner_for(channel.kind())?;
        let (flows, tap) = match channel {
            ChannelConfig::Bridge(bridge) => {
                let limits = FlowLimits::new(&bridge.buffer);
                let flows = vec![
                    Arc::new(FlowBuffer::new(limits)),
                    Arc::new(FlowBuffer::new(limits)),
                ];
                (flows, bridge.tap.framer != TapFramer::None)
            }
            ChannelConfig::Message(_) => (Vec::new(), false),
        };
        let shared = Arc::new(ChannelShared::new(
            channel, sides, flows, tap, version, notify,
        ));
        shared.keep_recent_errors(config.retain_recent_errors(&self.context.bootstrap));
        let (stop_tx, stop_rx) = stop::channel();
        let cx = RunContext {
            shared: Arc::clone(&shared),
            channel: channel.clone(),
            config: Arc::clone(&config),
            transports: Arc::clone(&self.options.transports),
            endpoint_counters: Arc::clone(&control.endpoint_counters),
            traffic_tap: self.context.tap.clone(),
            tap_sink: self.options.tap_sink.clone(),
            stop: stop_rx,
        };
        let task = runner::spawn(runner, cx);
        shared.notify();
        Some(Slot {
            shared,
            running: Some(Running {
                stop: stop_tx,
                task,
            }),
        })
    }

    pub(super) fn runner_for(&self, kind: ChannelKind) -> Option<Arc<dyn ChannelRunner>> {
        match kind {
            ChannelKind::Bridge => Some(Arc::clone(&self.bridge_runner)),
            ChannelKind::Message => self
                .message_runner
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    }

    pub(super) async fn command(
        &self,
        id: ChannelId,
        command: ChannelCommand,
    ) -> Result<(), RegistryError> {
        let mut control = self.control.lock().await;
        let config = Arc::clone(&control.config);
        let channel = config.channel(id).ok_or(RegistryError::NotFound(id))?;
        let result = match command {
            ChannelCommand::Pause => self.pause(&mut control, channel).await,
            ChannelCommand::Resume => self.resume(&mut control, channel),
            ChannelCommand::Restart => self.restart(&mut control, channel).await,
        };
        self.publish(&control);
        result
    }

    pub(super) async fn pause(
        &self,
        control: &mut Control,
        channel: &ChannelConfig,
    ) -> Result<(), RegistryError> {
        let id = channel.id();
        if !channel.enabled() {
            return Err(disabled_in_config());
        }
        let Some(slot) = control.slots.remove(&id) else {
            return Err(RegistryError::NotFound(id));
        };
        if slot.running.is_none() || slot.shared.state() == ChannelState::Stopped {
            control.slots.insert(id, slot);
            return Err(rejected(
                RejectionCode::AlreadyStopped,
                "Channel is already stopped.",
            ));
        }
        let shared = Arc::clone(&slot.shared);
        self.stop(slot, StopCause::Reason(StopReason::OperatorPaused))
            .await;
        control.slots.insert(
            id,
            Slot {
                shared,
                running: None,
            },
        );
        control.paused.insert(id);
        Ok(())
    }

    pub(super) fn resume(
        &self,
        control: &mut Control,
        channel: &ChannelConfig,
    ) -> Result<(), RegistryError> {
        let id = channel.id();
        if !channel.enabled() {
            return Err(disabled_in_config());
        }
        let paused = control
            .slots
            .get(&id)
            .is_some_and(|s| s.shared.stop_reason() == Some(StopReason::OperatorPaused));
        if !paused {
            return Err(rejected(RejectionCode::NotPaused, "Not paused."));
        }
        if self.shutting_down.load(Ordering::Relaxed) {
            return Err(shutting_down());
        }
        control.paused.remove(id);
        self.add(control, id);
        Ok(())
    }

    pub(super) async fn restart(
        &self,
        control: &mut Control,
        channel: &ChannelConfig,
    ) -> Result<(), RegistryError> {
        let id = channel.id();
        if control.paused.contains(id) {
            return Err(rejected(
                RejectionCode::OperatorPaused,
                "Channel is paused by an operator: use Resume.",
            ));
        }
        if !channel.enabled() {
            return Err(disabled_in_config());
        }
        if self.runner_for(channel.kind()).is_none() {
            return Err(RegistryError::NotImplemented(
                "This build of the engine does not run message channels yet.".into(),
            ));
        }
        if self.shutting_down.load(Ordering::Relaxed) {
            return Err(shutting_down());
        }
        if let Some(slot) = control.slots.remove(&id) {
            self.stop(slot, StopCause::OperatorRestart).await;
        }
        self.add(control, id);
        Ok(())
    }

    pub(super) async fn shutdown(&self, drain: Duration) {
        self.shutting_down.store(true, Ordering::Relaxed);
        let mut control = self.control.lock().await;
        let deadline = tokio::time::Instant::now() + drain;
        let mut stopping = Vec::new();
        for slot in control.slots.values_mut() {
            if let Some(running) = slot.running.take() {
                slot.shared
                    .begin_stop_for(StopCause::Reason(StopReason::Shutdown));
                let _ = running.stop.send(Some(StopMode::Drain(deadline)));
                stopping.push((Arc::clone(&slot.shared), running.task));
            }
        }
        // Every channel drains in parallel; each gets the same budget.
        for (shared, mut task) in stopping {
            if tokio::time::timeout_at(deadline + STOP_GRACE, &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
            shared.mark_stopped(StopCause::Reason(StopReason::Shutdown));
        }
        self.publish(&control);
    }
}

fn disabled_in_config() -> RegistryError {
    rejected(
        RejectionCode::DisabledInConfig,
        "Disabled in configuration: enable it in the configuration editor.",
    )
}

fn shutting_down() -> RegistryError {
    RegistryError::Failed("The engine is shutting down.".into())
}

/// Stops the instance in `slot` (if it runs) and records the stop.
async fn stop_slot(slot: Slot, cause: StopCause, mode: StopMode) {
    let Some(mut running) = slot.running else {
        return;
    };
    slot.shared.begin_stop_for(cause);
    let _ = running.stop.send(Some(mode));
    let limit = match mode {
        StopMode::Now => tokio::time::Instant::now() + STOP_GRACE,
        StopMode::Drain(deadline) => deadline + STOP_GRACE,
    };
    if tokio::time::timeout_at(limit, &mut running.task)
        .await
        .is_err()
    {
        running.task.abort();
    }
    slot.shared.mark_stopped(cause);
}

async fn wait_until_working(shared: &ChannelShared, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if matches!(
            shared.state(),
            ChannelState::Running | ChannelState::Degraded
        ) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// `pairing_timeout_ms` plus the longest `connect_timeout_ms` plus one second, at most
/// 15 s.
fn takeover_limit(config: &RuntimeConfig, channel: &ChannelConfig) -> Duration {
    let ChannelConfig::Bridge(bridge) = channel else {
        return TAKEOVER_LIMIT;
    };
    let connect = channel
        .endpoint_ids()
        .iter()
        .filter_map(|id| config.endpoint(*id))
        .filter_map(|e| match &e.transport {
            Transport::TcpClient(s) => Some(s.connect_timeout_ms),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let total = u64::from(bridge.connection.pairing_timeout_ms) + u64::from(connect) + 1000;
    Duration::from_millis(total).min(TAKEOVER_LIMIT)
}

/// Whether `channel` uses a serial port or a listener socket.
fn uses_exclusive(config: &RuntimeConfig, channel: &ChannelConfig) -> bool {
    channel.endpoint_ids().iter().any(|id| {
        config
            .endpoint(*id)
            .is_some_and(|e| is_exclusive(&e.transport))
    })
}

/// A serial port or a listener socket: one channel instance holds it at a time.
pub(super) const fn is_exclusive(transport: &Transport) -> bool {
    matches!(transport, Transport::Serial(_) | Transport::TcpListener(_))
}

fn sides_of(config: &RuntimeConfig, channel: &ChannelConfig) -> Vec<SideCell> {
    let roles: Vec<(Role, EndpointId)> = match channel {
        ChannelConfig::Bridge(bridge) => vec![
            (Role::Side(BridgeSide::A), bridge.endpoint_a),
            (Role::Side(BridgeSide::B), bridge.endpoint_b),
        ],
        ChannelConfig::Message(message) => std::iter::once((Role::Source, message.source.endpoint))
            .chain(
                message
                    .destinations
                    .iter()
                    .filter(|d| d.enabled)
                    .map(|d| (Role::Destination(d.id), d.endpoint)),
            )
            .collect(),
    };
    roles
        .into_iter()
        .filter_map(|(role, id)| config.endpoint(id).map(|e| SideCell::new(role, e)))
        .collect()
}

/// Keeps counters of endpoints that remain, adds new ones, drops removed ones.
pub(super) fn refresh_endpoints(control: &mut Control, config: &RuntimeConfig) {
    let now = Timestamp::now();
    let counters = config
        .endpoints
        .iter()
        .map(|e| {
            let counters = control
                .endpoint_counters
                .get(&e.id)
                .cloned()
                .unwrap_or_default();
            (e.id, counters)
        })
        .collect();
    control.endpoint_counters = Arc::new(counters);
    control
        .endpoint_since
        .retain(|id, _| config.endpoint(*id).is_some());
    for endpoint in &config.endpoints {
        control.endpoint_since.entry(endpoint.id).or_insert(now);
    }
}
