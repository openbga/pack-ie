//! Operator commands on deliveries: close without contacting the destination, and resend.

use std::collections::HashSet;

use packie_core::{ConnectorId, DeliveryId, DeliveryState, MessageId, MessageState, Timestamp};
use packie_ipc::messages::{
    CloseRequest, DeliveryRecord, MessageRecord, Rejection, RejectionCode, ResendMode,
    ResendOutcome, ResendRequest, ResendResult, ResendTargets,
};
use rusqlite::{Transaction, params};

use crate::Cached;
use crate::counters::increment;
use crate::receipt::next_sequence;
use crate::{Destination, Error, Result, Store, query};

impl Store {
    /// Closes unresolved work locally, without contacting its destination.
    ///
    /// # Errors
    /// [`Error::Rejected`] for a missing reason, an unknown delivery, a delivery in flight
    /// or one already resolved.
    pub async fn close_delivery(
        &self,
        request: CloseRequest,
        now: Timestamp,
    ) -> Result<DeliveryRecord> {
        let record = self
            .write(move |tx, _| {
                if !request.has_valid_reason() {
                    return Err(rejected(
                        RejectionCode::ReasonRequired,
                        "Provide a reason of 1–500 characters.",
                    ));
                }
                let record = command_delivery(tx, request.delivery_id)?;
                if record.state == DeliveryState::Sending {
                    return Err(rejected(
                        RejectionCode::DeliveryInProgress,
                        "Wait for the delivery attempt to finish.",
                    ));
                }
                if record.state.is_resolved() {
                    return Err(rejected(
                        RejectionCode::InvalidState,
                        "Only queued or unresolved deliveries can be closed.",
                    ));
                }
                tx.execute_cached(
                    "UPDATE deliveries SET state='cancelled',closed_reason=?,closed_at=?,
                updated_at=?,next_attempt_at=NULL,reason_code='operator_closed' WHERE id=?",
                    params![request.reason.trim(), now, now, request.delivery_id],
                )?;
                query::delivery(tx, request.delivery_id)
            })
            .await?;
        tracing::info!(channel_id=%record.channel_id,destination_id=%record.destination_id,
            message_id=%record.message_id,delivery_id=%record.delivery_id,"delivery.closed");
        Ok(record)
    }

    /// Applies per-destination resend preconditions atomically and reports each outcome.
    /// An unattempted bodyless resend stays blocked with `mapping_required`: the host
    /// must run the current mapping and record output before beginning its next attempt.
    /// For a null mapping, the host records the original bytes as the output.
    ///
    /// # Errors
    /// [`Error::Rejected`] when the message no longer exists. Per-destination refusals are
    /// reported in the result, not as errors.
    pub async fn resend(
        &self,
        request: ResendRequest,
        active_destinations: Vec<Destination>,
        config_version: u64,
        now: Timestamp,
    ) -> Result<ResendResult> {
        let message = request.message_id;
        let mode = request.mode;
        let (result, channel) = self
            .write(move |tx, _| {
                let source = query::message(tx, request.message_id).map_err(command_not_found)?;
                let destinations =
                    resend_outcomes(tx, &request, &source, &active_destinations, now)?;
                let new_message_id = if request.mode == ResendMode::NewMessage {
                    clone_for_targets(tx, &request, config_version, now, &destinations)?
                } else {
                    None
                };
                let result = ResendResult {
                    new_message_id,
                    destinations,
                };
                Ok((result, source.summary.channel_id))
            })
            .await?;
        tracing::info!(channel_id=%channel,message_id=%message,mode=?mode,
            accepted=result.destinations.iter().filter(|d| d.rejected.is_none()).count(),
            "delivery.resend_requested");
        Ok(result)
    }
}

fn rejected(code: RejectionCode, message: &str) -> Error {
    Error::Rejected {
        code,
        message: message.into(),
    }
}
fn command_not_found(error: Error) -> Error {
    if matches!(error, Error::NotFound) {
        rejected(
            RejectionCode::NotFound,
            "The requested record no longer exists.",
        )
    } else {
        error
    }
}
fn command_delivery(tx: &Transaction<'_>, id: DeliveryId) -> Result<DeliveryRecord> {
    query::delivery(tx, id).map_err(command_not_found)
}

fn selected_targets(targets: &ResendTargets, active: &[Destination]) -> Result<Vec<ConnectorId>> {
    let ids = match targets {
        ResendTargets::All => active
            .iter()
            .map(|d| {
                d.id.parse()
                    .map_err(|_| Error::Invalid("invalid destination id".into()))
            })
            .collect::<Result<Vec<_>>>()?,
        ResendTargets::Destinations(ids) => ids.clone(),
    };
    let mut seen = HashSet::new();
    Ok(ids
        .into_iter()
        .filter(|id| seen.insert(id.to_string()))
        .collect())
}

fn resend_outcomes(
    tx: &Transaction<'_>,
    request: &ResendRequest,
    source: &MessageRecord,
    active: &[Destination],
    now: Timestamp,
) -> Result<Vec<ResendOutcome>> {
    let targets = selected_targets(&request.destinations, active)?;
    let mut outcomes = Vec::with_capacity(targets.len());
    for target in targets {
        let existing = source
            .deliveries
            .iter()
            .find(|d| d.destination_id == target);
        let configured = active.iter().find(|d| d.id == target.to_string());
        let rejection = resend_rejection(request, source.summary.state, existing, configured);
        let label = match (configured, existing) {
            (Some(destination), _) => destination.label.clone(),
            (None, Some(delivery)) => delivery.destination_label.clone(),
            (None, None) => target.to_string(),
        };
        let mut outcome = ResendOutcome {
            destination_id: target,
            destination_label: label,
            delivery_id: None,
            rejected: rejection,
        };
        if outcome.rejected.is_none()
            && request.mode == ResendMode::SameMessage
            && let Some(delivery) = existing
        {
            reset_delivery(tx, delivery.delivery_id, resend_reason(request), now)?;
            outcome.delivery_id = Some(delivery.delivery_id);
        }
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

fn clone_for_targets(
    tx: &Transaction<'_>,
    request: &ResendRequest,
    version: u64,
    now: Timestamp,
    outcomes: &[ResendOutcome],
) -> Result<Option<MessageId>> {
    if !outcomes.iter().any(|outcome| outcome.rejected.is_none()) {
        return Ok(None);
    }
    let id = clone_message(tx, request.message_id, version, now)?;
    let reason = resend_reason(request);
    for target in outcomes.iter().filter(|outcome| outcome.rejected.is_none()) {
        tx.execute_cached(
            "INSERT INTO resend_targets(message_id,destination_id,reason) VALUES(?,?,?)",
            params![id, target.destination_id, reason],
        )?;
    }
    Ok(Some(id))
}

fn resend_rejection(
    request: &ResendRequest,
    source_state: MessageState,
    existing: Option<&DeliveryRecord>,
    configured: Option<&Destination>,
) -> Option<Rejection> {
    let condition = if configured.is_none() {
        Some((
            RejectionCode::DestinationNotFound,
            "Destination is absent or disabled in the active configuration.",
        ))
    } else if request.mode == ResendMode::NewMessage
        && matches!(
            source_state,
            MessageState::Receiving | MessageState::Observed
        )
    {
        Some((
            RejectionCode::InvalidState,
            "Only complete message-channel receipts can run through the pipeline.",
        ))
    } else if let Some(delivery) = existing {
        if request.mode == ResendMode::SameMessage && !delivery.state.is_final() {
            Some((
                RejectionCode::DeliveryInProgress,
                "Wait for the existing delivery to finish.",
            ))
        } else if matches!(
            delivery.state,
            DeliveryState::TimedOut | DeliveryState::OutcomeUnknown | DeliveryState::Acknowledged
        ) && !request.confirm_duplicate_risk
        {
            Some((
                RejectionCode::DuplicateRiskUnconfirmed,
                "Check the destination and confirm the risk of duplicate delivery.",
            ))
        } else {
            None
        }
    } else if request.mode == ResendMode::SameMessage {
        Some((
            RejectionCode::InvalidState,
            "No existing delivery; resend as a new message to run the current pipeline.",
        ))
    } else {
        None
    };
    condition.map(|(code, message)| Rejection {
        code,
        message: message.into(),
    })
}

/// The operator's reason for a resend, trimmed; `None` when absent or blank.
fn resend_reason(request: &ResendRequest) -> Option<&str> {
    request
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
}

fn reset_delivery(
    tx: &Transaction<'_>,
    id: DeliveryId,
    reason: Option<&str>,
    now: Timestamp,
) -> Result<()> {
    tx.execute_cached(
        "UPDATE deliveries SET state='queued',attempts=0,ack_level='none',next_attempt_at=?,
        updated_at=?,reason_code=CASE WHEN transformed_hash IS NULL
            AND (total_attempts=0 OR reason_code IN ('transform_error','mapping_required'))
            THEN 'mapping_required' ELSE NULL END,
        closed_reason=NULL,closed_at=NULL,last_error=NULL,resend_reason=? WHERE id=?",
        params![now, now, reason, id],
    )?;
    Ok(())
}

fn clone_message(
    tx: &Transaction<'_>,
    original: MessageId,
    config_version: u64,
    now: Timestamp,
) -> Result<MessageId> {
    let channel: String = tx.query_row_cached(
        "SELECT channel_id FROM messages WHERE id=?",
        [original],
        |r| r.get(0),
    )?;
    let seq = next_sequence(tx, &channel)?;
    let id = MessageId::generate();
    tx.execute_cached("INSERT INTO messages (id,channel_id,channel_label,seq,received_at,committed_at,
        direction,origin,protocol,state,control_id,sample_id,patient_id,message_type,byte_length,
        content_hash,source_identity,source_connector_id,source_endpoint_id,source_peer,source_file,
        config_version,resent_from,searchable_text)
        SELECT ?,channel_id,channel_label,?,?,?,direction,'resend',protocol,'received',control_id,
        sample_id,patient_id,message_type,byte_length,content_hash,source_identity,source_connector_id,
        source_endpoint_id,source_peer,source_file,?,?,searchable_text FROM messages WHERE id=?",
        params![id,seq,now,now,config_version,original,original])?;
    tx.execute_cached(
        "INSERT INTO message_bodies(message_id,delivery_id,kind,content_hash,byte_length)
        SELECT ?,NULL,'original',content_hash,byte_length FROM message_bodies
        WHERE message_id=? AND kind='original'",
        params![id, original],
    )?;
    increment(tx, &channel, "", "received", 1)?;
    Ok(id)
}

#[cfg(test)]
mod tests;
