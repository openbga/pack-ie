//! Background upkeep of the log: checkpoints, anchors, retention and the closing anchor.

use std::sync::atomic::Ordering;
use std::sync::{PoisonError, Weak};
use std::time::Duration;

use packie_core::Timestamp;
use packie_ipc::audit::{AuditAction, AuditObjectType};

use super::{Audit, Inner};
use crate::writer::Writer;
use crate::{Actor, Entry, Result, files};

/// Time between checkpoints; full maintenance runs every [`MAINTENANCE_TICKS`] of them.
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1);
/// Checkpoint ticks between maintenance passes (one minute).
const MAINTENANCE_TICKS: u32 = 60;

impl Audit {
    /// Removes sealed files whose newest event is older than the retention, oldest first,
    /// recording an `audit_prune` event (with the removed file's last seq and hash) before
    /// each removal. Returns the number removed.
    ///
    /// # Errors
    ///
    /// The prune event cannot be recorded or a file cannot be read or removed; files
    /// pruned before the failure stay removed.
    pub fn prune_expired(&self) -> Result<u32> {
        let now = (self.0.options.clock)();
        let retention = Duration::from_hours(u64::from(self.0.options.retention_days) * 24);
        let Some(cutoff) = now.checked_sub(retention) else {
            return Ok(0);
        };
        let mut removed = 0;
        for file in files::list(&self.0.dir)?
            .into_iter()
            .filter(|f| !f.is_current())
        {
            let Some(span) = Span::read(&file.path)? else {
                break;
            };
            if Timestamp::parse(&span.last_at).map_or(true, |at| at >= cutoff) {
                break;
            }
            let name = file.name();
            let entry = Entry::new(Actor::system(), AuditAction::AuditPrune)
                .object(AuditObjectType::Audit, name.clone())
                .details(serde_json::json!({
                    "first_seq": span.first_seq,
                    "last_seq": span.last_seq,
                    "last_hash": span.last_hash,
                    "last_at": span.last_at,
                    "retention_days": self.0.options.retention_days,
                }));
            self.record(&entry)?;
            files::restrict(&file.path, false)?;
            std::fs::remove_file(&file.path)?;
            files::sync_dir(&self.0.dir)?;
            let last_seq = span.last_seq;
            tracing::info!(event = "audit.pruned", file = %name, last_seq, "Sealed audit file {name} removed after its retention");
            removed += 1;
        }
        Ok(removed)
    }

    /// One maintenance pass: reopen when unavailable, write an anchor when due, apply
    /// retention. The background thread runs it every minute.
    pub fn run_maintenance(&self) {
        let now = (self.0.options.clock)();
        let interval = self.0.options.anchor_interval;
        let due = self
            .writer()
            .as_ref()
            .is_none_or(|writer| writer.anchor_due(now, interval));
        if due {
            // Also the reopen attempt when the writer is closed.
            let _ = self.anchor("daily");
        }
        if let Err(error) = self.prune_expired() {
            tracing::warn!(event = "audit.prune_failed", %error, "Audit retention could not run");
        }
    }

    /// Writes an anchor event and logs its seq and hash.
    fn anchor(&self, reason: &str) -> Result<u64> {
        let seq = self.record(&anchor_entry(reason))?;
        let hash = self.head().map(|head| head.hash).unwrap_or_default();
        log_anchor(seq, &hash, reason);
        Ok(seq)
    }

    /// Syncs committed events to disk.
    fn checkpoint(&self) {
        if !self.0.dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Some(writer) = self.writer().as_ref()
            && let Err(error) = writer.checkpoint()
        {
            self.0.dirty.store(true, Ordering::Release);
            tracing::warn!(event = "audit.checkpoint_failed", %error, "Audit events not yet synced to disk");
        }
    }
}

impl Drop for Inner {
    /// The closing anchor, when events arrived since the last one.
    fn drop(&mut self) {
        let slot = self
            .writer
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(Writer::has_unanchored_events) {
            let entry = anchor_entry("shutdown");
            let at = (self.options.clock)();
            let appended = entry
                .checked_details()
                .and_then(|details| Writer::append(slot, &entry, &details, at));
            match appended {
                Ok(seq) => {
                    let hash = slot.as_ref().map(|writer| writer.head.hash.as_str());
                    log_anchor(seq, hash.unwrap_or_default(), "shutdown");
                }
                Err(error) => {
                    tracing::warn!(event = "audit.anchor_failed", %error, "The closing audit anchor could not be written");
                }
            }
        }
        if let Some(writer) = slot.as_ref() {
            let _ = writer.checkpoint();
        }
    }
}

/// The first and last events of one sealed file.
struct Span {
    first_seq: u64,
    last_seq: u64,
    last_hash: String,
    last_at: String,
}

impl Span {
    /// Reads the span of the file at `path`; `None` when it holds no events.
    fn read(path: &std::path::Path) -> Result<Option<Self>> {
        let connection = files::read_only(path)?;
        let first = files::first_event(&connection)?;
        let last = files::last_event(&connection)?;
        Ok(first
            .zip(last)
            .map(|((first_seq, _), (last_seq, last_hash, last_at))| Self {
                first_seq,
                last_seq,
                last_hash,
                last_at,
            }))
    }
}

fn anchor_entry(reason: &str) -> Entry {
    Entry::new(Actor::system(), AuditAction::AuditAnchor)
        .details(serde_json::json!({ "reason": reason }))
}

fn log_anchor(seq: u64, hash: &str, reason: &str) {
    tracing::info!(
        event = "audit.anchor",
        seq,
        hash,
        reason,
        "Audit chain head {seq} {hash}"
    );
}

/// Checkpoint every second; full maintenance every minute; ends with the last handle.
pub(super) fn spawn(inner: Weak<Inner>) {
    let spawned = std::thread::Builder::new()
        .name("packie-audit".into())
        .spawn(move || {
            let mut ticks: u32 = 0;
            loop {
                std::thread::sleep(CHECKPOINT_INTERVAL);
                let Some(strong) = inner.upgrade() else {
                    return;
                };
                let audit = Audit(strong);
                audit.checkpoint();
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(MAINTENANCE_TICKS) {
                    audit.run_maintenance();
                }
            }
        });
    if let Err(error) = spawned {
        tracing::error!(event = "audit.maintenance_unavailable", %error, "Audit maintenance thread not started");
    }
}
