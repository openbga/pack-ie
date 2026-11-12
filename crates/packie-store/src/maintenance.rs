//! Recovery, retention and persisted operator diagnostics.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use packie_core::{MessageId, Timestamp};
use rusqlite::{Transaction, params};

use crate::Cached;
use crate::bodies;
use crate::counters::increment;
use crate::{
    BODIES_DIR, PurgeCounts, Reconciliation, Result, Retention, Store, StoreTotals, column,
};

/// Most messages (or cleared alerts) one retention transaction deletes. A transaction
/// holds every page it changes in memory until it commits (and again in its savepoint
/// journal), so the batch bounds the memory a retention run needs: about 7 MB.
const PURGE_BATCH: u64 = 100;
/// Most purge candidates one read selects; they are deleted [`PURGE_BATCH`] at a time.
const PURGE_SCAN: u64 = 1000;
const DAY: Duration = Duration::from_hours(24);
/// How long cleared alert episodes stay in history.
const ALERT_HISTORY_DAYS: u32 = 30;

impl Store {
    /// Returns interrupted transfers for host protocol parsers to inspect before recovery.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn receiving_messages(&self) -> Result<Vec<MessageId>> {
        self.read(|conn| {
            column(
                conn,
                "SELECT id FROM messages WHERE state = 'receiving' ORDER BY channel_id, seq",
                [],
            )
        })
    }

    /// Reconciles startup state conservatively, aborting partial receipts. Hosts that
    /// parsed complete committed transfers can use `reconcile_with_recovered` instead.
    ///
    /// # Errors
    /// The write fails.
    pub async fn reconcile_at_startup(&self) -> Result<Reconciliation> {
        self.reconcile_with_recovered(Vec::new()).await
    }

    /// Restores complete interrupted transfers identified by the host's protocol parser,
    /// aborts other partial receipts and returns complete receipts for pipeline reprocessing.
    ///
    /// # Errors
    /// The write fails.
    pub async fn reconcile_with_recovered(
        &self,
        recovered: Vec<MessageId>,
    ) -> Result<Reconciliation> {
        let report = self.write(move |tx, _| {
            let mut report = Reconciliation::default();
            for id in recovered {
                report.recovered += tx.execute_cached("UPDATE messages SET state = 'received', committed_at = COALESCE(committed_at, ?) WHERE id = ? AND state = 'receiving'",
                    params![Timestamp::now(), id])? as u64;
            }
            abort_interrupted(tx, &mut report)?;
            report.requeued = column(tx, "SELECT id FROM messages WHERE state = 'received' ORDER BY channel_id, seq", [])?;
            reconcile_sending(tx, &mut report)?;
            report.paused_channels = column(tx, "SELECT channel_id FROM channel_flags WHERE paused = 1 ORDER BY channel_id", [])?;
            Ok(report)
        }).await?;
        // The host logs `reconciliation.completed` once, with its own recovery counts.
        Ok(report)
    }

    /// Saves the operator's paused flag independently of channel configuration.
    ///
    /// # Errors
    /// The write fails.
    pub async fn set_paused(&self, channel: impl AsRef<str>, paused: bool) -> Result<()> {
        let channel = channel.as_ref().to_owned();
        self.write(move |tx, _| {
            tx.execute_cached("INSERT INTO channel_flags(channel_id, paused) VALUES (?, ?) ON CONFLICT(channel_id) DO UPDATE SET paused = excluded.paused", params![channel, paused])?;
            Ok(())
        }).await
    }

    /// Purges resolved history in bounded writer transactions. Omitted channels use the
    /// default retention, including history of channels removed from configuration.
    ///
    /// # Errors
    /// A read or write fails; batches already committed stay purged.
    pub async fn purge(
        &self,
        policies: BTreeMap<String, Retention>,
        now: Timestamp,
    ) -> Result<PurgeCounts> {
        let channels: Vec<String> = self.read(|conn| {
            column(
                conn,
                "SELECT DISTINCT channel_id FROM messages ORDER BY channel_id",
                [],
            )
        })?;
        let mut total = PurgeCounts::default();
        for channel in channels {
            let policy = policies.get(&channel).cloned().unwrap_or_default();
            let channel_total = self.purge_channel(channel.clone(), policy, now).await?;
            if channel_total.messages != 0 {
                tracing::info!(
                    event = "retention.purged",
                    channel_id = channel,
                    messages = channel_total.messages,
                    deliveries = channel_total.deliveries,
                    attempts = channel_total.attempts,
                    bodies = channel_total.bodies,
                    "Resolved message history purged"
                );
            }
            accumulate(&mut total, &channel_total);
        }
        // Deletions must commit before removing spill files. This second writer job
        // prevents a new body from being committed between reference checking and unlink.
        total.files = self.cleanup_bodies().await?;
        total.alerts = self.purge_alert_history(now).await?;
        Ok(total)
    }

    /// Selects the channel's purge candidates on a read connection, so the writer is not
    /// held while they are ranked, then deletes them in [`PURGE_BATCH`] transactions that
    /// check each one again.
    async fn purge_channel(
        &self,
        channel: String,
        policy: Retention,
        now: Timestamp,
    ) -> Result<PurgeCounts> {
        let mut total = PurgeCounts::default();
        loop {
            let ids = self.read(|conn| purge_candidates(conn, &channel, &policy, now))?;
            let before = total.messages;
            for batch in ids.chunks(usize::try_from(PURGE_BATCH).unwrap_or(usize::MAX)) {
                let batch = batch.to_vec();
                let counts = self.write(move |tx, _| purge_batch(tx, &batch)).await?;
                accumulate(&mut total, &counts);
            }
            // A short selection was the last; a pass that deleted nothing (every candidate
            // changed meanwhile) waits for the next run.
            if (ids.len() as u64) < PURGE_SCAN || total.messages == before {
                return Ok(total);
            }
        }
    }

    /// Removes cleared alert episodes after thirty days, preserving all active conditions.
    async fn purge_alert_history(&self, now: Timestamp) -> Result<u64> {
        let Some(cutoff) = now.checked_sub(DAY.saturating_mul(ALERT_HISTORY_DAYS)) else {
            return Ok(0);
        };
        let mut total = 0;
        loop {
            let count = self.write(move |tx, _| {
                Ok(tx.execute_cached("DELETE FROM alerts WHERE id IN (SELECT id FROM alerts WHERE state = 'cleared' AND cleared_at < ? ORDER BY cleared_at, id LIMIT ?)", params![cutoff, PURGE_BATCH])? as u64)
            }).await?;
            total += count;
            if count < PURGE_BATCH {
                return Ok(total);
            }
        }
    }

    /// Store-wide figures for the node snapshot: row counts and size on disk.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn totals(&self) -> Result<StoreTotals> {
        let (messages, deliveries_unresolved, deliveries_queued) = self.read(|conn| {
            Ok(conn.query_row_cached(
                "SELECT (SELECT COUNT(*) FROM messages),
                    (SELECT COUNT(*) FROM deliveries
                     WHERE state IN ('rejected','failed','timed_out','outcome_unknown')),
                    (SELECT COUNT(*) FROM deliveries WHERE state = 'queued')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?)
        })?;
        let database = self.database_path();
        let file = |path: &Path| std::fs::metadata(path).map_or(0, |m| m.len());
        let wal = database.with_extension("sqlite3-wal");
        let size_bytes =
            file(database) + file(&wal) + directory_size(&database.with_file_name(BODIES_DIR));
        Ok(StoreTotals {
            messages,
            deliveries_unresolved,
            deliveries_queued,
            size_bytes,
        })
    }
}

fn abort_interrupted(tx: &Transaction<'_>, report: &mut Reconciliation) -> Result<()> {
    let counts = tx.prepare("SELECT channel_id, COUNT(*) FROM messages WHERE state = 'receiving' GROUP BY channel_id")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (channel, count) in counts {
        increment(tx, &channel, "", "aborted", count)?;
        increment(tx, &channel, "", "errors", count)?;
        report.aborted += count;
    }
    tx.execute_cached("UPDATE messages SET state = 'aborted', abort_reason = 'startup_interrupted' WHERE state = 'receiving'", [])?;
    Ok(())
}

fn reconcile_sending(tx: &Transaction<'_>, report: &mut Reconciliation) -> Result<()> {
    let now = Timestamp::now();
    let counts = tx.prepare("SELECT channel_id, destination_id, COUNT(*) FROM deliveries WHERE state = 'sending' GROUP BY channel_id, destination_id")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, u64>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (channel, destination, count) in counts {
        increment(tx, &channel, &destination, "outcome_unknown", count)?;
        increment(tx, &channel, "", "outcome_unknown", count)?;
        increment(tx, &channel, "", "errors", count)?;
        report.unknown += count;
    }
    tx.execute_cached("UPDATE delivery_attempts SET ended_at = ?, result = 'engine_stopped', error_code = 'engine_restarted' WHERE ended_at IS NULL AND delivery_id IN (SELECT id FROM deliveries WHERE state = 'sending')", [now])?;
    tx.execute_cached("UPDATE deliveries SET state = 'outcome_unknown', reason_code = 'engine_restarted', updated_at = ?, next_attempt_at = NULL WHERE state = 'sending'", [now])?;
    Ok(())
}

/// The oldest messages `policy` lets go of, at most [`PURGE_SCAN`]: final, every
/// delivery resolved, and no kept revision pointing at them.
fn purge_candidates(
    conn: &rusqlite::Connection,
    channel: &str,
    policy: &Retention,
    now: Timestamp,
) -> Result<Vec<String>> {
    let cutoff = policy
        .max_age_days
        .and_then(|days| now.checked_sub(DAY.saturating_mul(days)));
    let max_messages = policy
        .max_messages
        .map(|count| i64::try_from(count).unwrap_or(i64::MAX));
    let mut statement = conn.prepare_cached(
        "WITH channel_total AS (SELECT COUNT(*) AS total FROM messages WHERE channel_id = ?1),
         eligible AS (SELECT id, received_at, seq FROM messages m
           WHERE channel_id = ?1 AND state NOT IN ('receiving', 'received')
           AND NOT EXISTS (SELECT 1 FROM deliveries d WHERE d.message_id = m.id
             AND d.state NOT IN ('acknowledged', 'cancelled'))
           -- An original stays while a kept message is its revision (CAP GEN.41310).
           AND NOT EXISTS (SELECT 1 FROM messages r WHERE r.revision_of = m.id)),
         ranked AS (SELECT id, received_at,
           ROW_NUMBER() OVER (ORDER BY received_at, seq) AS eligible_position FROM eligible)
         SELECT id FROM ranked
         WHERE (?2 IS NOT NULL AND received_at < ?2)
           OR (?3 IS NOT NULL AND eligible_position <= MAX((SELECT total FROM channel_total) - ?3, 0))
         ORDER BY eligible_position LIMIT ?4",
    )?;
    let ids = statement
        .query_map(params![channel, cutoff, max_messages, PURGE_SCAN], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

/// Deletes the candidates `ids` that are still purgeable (a delivery may have been queued
/// again, or a revision made, since they were selected), and the body contents only they
/// referenced, in one bounded transaction.
fn purge_batch(tx: &Transaction<'_>, ids: &[String]) -> Result<PurgeCounts> {
    let mut counts = PurgeCounts::default();
    let mut released = Vec::new();
    for id in ids {
        let (deliveries, attempts, bodies): (u64, u64, u64) = tx.query_row_cached(
            "SELECT (SELECT COUNT(*) FROM deliveries WHERE message_id = ?1),
             (SELECT COUNT(*) FROM delivery_attempts a JOIN deliveries d ON d.id = a.delivery_id WHERE d.message_id = ?1),
             (SELECT COUNT(*) FROM message_bodies WHERE message_id = ?1)", [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        let hashes: Vec<String> = {
            let mut statement =
                tx.prepare_cached("SELECT content_hash FROM message_bodies WHERE message_id = ?1")?;
            statement
                .query_map([id], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let deleted = tx.execute_cached(
            "DELETE FROM messages WHERE id = ?1 AND state NOT IN ('receiving', 'received')
             AND NOT EXISTS (SELECT 1 FROM deliveries d WHERE d.message_id = ?1
               AND d.state NOT IN ('acknowledged', 'cancelled'))
             AND NOT EXISTS (SELECT 1 FROM messages r WHERE r.revision_of = ?1)",
            [id],
        )?;
        if deleted == 0 {
            continue;
        }
        counts.messages += 1;
        counts.deliveries += deliveries;
        counts.attempts += attempts;
        counts.bodies += bodies;
        released.extend(hashes);
    }
    // Contents no other body shares go with them; spilled files are removed after the
    // commit by the body cleanup.
    for hash in released {
        bodies::release(tx, &hash)?;
    }
    Ok(counts)
}

fn accumulate(total: &mut PurgeCounts, counts: &PurgeCounts) {
    total.messages += counts.messages;
    total.deliveries += counts.deliveries;
    total.attempts += counts.attempts;
    total.bodies += counts.bodies;
    total.files += counts.files;
    total.alerts += counts.alerts;
}

/// Bytes of the regular files under `dir` (spilled bodies), recursively.
fn directory_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => directory_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use rusqlite::Connection;

    use super::*;

    /// A routed message with one delivery in `delivery_state` and an original body.
    fn routed(tx: &Transaction<'_>, delivery_state: &str, body: &[u8]) -> String {
        let id = MessageId::generate().to_string();
        tx.execute(
            "INSERT INTO messages
                (id, channel_id, channel_label, seq, received_at, direction, origin,
                 protocol, state, byte_length, content_hash, config_version)
             VALUES (?1, 'ch', 'Channel', (SELECT COUNT(*) FROM messages) + 1,
                 '2026-01-01T00:00:00Z', 'inbound', 'received', 'hl7_mllp', 'routed', 0, '', 1)",
            [&id],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO deliveries
                (id, message_id, channel_id, destination_id, destination_label, state,
                 ack_level, attempts, total_attempts, created_at, updated_at, max_attempts,
                 retry_unacknowledged, ack_required)
             VALUES (?1 || '-d', ?1, 'ch', 'lis', 'LIS', ?2, 'accepted', 1, 1,
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 3, 0, 'accepted')",
            params![id, delivery_state],
        )
        .unwrap();
        let hash = format!("hash-{id}");
        tx.execute(
            "INSERT INTO body_contents(content_hash, byte_length, inline_bytes) VALUES (?1, ?2, ?3)",
            params![hash, body.len(), body],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO message_bodies(message_id, kind, content_hash, byte_length)
             VALUES (?1, 'original', ?2, ?3)",
            params![id, hash, body.len()],
        )
        .unwrap();
        id
    }

    /// A candidate whose delivery was queued again after it was selected stays; a
    /// purgeable one goes with the body content only it referenced.
    #[test]
    fn a_batch_checks_each_candidate_again_and_releases_its_bodies() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        crate::schema::migrate(&mut connection).unwrap();
        let tx = connection.transaction().unwrap();
        let done = routed(&tx, "acknowledged", b"done");
        let requeued = routed(&tx, "queued", b"requeued");
        let counts = purge_batch(&tx, &[done, requeued.clone()]).unwrap();
        assert_eq!(
            (counts.messages, counts.deliveries, counts.bodies),
            (1, 1, 1)
        );
        let left: Vec<String> = column(&tx, "SELECT id FROM messages", []).unwrap();
        assert_eq!(left, [requeued]);
        let contents: Vec<String> =
            column(&tx, "SELECT content_hash FROM body_contents", []).unwrap();
        assert_eq!(contents, [format!("hash-{}", left[0])]);
    }
}
