//! The pipeline after a commit: parse, source filters, destination filters, mapping,
//! delivery creation (`docs/spec/message-channels.md` §6). Filters and mappings are the
//! definition's [`Compiled`] form.

use packie_config::{Destination, MessageChannel};
use packie_core::{ConnectorId, MessageId};

use crate::message::store::{
    DestinationPolicy, MessageStore, NewDelivery, StoreError, StoreResult,
};
use crate::message::transform::{Compiled, Parsed};

/// How the pipeline ended for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Routed {
    /// Deliveries were created for these destinations.
    To(Vec<ConnectorId>),
    /// No delivery: the reason code.
    Filtered(&'static str),
    /// The message does not parse: the reason code.
    Invalid(String),
}

/// What the pipeline runs with: the store, the definition and its compiled form.
pub(crate) struct Pipeline<'a> {
    pub(crate) store: &'a dyn MessageStore,
    pub(crate) compiled: &'a Compiled,
    pub(crate) channel: &'a MessageChannel,
}

/// Runs the pipeline for a committed `received` message. `targets` restricts the
/// destinations (a `new_message` resend); empty means all.
pub(crate) async fn process(
    pipeline: Pipeline<'_>,
    message: MessageId,
    original: &[u8],
    targets: &[ConnectorId],
) -> StoreResult<Routed> {
    let Pipeline {
        store,
        compiled,
        channel,
    } = pipeline;
    let parsed = match compiled.parse(original) {
        Ok((parsed, _)) => parsed,
        Err(reason) => {
            store.record_invalid(message, reason.clone()).await?;
            return Ok(Routed::Invalid(reason));
        }
    };
    if !compiled.source_passes(&parsed) {
        store
            .record_filtered(message, "source_filter".into())
            .await?;
        return Ok(Routed::Filtered("source_filter"));
    }
    let enabled: Vec<&Destination> = channel
        .destinations
        .iter()
        .filter(|d| d.enabled && (targets.is_empty() || targets.contains(&d.id)))
        .collect();
    if enabled.is_empty() {
        store
            .record_filtered(message, "no_destination".into())
            .await?;
        return Ok(Routed::Filtered("no_destination"));
    }
    let deliveries: Vec<NewDelivery> = enabled
        .iter()
        .filter(|d| compiled.destination_passes(d.id, &parsed))
        .map(|d| new_delivery(compiled, channel, d, (&parsed, original)))
        .collect();
    if deliveries.is_empty() {
        store
            .record_filtered(message, "destination_filters".into())
            .await?;
        return Ok(Routed::Filtered("destination_filters"));
    }
    let ids = deliveries.iter().map(|d| d.policy.destination_id).collect();
    for delivery in deliveries.iter().filter(|d| d.transform_error.is_some()) {
        tracing::warn!(
            event = "delivery.transform_failed",
            destination_id = %delivery.policy.destination_id,
            message_id = %message,
            error_summary = delivery.transform_error.as_deref().unwrap_or(""),
            "Mapping for destination {} failed; the delivery is created failed",
            delivery.policy.destination_id
        );
    }
    store.route(message, deliveries, compiled.version).await?;
    Ok(Routed::To(ids))
}

/// The frozen policy and output of one destination.
fn new_delivery(
    compiled: &Compiled,
    channel: &MessageChannel,
    destination: &Destination,
    (parsed, original): (&Parsed, &[u8]),
) -> NewDelivery {
    let output = compiled.output(destination.id, parsed, original);
    NewDelivery {
        policy: policy_of(channel, destination),
        body: output.as_ref().ok().cloned(),
        transform_error: output.err().map(|e| e.0),
        mapping_version: compiled.version,
    }
}

fn policy_of(channel: &MessageChannel, destination: &Destination) -> DestinationPolicy {
    let delivery = destination.effective_delivery(&channel.delivery);
    DestinationPolicy {
        destination_id: destination.id,
        label: destination.label.clone(),
        max_attempts: delivery.max_attempts,
        retry_unacknowledged: delivery.retry_unacknowledged,
        ack_required: delivery.ack_required,
    }
}

/// The frozen policies of a channel's enabled destinations (for resend).
pub(crate) fn active_policies(channel: &MessageChannel) -> Vec<DestinationPolicy> {
    channel
        .destinations
        .iter()
        .filter(|d| d.enabled)
        .map(|d| policy_of(channel, d))
        .collect()
}

/// A store failure while the pipeline runs leaves the message `received`; startup
/// reconciliation runs it again.
pub(crate) fn log_pipeline_failure(message: MessageId, error: &StoreError) {
    tracing::error!(
        event = "store.error",
        message_id = %message,
        error_code = "pipeline_commit_failed",
        "The pipeline could not record its result: {error}; the message stays received"
    );
}
