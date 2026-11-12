//! Backups taken by the running engine (`docs/spec/backup.md`, decision 17): on request
//! (`create_backup`), daily per `backup.schedule`, and the staleness the `backup_stale`
//! alert reads. The work itself is `packie-backup`; the store is read through its own
//! read-only connection, so commits continue meanwhile.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use packie_backup::Layout;
use packie_config::Bootstrap;
use packie_core::{Actor, Timestamp};
use packie_diagnostics::StaleBackup;
use packie_ipc::audit::{AuditAction, AuditObjectType, AuditOutcome};
use packie_ipc::backup::{BackupStatusReport, BackupSummary};
use packie_ipc::{ErrorCode, IpcError};

use crate::audit::{AuditLog, AuditRecord};
use crate::engine::record_logged;

/// How often the staleness is re-read without a backup in between.
const STALE_REFRESH: Duration = Duration::from_secs(60);

/// The engine's backup state.
pub(crate) struct Backups {
    layout: Layout,
    settings: packie_config::BackupSettings,
    started_at: Timestamp,
    stale: Mutex<Option<StaleBackup>>,
}

impl Backups {
    pub(crate) fn new(bootstrap: &Bootstrap, bootstrap_file: &Path, started_at: Timestamp) -> Self {
        let backups = Self {
            layout: Layout::of(bootstrap, bootstrap_file),
            settings: bootstrap.backup.clone(),
            started_at,
            stale: Mutex::new(None),
        };
        backups.refresh();
        backups
    }

    /// Takes a backup into `directory` (absolute; new or empty), or into a new
    /// `packie-backup-<time>` directory under `backup.directory`. Blocking: call it from a
    /// blocking task.
    pub(crate) fn create(
        &self,
        directory: Option<&str>,
        actor: &Actor,
    ) -> Result<BackupSummary, IpcError> {
        let dest = match (directory, &self.settings.directory) {
            (Some(dir), _) => PathBuf::from(dir),
            (None, Some(root)) => packie_backup::scheduled_dir(root, Timestamp::now()),
            (None, None) => {
                return Err(IpcError::new(
                    ErrorCode::BadRequest,
                    "No backup.directory is configured: give a directory for the backup.",
                ));
            }
        };
        let started = Instant::now();
        let result = packie_backup::create(&self.layout, &dest, actor, None);
        self.refresh();
        let manifest = result.map_err(|error| match error {
            packie_backup::Error::Invalid(text) => IpcError::new(ErrorCode::BadRequest, text),
            other => IpcError::new(ErrorCode::Internal, format!("The backup failed: {other}")),
        })?;
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(manifest.summary(&dest, elapsed))
    }

    /// The schedule, the newest attempts and whether backups are stale.
    pub(crate) fn status(&self) -> BackupStatusReport {
        packie_backup::status(
            &self.layout,
            &self.settings,
            self.started_at,
            Timestamp::now(),
        )
    }

    /// What the `backup_stale` alert reads (refreshed after every backup and each minute).
    pub(crate) fn stale(&self) -> Option<StaleBackup> {
        self.stale
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn refresh(&self) {
        let report = self.status();
        let stale = report
            .stale
            .then(|| {
                report.stale_after_hours.map(|limit_hours| StaleBackup {
                    newest_success_at: report.last_success.as_ref().map(|a| a.at),
                    limit_hours,
                })
            })
            .flatten();
        *self.stale.lock().unwrap_or_else(PoisonError::into_inner) = stale;
    }

    /// Takes the scheduled backups at `backup.schedule.daily_at` and keeps the staleness
    /// current, until the engine stops (the task is aborted).
    pub(crate) async fn run(self: Arc<Self>, audit: Arc<dyn AuditLog>) {
        loop {
            let next = self
                .settings
                .schedule
                .as_ref()
                .and_then(|schedule| packie_backup::next_run(schedule, Timestamp::now()));
            let wait = next.map_or(STALE_REFRESH, |at| {
                at.saturating_duration_since(Timestamp::now())
                    .min(STALE_REFRESH)
            });
            tokio::time::sleep(wait).await;
            if next.is_some_and(|at| Timestamp::now() >= at) {
                let backups = Arc::clone(&self);
                let audit = Arc::clone(&audit);
                let _ =
                    tokio::task::spawn_blocking(move || backups.scheduled(audit.as_ref())).await;
            } else {
                self.refresh();
            }
        }
    }

    /// One scheduled backup, attributed to the engine and recorded in the audit log. The
    /// backup is taken even when the audit log cannot be written: losing backups would make
    /// an outage worse, and the failure is logged.
    fn scheduled(&self, audit: &dyn AuditLog) {
        let result =
            packie_backup::run_scheduled(&self.layout, &self.settings, &Actor::system(), None);
        self.refresh();
        let (outcome, details) = match &result {
            Ok(manifest) => (
                AuditOutcome::Succeeded,
                serde_json::json!({ "scheduled": true, "files": manifest.files.len() }),
            ),
            Err(error) => (
                AuditOutcome::Failed,
                serde_json::json!({ "scheduled": true, "error": error.to_string() }),
            ),
        };
        let record = AuditRecord {
            actor: Actor::system(),
            action: AuditAction::BackupCreate,
            object: Some((AuditObjectType::Backup, "scheduled".into())),
            outcome,
            details,
        };
        record_logged(audit, &record);
    }
}
