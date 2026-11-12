//! Conversions between the pipeline's store types and `packie_store`'s records.

use packie_core::{
    DeliveryState, MessageDirection, MessageOrigin, MessageState, hex_dump, visible_text,
};
use packie_ipc::messages::{BodyFormat, BodyRequest, MessageBody};

use crate::message::store::{
    DestinationPolicy, Identifiers, QueueCounts, Receipt, ReceiptOutcome, StoreError,
};

pub(super) fn error(error: packie_store::Error) -> StoreError {
    match error {
        packie_store::Error::Sql(_) | packie_store::Error::Io(_) | packie_store::Error::Stopped => {
            StoreError::Unavailable(error.to_string())
        }
        packie_store::Error::NotFound => StoreError::NotFound,
        packie_store::Error::Rejected { code, message } => StoreError::Rejected { code, message },
        packie_store::Error::Invalid(_) | packie_store::Error::Json(_) => {
            StoreError::Invalid(error.to_string())
        }
    }
}

pub(super) fn identifiers(ids: Identifiers) -> packie_store::Identifiers {
    // Safe metadata only: identifiers the parser extracted, never body text.
    let searchable_text = [&ids.control_id, &ids.sample_id, &ids.patient_id]
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    packie_store::Identifiers {
        control_id: ids.control_id,
        sample_id: ids.sample_id,
        patient_id: ids.patient_id,
        message_type: ids.message_type,
        source_identity: ids.source_identity,
        searchable_text,
    }
}

pub(super) fn receipt(receipt: Receipt, initial_state: MessageState) -> packie_store::Receipt {
    packie_store::Receipt {
        channel_id: receipt.channel_id.to_string(),
        channel_label: receipt.channel_label,
        source_connector_id: "source".to_owned(),
        source_endpoint_id: receipt.source_endpoint_id.to_string(),
        source_peer: receipt.source_peer,
        source_file: receipt.source_file,
        protocol: receipt.protocol,
        direction: MessageDirection::Inbound,
        origin: MessageOrigin::Received,
        initial_state,
        identifiers: identifiers(receipt.identifiers),
        received_at: receipt.received_at,
        config_version: receipt.config_version,
        duplicate_detection: receipt.duplicate_detection,
        destinations: Vec::new(),
    }
}

pub(super) fn outcome(outcome: &packie_store::ReceiptOutcome) -> ReceiptOutcome {
    ReceiptOutcome {
        message_id: outcome.message_id,
        sequence: outcome.sequence,
        state: if outcome.duplicate_of.is_some() {
            MessageState::Duplicate
        } else {
            MessageState::Received
        },
        duplicate_of: outcome.duplicate_of,
        revision_of: outcome.revision_of,
    }
}

pub(super) fn destination(
    policy: &DestinationPolicy,
    transform_error: Option<String>,
) -> packie_store::Destination {
    packie_store::Destination {
        id: policy.destination_id.to_string(),
        label: policy.label.clone(),
        max_attempts: policy.max_attempts,
        retry_unacknowledged: policy.retry_unacknowledged,
        ack_required: policy.ack_required,
        transform_error,
    }
}

pub(super) fn state_count(counts: &packie_store::DestinationCounts, state: DeliveryState) -> u64 {
    counts.states.get(state.as_str()).copied().unwrap_or(0)
}

pub(super) fn queue_counts(counts: &packie_store::DestinationCounts) -> QueueCounts {
    QueueCounts {
        queued: state_count(counts, DeliveryState::Queued),
        sending: state_count(counts, DeliveryState::Sending),
        rejected: state_count(counts, DeliveryState::Rejected),
        timed_out: state_count(counts, DeliveryState::TimedOut),
        outcome_unknown: state_count(counts, DeliveryState::OutcomeUnknown),
        failed: state_count(counts, DeliveryState::Failed),
        transform_errors: counts.transform_errors,
        oldest_queued_at: counts.oldest_queued_at,
        next_attempt_at: counts.next_attempt_at,
        attempts: counts.totals.attempts,
        acknowledged: counts.totals.acknowledged,
        bytes_out: counts.totals.bytes_out,
        last_attempt_at: counts.last_attempt_at,
        last_ack_at: counts.last_ack_at,
        last_error: counts.last_error.clone(),
    }
}

/// Body text for the Messages detail view: visible text or a hex dump.
pub(super) fn render(request: BodyRequest, bytes: &[u8], hash: String) -> MessageBody {
    let content = match request.format {
        BodyFormat::Text => visible_text(bytes),
        BodyFormat::Hex => hex_dump(bytes),
    };
    MessageBody {
        message_id: request.message_id,
        which: request.which,
        format: request.format,
        content,
        byte_length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        hash,
        truncated: false,
    }
}
