//! The backup status: newest attempts, the schedule and staleness.

use std::time::Duration;

use packie_config::BackupSettings;
use packie_core::Timestamp;
use packie_ipc::backup::{BackupAttempt, BackupScheduleInfo, BackupStatusReport};
use serde::{Deserialize, Serialize};

use crate::fsutil::private_dir;
use crate::{Layout, Result};

/// `<state>/backup/status.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct StatusFile {
    last_attempt: Option<BackupAttempt>,
    last_success: Option<BackupAttempt>,
}

fn read(layout: &Layout) -> StatusFile {
    std::fs::read(layout.status_file())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Records one attempt (and, when it succeeded, the newest success). A failure to write
/// the status is logged; it never fails the backup.
pub fn record_attempt(layout: &Layout, attempt: BackupAttempt) {
    let mut status = read(layout);
    if attempt.ok {
        status.last_success = Some(attempt.clone());
    }
    status.last_attempt = Some(attempt);
    if let Err(error) = write(layout, &status) {
        tracing::warn!(event = "backup.status_unwritten", %error, "The backup status could not be saved");
    }
}

fn write(layout: &Layout, status: &StatusFile) -> Result<()> {
    let path = layout.status_file();
    if let Some(parent) = path.parent() {
        private_dir(parent)?;
    }
    packie_config::write_atomic(&path, &serde_json::to_vec_pretty(status)?)?;
    Ok(())
}

/// The status for `packie backup status`, the console and the `backup_stale` alert. With a
/// limit configured, the backups are stale when the newest success is older than the
/// limit, or, without any success, when `since` (the engine's start) is.
#[must_use]
pub fn status(
    layout: &Layout,
    settings: &BackupSettings,
    since: Timestamp,
    now: Timestamp,
) -> BackupStatusReport {
    let file = read(layout);
    let limit = settings.stale_limit_hours();
    let newest = file.last_success.as_ref().map_or(since, |a| a.at);
    let stale = limit.is_some_and(|hours| {
        now.saturating_duration_since(newest) > Duration::from_secs(u64::from(hours) * 3600)
    });
    BackupStatusReport {
        directory: settings.directory.as_ref().map(|d| d.display().to_string()),
        schedule: settings
            .schedule
            .as_ref()
            .map(|schedule| BackupScheduleInfo {
                daily_at: schedule.daily_at.clone(),
                keep: schedule.keep,
                next_at: crate::next_run(schedule, now),
            }),
        stale_after_hours: limit,
        last_attempt: file.last_attempt,
        last_success: file.last_success,
        stale,
    }
}
