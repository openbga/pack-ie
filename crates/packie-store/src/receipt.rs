use std::collections::HashSet;

use packie_core::{AckLevel, DeliveryId, MessageId, MessageOrigin, MessageState, Timestamp};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::Cached;
use crate::bodies::{BodyStorage, content_hash};
use crate::counters::{increment, note_error};
use crate::{Destination, Error, Receipt, ReceiptOutcome, Result, Store, column};

impl Store {
    /// Commits receipt and original bytes before returning. If destinations are already
    /// selected, inserts them atomically; otherwise leaves `received` for the pipeline.
    /// Composite identity extraction belongs to the protocol parser.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a malformed receipt or delivery policy; the write fails.
    pub async fn commit_received(
        &self,
        receipt: Receipt,
        bytes: Vec<u8>,
    ) -> Result<ReceiptOutcome> {
        validate_receipt(&receipt)?;
        let channel = receipt.channel_id.clone();
        let outcome = self
            .write(move |tx, bodies| commit_one(tx, bodies, &receipt, &bytes))
            .await?;
        log_commit(&channel, &outcome);
        Ok(outcome)
    }

    /// Commits several units in one transaction, all or none: the records or groups of one
    /// source file, so the file is consumed only after every message from it is durable.
    /// Outcomes are in input order.
    ///
    /// # Errors
    /// As [`Store::commit_received`]; on any error no unit is stored.
    pub async fn commit_received_batch(
        &self,
        units: Vec<(Receipt, Vec<u8>)>,
    ) -> Result<Vec<ReceiptOutcome>> {
        for (receipt, _) in &units {
            validate_receipt(receipt)?;
        }
        let channels: Vec<String> = units.iter().map(|(r, _)| r.channel_id.clone()).collect();
        let outcomes = self
            .write(move |tx, bodies| {
                units
                    .iter()
                    .map(|(receipt, bytes)| commit_one(tx, bodies, receipt, bytes))
                    .collect::<Result<Vec<_>>>()
            })
            .await?;
        for (channel, outcome) in channels.iter().zip(&outcomes) {
            log_commit(channel, outcome);
        }
        Ok(outcomes)
    }

    /// Completes an unfinished pipeline in one transaction. A duplicate invocation is
    /// rejected, preventing two pipeline workers from creating overlapping fan-outs.
    ///
    /// # Errors
    /// As [`Store::route_with_outputs`].
    pub async fn route_received(
        &self,
        message: MessageId,
        destinations: Vec<Destination>,
        version: u64,
    ) -> Result<Vec<DeliveryId>> {
        let routed = destinations.into_iter().map(|d| (d, None)).collect();
        self.route_with_outputs(message, routed, version).await
    }

    /// Like `route_received`, also storing each destination's output (the transformed
    /// body, or the original for a pass-through) in the same transaction, so a delivery
    /// never exists without the bytes it will send. `None` leaves the body to be recorded
    /// later (a transform error, or a host that maps before each attempt).
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown message; [`Error::Invalid`] when the message is
    /// not `received`, a destination was not selected for a resend, or a policy is invalid.
    pub async fn route_with_outputs(
        &self,
        message: MessageId,
        destinations: Vec<(Destination, Option<Vec<u8>>)>,
        version: u64,
    ) -> Result<Vec<DeliveryId>> {
        self.write(move |tx, bodies| {
            let (channel, state) = channel_and_state(tx, message)?;
            if state != "received" {
                return Err(Error::Invalid(
                    "pipeline requires a received message".into(),
                ));
            }
            validate_route(tx, message, destinations.iter().map(|(d, _)| d.id.as_str()))?;
            let now = Timestamp::now();
            let mut ids = Vec::with_capacity(destinations.len());
            for (destination, output) in &destinations {
                let id = insert_delivery(tx, message, &channel, destination, now)?;
                let output = output
                    .as_ref()
                    .filter(|_| destination.transform_error.is_none());
                store_output(tx, bodies, message, id, output)?;
                ids.push(id);
            }
            tx.execute_cached(
                "UPDATE deliveries SET mapping_version=? WHERE message_id=?",
                params![version, message],
            )?;
            let (state, reason) = if ids.is_empty() {
                ("filtered", Some("no_destination"))
            } else {
                ("routed", None)
            };
            tx.execute_cached(
                "UPDATE messages SET state=?,reason_code=? WHERE id=?",
                params![state, reason, message],
            )?;
            // A new-message resend: its deliveries carry the operator's reason.
            tx.execute_cached(
                "UPDATE deliveries SET resend_reason=(SELECT t.reason FROM resend_targets t
                 WHERE t.message_id=deliveries.message_id
                   AND t.destination_id=deliveries.destination_id)
                 WHERE message_id=?",
                [message],
            )?;
            tx.execute_cached("DELETE FROM resend_targets WHERE message_id=?", [message])?;
            if ids.is_empty() {
                increment(tx, &channel, "", "filtered", 1)?;
            }
            Ok(ids)
        })
        .await
    }

    /// Marks a complete unprocessed message filtered and cancels unstarted provisional
    /// fan-out. Once an attempt exists the pipeline cannot revise the receipt verdict.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a blank reason or an already processed message;
    /// [`Error::NotFound`] for an unknown message.
    pub async fn record_filtered(
        &self,
        message: MessageId,
        reason: impl Into<String>,
    ) -> Result<()> {
        self.record_verdict(message, MessageState::Filtered, reason.into())
            .await
    }
    /// Marks a complete unprocessed message invalid, as [`Store::record_filtered`] does
    /// for filtered ones.
    ///
    /// # Errors
    /// As [`Store::record_filtered`].
    pub async fn record_invalid(
        &self,
        message: MessageId,
        reason: impl Into<String>,
    ) -> Result<()> {
        self.record_verdict(message, MessageState::Invalid, reason.into())
            .await
    }
    async fn record_verdict(
        &self,
        message: MessageId,
        state: MessageState,
        reason: String,
    ) -> Result<()> {
        if reason.trim().is_empty() {
            return Err(Error::Invalid("reason required".into()));
        }
        self.write(move |tx, _| {
            let (channel, current) = channel_and_state(tx, message)?;
            let attempts: bool = tx.query_row_cached(
                "SELECT EXISTS(SELECT 1 FROM deliveries WHERE message_id=? AND total_attempts>0)",
                [message],
                |r| r.get(0),
            )?;
            if !matches!(current.as_str(), "received" | "routed") || attempts {
                return Err(Error::Invalid("message already processed".into()));
            }
            tx.execute_cached("DELETE FROM deliveries WHERE message_id=?", [message])?;
            tx.execute_cached(
                "UPDATE messages SET state=?,reason_code=? WHERE id=?",
                params![state.to_string(), reason, message],
            )?;
            increment(
                tx,
                &channel,
                "",
                if state == MessageState::Filtered {
                    "filtered"
                } else {
                    "invalid"
                },
                1,
            )?;
            if state == MessageState::Invalid {
                increment(tx, &channel, "", "errors", 1)?;
            }
            Ok(())
        })
        .await
    }
    /// Records the protocol response sent to the message's source.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown message.
    pub async fn record_response(
        &self,
        message: MessageId,
        code: String,
        at: Timestamp,
    ) -> Result<()> {
        self.write(move |tx, _| {
            if tx.execute_cached(
                "UPDATE messages SET response_code=?,responded_at=? WHERE id=?",
                params![code, at, message],
            )? == 0
            {
                return Err(Error::NotFound);
            }
            Ok(())
        })
        .await
    }
    /// The destinations a new-message resend selected, until its pipeline run consumes them.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn resend_targets(&self, message: MessageId) -> Result<Vec<String>> {
        self.read(|conn| {
            column(
                conn,
                "SELECT destination_id FROM resend_targets WHERE message_id=? ORDER BY rowid",
                [message],
            )
        })
    }
}

/// The channel and state name of a stored message.
fn channel_and_state(tx: &Transaction<'_>, message: MessageId) -> Result<(String, String)> {
    tx.query_row_cached(
        "SELECT channel_id,state FROM messages WHERE id=?",
        [message],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .ok_or(Error::NotFound)
}

fn commit_one(
    tx: &Transaction<'_>,
    bodies: &BodyStorage,
    receipt: &Receipt,
    bytes: &[u8],
) -> Result<ReceiptOutcome> {
    let id = MessageId::generate();
    let hash = content_hash(bytes);
    let sequence = next_sequence(tx, &receipt.channel_id)?;
    let (duplicate_of, revision_of) = duplicate(tx, receipt, &hash)?;
    insert_message(
        tx,
        receipt,
        id,
        sequence,
        (&hash, bytes.len()),
        (duplicate_of, revision_of),
    )?;
    bodies.put(tx, id, None, bytes)?;
    let deliveries = if duplicate_of.is_none() && receipt.initial_state == MessageState::Received {
        create_deliveries(tx, receipt, id, Timestamp::now())?
    } else {
        Vec::new()
    };
    count_receipt(tx, receipt, duplicate_of.is_some(), bytes.len())?;
    Ok(ReceiptOutcome {
        message_id: id,
        sequence,
        duplicate_of,
        revision_of,
        deliveries,
    })
}

fn log_commit(channel: &str, outcome: &ReceiptOutcome) {
    tracing::debug!(channel_id=%channel,message_id=%outcome.message_id,sequence=outcome.sequence,"message.committed");
    if outcome.revision_of.is_some() {
        tracing::info!(channel_id=%channel,message_id=%outcome.message_id,"message.identity_reused");
    }
}

fn validate_receipt(receipt: &Receipt) -> Result<()> {
    if receipt.channel_id.is_empty() || receipt.source_endpoint_id.is_empty() {
        return Err(Error::Invalid(
            "channel and source endpoint required".into(),
        ));
    }
    if !matches!(
        receipt.initial_state,
        MessageState::Received
            | MessageState::Receiving
            | MessageState::Invalid
            | MessageState::Aborted
            | MessageState::Observed
    ) {
        return Err(Error::Invalid("invalid initial receipt state".into()));
    }
    if receipt.initial_state != MessageState::Received && !receipt.destinations.is_empty() {
        return Err(Error::Invalid(
            "only received messages can have destinations".into(),
        ));
    }
    Ok(())
}

pub(crate) fn next_sequence(tx: &Transaction<'_>, channel: &str) -> Result<u64> {
    Ok(tx.query_row_cached("INSERT INTO channel_sequences(channel_id,last_seq) VALUES(?,1) ON CONFLICT(channel_id) DO UPDATE SET last_seq=last_seq+1 RETURNING last_seq",[channel],|r|r.get(0))?)
}

fn duplicate(
    tx: &Transaction<'_>,
    receipt: &Receipt,
    hash: &str,
) -> Result<(Option<MessageId>, Option<MessageId>)> {
    let identity = receipt.identifiers.source_identity.as_deref();
    if !receipt.duplicate_detection
        || receipt.origin != MessageOrigin::Received
        || receipt.initial_state != MessageState::Received
        || identity.is_none()
    {
        return Ok((None, None));
    }
    duplicate_identity(tx, &receipt.channel_id, identity, hash)
}

pub(crate) fn duplicate_identity(
    tx: &Transaction<'_>,
    channel: &str,
    identity: Option<&str>,
    hash: &str,
) -> Result<(Option<MessageId>, Option<MessageId>)> {
    let identity = identity.filter(|value| value.chars().any(|c| !c.is_whitespace() && c != '|'));
    if identity.is_none() {
        return Ok((None, None));
    }
    let eligible = "FROM messages WHERE channel_id=?1 AND source_identity=?2 AND state IN ('received','routed','filtered')";
    let duplicate = tx
        .query_row(
            &format!("SELECT id {eligible} AND content_hash=?3 ORDER BY seq DESC LIMIT 1"),
            params![channel, identity, hash],
            |r| r.get(0),
        )
        .optional()?;
    let revision = if duplicate.is_none() {
        tx.query_row(
            &format!("SELECT id {eligible} ORDER BY seq DESC LIMIT 1"),
            params![channel, identity],
            |r| r.get(0),
        )
        .optional()?
    } else {
        None
    };
    Ok((duplicate, revision))
}

fn insert_message(
    tx: &Transaction<'_>,
    r: &Receipt,
    id: MessageId,
    seq: u64,
    body: (&str, usize),
    links: (Option<MessageId>, Option<MessageId>),
) -> Result<()> {
    let i = &r.identifiers;
    let state = if links.0.is_some() {
        MessageState::Duplicate
    } else {
        r.initial_state
    };
    tx.execute_cached("INSERT INTO messages(id,channel_id,channel_label,seq,received_at,committed_at,direction,origin,protocol,state,control_id,sample_id,patient_id,message_type,byte_length,content_hash,source_identity,source_connector_id,source_endpoint_id,source_peer,source_file,config_version,duplicate_of,revision_of,searchable_text) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
        params![id,r.channel_id,r.channel_label,seq,r.received_at,if r.initial_state==MessageState::Receiving {None}else{Some(Timestamp::now())},r.direction.to_string(),r.origin.to_string(),r.protocol.to_string(),state.to_string(),i.control_id,i.sample_id,i.patient_id,i.message_type,body.1 as u64,body.0,i.source_identity,r.source_connector_id,r.source_endpoint_id,r.source_peer,r.source_file.as_ref().map(serde_json::to_string).transpose()?,r.config_version,links.0,links.1,i.searchable_text])?;
    Ok(())
}

pub(crate) fn create_deliveries(
    tx: &Transaction<'_>,
    r: &Receipt,
    id: MessageId,
    now: Timestamp,
) -> Result<Vec<DeliveryId>> {
    let ids = r
        .destinations
        .iter()
        .map(|d| insert_delivery(tx, id, &r.channel_id, d, now))
        .collect::<Result<Vec<_>>>()?;
    if !ids.is_empty() {
        tx.execute_cached("UPDATE messages SET state='routed' WHERE id=?", [id])?;
    }
    Ok(ids)
}

fn insert_delivery(
    tx: &Transaction<'_>,
    message: MessageId,
    channel: &str,
    d: &Destination,
    now: Timestamp,
) -> Result<DeliveryId> {
    if d.max_attempts == 0 || d.ack_required == AckLevel::None {
        return Err(Error::Invalid(
            "delivery policy requires attempts and acknowledgement level".into(),
        ));
    }
    let id = DeliveryId::generate();
    let state = if d.transform_error.is_some() {
        "failed"
    } else {
        "queued"
    };
    tx.execute_cached("INSERT INTO deliveries(id,message_id,channel_id,destination_id,destination_label,state,ack_level,attempts,total_attempts,created_at,updated_at,max_attempts,retry_unacknowledged,ack_required,reason_code,last_error,seq) VALUES(?1,?2,?3,?4,?5,?6,'none',0,0,?7,?7,?8,?9,?10,?11,?12,(SELECT seq FROM messages WHERE id=?2))",params![id,message,channel,d.id,d.label,state,now,d.max_attempts,d.retry_unacknowledged,d.ack_required.to_string(),d.transform_error.as_ref().map(|_|"transform_error"),d.transform_error])?;
    if let Some(summary) = &d.transform_error {
        note_error(tx, channel, &d.id, summary, now)?;
        increment(tx, channel, &d.id, "failed", 1)?;
        increment(tx, channel, "", "errors", 1)?;
    }
    Ok(id)
}

fn count_receipt(tx: &Transaction<'_>, r: &Receipt, duplicate: bool, size: usize) -> Result<()> {
    increment(tx, &r.channel_id, "", "received", 1)?;
    increment(tx, &r.channel_id, "", "bytes_in", size as u64)?;
    if duplicate {
        increment(tx, &r.channel_id, "", "duplicates", 1)?;
    }
    if matches!(
        r.initial_state,
        MessageState::Invalid | MessageState::Aborted
    ) {
        increment(
            tx,
            &r.channel_id,
            "",
            if r.initial_state == MessageState::Invalid {
                "invalid"
            } else {
                "aborted"
            },
            1,
        )?;
        increment(tx, &r.channel_id, "", "errors", 1)?;
    }
    Ok(())
}

/// Records a delivery's output body, when there is one.
fn store_output(
    tx: &Transaction<'_>,
    bodies: &BodyStorage,
    message: MessageId,
    delivery: DeliveryId,
    output: Option<&Vec<u8>>,
) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let hash = bodies.put(tx, message, Some(delivery), output)?;
    tx.execute_cached(
        "UPDATE deliveries SET transformed_hash=? WHERE id=?",
        params![hash, delivery],
    )?;
    Ok(())
}

fn validate_route<'a>(
    tx: &Transaction<'_>,
    message: MessageId,
    mut destinations: impl Iterator<Item = &'a str>,
) -> Result<()> {
    let targets: HashSet<String> = column(
        tx,
        "SELECT destination_id FROM resend_targets WHERE message_id=?",
        [message],
    )?
    .into_iter()
    .collect();
    if !targets.is_empty() && destinations.any(|id| !targets.contains(id)) {
        return Err(Error::Invalid(
            "destination was not selected for this resend".into(),
        ));
    }
    Ok(())
}
