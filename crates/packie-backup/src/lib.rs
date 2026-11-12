//! Backup, verification and restore of an engine's state (`docs/spec/backup.md`,
//! decision #17).
//!
//! A backup is a directory:
//!
//! | Path | Content |
//! | --- | --- |
//! | `store/messages.sqlite3`, `store/bodies/<xx>/<hash>` | A consistent store snapshot and every spilled body it references |
//! | `audit/` | Every audit file: sealed years as they are, the current file as a snapshot |
//! | `config/` | Saved runtime versions with their notes |
//! | `runtime.json`, `bootstrap.json`, `secrets.names.json` | The active document; the bootstrap and secret names for reference |
//! | `manifest.json` | SHA-256 and size per file, versions, engine version, time, actor; written last |
//!
//! [`create`] takes one with an open [`packie_store::Store`] (the engine) or with the store
//! closed (the CLI holding the instance lock); [`begin`] and [`Pending::finish`] split it
//! for a caller that snapshots the store itself. [`verify`] re-hashes every file, runs
//! `PRAGMA integrity_check` on the database copies and walks the audit chain. [`restore`]
//! refuses while an engine holds the state directory, moves the current state aside into
//! `pre-restore-<time>/`, copies the backup in and records `backup_restore` in the
//! restored audit log (undoing the restore when that record fails). Secret values are
//! never in a backup; they are entered again after a restore on a new computer.

mod copy;
mod fsutil;
mod manifest;
mod restore;
mod schedule;
mod status;
mod verify;

use std::path::{Path, PathBuf};

use packie_config::Bootstrap;
use packie_core::Timestamp;

pub use copy::{Pending, begin, create};
pub use manifest::{AuditInfo, FileEntry, MANIFEST, Manifest, StoreInfo};
pub use restore::{Restored, restore};
pub use schedule::{next_run, next_run_in, prune_scheduled, run_scheduled, scheduled_dir};
pub use status::{record_attempt, status};
pub use verify::{Verification, verify};

/// Backup operation result.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors carry paths and causes, never message content.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request cannot be done as asked; the text says what to change.
    #[error("{0}")]
    Invalid(String),
    /// An engine holds the state directory.
    #[error("an engine (pid {pid}) is running on this state directory; stop it before restoring")]
    EngineRunning {
        /// The process id of the running engine.
        pid: u32,
    },
    /// The backup failed verification; one entry per problem.
    #[error("the backup does not verify: {}", .0.join("; "))]
    NotVerified(Vec<String>),
    /// A file or directory operation failed.
    #[error("{path}: {source}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The operating system error.
        #[source]
        source: std::io::Error,
    },
    /// A JSON document could not be read or written.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A database copy could not be queried.
    #[error("SQLite: {0}")]
    Sql(#[from] rusqlite::Error),
    /// The message store failed.
    #[error("store: {0}")]
    Store(#[from] packie_store::Error),
    /// The audit log failed.
    #[error("audit: {0}")]
    Audit(#[from] packie_audit::Error),
    /// The instance lock could not be taken.
    #[error("{0}")]
    Host(#[from] packie_host::HostError),
    /// A configuration file could not be written.
    #[error("{0}")]
    Config(#[from] packie_config::ConfigError),
}

impl Error {
    /// Maps an I/O error on `path` (for `map_err`).
    pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self::Io {
            path: path.to_owned(),
            source,
        }
    }
}

/// Where an engine's state lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `paths.state_directory`.
    pub state_dir: PathBuf,
    /// The active runtime document (`runtime.file`).
    pub runtime_file: PathBuf,
    /// The bootstrap document, copied into backups for reference.
    pub bootstrap_file: Option<PathBuf>,
    /// The node id, recorded in the manifest.
    pub node_id: String,
    /// Files that hold secret values (`secrets.file`): never copied into a backup, wherever
    /// they are placed.
    pub secret_files: Vec<PathBuf>,
}

impl Layout {
    /// The layout of an engine started from `bootstrap` (read from `bootstrap_file`).
    #[must_use]
    pub fn of(bootstrap: &Bootstrap, bootstrap_file: &Path) -> Self {
        Self {
            state_dir: bootstrap.state_dir().to_owned(),
            runtime_file: bootstrap.runtime_file(),
            bootstrap_file: Some(
                std::path::absolute(bootstrap_file).unwrap_or_else(|_| bootstrap_file.to_owned()),
            ),
            node_id: bootstrap.node.node_id.clone(),
            secret_files: bootstrap.secrets.file.iter().cloned().collect(),
        }
    }

    /// Whether `path` is one of the secret files (compared after resolving links and
    /// relative parts, so another spelling of the same file matches too).
    pub(crate) fn is_secret(&self, path: &Path) -> bool {
        let resolved = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned());
        let candidate = resolved(path);
        self.secret_files
            .iter()
            .any(|secret| resolved(secret) == candidate)
    }

    /// `<state>/store`.
    #[must_use]
    pub fn store_dir(&self) -> PathBuf {
        self.state_dir.join(STORE)
    }

    /// `<state>/audit`.
    #[must_use]
    pub fn audit_dir(&self) -> PathBuf {
        self.state_dir.join(AUDIT)
    }

    /// `<state>/config`.
    #[must_use]
    pub fn config_dir(&self) -> PathBuf {
        self.state_dir.join(CONFIG)
    }

    fn status_file(&self) -> PathBuf {
        self.state_dir.join("backup").join("status.json")
    }
}

const STORE: &str = "store";
const AUDIT: &str = "audit";
const CONFIG: &str = "config";
const RUNTIME: &str = "runtime.json";
const BOOTSTRAP: &str = "bootstrap.json";
const SECRET_NAMES: &str = "secrets.names.json";

/// The engine version written into manifests (the workspace version).
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `20261107T120000Z`: the UTC time as used in directory names (sorts lexically).
pub(crate) fn utc_stamp(at: Timestamp) -> String {
    at.as_jiff().strftime("%Y%m%dT%H%M%SZ").to_string()
}
