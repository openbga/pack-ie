//! Backups over IPC (`docs/spec/backup.md`): taking one while the engine runs and the
//! backup status the console's Settings → Backup panel and `packie backup status` show.

use packie_core::Timestamp;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What one backup contains, as recorded in its `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupSummary {
    /// The backup directory (absolute).
    pub directory: String,
    /// When the snapshot was taken.
    pub created_at: Timestamp,
    /// Files listed in the manifest.
    pub files: u32,
    /// Their total size.
    pub bytes: u64,
    /// Messages in the store snapshot, when the store was included.
    pub messages: Option<u64>,
    /// The newest configuration version included.
    pub config_version: Option<u64>,
    /// The audit chain head included.
    pub audit_head_seq: Option<u64>,
    /// How long it took.
    pub duration_ms: u64,
}

/// One backup attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupAttempt {
    /// When it started.
    pub at: Timestamp,
    /// The backup directory.
    pub directory: String,
    /// Whether it completed with a manifest.
    pub ok: bool,
    /// Why it failed (no message content).
    pub error: Option<String>,
    /// What it contains, when it completed.
    pub summary: Option<BackupSummary>,
}

/// The `backup.schedule` of the bootstrap document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupScheduleInfo {
    /// Local time of day, `HH:MM`.
    pub daily_at: String,
    /// Scheduled backups kept in the backup directory.
    pub keep: u32,
    /// The next scheduled run.
    pub next_at: Option<Timestamp>,
}

/// Answer to [`Request::BackupStatus`](crate::Request::BackupStatus).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupStatusReport {
    /// `backup.directory`, where scheduled backups and backups without a directory go.
    pub directory: Option<String>,
    /// The schedule, when configured.
    pub schedule: Option<BackupScheduleInfo>,
    /// `backup_stale` fires when the newest successful backup is older than this.
    pub stale_after_hours: Option<u32>,
    /// The newest attempt.
    pub last_attempt: Option<BackupAttempt>,
    /// The newest successful attempt.
    pub last_success: Option<BackupAttempt>,
    /// The newest successful backup is older than `stale_after_hours` (or there is none
    /// that many hours after the engine started).
    pub stale: bool,
}
