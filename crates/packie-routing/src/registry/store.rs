//! The message store's lifecycle under the registry (opening again after a failed
//! start, startup reconciliation, retention) and the operator's message commands.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use packie_config::{ChannelConfig, ProtocolSettings};
use packie_core::{ChannelId, ConnectorId, StopReason, Timestamp};
use packie_engine::RegistryError;
use packie_ipc::messages::{
    CloseRequest, DeliveryRecord, RejectionCode, ResendRequest, ResendResult,
};

use super::{Control, Inner, RoutingRegistry, message_runner, rejected};
use crate::message::store::{RetentionPolicy, StoreError};
use crate::message::{MessageStore, pipeline};
use crate::store_open::{OpenedStore, StoreOpener, StoreRetry};

/// A store failure as a command result.
pub(super) fn store_error(error: StoreError) -> RegistryError {
    match error {
        StoreError::Rejected { code, message } => rejected(code, &message),
        StoreError::NotFound => rejected(RejectionCode::NotFound, "No such message or delivery."),
        other => RegistryError::Failed(other.to_string()),
    }
}

/// How often retention runs (`message-channels.md` §10).
const RETENTION_INTERVAL: Duration = Duration::from_secs(600);

impl RoutingRegistry {
    /// Purges resolved history by each channel's retention: once after startup
    /// reconciliation, then every ten minutes, while the registry lives.
    pub(super) fn schedule_retention(&self) {
        if self.inner.store_handle().is_none() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(retention_loop(Arc::downgrade(&self.inner)));
    }
}

/// Waits for startup reconciliation (trying a failed one again with the store-reopen
/// backoff) and the first configuration, then purges every [`RETENTION_INTERVAL`] until
/// the registry is dropped.
async fn retention_loop(registry: std::sync::Weak<Inner>) {
    let mut pacing = RetryPacing::default();
    loop {
        let Some(inner) = registry.upgrade() else {
            return;
        };
        let configured = inner.view().config.config_version > 0;
        let wait = if inner.reconciled.load(Ordering::Relaxed) && configured {
            inner.purge().await;
            RETENTION_INTERVAL
        } else {
            if inner.reconcile_failed.load(Ordering::Relaxed)
                && pacing.due(inner.options.store_retry)
            {
                inner.retry_reconciliation().await;
            }
            Duration::from_millis(200)
        };
        drop(inner);
        tokio::time::sleep(wait).await;
    }
}

/// When a failed reconciliation is tried again: after `initial`, doubling up to `max`.
#[derive(Default)]
struct RetryPacing {
    next: Option<tokio::time::Instant>,
    delay: Duration,
}

impl RetryPacing {
    /// Whether an attempt is due now; schedules the one after it.
    fn due(&mut self, retry: StoreRetry) -> bool {
        let now = tokio::time::Instant::now();
        match self.next {
            Some(at) if now < at => false,
            Some(_) => {
                self.delay = self.delay.saturating_mul(2).min(retry.max);
                self.next = Some(now + self.delay);
                true
            }
            None => {
                self.delay = retry.initial;
                self.next = Some(now + self.delay);
                false
            }
        }
    }
}

impl RoutingRegistry {
    /// Retries opening the store when it could not be opened at start.
    pub(super) fn schedule_store_reopen(&self) {
        let Some(opener) = self.inner.options.store_opener.clone() else {
            return;
        };
        if !self.inner.store_unavailable.load(Ordering::Relaxed) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let retry = self.inner.options.store_retry;
        runtime.spawn(reopen_loop(Arc::downgrade(&self.inner), opener, retry));
    }
}

/// Opens the store again after `retry.initial`, doubling the wait up to `retry.max`,
/// until it opens or the registry is gone.
async fn reopen_loop(registry: std::sync::Weak<Inner>, opener: StoreOpener, retry: StoreRetry) {
    let mut delay = retry.initial;
    loop {
        tokio::time::sleep(delay).await;
        let Some(inner) = registry.upgrade() else {
            return;
        };
        if inner.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        let attempt = Arc::clone(&opener);
        let opened = tokio::task::spawn_blocking(move || attempt())
            .await
            .unwrap_or_else(|_| Err("the attempt to open the store failed".to_owned()));
        delay = delay.saturating_mul(2).min(retry.max);
        match opened {
            Ok(opened) => {
                inner.store_opened(opened).await;
                let registry = RoutingRegistry { inner };
                registry.schedule_retention();
                return;
            }
            Err(error) => tracing::warn!(
                event = "store.error",
                error_code = "open_failed",
                next_attempt_seconds = delay.as_secs(),
                "The message store still cannot be opened: {error}; next attempt in {}",
                crate::retry::describe_delay(delay)
            ),
        }
    }
}

impl Inner {
    /// The store opened after failing at start: install it, reconcile, then start the
    /// message channels that waited for it. Bridges keep running; their taps start
    /// recording.
    pub(super) async fn store_opened(&self, opened: OpenedStore) {
        let mut control = self.control.lock().await;
        if self.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        *self
            .message_runner
            .write()
            .unwrap_or_else(PoisonError::into_inner) =
            Some(message_runner(&opened.store, &self.wakeups));
        *self.store.write().unwrap_or_else(PoisonError::into_inner) = Some(opened.store);
        if let (Some(deferred), Some(sink)) = (&self.deferred_tap, opened.tap_sink) {
            deferred.connect(sink);
        }
        self.store_unavailable.store(false, Ordering::Relaxed);
        tracing::info!(
            event = "store.opened",
            "The message store opened; message channels start"
        );
        self.reconcile_once().await;
        self.start_waiting(&mut control);
        self.publish(&control);
    }

    /// A failed startup reconciliation, tried again: when it completes, the message
    /// channels that waited for it start.
    pub(super) async fn retry_reconciliation(&self) {
        let mut control = self.control.lock().await;
        if self.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        if self.reconcile_once().await {
            self.start_waiting(&mut control);
            self.publish(&control);
        }
    }

    /// Starts again the channels stopped because the store was not ready.
    fn start_waiting(&self, control: &mut Control) {
        let waiting: Vec<ChannelId> = control
            .slots
            .iter()
            .filter(|(_, slot)| slot.shared.stop_reason() == Some(StopReason::StoreUnavailable))
            .map(|(id, _)| *id)
            .collect();
        for id in waiting {
            control.slots.remove(&id);
            self.add(control, id);
        }
    }

    /// Startup reconciliation (`message-channels.md` §13), before any message channel
    /// starts; `true` once it has completed. A failure is logged, leaves message channels
    /// stopped (`store_unavailable`) and is tried again by the retention loop. Callers
    /// hold the control lock, so attempts never overlap. Messages left `received` run
    /// through their channel's pipeline as soon as it runs.
    pub(super) async fn reconcile_once(&self) -> bool {
        let Some(store) = self.store_handle() else {
            return false;
        };
        if self.reconciled.load(Ordering::Relaxed) {
            return true;
        }
        let config = Arc::clone(&self.view().config);
        store.set_astm_sample_positions(
            config
                .channels
                .iter()
                .filter_map(|c| match c {
                    ChannelConfig::Message(m) => match &m.source.protocol {
                        ProtocolSettings::Astm(settings) => {
                            Some((m.id, crate::message::astm::sample_position(settings)))
                        }
                        _ => None,
                    },
                    ChannelConfig::Bridge(_) => None,
                })
                .collect(),
        );
        let report = match store.reconcile_at_startup().await {
            Ok(report) => report,
            Err(error) => {
                self.reconcile_failed.store(true, Ordering::Relaxed);
                tracing::error!(
                    event = "store.error",
                    error_code = "reconciliation_failed",
                    "Startup reconciliation failed: {error}; message channels wait while it \
                     is tried again"
                );
                return false;
            }
        };
        self.reconciled.store(true, Ordering::Relaxed);
        self.reconcile_failed.store(false, Ordering::Relaxed);
        for message in &report.requeued {
            if let Ok(stored) = store.stored_message(*message).await {
                self.wakeups.run_pipeline(stored.channel_id, *message);
            }
        }
        tracing::info!(
            event = "reconciliation.completed",
            recovered = report.recovered,
            aborted = report.aborted,
            requeued = report.requeued.len(),
            unknown = report.unknown,
            "Startup reconciliation: {} recovered, {} aborted, {} requeued, {} outcome unknown",
            report.recovered,
            report.aborted,
            report.requeued.len(),
            report.unknown
        );
        true
    }

    /// One retention run over the configured channels' policies: message channels' own,
    /// bridges' tap retention.
    pub(super) async fn purge(&self) {
        let Some(store) = self.store_handle() else {
            return;
        };
        let config = Arc::clone(&self.view().config);
        let policies = config
            .channels
            .iter()
            .map(|c| match c {
                ChannelConfig::Message(m) => RetentionPolicy {
                    channel_id: m.id,
                    max_age_days: m.retention.max_age_days,
                    max_messages: m.retention.max_messages,
                },
                // A bridge's stored messages are what its tap recorded.
                ChannelConfig::Bridge(b) => RetentionPolicy {
                    channel_id: b.id,
                    max_age_days: b.tap.retention.max_age_days,
                    max_messages: b.tap.retention.max_messages,
                },
            })
            .collect();
        if let Err(error) = store.purge(policies).await {
            tracing::error!(
                event = "store.error",
                error_code = "retention_failed",
                "Retention failed: {error}"
            );
        }
    }

    pub(super) async fn cancel_unresolved(
        &self,
        channel: ChannelId,
        destination: Option<ConnectorId>,
        reason: &'static str,
    ) {
        let Some(store) = self.store_handle() else {
            return;
        };
        if let Err(error) = store.cancel_unresolved(channel, destination, reason).await {
            tracing::error!(event = "store.error", channel_id = %channel, error_code = "cancel_failed", "{error}");
        }
    }

    /// Operator resend (`message-channels.md` §9).
    pub(super) async fn resend(
        &self,
        request: ResendRequest,
    ) -> Result<ResendResult, RegistryError> {
        let control = self.control.lock().await;
        let store = self.store()?;
        let message = request.message_id;
        let stored = store.stored_message(message).await.map_err(store_error)?;
        let Some(ChannelConfig::Message(channel)) = control.config.channel(stored.channel_id)
        else {
            return Err(rejected(
                RejectionCode::NotFound,
                "The message's channel is no longer configured.",
            ));
        };
        let active = pipeline::active_policies(channel);
        let result = store
            .resend(
                request,
                active,
                control.config.config_version,
                Timestamp::now(),
            )
            .await
            .map_err(store_error)?;
        for outcome in result
            .destinations
            .iter()
            .filter(|d| d.delivery_id.is_some())
        {
            self.wakeups.wake(channel.id, outcome.destination_id);
        }
        if let Some(new_message) = result.new_message_id {
            self.wakeups.run_pipeline(channel.id, new_message);
        }
        Ok(result)
    }

    /// Operator close of a delivery (`message-channels.md` §9).
    pub(super) async fn close_delivery(
        &self,
        request: CloseRequest,
    ) -> Result<DeliveryRecord, RegistryError> {
        let _control = self.control.lock().await;
        let store = self.store()?;
        let record = store
            .close_delivery(request, Timestamp::now())
            .await
            .map_err(store_error)?;
        self.wakeups.wake(record.channel_id, record.destination_id);
        Ok(record)
    }

    pub(super) fn store(&self) -> Result<Arc<dyn MessageStore>, RegistryError> {
        if self.store_unavailable.load(Ordering::Relaxed) {
            return Err(RegistryError::Failed(
                "The message store could not be opened; the engine keeps trying. Check free \
                 disk space and the permissions of the state directory."
                    .into(),
            ));
        }
        self.store_handle().ok_or_else(|| {
            RegistryError::NotImplemented("This engine runs without a message store.".into())
        })
    }

    /// The message store, once open.
    pub(super) fn store_handle(&self) -> Option<Arc<dyn MessageStore>> {
        self.store
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
