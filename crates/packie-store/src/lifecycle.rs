//! Delivery attempts: queue order, begin, acknowledgement progress, finish, abandon,
//! recorded output, and cancellation of removed configuration. Delivery transitions are
//! serialized with receipts by the durable writer.

use packie_core::{AckLevel, DeliveryId, DeliveryState, Timestamp};
use packie_ipc::messages::DeliveryRecord;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::Cached;
use crate::counters::{increment, note_error, note_time};
use crate::finish::{self, FrozenPolicy};
use crate::{AttemptFinish, Error, QueuedDelivery, Result, Store, query};

impl Store {
    /// Commits the attempt before the caller writes any destination bytes.
    ///
    /// # Errors
    /// [`Error::Invalid`] unless the delivery is queued, due, has attempts left and heads
    /// its destination's queue.
    pub async fn begin_attempt(&self, delivery: DeliveryId, now: Timestamp) -> Result<u32> {
        let (total, record) = self
            .write(move |tx, _| {
                let record = query::delivery(tx, delivery)?;
                let policy = finish::frozen_policy(tx, delivery)?;
                check_begin(tx, &record, policy, now)?;
                let total = record
                    .total_attempts
                    .checked_add(1)
                    .ok_or_else(|| Error::Invalid("attempt count exhausted".into()))?;
                tx.execute_cached(
                    "INSERT INTO delivery_attempts
                (id,delivery_id,attempt_no,started_at,ack_level) VALUES(?,?,?,?,'none')",
                    params![uuid::Uuid::now_v7().to_string(), delivery, total, now],
                )?;
                tx.execute_cached(
                    "UPDATE deliveries SET state='sending', attempts=attempts+1,
                total_attempts=?, ack_level='none', updated_at=?, next_attempt_at=NULL,
                reason_code=NULL, last_error=NULL WHERE id=?",
                    params![total, now, delivery],
                )?;
                finish::count(tx, &record, "attempts", 1)?;
                note_time(
                    tx,
                    record.channel_id.as_ref(),
                    record.destination_id.as_ref(),
                    "last_attempt_at",
                    now,
                )?;
                Ok((total, record))
            })
            .await?;
        tracing::info!(channel_id=%record.channel_id, destination_id=%record.destination_id,
            message_id=%record.message_id,delivery_id=%record.delivery_id,attempt_no=total,
            "delivery.attempt_started");
        Ok(total)
    }

    /// Records a lower acknowledgement without ending the in-flight attempt.
    ///
    /// # Errors
    /// [`Error::Invalid`] without an in-flight attempt, or for a level that goes backwards
    /// or reaches the required one.
    pub async fn record_ack_progress(
        &self,
        delivery: DeliveryId,
        level: AckLevel,
        code: Option<String>,
        now: Timestamp,
    ) -> Result<()> {
        let record = self
            .write(move |tx, _| {
                let record = query::delivery(tx, delivery)?;
                let policy = finish::frozen_policy(tx, delivery)?;
                require_sending(&record)?;
                if level < record.ack_level || level >= policy.ack_required {
                    return Err(Error::Invalid(
                        "progress must be monotonic and below the required acknowledgement".into(),
                    ));
                }
                check_attempt_time(tx, &record, now)?;
                finish::record_bytes(tx, &record, level)?;
                tx.execute_cached(
                    "UPDATE deliveries SET ack_level=?,updated_at=? WHERE id=?",
                    params![level.as_str(), now, delivery],
                )?;
                let changed = tx.execute_cached(
                    "UPDATE delivery_attempts SET ack_level=?,ack_code=COALESCE(?,ack_code)
                WHERE delivery_id=? AND attempt_no=? AND ended_at IS NULL",
                    params![level.as_str(), code, delivery, record.total_attempts],
                )?;
                finish::require_attempt(changed)?;
                Ok(record)
            })
            .await?;
        tracing::info!(channel_id=%record.channel_id,destination_id=%record.destination_id,
            message_id=%record.message_id,delivery_id=%record.delivery_id,ack_level=%level,
            "delivery.ack_progress");
        Ok(())
    }

    /// Completes an attempt using the policy frozen when the delivery was created.
    ///
    /// # Errors
    /// [`Error::Invalid`] without an in-flight attempt, or when the outcome contradicts the
    /// policy (see [`delivery_state_after`](crate::delivery_state_after)).
    pub async fn finish_attempt(
        &self,
        delivery: DeliveryId,
        finish: AttemptFinish,
    ) -> Result<DeliveryState> {
        let (state, record, finish) = self
            .write(move |tx, _| {
                let record = query::delivery(tx, delivery)?;
                let policy = finish::frozen_policy(tx, delivery)?;
                require_sending(&record)?;
                check_attempt_time(tx, &record, finish.ended_at)?;
                if finish.ack_level < record.ack_level {
                    return Err(Error::Invalid(
                        "acknowledgement cannot move backwards".into(),
                    ));
                }
                let state = finish::delivery_state_after(&finish, record.attempts, &policy)?;
                finish::apply_finish(tx, &record, &finish, state)?;
                Ok((state, record, finish))
            })
            .await?;
        finish::log_finished(&record, &finish, state);
        Ok(state)
    }

    /// Abandons the attempt in flight without counting it: a channel stopped before every
    /// byte was written, so the destination cannot have received the message. The
    /// delivery returns to `queued` with `reason`, its attempt record removed.
    ///
    /// # Errors
    /// [`Error::Invalid`] without an in-flight attempt.
    pub async fn abandon_attempt(
        &self,
        delivery: DeliveryId,
        reason: String,
        now: Timestamp,
    ) -> Result<DeliveryState> {
        self.write(move |tx, _| {
            let record = query::delivery(tx, delivery)?;
            require_sending(&record)?;
            let removed = tx.execute_cached(
                "DELETE FROM delivery_attempts WHERE delivery_id=? AND attempt_no=? AND ended_at IS NULL",
                params![delivery, record.total_attempts],
            )?;
            finish::require_attempt(removed)?;
            tx.execute_cached(
                "UPDATE deliveries SET state='queued',attempts=attempts-1,
                total_attempts=total_attempts-1,ack_level='none',reason_code=?,updated_at=?,
                next_attempt_at=NULL WHERE id=?",
                params![reason, now, delivery],
            )?;
            Ok(DeliveryState::Queued)
        })
        .await
    }

    /// The head of a destination's queue: the `queued` delivery of the earliest message,
    /// unless an attempt is in flight. Carries the stored output (`None` until recorded)
    /// and the original bytes.
    ///
    /// # Errors
    /// The database cannot be read, or a stored body fails its integrity check.
    pub fn next_delivery(
        &self,
        channel: &str,
        destination: &str,
    ) -> Result<Option<QueuedDelivery>> {
        self.snapshot(|tx| {
            let Some(mut head) = queue_head(tx, channel, destination)? else {
                return Ok(None);
            };
            if head.body.is_some() {
                head.body = Some(
                    self.0
                        .bodies
                        .get(tx, head.message_id, Some(head.delivery_id))?,
                );
            }
            head.original = self.0.bodies.get(tx, head.message_id, None)?;
            Ok(Some(head))
        })
    }

    /// Cancels unresolved deliveries after a channel or destination is removed. The
    /// host drains in-flight attempts first; this operation leaves sending work intact.
    ///
    /// # Errors
    /// The write fails.
    pub async fn cancel_removed(
        &self,
        channel: impl AsRef<str>,
        destination: Option<String>,
        now: Timestamp,
    ) -> Result<u64> {
        let channel = channel.as_ref().to_owned();
        let (changed, channel, destination) = self
            .write(move |tx, _| {
                let reason = if destination.is_some() {
                    "destination_removed"
                } else {
                    "channel_removed"
                };
                let changed = tx.execute_cached(
                    "UPDATE deliveries SET state='cancelled',reason_code=?,
                closed_at=?,closed_reason=NULL,updated_at=?,next_attempt_at=NULL WHERE channel_id=?
                AND (? IS NULL OR destination_id=?)
                AND state IN ('queued','rejected','failed','timed_out','outcome_unknown')",
                    params![reason, now, now, channel, destination, destination],
                )?;
                let changed = u64::try_from(changed)
                    .map_err(|_| Error::Invalid("cancellation count exceeds capacity".into()))?;
                Ok((changed, channel, destination))
            })
            .await?;
        tracing::info!(channel_id=%channel,destination_id=?destination,cancelled=changed,
            "delivery.configuration_removed");
        Ok(changed)
    }

    /// Stores output before attempting delivery. An attempted body's mapping stays frozen.
    ///
    /// # Errors
    /// [`Error::Invalid`] during an attempt, or once an attempted body is frozen.
    pub async fn record_transformed_body(
        &self,
        delivery: DeliveryId,
        bytes: Vec<u8>,
        mapping_version: u64,
    ) -> Result<()> {
        self.write(move |tx, bodies| {
            let record = query::delivery(tx, delivery)?;
            if record.state == DeliveryState::Sending {
                return Err(Error::Invalid(
                    "cannot replace a body during an attempt".into(),
                ));
            }
            if record.transformed_hash.is_some() && record.total_attempts > 0 {
                return Err(Error::Invalid(
                    "the attempted transformed body is frozen; resend as a new message".into(),
                ));
            }
            let hash = bodies.put(tx, record.message_id, Some(delivery), &bytes)?;
            tx.execute_cached(
                "UPDATE deliveries SET transformed_hash=?,mapping_version=?,
                    reason_code=CASE WHEN reason_code='mapping_required' THEN NULL ELSE reason_code END
                    WHERE id=?",
                params![hash, mapping_version, delivery],
            )?;
            Ok(())
        })
        .await
    }

    /// Records a mapping failure before any attempt in this cycle. Summaries name the
    /// rule or condition, never the message value that caused it.
    ///
    /// # Errors
    /// [`Error::Invalid`] unless the delivery is queued and unattempted in this cycle.
    pub async fn record_transform_error(
        &self,
        delivery: DeliveryId,
        summary: String,
        mapping_version: u64,
        now: Timestamp,
    ) -> Result<()> {
        let record = self.write(move |tx, _| {
            let record = query::delivery(tx, delivery)?;
            if record.state != DeliveryState::Queued || record.attempts != 0 {
                return Err(Error::Invalid("mapping failure requires an unattempted queued cycle".into()));
            }
            if record.transformed_hash.is_some() && record.total_attempts > 0 {
                return Err(Error::Invalid("the attempted transformed body is frozen; resend as a new message".into()));
            }
            let transformed: Option<String> = tx.query_row_cached("SELECT content_hash FROM message_bodies WHERE delivery_id=? AND kind='transformed'",[delivery],|row| row.get(0)).optional()?;
            tx.execute_cached("DELETE FROM message_bodies WHERE delivery_id=? AND kind='transformed'",[delivery])?;
            if let Some(hash) = transformed {
                crate::bodies::release(tx, &hash)?;
            }
            tx.execute_cached("UPDATE deliveries SET state='failed',reason_code='transform_error',
                last_error=?,mapping_version=?,updated_at=?,next_attempt_at=NULL,transformed_hash=NULL
                WHERE id=?",params![summary,mapping_version,now,delivery])?;
            finish::count(tx,&record,"failed",1)?;
            increment(tx,record.channel_id.as_ref(),"","errors",1)?;
            note_error(tx,record.channel_id.as_ref(),record.destination_id.as_ref(),&summary,now)?;
            Ok(record)
        }).await?;
        tracing::warn!(channel_id=%record.channel_id,destination_id=%record.destination_id,
            message_id=%record.message_id,delivery_id=%record.delivery_id,mapping_version,
            state="failed","delivery.transform_failed");
        Ok(())
    }
}

fn queue_head(
    tx: &Transaction<'_>,
    channel: &str,
    destination: &str,
) -> Result<Option<QueuedDelivery>> {
    let sending: bool = tx.query_row_cached(
        "SELECT EXISTS(SELECT 1 FROM deliveries WHERE channel_id=? AND destination_id=? AND state='sending')",
        params![channel, destination],
        |r| r.get(0),
    )?;
    if sending {
        return Ok(None);
    }
    Ok(tx
        .query_row_cached(
            "SELECT d.id,d.message_id,m.seq,d.next_attempt_at,d.attempts,d.max_attempts,
                d.transformed_hash IS NOT NULL,m.control_id,m.sample_id,d.ack_required
             FROM deliveries d JOIN messages m ON m.id=d.message_id
             WHERE d.channel_id=? AND d.destination_id=? AND d.state='queued'
             ORDER BY d.seq LIMIT 1",
            params![channel, destination],
            |r| {
                Ok(QueuedDelivery {
                    delivery_id: r.get(0)?,
                    message_id: r.get(1)?,
                    sequence: r.get(2)?,
                    next_attempt_at: r.get(3)?,
                    attempts: r.get(4)?,
                    max_attempts: r.get(5)?,
                    ack_required: ack_level(r, 9)?,
                    body: r.get::<_, bool>(6)?.then(Vec::new),
                    original: Vec::new(),
                    control_id: r.get(7)?,
                    sample_id: r.get(8)?,
                })
            },
        )
        .optional()?)
}

/// The acknowledgement level stored in column `index` (its text name).
fn ack_level(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<AckLevel> {
    row.get::<_, String>(index)?.parse().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            "invalid stored acknowledgement level".into(),
        )
    })
}

fn check_begin(
    tx: &Transaction<'_>,
    record: &DeliveryRecord,
    policy: FrozenPolicy,
    now: Timestamp,
) -> Result<()> {
    if record.state != DeliveryState::Queued || record.attempts >= policy.max_attempts {
        return Err(Error::Invalid(
            "only queued deliveries with attempts remaining can start".into(),
        ));
    }
    if record.reason_code.as_deref() == Some("mapping_required") {
        return Err(Error::Invalid(
            "delivery requires current mapping output before an attempt".into(),
        ));
    }
    if record.next_attempt_at.is_some_and(|due| due > now) {
        return Err(Error::Invalid("delivery retry is not due".into()));
    }
    let blocked: bool = tx.query_row_cached(
        "SELECT EXISTS(SELECT 1 FROM deliveries WHERE channel_id=?1 AND destination_id=?2
            AND state='sending')
         OR EXISTS(SELECT 1 FROM deliveries WHERE channel_id=?1 AND destination_id=?2
            AND state='queued' AND seq < (SELECT seq FROM messages WHERE id=?3))",
        params![
            record.channel_id.to_string(),
            record.destination_id.to_string(),
            record.message_id
        ],
        |r| r.get(0),
    )?;
    if blocked {
        return Err(Error::Invalid(
            "destination has an in-flight or earlier queued delivery".into(),
        ));
    }
    Ok(())
}

fn require_sending(record: &DeliveryRecord) -> Result<()> {
    if record.state != DeliveryState::Sending {
        return Err(Error::Invalid("delivery has no in-flight attempt".into()));
    }
    Ok(())
}

fn check_attempt_time(tx: &Transaction<'_>, record: &DeliveryRecord, now: Timestamp) -> Result<()> {
    let started: Option<Timestamp> = tx
        .query_row_cached(
            "SELECT started_at FROM delivery_attempts
        WHERE delivery_id=? AND attempt_no=? AND ended_at IS NULL",
            params![record.delivery_id, record.total_attempts],
            |r| r.get(0),
        )
        .optional()?;
    match started {
        Some(started) if now >= started && now >= record.updated_at => Ok(()),
        Some(_) => Err(Error::Invalid(
            "attempt timestamp cannot move backwards".into(),
        )),
        None => Err(Error::Invalid("delivery has no open attempt record".into())),
    }
}

#[cfg(test)]
mod tests;
