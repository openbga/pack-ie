//! Durable accumulation of multi-frame source transfers.

use crate::Cached;
use crate::counters::increment;
use crate::receipt::duplicate_identity;
use crate::{Error, Identifiers, ReceiptOutcome, Result, Store};
use packie_core::{MessageId, Timestamp};
use rusqlite::{OptionalExtension, Transaction, params};

struct Receiving {
    channel: String,
    sequence: u64,
    origin: String,
}

type Links = (Option<MessageId>, Option<MessageId>);

impl Store {
    /// Durably appends one accepted frame before its protocol acknowledgement. The
    /// final frame supplies aggregate identifiers and makes the transfer processable.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown message; [`Error::Invalid`] unless it is
    /// still `receiving`.
    pub async fn append_receiving(
        &self,
        message: MessageId,
        chunk: Vec<u8>,
        complete: bool,
        identifiers: Identifiers,
        duplicate_detection: bool,
    ) -> Result<ReceiptOutcome> {
        self.write(move |tx, bodies| {
            let receiving = receiving(tx, message)?;
            let mut bytes = bodies.get(tx, message, None)?;
            bytes.extend_from_slice(&chunk);
            let hash = bodies.put(tx, message, None, &bytes)?;
            tx.execute_cached(
                "UPDATE messages SET byte_length=?1,content_hash=?2 WHERE id=?3",
                params![bytes.len() as u64, hash, message],
            )?;
            increment(tx, &receiving.channel, "", "bytes_in", chunk.len() as u64)?;
            let (duplicate_of, revision_of) = if complete {
                let links = completed_links(
                    tx,
                    &receiving.channel,
                    &receiving.origin,
                    &identifiers,
                    &hash,
                    duplicate_detection,
                )?;
                complete_transfer(tx, message, &identifiers, links)?;
                links
            } else {
                (None, None)
            };
            if duplicate_of.is_some() {
                increment(tx, &receiving.channel, "", "duplicates", 1)?;
            }
            tracing::debug!(channel_id=%receiving.channel,message_id=%message,
                complete,byte_length=bytes.len(),"message.frame_committed");
            Ok(ReceiptOutcome {
                message_id: message,
                sequence: receiving.sequence,
                duplicate_of,
                revision_of,
                deliveries: Vec::new(),
            })
        })
        .await
    }

    /// Completes a transfer whose protocol layer assembled the message: `original` (for
    /// ASTM the records, without transport framing) replaces the accumulated frames as the
    /// message's original bytes, and the duplicate check runs on it.
    ///
    /// # Errors
    /// As [`Store::append_receiving`].
    pub async fn complete_receiving(
        &self,
        message: MessageId,
        original: Vec<u8>,
        identifiers: Identifiers,
        duplicate_detection: bool,
    ) -> Result<ReceiptOutcome> {
        self.write(move |tx, bodies| {
            let receiving = receiving(tx, message)?;
            let hash = bodies.put(tx, message, None, &original)?;
            tx.execute_cached(
                "UPDATE messages SET byte_length=?1,content_hash=?2 WHERE id=?3",
                params![original.len() as u64, hash, message],
            )?;
            let links = completed_links(
                tx,
                &receiving.channel,
                &receiving.origin,
                &identifiers,
                &hash,
                duplicate_detection,
            )?;
            complete_transfer(tx, message, &identifiers, links)?;
            if links.0.is_some() {
                increment(tx, &receiving.channel, "", "duplicates", 1)?;
            }
            Ok(ReceiptOutcome {
                message_id: message,
                sequence: receiving.sequence,
                duplicate_of: links.0,
                revision_of: links.1,
                deliveries: Vec::new(),
            })
        })
        .await
    }

    /// Ends an unfinished transfer while retaining every committed frame.
    ///
    /// # Errors
    /// [`Error::Invalid`] for an unknown abort reason or a message not `receiving`;
    /// [`Error::NotFound`] for an unknown message.
    pub async fn abort_receiving(&self, message: MessageId, reason: String) -> Result<()> {
        if !matches!(
            reason.as_str(),
            "retry_limit_exceeded"
                | "receive_timeout"
                | "incomplete_record"
                | "empty_transfer"
                | "message_too_large"
                | "connection_lost"
                | "transport_error"
                | "channel_stopped"
                | "file_too_large"
                | "startup_interrupted"
        ) {
            return Err(Error::Invalid("unknown transfer abort reason".into()));
        }
        self.write(move |tx, _| {
            let receiving = receiving(tx, message)?;
            tx.execute_cached(
                "UPDATE messages SET state='aborted',abort_reason=?1 WHERE id=?2",
                params![reason, message],
            )?;
            increment(tx, &receiving.channel, "", "aborted", 1)?;
            increment(tx, &receiving.channel, "", "errors", 1)?;
            tracing::info!(channel_id=%receiving.channel,message_id=%message,
                reason_code=%reason,"message.aborted");
            Ok(())
        })
        .await
    }
}

fn receiving(tx: &Transaction<'_>, message: MessageId) -> Result<Receiving> {
    let (receiving, state) = tx
        .query_row_cached(
            "SELECT channel_id,seq,origin,state FROM messages WHERE id=?1",
            [message],
            |row| {
                Ok((
                    Receiving {
                        channel: row.get(0)?,
                        sequence: row.get(1)?,
                        origin: row.get(2)?,
                    },
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    if state != "receiving" {
        return Err(Error::Invalid(
            "transfer requires a receiving message".into(),
        ));
    }
    Ok(receiving)
}

fn completed_links(
    tx: &Transaction<'_>,
    channel: &str,
    origin: &str,
    identifiers: &Identifiers,
    hash: &str,
    enabled: bool,
) -> Result<Links> {
    if !enabled || origin != "received" {
        return Ok((None, None));
    }
    duplicate_identity(tx, channel, identifiers.source_identity.as_deref(), hash)
}

fn complete_transfer(
    tx: &Transaction<'_>,
    message: MessageId,
    identifiers: &Identifiers,
    links: Links,
) -> Result<()> {
    let state = if links.0.is_some() {
        "duplicate"
    } else {
        "received"
    };
    tx.execute_cached(
        "UPDATE messages SET state=?1,committed_at=?2,control_id=?3,sample_id=?4,
            patient_id=?5,message_type=?6,source_identity=?7,searchable_text=?8,
            duplicate_of=?9,revision_of=?10 WHERE id=?11",
        params![
            state,
            Timestamp::now(),
            identifiers.control_id,
            identifiers.sample_id,
            identifiers.patient_id,
            identifiers.message_type,
            identifiers.source_identity,
            identifiers.searchable_text,
            links.0,
            links.1,
            message
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{opened, receipt as base_receipt};
    use crate::{OpenOptions, Receipt};
    use packie_core::{MessageState, ProtocolKind};
    use rusqlite::Connection;

    fn receipt() -> Receipt {
        Receipt {
            protocol: ProtocolKind::Astm,
            initial_state: MessageState::Receiving,
            ..base_receipt()
        }
    }

    fn identifiers() -> Identifiers {
        Identifiers {
            source_identity: Some("instrument|sample".into()),
            sample_id: Some("sample".into()),
            searchable_text: "safe metadata".into(),
            ..Identifiers::default()
        }
    }

    async fn completed(store: &Store, receipt: &Receipt, tail: &[u8]) -> ReceiptOutcome {
        let started = store
            .commit_received(receipt.clone(), b"H".to_vec())
            .await
            .unwrap();
        store
            .append_receiving(started.message_id, tail.to_vec(), true, identifiers(), true)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn accumulated_frames_are_durable_before_completion_and_keep_sequence() {
        let (directory, store) = opened();
        let receipt = receipt();
        let first = store
            .commit_received(receipt.clone(), b"H|".to_vec())
            .await
            .unwrap();
        let next = store
            .append_receiving(
                first.message_id,
                b"P|".to_vec(),
                false,
                Identifiers::default(),
                true,
            )
            .await
            .unwrap();
        assert_eq!(next.sequence, first.sequence);
        let connection = Connection::open(store.database_path()).unwrap();
        let bytes: Vec<u8> = connection.query_row(
            "SELECT c.inline_bytes FROM message_bodies b JOIN body_contents c USING(content_hash)
             WHERE b.message_id=?1", [first.message_id], |row| row.get(0)).unwrap();
        assert_eq!(bytes, b"H|P|");
        assert_eq!(
            store.message(first.message_id).unwrap().summary.state,
            MessageState::Receiving
        );
        drop(connection);
        drop(store);
        let store = Store::open(directory.path(), OpenOptions::default()).unwrap();
        let complete = store
            .append_receiving(first.message_id, b"L|".to_vec(), true, identifiers(), true)
            .await
            .unwrap();
        assert_eq!(complete.sequence, first.sequence);
        let record = store.message(first.message_id).unwrap();
        assert_eq!(record.summary.state, MessageState::Received);
        assert_eq!(record.summary.sample_id.as_deref(), Some("sample"));
        assert!(record.committed_at.is_some());
        assert_eq!(store.body(first.message_id, None).unwrap(), b"H|P|L|");
        assert_eq!(record.content_hash, crate::bodies::content_hash(b"H|P|L|"));
        let counts = store.counters(&receipt.channel_id, None).unwrap();
        assert_eq!((counts.received, counts.bytes_in), (1, 6));
        assert!(
            store
                .append_receiving(first.message_id, vec![], true, identifiers(), true)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn completion_detects_duplicates_and_revisions_and_aborts_preserve_bytes() {
        let (_directory, store) = opened();
        let receipt = receipt();
        let first = completed(&store, &receipt, b"L").await;
        let duplicate = completed(&store, &receipt, b"L").await;
        assert_eq!(duplicate.duplicate_of, Some(first.message_id));
        assert_eq!(
            store.message(duplicate.message_id).unwrap().summary.state,
            MessageState::Duplicate
        );
        let revision = completed(&store, &receipt, b"R").await;
        assert_eq!(revision.revision_of, Some(first.message_id));
        let aborted = store
            .commit_received(receipt.clone(), b"partial".to_vec())
            .await
            .unwrap();
        assert!(
            store
                .abort_receiving(aborted.message_id, "unknown".into())
                .await
                .is_err()
        );
        store
            .abort_receiving(aborted.message_id, "receive_timeout".into())
            .await
            .unwrap();
        let record = store.message(aborted.message_id).unwrap();
        assert_eq!(record.summary.state, MessageState::Aborted);
        assert_eq!(record.abort_reason.as_deref(), Some("receive_timeout"));
        assert_eq!(store.body(aborted.message_id, None).unwrap(), b"partial");
        assert!(
            store
                .abort_receiving(aborted.message_id, "receive_timeout".into())
                .await
                .is_err()
        );
        assert!(
            store
                .append_receiving(
                    aborted.message_id,
                    b"late".to_vec(),
                    true,
                    identifiers(),
                    true
                )
                .await
                .is_err()
        );
        let counts = store.counters(&receipt.channel_id, None).unwrap();
        assert_eq!(
            (
                counts.received,
                counts.duplicates,
                counts.aborted,
                counts.errors
            ),
            (4, 1, 1, 1)
        );
        assert_eq!(counts.bytes_in, 13);
    }
}
