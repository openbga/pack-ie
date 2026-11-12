//! The single writer: one connection to the current file, the chain head, and sealing at
//! the turn of the year.

use std::path::{Path, PathBuf};
use std::time::Duration;

use packie_core::Timestamp;
use packie_ipc::audit::{AuditAction, AuditObjectType};
use rusqlite::{Connection, OptionalExtension, params};

use crate::chain::{self, Fields, GENESIS};
use crate::files::{self, CURRENT};
use crate::{Actor, Entry, Error, Result};

/// The newest event of the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Head {
    pub(crate) seq: u64,
    pub(crate) hash: String,
}

/// The one connection to the current file, with the chain head it appends to.
pub(crate) struct Writer {
    dir: PathBuf,
    busy_timeout: Duration,
    conn: Connection,
    pub(crate) head: Head,
    /// The current file's first event: its seq and UTC year.
    first: Option<(u64, i16)>,
    /// The newest anchor in the current file.
    last_anchor: Option<(u64, Timestamp)>,
}

impl Writer {
    /// Opens (creating and migrating) the current file; an empty one continues the chain of
    /// the newest sealed file.
    pub(crate) fn open(dir: &Path, busy_timeout: Duration) -> Result<Self> {
        files::private_dir(dir)?;
        let path = dir.join(CURRENT);
        files::create_private(&path)?;
        let mut conn = Connection::open(&path)?;
        conn.busy_timeout(busy_timeout)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Commits reach the OS at once (an engine crash loses nothing); the WAL is synced
        // to disk by the checkpoint the maintenance thread runs every second.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        crate::schema::migrate(&mut conn)?;
        let head = match files::last_event(&conn)? {
            Some((seq, hash, _)) => Head { seq, hash },
            None => sealed_head(dir)?,
        };
        let first = files::first_event(&conn)?.map(|(seq, at)| (seq, year_of(at)));
        let last_anchor = conn
            .query_row(
                "SELECT seq, at FROM events WHERE action = 'audit_anchor' ORDER BY seq DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(Self {
            dir: dir.to_owned(),
            busy_timeout,
            conn,
            head,
            first,
            last_anchor,
        })
    }

    /// Appends one event at `at`, sealing the current file first when `at` is in a later
    /// year than the file's first event. A seal that cannot complete is retried at the next
    /// event; the event itself still goes into the current file.
    pub(crate) fn append(
        slot: &mut Option<Self>,
        entry: &Entry,
        details: &str,
        at: Timestamp,
    ) -> Result<u64> {
        let due = slot
            .as_ref()
            .and_then(|writer| writer.first)
            .is_some_and(|(_, year)| year_of(at) > year);
        if due && let Some(writer) = slot.take() {
            *slot = Some(writer.seal(at)?);
        }
        let Some(writer) = slot.as_mut() else {
            return Err(Error::Unavailable("the audit writer is closed".into()));
        };
        writer.insert(entry, details, at)
    }

    fn insert(&mut self, entry: &Entry, details: &str, at: Timestamp) -> Result<u64> {
        let seq = self.head.seq + 1;
        let at_text = at.to_string();
        let (object_type, object_id) = match &entry.object {
            Some((kind, id)) => (Some(kind.as_str()), Some(id.as_str())),
            None => (None, None),
        };
        let fields = Fields {
            seq,
            at: &at_text,
            actor_uid: &entry.actor.uid_or_sid,
            actor_name: &entry.actor.name,
            origin: crate::origin_of(&entry.actor).as_str(),
            action: entry.action.as_str(),
            object_type,
            object_id,
            outcome: entry.outcome.as_str(),
            details,
        };
        let hash = chain::hash(&self.head.hash, &fields);
        let seq_value =
            i64::try_from(seq).map_err(|_| Error::invalid("the audit sequence is exhausted"))?;
        self.conn
            .prepare_cached(
                "INSERT INTO events (seq, at, actor_uid, actor_name, origin, action, object_type, \
                 object_id, outcome, details, prev_hash, hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?
            .execute(params![
                seq_value,
                at_text,
                fields.actor_uid,
                fields.actor_name,
                fields.origin,
                fields.action,
                fields.object_type,
                fields.object_id,
                fields.outcome,
                details,
                self.head.hash,
                hash,
            ])?;
        self.head = Head { seq, hash };
        if self.first.is_none() {
            self.first = Some((seq, year_of(at)));
        }
        if entry.action == AuditAction::AuditAnchor {
            self.last_anchor = Some((seq, at));
        }
        Ok(seq)
    }

    /// Events arrived since the newest anchor in this file (or it has none yet).
    pub(crate) fn has_unanchored_events(&self) -> bool {
        self.head.seq > self.last_anchor.map_or(0, |(seq, _)| seq)
    }

    /// Whether an anchor is due at `now`: events arrived and the newest anchor is at least
    /// `interval` old.
    pub(crate) fn anchor_due(&self, now: Timestamp, interval: Duration) -> bool {
        self.has_unanchored_events()
            && self
                .last_anchor
                .is_none_or(|(_, at)| now.saturating_duration_since(at) >= interval)
    }

    /// Syncs the WAL to disk and folds it into the database file.
    pub(crate) fn checkpoint(&self) -> Result<()> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// Renames the current file to its sealed name, read-only, and opens a new current
    /// file whose first event (`audit_seal`) links to the sealed file's last. When the
    /// rename is impossible (a reader holds the file on Windows), the current file stays.
    fn seal(self, at: Timestamp) -> Result<Self> {
        let Some((first_seq, year)) = self.first else {
            return Ok(self);
        };
        let (dir, busy_timeout) = (self.dir.clone(), self.busy_timeout);
        let last_seq = self.head.seq;
        let name = files::sealed_name(year, first_seq);
        let prepared = self
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;");
        if let Err(error) = prepared {
            warn_seal_deferred(&error);
            return Ok(self);
        }
        // Closing or renaming failed: the file is intact, so reopen it and retry later.
        let sealed = dir.join(&name);
        let renamed = self
            .conn
            .close()
            .map_err(|(_, error)| error.to_string())
            .and_then(|()| {
                std::fs::rename(dir.join(CURRENT), &sealed).map_err(|error| error.to_string())
            });
        if let Err(error) = renamed {
            warn_seal_deferred(&error);
            return Self::open(&dir, busy_timeout);
        }
        files::restrict(&sealed, true)?;
        files::sync_dir(&dir)?;
        let mut next = Self::open(&dir, busy_timeout)?;
        let details = chain::canonical_json(&serde_json::json!({
            "year": year,
            "first_seq": first_seq,
            "last_seq": last_seq,
        }));
        let entry = Entry::new(Actor::system(), AuditAction::AuditSeal)
            .object(AuditObjectType::Audit, name.clone());
        next.insert(&entry, &details, at)?;
        tracing::info!(event = "audit.sealed", file = %name, last_seq, "Audit file for {year} sealed as {name}");
        Ok(next)
    }
}

fn warn_seal_deferred(error: &dyn std::fmt::Display) {
    tracing::warn!(event = "audit.seal_deferred", %error, "Audit file not sealed yet; retrying at the next event");
}

/// The head of the newest sealed file, or the start of the chain.
fn sealed_head(dir: &Path) -> Result<Head> {
    let sealed = files::list(dir)?
        .into_iter()
        .rev()
        .find(|file| !file.is_current());
    if let Some(file) = sealed
        && let Some((seq, hash, _)) = files::last_event(&files::read_only(&file.path)?)?
    {
        return Ok(Head { seq, hash });
    }
    Ok(Head {
        seq: 0,
        hash: GENESIS.to_owned(),
    })
}

/// The UTC year of `at`.
fn year_of(at: Timestamp) -> i16 {
    at.as_jiff().to_zoned(jiff::tz::TimeZone::UTC).year()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn years_are_utc() {
        let at = Timestamp::parse("2026-12-31T23:30:00-02:00").unwrap();
        assert_eq!(year_of(at), 2027);
    }
}
