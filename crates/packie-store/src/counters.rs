//! Persistent per-channel and per-destination counters and gauges: the write helpers every
//! mutation uses, and the reads behind the node snapshot.

use std::collections::BTreeMap;

use packie_core::Timestamp;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::Cached;
use crate::{Counters, DestinationCounts, Error, Result, Store};

impl Store {
    /// Persistent monotonic counters; `None` selects channel-wide totals.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn counters(&self, channel: &str, destination: Option<&str>) -> Result<Counters> {
        self.read(|conn| read_counters(conn, channel, destination.unwrap_or("")))
    }

    /// Current queue and unresolved counts plus persistent destination totals.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn destination_counts(
        &self,
        channel: &str,
        destination: &str,
        now: Timestamp,
    ) -> Result<DestinationCounts> {
        self.snapshot(|tx| {
            let mut counts = destination_gauges(tx, channel, destination)?;
            counts.totals = read_counters(tx, channel, destination)?;
            counts.oldest_queued_age_seconds = counts
                .oldest_queued_at
                .map(|at| now.saturating_duration_since(at).as_secs());
            Ok(counts)
        })
    }
}

/// Adds `amount` to one counter column of a channel (`destination` empty) or destination.
pub(crate) fn increment(
    tx: &Transaction<'_>,
    channel: &str,
    destination: &str,
    column: &str,
    amount: u64,
) -> Result<()> {
    if !matches!(
        column,
        "received"
            | "filtered"
            | "duplicates"
            | "errors"
            | "invalid"
            | "aborted"
            | "bytes_in"
            | "attempts"
            | "acknowledged"
            | "rejected"
            | "timed_out"
            | "outcome_unknown"
            | "failed"
            | "bytes_out"
    ) {
        return Err(Error::Invalid("unknown counter".into()));
    }
    tx.prepare_cached(&format!("INSERT INTO channel_counters(channel_id,destination_id,{column}) VALUES(?1,?2,?3) ON CONFLICT(channel_id,destination_id) DO UPDATE SET {column}={column}+excluded.{column}"))?
        .execute(params![channel, destination, amount])?;
    Ok(())
}

/// Moves a destination's `last_attempt_at` or `last_ack_at` gauge forward to `at`.
pub(crate) fn note_time(
    tx: &Transaction<'_>,
    channel: &str,
    destination: &str,
    column: &str,
    at: Timestamp,
) -> Result<()> {
    if !matches!(column, "last_attempt_at" | "last_ack_at") {
        return Err(Error::Invalid("unknown gauge".into()));
    }
    tx.prepare_cached(&format!(
        "INSERT INTO channel_counters(channel_id,destination_id,{column}) VALUES(?1,?2,?3)
         ON CONFLICT(channel_id,destination_id) DO UPDATE SET {column}=excluded.{column}
         WHERE {column} IS NULL OR {column}<=excluded.{column}"
    ))?
    .execute(params![channel, destination, at])?;
    Ok(())
}

/// Records a destination's latest failure summary (names a rule or condition, no data).
pub(crate) fn note_error(
    tx: &Transaction<'_>,
    channel: &str,
    destination: &str,
    summary: &str,
    at: Timestamp,
) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO channel_counters(channel_id,destination_id,last_error,last_error_at)
         VALUES(?1,?2,?3,?4) ON CONFLICT(channel_id,destination_id) DO UPDATE SET
         last_error=excluded.last_error,last_error_at=excluded.last_error_at
         WHERE last_error_at IS NULL OR last_error_at<=excluded.last_error_at",
    )?
    .execute(params![channel, destination, summary, at])?;
    Ok(())
}

fn read_counters(conn: &Connection, channel: &str, destination: &str) -> Result<Counters> {
    Ok(conn
        .query_row_cached(
            "SELECT * FROM channel_counters WHERE channel_id = ? AND destination_id = ?",
            params![channel, destination],
            |row| {
                Ok(Counters {
                    received: row.get("received")?,
                    filtered: row.get("filtered")?,
                    duplicates: row.get("duplicates")?,
                    errors: row.get("errors")?,
                    invalid: row.get("invalid")?,
                    aborted: row.get("aborted")?,
                    bytes_in: row.get("bytes_in")?,
                    attempts: row.get("attempts")?,
                    acknowledged: row.get("acknowledged")?,
                    rejected: row.get("rejected")?,
                    timed_out: row.get("timed_out")?,
                    outcome_unknown: row.get("outcome_unknown")?,
                    failed: row.get("failed")?,
                    bytes_out: row.get("bytes_out")?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

fn destination_gauges(
    conn: &Connection,
    channel: &str,
    destination: &str,
) -> Result<DestinationCounts> {
    // Only unresolved states: resolved history grows without bound and is counted by the
    // persistent totals instead.
    let states = conn.prepare("SELECT state, COUNT(*) FROM deliveries WHERE channel_id = ? AND destination_id = ?
            AND state IN ('queued', 'sending', 'rejected', 'timed_out', 'outcome_unknown', 'failed') GROUP BY state")?
        .query_map(params![channel, destination], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let (oldest_queued_at, next_attempt_at) = conn.query_row_cached(
        "SELECT MIN(m.received_at), MIN(d.next_attempt_at)
         FROM deliveries d JOIN messages m ON m.id = d.message_id
         WHERE d.channel_id = ? AND d.destination_id = ? AND d.state = 'queued'",
        params![channel, destination],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let (last_attempt_at, last_ack_at, last_error) = conn
        .query_row_cached(
            "SELECT last_attempt_at, last_ack_at, last_error FROM channel_counters
             WHERE channel_id = ? AND destination_id = ?",
            params![channel, destination],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .unwrap_or((None, None, None));
    let transform_errors = conn.query_row_cached(
        "SELECT COUNT(*) FROM deliveries WHERE channel_id = ? AND destination_id = ?
         AND state = 'failed' AND reason_code = 'transform_error'",
        params![channel, destination],
        |row| row.get(0),
    )?;
    Ok(DestinationCounts {
        states,
        transform_errors,
        oldest_queued_at,
        next_attempt_at,
        last_attempt_at,
        last_ack_at,
        last_error,
        ..DestinationCounts::default()
    })
}
