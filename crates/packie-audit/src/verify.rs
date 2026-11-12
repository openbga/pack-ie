//! Walks the chain over every audit file of a directory, read-only.

use std::path::Path;

use packie_ipc::audit::{AuditAction, AuditProblem, AuditProblemKind, AuditVerification};

use crate::chain::{self, GENESIS};
use crate::query::{COLUMNS, Raw};
use crate::{Result, files};

/// Problems reported at most.
const MAX_PROBLEMS: usize = 100;

/// Verifies the audit files of `dir` (a state's `audit` directory or a backup's copy):
/// every event's hash, every link, the sequence without gaps across files, and a chain
/// start that is either the first event or follows a recorded `audit_prune`. Removing the
/// newest events leaves a valid shorter chain; compare `head_hash` with the last
/// `audit.anchor` line of the engine log to detect that.
///
/// # Errors
///
/// `dir` cannot be listed. A file that cannot be read is reported as a problem instead.
pub fn verify_dir(dir: &Path) -> Result<AuditVerification> {
    let mut walk = Walk::default();
    let list = files::list(dir)?;
    for file in &list {
        let name = file.name();
        if let Err(error) = walk.file(&file.path, &name) {
            walk.problem(
                &name,
                None,
                AuditProblemKind::Unreadable,
                format!("{name} cannot be read: {error}"),
            );
        }
    }
    walk.check_start();
    Ok(AuditVerification {
        ok: walk.problems.is_empty(),
        files: u32::try_from(list.len()).unwrap_or(u32::MAX),
        events: walk.events,
        first_seq: walk.start.as_ref().map(|s| s.seq),
        last_seq: walk.head.as_ref().map(|(seq, _)| *seq),
        head_hash: walk.head.map(|(_, hash)| hash),
        problems: walk.problems,
    })
}

struct Start {
    seq: u64,
    prev_hash: String,
    file: String,
}

#[derive(Default)]
struct Walk {
    events: u64,
    head: Option<(u64, String)>,
    start: Option<Start>,
    /// `(last_seq, last_hash)` of every recorded prune.
    prunes: Vec<(u64, String)>,
    problems: Vec<AuditProblem>,
}

impl Walk {
    fn file(&mut self, path: &Path, name: &str) -> Result<()> {
        let connection = files::read_only(path)?;
        let mut statement =
            connection.prepare(&format!("SELECT {COLUMNS} FROM events ORDER BY seq"))?;
        let rows = statement.query_map([], Raw::read)?;
        for row in rows {
            self.event(&row?, name);
        }
        Ok(())
    }

    fn event(&mut self, raw: &Raw, file: &str) {
        self.events += 1;
        match self.head.clone() {
            None => {
                self.start = Some(Start {
                    seq: raw.seq,
                    prev_hash: raw.prev_hash.clone(),
                    file: file.to_owned(),
                });
            }
            Some((seq, hash)) => {
                if raw.seq != seq + 1 {
                    let message = format!("event {} follows event {seq}", raw.seq);
                    self.problem(file, Some(raw.seq), AuditProblemKind::SequenceGap, message);
                }
                if raw.prev_hash != hash {
                    let message = format!(
                        "event {} does not link to the event before it: events were removed, inserted or reordered",
                        raw.seq
                    );
                    self.problem(file, Some(raw.seq), AuditProblemKind::BrokenLink, message);
                }
            }
        }
        if chain::hash(&raw.prev_hash, &raw.fields()) != raw.hash {
            let message = format!("event {} was changed after it was recorded", raw.seq);
            self.problem(file, Some(raw.seq), AuditProblemKind::HashMismatch, message);
        }
        if raw.action == AuditAction::AuditPrune.as_str()
            && let Ok(details) = serde_json::from_str::<serde_json::Value>(&raw.details)
            && let (Some(seq), Some(hash)) = (
                details.get("last_seq").and_then(serde_json::Value::as_u64),
                details.get("last_hash").and_then(serde_json::Value::as_str),
            )
        {
            self.prunes.push((seq, hash.to_owned()));
        }
        // Continue from the stored hash so one edited event is reported once.
        self.head = Some((raw.seq, raw.hash.clone()));
    }

    fn check_start(&mut self) {
        let Some(start) = self.start.take() else {
            return;
        };
        let genesis = start.seq == 1 && start.prev_hash == GENESIS;
        let after_prune = self
            .prunes
            .iter()
            .any(|(seq, hash)| seq + 1 == start.seq && *hash == start.prev_hash);
        if !genesis && !after_prune {
            let message = format!(
                "the oldest kept event {} neither starts the chain nor follows a recorded retention removal: older events were removed",
                start.seq
            );
            self.problem(
                &start.file,
                Some(start.seq),
                AuditProblemKind::UnexplainedStart,
                message,
            );
        }
        self.start = Some(start);
    }

    fn problem(&mut self, file: &str, seq: Option<u64>, kind: AuditProblemKind, message: String) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(AuditProblem {
                file: file.to_owned(),
                seq,
                kind,
                message,
            });
        }
    }
}
