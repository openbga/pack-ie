//! What ending an attempt changes: the delivery rules, the stored policy they read, and the
//! attempt, delivery, gauge and counter rows an outcome updates.

use packie_core::{AckLevel, AttemptResult, DeliveryId, DeliveryState};
use packie_ipc::messages::DeliveryRecord;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::Cached;
use crate::counters::{increment, note_error, note_time};
use crate::{AttemptFinish, Error, Result};

/// The delivery policy frozen when a delivery was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrozenPolicy {
    /// Attempts allowed per cycle.
    pub max_attempts: u32,
    /// Whether an attempt that may have reached the destination (acknowledgement timeout,
    /// connection lost after sending) is retried.
    pub retry_unacknowledged: bool,
    /// The acknowledgement level that completes a delivery.
    pub ack_required: AckLevel,
}

/// Reads the policy a delivery was created with.
pub(crate) fn frozen_policy(tx: &Transaction<'_>, id: DeliveryId) -> Result<FrozenPolicy> {
    let (max_attempts, retry_unacknowledged, ack): (u32, bool, String) = tx.query_row_cached(
        "SELECT max_attempts,retry_unacknowledged,ack_required FROM deliveries WHERE id=?",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(FrozenPolicy {
        max_attempts,
        retry_unacknowledged,
        ack_required: ack
            .parse()
            .map_err(|_| Error::Invalid("invalid stored acknowledgement policy".into()))?,
    })
}

/// The state a delivery takes after an attempt (`message-channels.md` §4, §5): the one
/// set of rules every store implementation applies. `attempts` counts the attempts of
/// the current cycle, including the one that ended.
///
/// # Errors
/// The finish contradicts the policy (acknowledged below the required level, or a
/// required level reached without acknowledging), or a retry lacks its backoff time.
pub fn delivery_state_after(
    finish: &AttemptFinish,
    attempts: u32,
    policy: &FrozenPolicy,
) -> Result<DeliveryState> {
    if finish.result != AttemptResult::Acknowledged && finish.ack_level >= policy.ack_required {
        return Err(Error::Invalid(
            "required acknowledgement was reached; the attempt must finish acknowledged".into(),
        ));
    }
    let remaining = attempts < policy.max_attempts;
    let state = match finish.result {
        AttemptResult::Acknowledged if finish.ack_level < policy.ack_required => {
            return Err(Error::Invalid(
                "acknowledgement is below the required level".into(),
            ));
        }
        AttemptResult::Acknowledged => DeliveryState::Acknowledged,
        AttemptResult::Rejected => DeliveryState::Rejected,
        AttemptResult::WriteFailed | AttemptResult::SendTimeout if remaining => {
            DeliveryState::Queued
        }
        AttemptResult::WriteFailed | AttemptResult::SendTimeout | AttemptResult::TransformError => {
            DeliveryState::Failed
        }
        AttemptResult::AckTimeout
        | AttemptResult::ConnectionLostAfterSend
        | AttemptResult::EngineStopped
            if policy.retry_unacknowledged && remaining =>
        {
            DeliveryState::Queued
        }
        AttemptResult::AckTimeout => DeliveryState::TimedOut,
        AttemptResult::ConnectionLostAfterSend | AttemptResult::EngineStopped => {
            DeliveryState::OutcomeUnknown
        }
    };
    if state == DeliveryState::Queued && finish.retry_at.is_none_or(|due| due < finish.ended_at) {
        return Err(Error::Invalid(
            "retryable attempt requires a backoff timestamp at or after completion".into(),
        ));
    }
    Ok(state)
}

/// Applies a finished attempt that `delivery_state_after` resolved to `state`.
pub(crate) fn apply_finish(
    tx: &Transaction<'_>,
    record: &DeliveryRecord,
    finish: &AttemptFinish,
    state: DeliveryState,
) -> Result<()> {
    persist_finish(tx, record, finish, state)?;
    note_finish(tx, record, finish)?;
    record_bytes(tx, record, finish.ack_level)?;
    count_finish(tx, record, finish, state)
}

fn persist_finish(
    tx: &Transaction<'_>,
    record: &DeliveryRecord,
    finish: &AttemptFinish,
    state: DeliveryState,
) -> Result<()> {
    let changed = tx.execute_cached(
        "UPDATE delivery_attempts SET ended_at=?,result=?,ack_level=?,
        ack_code=COALESCE(?,ack_code),ack_control_id=COALESCE(?,ack_control_id),
        error_code=?,error_summary=?
        WHERE delivery_id=? AND attempt_no=? AND ended_at IS NULL",
        params![
            finish.ended_at,
            finish.result.as_str(),
            finish.ack_level.as_str(),
            finish.ack_code,
            finish.ack_control_id,
            finish.error_code,
            finish.error_summary,
            record.delivery_id,
            record.total_attempts
        ],
    )?;
    require_attempt(changed)?;
    let retry = finish.retry_at.filter(|_| state == DeliveryState::Queued);
    let reason = if state == DeliveryState::Acknowledged {
        None
    } else {
        finish
            .error_code
            .as_deref()
            .or(Some(finish.result.as_str()))
    };
    tx.execute_cached(
        "UPDATE deliveries SET state=?,ack_level=?,updated_at=?,next_attempt_at=?,
        reason_code=?,last_error=? WHERE id=?",
        params![
            state.as_str(),
            finish.ack_level.as_str(),
            finish.ended_at,
            retry,
            reason,
            finish.error_summary,
            record.delivery_id
        ],
    )?;
    Ok(())
}

/// The destination gauges an ended attempt moves: last acknowledgement, last failure.
fn note_finish(
    tx: &Transaction<'_>,
    record: &DeliveryRecord,
    finish: &AttemptFinish,
) -> Result<()> {
    let (channel, destination) = (record.channel_id.as_ref(), record.destination_id.as_ref());
    if finish.result == AttemptResult::Acknowledged {
        note_time(tx, channel, destination, "last_ack_at", finish.ended_at)?;
    }
    if let Some(summary) = &finish.error_summary {
        note_error(tx, channel, destination, summary, finish.ended_at)?;
    }
    Ok(())
}

/// An update must have hit exactly the one open attempt record.
pub(crate) fn require_attempt(changed: usize) -> Result<()> {
    if changed != 1 {
        return Err(Error::Invalid("delivery has no unique open attempt".into()));
    }
    Ok(())
}

/// Adds to a counter of the delivery's channel and of its destination.
pub(crate) fn count(
    tx: &Transaction<'_>,
    record: &DeliveryRecord,
    column: &str,
    amount: u64,
) -> Result<()> {
    increment(tx, record.channel_id.as_ref(), "", column, amount)?;
    increment(
        tx,
        record.channel_id.as_ref(),
        record.destination_id.as_ref(),
        column,
        amount,
    )
}

/// Counts the bytes sent once the acknowledgement level first reaches `HandedOff`.
pub(crate) fn record_bytes(
    tx: &Transaction<'_>,
    record: &DeliveryRecord,
    level: AckLevel,
) -> Result<()> {
    if record.ack_level < AckLevel::HandedOff && level >= AckLevel::HandedOff {
        let bytes: u64 = tx
            .query_row_cached(
                "SELECT byte_length FROM message_bodies
            WHERE delivery_id=? AND kind='transformed'",
                [record.delivery_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(tx.query_row_cached(
                "SELECT byte_length FROM messages WHERE id=?",
                [record.message_id],
                |r| r.get(0),
            )?);
        count(tx, record, "bytes_out", bytes)?;
    }
    Ok(())
}

fn count_finish(
    tx: &Transaction<'_>,
    record: &DeliveryRecord,
    finish: &AttemptFinish,
    state: DeliveryState,
) -> Result<()> {
    if finish.result != AttemptResult::Acknowledged {
        increment(tx, record.channel_id.as_ref(), "", "errors", 1)?;
    }
    let column = match finish.result {
        AttemptResult::Acknowledged => Some("acknowledged"),
        AttemptResult::Rejected => Some("rejected"),
        AttemptResult::AckTimeout => Some("timed_out"),
        AttemptResult::ConnectionLostAfterSend | AttemptResult::EngineStopped => {
            Some("outcome_unknown")
        }
        _ => None,
    };
    if let Some(column) = column {
        count(tx, record, column, 1)?;
    }
    if state == DeliveryState::Failed {
        count(tx, record, "failed", 1)?;
    }
    Ok(())
}

/// Logs the state an ended attempt left its delivery in.
pub(crate) fn log_finished(record: &DeliveryRecord, finish: &AttemptFinish, state: DeliveryState) {
    if state.is_final() && !state.is_resolved() {
        tracing::warn!(channel_id=%record.channel_id,destination_id=%record.destination_id,
            message_id=%record.message_id,delivery_id=%record.delivery_id,result=%finish.result,
            state=%state,ack_level=%finish.ack_level,"delivery.state_changed");
    } else {
        tracing::info!(channel_id=%record.channel_id,destination_id=%record.destination_id,
            message_id=%record.message_id,delivery_id=%record.delivery_id,result=%finish.result,
            state=%state,ack_level=%finish.ack_level,"delivery.state_changed");
    }
}
