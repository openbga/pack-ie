//! Persisted alert episodes. Retention of cleared episodes lives with the other purges.

use packie_core::{AlertId, Timestamp};
use rusqlite::{OptionalExtension, Row, Transaction, params};

use crate::Cached;
use crate::query::parse;
use crate::{Alert, AlertFilter, Result, Store};

impl Store {
    /// Raises one stable condition identity. Repeats update the active episode; a new
    /// episode after clearing uses the caller's fresh `UUIDv7`.
    ///
    /// # Errors
    /// [`Error::Invalid`](crate::Error::Invalid) when a new episode reuses an existing
    /// identifier, or the write fails.
    pub async fn record_alert_raised(&self, alert: Alert) -> Result<AlertId> {
        self.write(move |tx, _| raise_alert(tx, &alert)).await
    }

    /// Clears the active episode, retaining it in history. Repeated clears are harmless.
    ///
    /// # Errors
    /// The write fails.
    pub async fn record_alert_cleared(
        &self,
        identity: impl AsRef<str>,
        now: Timestamp,
    ) -> Result<()> {
        let identity = identity.as_ref().to_owned();
        self.write(move |tx, _| {
            tx.execute_cached("UPDATE alerts SET state = 'cleared', cleared_at = ? WHERE identity = ? AND state = 'active'", params![now, identity])?;
            Ok(())
        }).await
    }

    /// Returns alert episodes in descending last-seen order. A zero limit defaults to 100.
    ///
    /// # Errors
    /// The database cannot be read, or a stored episode is malformed.
    pub fn alert_history(&self, filter: &AlertFilter) -> Result<Vec<Alert>> {
        self.read(|conn| {
            let mut statement = conn.prepare(
                "SELECT * FROM alerts WHERE (?1 IS NULL OR channel_id = ?1)
                 AND (?2 IS NULL OR endpoint_id = ?2) AND (?3 IS NULL OR code = ?3)
                 AND (?4 = 0 OR state = 'active') AND (?5 IS NULL OR last_seen_at >= ?5)
                 AND (?6 IS NULL OR first_seen_at <= ?6)
                 ORDER BY last_seen_at DESC, id DESC LIMIT ?7 OFFSET ?8",
            )?;
            Ok(statement
                .query_map(
                    params![
                        filter.channel_id,
                        filter.endpoint_id,
                        filter.code,
                        filter.active_only,
                        filter.since,
                        filter.until,
                        if filter.limit == 0 {
                            100
                        } else {
                            filter.limit.min(1000)
                        },
                        filter.offset
                    ],
                    alert_row,
                )?
                .collect::<rusqlite::Result<_>>()?)
        })
    }
}

fn raise_alert(tx: &Transaction<'_>, alert: &Alert) -> Result<AlertId> {
    let existing: Option<AlertId> = tx.query_row_cached("SELECT id FROM alerts WHERE identity = ? AND state = 'active' ORDER BY last_seen_at DESC LIMIT 1",
        [&alert.identity], |row| row.get(0)).optional()?;
    let id = existing.unwrap_or(alert.id);
    if existing.is_none()
        && tx.query_row_cached(
            "SELECT EXISTS(SELECT 1 FROM alerts WHERE id = ?)",
            [id],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Err(crate::Error::Invalid(
            "a new alert episode requires a fresh identifier".into(),
        ));
    }
    let details = serde_json::to_string(&alert.details)?;
    tx.execute_cached("INSERT INTO alerts(id, identity, code, severity, state, channel_id, endpoint_id,
        destination_id, summary, next_action, first_seen_at, last_seen_at, occurrences, details_json)
        VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12)
        ON CONFLICT(id) DO UPDATE SET code = excluded.code, severity = excluded.severity,
        channel_id = excluded.channel_id, endpoint_id = excluded.endpoint_id,
        destination_id = excluded.destination_id, summary = excluded.summary,
        next_action = excluded.next_action, first_seen_at = MIN(first_seen_at, excluded.first_seen_at),
        last_seen_at = MAX(last_seen_at, excluded.last_seen_at), occurrences = occurrences + 1,
        details_json = excluded.details_json", params![id, alert.identity, alert.code, alert.severity.as_str(),
        alert.channel_id, alert.endpoint_id, alert.destination_id, alert.summary, alert.next_action,
        alert.first_seen_at, alert.last_seen_at, details])?;
    Ok(id)
}

fn alert_row(row: &Row<'_>) -> rusqlite::Result<Alert> {
    let details: String = row.get("details_json")?;
    let details = serde_json::from_str(&details).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(Alert {
        id: row.get("id")?,
        identity: row.get("identity")?,
        code: row.get("code")?,
        severity: parse(row.get("severity")?)?,
        channel_id: row.get("channel_id")?,
        endpoint_id: row.get("endpoint_id")?,
        destination_id: row.get("destination_id")?,
        summary: row.get("summary")?,
        next_action: row.get("next_action")?,
        first_seen_at: row.get("first_seen_at")?,
        last_seen_at: row.get("last_seen_at")?,
        cleared_at: row.get("cleared_at")?,
        occurrences: row.get("occurrences")?,
        details,
    })
}
