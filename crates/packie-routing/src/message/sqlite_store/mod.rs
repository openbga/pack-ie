//! [`MessageStore`] over the durable SQLite store (`packie_store::Store`).
//!
//! Every method maps onto one store call; the store commits before it answers, so the
//! pipeline's ordering guarantees (commit before ACK, attempt record before the first
//! byte) hold. Store errors that mean "nothing was committed" (SQLite, I/O, writer
//! stopped) become [`StoreError::Unavailable`]. Type conversions live in `convert`, the
//! tap recorder in `tap`.

mod convert;
mod tap;

pub use tap::StoreTapSink;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use packie_core::{
    AckLevel, ChannelId, ConnectorId, DeliveryId, DeliveryState, MessageId, MessageState, Timestamp,
};
use packie_ipc::messages::{
    BodyRequest, BodySelector, CloseRequest, DeliveryRecord, MessageBody, MessageList,
    MessageQuery, MessageRecord, ResendRequest, ResendResult,
};
use packie_store::Store;
use packie_transport::BoxFuture;

use crate::message::astm;
use crate::message::store::{
    AttemptFinish, DestinationPolicy, Identifiers, MessageStore, NewDelivery, PendingDelivery,
    QueueCounts, Receipt, ReceiptOutcome, Reconciliation, RetentionPolicy, StoreResult,
    StoredMessage,
};
use convert::{destination, error, identifiers, outcome, queue_counts, receipt, render};

/// The SQLite store as the pipeline's [`MessageStore`].
///
/// Snapshots read every destination's queue counts and the store totals once a second.
/// Both only change through this type's writes, so they are cached against a count of
/// completed writes and read again only after one: an idle engine runs no count queries.
#[derive(Clone)]
pub struct SqliteStore {
    store: Store,
    figures: Arc<Figures>,
    /// Specimen id positions of the ASTM source channels, for recovered transfers.
    astm_samples: Arc<Mutex<HashMap<ChannelId, astm::SamplePosition>>>,
}

/// Cached snapshot figures and the write count they were read at.
#[derive(Default)]
struct Figures {
    /// Writes completed through this store.
    writes: AtomicU64,
    queues: Mutex<HashMap<(ChannelId, ConnectorId), (u64, QueueCounts)>>,
    totals: Mutex<Option<(u64, packie_store::StoreTotals)>>,
}

impl Figures {
    /// A write completed (whatever its outcome).
    fn wrote(&self) {
        self.writes.fetch_add(1, Ordering::AcqRel);
    }
}

impl std::fmt::Debug for SqliteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStore")
            .field("database", &self.store.database_path())
            .finish()
    }
}

impl SqliteStore {
    /// Wraps an opened store.
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self {
            store,
            figures: Arc::default(),
            astm_samples: Arc::default(),
        }
    }

    /// Runs a write and counts it once it completed (whatever its outcome), so a figure
    /// read before or during it is read again.
    fn write<'a, T: Send + 'a>(
        &'a self,
        write: impl Future<Output = T> + Send + 'a,
    ) -> BoxFuture<'a, T> {
        Box::pin(async move {
            let result = write.await;
            self.figures.wrote();
            result
        })
    }

    /// The write count to label a figure with; read before the figure is.
    fn writes(&self) -> u64 {
        self.figures.writes.load(Ordering::Acquire)
    }

    /// The sink that records bridge tap observations in this store.
    #[must_use]
    pub fn tap_sink(&self) -> Option<StoreTapSink> {
        StoreTapSink::new(self.store.clone(), Arc::clone(&self.figures))
    }

    /// Completes interrupted ASTM transfers whose committed frames end with the message
    /// terminator; returns how many. Other protocols' partial transfers stay for the
    /// store to abort.
    async fn recover_transfers(&self) -> u64 {
        let Ok(receiving) = self.store.receiving_messages() else {
            return 0;
        };
        let mut recovered = 0;
        for message in receiving {
            let Ok(record) = self.store.message(message) else {
                continue;
            };
            if record.summary.protocol != packie_core::ProtocolKind::Astm {
                continue;
            }
            let Ok(frames) = self.store.body(message, None) else {
                continue;
            };
            let Some(original) = crate::message::astm::recover(&frames) else {
                continue;
            };
            let sample = self
                .astm_samples
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&record.summary.channel_id)
                .copied()
                .unwrap_or(astm::STANDARD_SAMPLE);
            let ids = astm::identifiers(&original, sample).unwrap_or_default();
            let completed = self
                .store
                .complete_receiving(message, original, identifiers(ids), true)
                .await;
            recovered += u64::from(completed.is_ok());
        }
        recovered
    }

    /// The underlying store (maintenance, alerts, figures).
    #[must_use]
    pub const fn store(&self) -> &Store {
        &self.store
    }
}

/// A store error as the pipeline sees it.
impl MessageStore for SqliteStore {
    fn commit_received(
        &self,
        r: Receipt,
        bytes: Vec<u8>,
    ) -> BoxFuture<'_, StoreResult<ReceiptOutcome>> {
        self.write(async move {
            self.store
                .commit_received(receipt(r, MessageState::Received), bytes)
                .await
                .map(|o| outcome(&o))
                .map_err(error)
        })
    }

    fn commit_batch(
        &self,
        units: Vec<(Receipt, Vec<u8>)>,
    ) -> BoxFuture<'_, StoreResult<Vec<ReceiptOutcome>>> {
        self.write(async move {
            let units = units
                .into_iter()
                .map(|(r, bytes)| (receipt(r, MessageState::Received), bytes))
                .collect();
            self.store
                .commit_received_batch(units)
                .await
                .map(|outcomes| outcomes.iter().map(outcome).collect())
                .map_err(error)
        })
    }

    fn begin_transfer(&self, r: Receipt, frame: Vec<u8>) -> BoxFuture<'_, StoreResult<MessageId>> {
        self.write(async move {
            self.store
                .commit_received(receipt(r, MessageState::Receiving), frame)
                .await
                .map(|o| o.message_id)
                .map_err(error)
        })
    }

    fn append_frame(&self, message: MessageId, frame: Vec<u8>) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .append_receiving(
                    message,
                    frame,
                    false,
                    packie_store::Identifiers::default(),
                    false,
                )
                .await
                .map(|_| ())
                .map_err(error)
        })
    }

    fn complete_transfer(
        &self,
        message: MessageId,
        bytes: Vec<u8>,
        ids: Identifiers,
        duplicate_detection: bool,
    ) -> BoxFuture<'_, StoreResult<ReceiptOutcome>> {
        self.write(async move {
            self.store
                .complete_receiving(message, bytes, identifiers(ids), duplicate_detection)
                .await
                .map(|o| outcome(&o))
                .map_err(error)
        })
    }

    fn abort_transfer(
        &self,
        message: MessageId,
        reason: &'static str,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .abort_receiving(message, reason.to_owned())
                .await
                .map_err(error)
        })
    }

    fn record_response(
        &self,
        message: MessageId,
        code: String,
        at: Timestamp,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .record_response(message, code, at)
                .await
                .map_err(error)
        })
    }

    fn record_invalid(&self, message: MessageId, reason: String) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .record_invalid(message, reason)
                .await
                .map_err(error)
        })
    }

    fn record_filtered(
        &self,
        message: MessageId,
        reason: String,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .record_filtered(message, reason)
                .await
                .map_err(error)
        })
    }

    fn route(
        &self,
        message: MessageId,
        deliveries: Vec<NewDelivery>,
        config_version: u64,
    ) -> BoxFuture<'_, StoreResult<Vec<DeliveryId>>> {
        self.write(async move {
            let routed = deliveries
                .into_iter()
                .map(|d| (destination(&d.policy, d.transform_error), d.body))
                .collect();
            self.store
                .route_with_outputs(message, routed, config_version)
                .await
                .map_err(error)
        })
    }

    fn stored_message(&self, message: MessageId) -> BoxFuture<'_, StoreResult<StoredMessage>> {
        Box::pin(async move {
            let record = self.store.message(message).map_err(error)?;
            let bytes = self.store.body(message, None).map_err(error)?;
            let resend_targets = self
                .store
                .resend_targets(message)
                .map_err(error)?
                .iter()
                .filter_map(|id| ConnectorId::from_str(id).ok())
                .collect();
            Ok(StoredMessage {
                message_id: message,
                channel_id: record.summary.channel_id,
                protocol: record.summary.protocol,
                state: record.summary.state,
                bytes,
                resend_targets,
            })
        })
    }

    fn next_delivery(
        &self,
        channel: ChannelId,
        destination: ConnectorId,
    ) -> BoxFuture<'_, StoreResult<Option<PendingDelivery>>> {
        Box::pin(async move {
            let head = self
                .store
                .next_delivery(channel.as_str(), destination.as_str())
                .map_err(error)?;
            Ok(head.map(|h| PendingDelivery {
                delivery_id: h.delivery_id,
                message_id: h.message_id,
                sequence: h.sequence,
                next_attempt_at: h.next_attempt_at,
                attempts: h.attempts,
                max_attempts: h.max_attempts,
                ack_required: h.ack_required,
                body: h.body,
                original: h.original,
                control_id: h.control_id,
                sample_id: h.sample_id,
            }))
        })
    }

    fn record_transformed_body(
        &self,
        delivery: DeliveryId,
        body: Vec<u8>,
        mapping_version: u64,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .record_transformed_body(delivery, body, mapping_version)
                .await
                .map_err(error)
        })
    }

    fn record_transform_error(
        &self,
        delivery: DeliveryId,
        summary: String,
        mapping_version: u64,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .record_transform_error(delivery, summary, mapping_version, Timestamp::now())
                .await
                .map_err(error)
        })
    }

    fn begin_attempt(
        &self,
        delivery: DeliveryId,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<u32>> {
        self.write(async move { self.store.begin_attempt(delivery, now).await.map_err(error) })
    }

    fn record_ack_progress(
        &self,
        delivery: DeliveryId,
        level: AckLevel,
        code: Option<String>,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.write(async move {
            self.store
                .record_ack_progress(delivery, level, code, now)
                .await
                .map_err(error)
        })
    }

    fn finish_attempt(
        &self,
        delivery: DeliveryId,
        finish: AttemptFinish,
    ) -> BoxFuture<'_, StoreResult<DeliveryState>> {
        self.write(async move {
            let stopped_partway = finish.result == packie_core::AttemptResult::EngineStopped
                && !finish.sent_completely;
            let result = if stopped_partway {
                let reason = finish
                    .reason_code
                    .unwrap_or_else(|| "channel_stopped".to_owned());
                self.store
                    .abandon_attempt(delivery, reason, finish.ended_at)
                    .await
            } else {
                self.store.finish_attempt(delivery, finish.to_store()).await
            };
            result.map_err(error)
        })
    }

    fn resend(
        &self,
        request: ResendRequest,
        active: Vec<DestinationPolicy>,
        config_version: u64,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<ResendResult>> {
        self.write(async move {
            let active = active.iter().map(|p| destination(p, None)).collect();
            self.store
                .resend(request, active, config_version, now)
                .await
                .map_err(error)
        })
    }

    fn close_delivery(
        &self,
        request: CloseRequest,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<DeliveryRecord>> {
        self.write(async move { self.store.close_delivery(request, now).await.map_err(error) })
    }

    fn cancel_unresolved(
        &self,
        channel: ChannelId,
        destination: Option<ConnectorId>,
        _reason: &'static str,
    ) -> BoxFuture<'_, StoreResult<u64>> {
        // The store derives the reason (`destination_removed`, `channel_removed`).
        self.write(async move {
            self.store
                .cancel_removed(
                    channel.as_str(),
                    destination.map(|d| d.to_string()),
                    Timestamp::now(),
                )
                .await
                .map_err(error)
        })
    }

    fn set_astm_sample_positions(&self, positions: HashMap<ChannelId, astm::SamplePosition>) {
        *self
            .astm_samples
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = positions;
    }

    fn reconcile_at_startup(&self) -> BoxFuture<'_, StoreResult<Reconciliation>> {
        self.write(async move {
            // Step 1: a transfer whose committed frames form a complete message is
            // recovered (the protocol parser decides); the store aborts the rest.
            let recovered = self.recover_transfers().await;
            let mut report = self.store.reconcile_at_startup().await.map_err(error)?;
            report.recovered += recovered;
            Ok(Reconciliation {
                recovered: report.recovered,
                aborted: report.aborted,
                requeued: report.requeued,
                unknown: report.unknown,
            })
        })
    }

    fn queue_counts(
        &self,
        channel: ChannelId,
        destination: ConnectorId,
    ) -> StoreResult<QueueCounts> {
        let writes = self.writes();
        let key = (channel, destination);
        if let Some((at, counts)) = lock(&self.figures.queues).get(&key)
            && *at == writes
        {
            return Ok(counts.clone());
        }
        let counts = self
            .store
            .destination_counts(channel.as_str(), destination.as_str(), Timestamp::now())
            .map(|c| queue_counts(&c))
            .map_err(error)?;
        lock(&self.figures.queues).insert(key, (writes, counts.clone()));
        Ok(counts)
    }

    fn totals(&self) -> Option<packie_store::StoreTotals> {
        let writes = self.writes();
        if let Some((at, totals)) = *lock(&self.figures.totals)
            && at == writes
        {
            return Some(totals);
        }
        let totals = self.store.totals().ok()?;
        *lock(&self.figures.totals) = Some((writes, totals));
        Some(totals)
    }

    fn purge(&self, policies: Vec<RetentionPolicy>) -> BoxFuture<'_, StoreResult<u64>> {
        self.write(async move {
            let policies = policies
                .into_iter()
                .map(|p| {
                    let retention = packie_store::Retention {
                        max_age_days: p.max_age_days,
                        max_messages: p.max_messages,
                    };
                    (p.channel_id.to_string(), retention)
                })
                .collect();
            let purged = self
                .store
                .purge(policies, Timestamp::now())
                .await
                .map_err(error)?;
            Ok(purged.messages)
        })
    }

    fn query_messages(&self, query: MessageQuery) -> BoxFuture<'_, StoreResult<MessageList>> {
        Box::pin(async move { self.store.query_messages(&query).map_err(error) })
    }

    fn transmission_totals(
        &self,
        filter: packie_ipc::messages::MessageFilter,
    ) -> BoxFuture<'_, StoreResult<Vec<packie_ipc::reports::DestinationTransmission>>> {
        Box::pin(async move { self.store.transmission_totals(&filter).map_err(error) })
    }

    fn message_record(&self, message: MessageId) -> BoxFuture<'_, StoreResult<MessageRecord>> {
        Box::pin(async move { self.store.message(message).map_err(error) })
    }

    fn message_body(&self, request: BodyRequest) -> BoxFuture<'_, StoreResult<MessageBody>> {
        Box::pin(async move {
            let (delivery, hash) = match request.which {
                BodySelector::Original => {
                    let record = self.store.message(request.message_id).map_err(error)?;
                    (None, record.content_hash)
                }
                BodySelector::Transformed { delivery_id } => {
                    let record = self.store.delivery(delivery_id).map_err(error)?;
                    (
                        Some(delivery_id),
                        record.transformed_hash.unwrap_or_default(),
                    )
                }
            };
            let bytes = self
                .store
                .body(request.message_id, delivery)
                .map_err(error)?;
            Ok(render(request, &bytes, hash))
        })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
