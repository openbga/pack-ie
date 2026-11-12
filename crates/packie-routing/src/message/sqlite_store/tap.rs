//! Recording bridge tap observations in the durable store, off the bridge's path.

use std::sync::Arc;

use packie_core::{BridgeSide, MessageDirection, MessageOrigin, MessageState, Timestamp};
use packie_store::Store;
use tokio::sync::mpsc;

use super::Figures;
use super::convert::identifiers;
use crate::{TapObservation, TapSink};

/// Observed messages queued for the store before the tap drops them.
const TAP_QUEUE: usize = 1024;

/// Records bridge tap observations as `observed` messages (origin `tap`, direction
/// `a_to_b` or `b_to_a`, never deduplicated, no deliveries), in order, from one task.
/// The bridge never waits: when the queue is full the observation is dropped and logged.
#[derive(Debug, Clone)]
pub struct StoreTapSink {
    queue: mpsc::Sender<TapObservation>,
}

impl StoreTapSink {
    /// A sink writing to `store` that counts its writes in `figures`; `None` outside a
    /// tokio runtime.
    pub(super) fn new(store: Store, figures: Arc<Figures>) -> Option<Self> {
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let (queue, observations) = mpsc::channel(TAP_QUEUE);
        runtime.spawn(record_observations(store, figures, observations));
        Some(Self { queue })
    }
}

impl TapSink for StoreTapSink {
    fn observed(&self, observation: TapObservation) {
        if self.queue.try_send(observation).is_err() {
            tracing::warn!(
                event = "tap.dropped",
                "The store fell behind; a tap observation was not recorded"
            );
        }
    }
}

async fn record_observations(
    store: Store,
    figures: Arc<Figures>,
    mut observations: mpsc::Receiver<TapObservation>,
) {
    while let Some(observation) = observations.recv().await {
        let ids =
            crate::message::transform::tap_identifiers(observation.protocol, &observation.message);
        let receipt = packie_store::Receipt {
            channel_id: observation.channel_id.to_string(),
            channel_label: observation.channel_label,
            source_connector_id: "tap".to_owned(),
            source_endpoint_id: observation.endpoint_id.to_string(),
            source_peer: None,
            source_file: None,
            protocol: observation.protocol,
            direction: match observation.from {
                BridgeSide::A => MessageDirection::AToB,
                BridgeSide::B => MessageDirection::BToA,
            },
            origin: MessageOrigin::Tap,
            initial_state: MessageState::Observed,
            identifiers: identifiers(ids),
            received_at: Timestamp::now(),
            config_version: observation.config_version,
            duplicate_detection: false,
            destinations: Vec::new(),
        };
        let committed = store.commit_received(receipt, observation.message).await;
        figures.wrote();
        if let Err(error) = committed {
            tracing::error!(
                event = "store.error",
                channel_id = %observation.channel_id,
                error_code = "tap_record_failed",
                "A tap observation was not recorded: {error}"
            );
        }
    }
}
