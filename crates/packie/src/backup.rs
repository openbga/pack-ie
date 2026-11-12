//! `packie backup`: create, verify and restore backups of the engine's state, and show the
//! backup status. `create` and `status` ask a running engine; without one they work on the
//! state directory directly. `verify` and `restore` work on files.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Args, Subcommand};
use packie_audit::{Audit, Entry, Options};
use packie_backup::Layout;
use packie_config::Bootstrap;
use packie_core::{ByteSize, Timestamp};
use packie_host::InstanceLock;
use packie_ipc::audit::{AuditAction, AuditObjectType, AuditOutcome, AuditVerification};
use packie_ipc::backup::{BackupAttempt, BackupStatusReport, BackupSummary};
use packie_ipc::{Request, Response};
use serde::Serialize;

use crate::client::{Target, ask, unexpected};
use crate::output::{count, duration, opt_time, print_json, print_line, short_time};
use crate::{Failure, service};

/// Said after every backup: the files hold message content.
const PHI_REMINDER: &str =
    "The backup contains message content (PHI). Keep it on an encrypted volume.";

#[derive(Debug, Subcommand)]
pub(crate) enum BackupCommand {
    /// Take a consistent backup of the store, audit log and configuration.
    Create(CreateArgs),
    /// Check a backup's files, store copy and audit chain without changing anything.
    Verify(VerifyArgs),
    /// Replace the engine's state with a backup (the engine must be stopped).
    Restore(RestoreArgs),
    /// Show the schedule, the newest backup attempts and whether backups are stale.
    Status(StatusArgs),
}

#[derive(Debug, Args)]
pub(crate) struct CreateArgs {
    /// A new or empty directory for the backup. Without it the backup goes into a new
    /// `packie-backup-<time>` directory under `backup.directory` of the bootstrap file.
    dir: Option<PathBuf>,
    /// Print the summary as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct VerifyArgs {
    /// The backup directory.
    dir: PathBuf,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RestoreArgs {
    /// The backup directory.
    dir: PathBuf,
    /// Confirm the restore. It replaces the store, the audit log and the configuration; the
    /// replaced state is moved aside, not deleted.
    #[arg(long)]
    yes: bool,
    /// Print the outcome as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct StatusArgs {
    /// Print the status as JSON.
    #[arg(long)]
    json: bool,
}

pub(crate) async fn run(target: &Target, command: BackupCommand) -> Result<(), Failure> {
    match command {
        BackupCommand::Create(args) => create(target, &args).await,
        BackupCommand::Verify(args) => verify(&args),
        BackupCommand::Restore(args) => restore(target, &args),
        BackupCommand::Status(args) => status(target, &args).await,
    }
}

/// The bootstrap document and the state layout it names.
struct Setup {
    bootstrap: Bootstrap,
    layout: Layout,
}

impl Setup {
    fn load(target: &Target) -> Result<Self, Failure> {
        let bootstrap = Bootstrap::load(&target.bootstrap).map_err(|e| {
            Failure::new(format!(
                "{e}\nPass --bootstrap <file> to name the engine's bootstrap file."
            ))
        })?;
        let layout = Layout::of(&bootstrap, &target.bootstrap);
        Ok(Self { bootstrap, layout })
    }

    /// The process id of the engine holding the state directory, when one runs.
    fn engine_pid(&self) -> Option<u32> {
        InstanceLock::holder(self.bootstrap.state_dir()).map(|holder| holder.pid)
    }
}

fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    std::path::absolute(path)
        .map_err(|e| Failure::new(format!("cannot resolve {}: {e}", path.display())))
}

/// The directory of a backup taken without an engine: the one given, or a new directory
/// under `backup.directory`.
fn offline_dir(
    given: Option<&Path>,
    configured: Option<&Path>,
    now: Timestamp,
) -> Result<PathBuf, Failure> {
    match (given, configured) {
        (Some(dir), _) => absolute(dir),
        (None, Some(root)) => absolute(&packie_backup::scheduled_dir(root, now)),
        (None, None) => Err(no_directory()),
    }
}

fn no_directory() -> Failure {
    Failure::invalid(
        "no backup directory: pass one (`packie backup create <dir>`) or set `backup.directory` in the bootstrap file.",
    )
}

async fn create(target: &Target, args: &CreateArgs) -> Result<(), Failure> {
    let setup = Setup::load(target)?;
    let summary = if setup.engine_pid().is_some() {
        create_through_engine(target, &setup, args.dir.as_deref()).await?
    } else {
        create_offline(target, &setup, args.dir.as_deref())?
    };
    if args.json {
        return print_json(&summary);
    }
    for line in summary_lines(&summary) {
        print_line(&line);
    }
    print_line(PHI_REMINDER);
    Ok(())
}

async fn create_through_engine(
    target: &Target,
    setup: &Setup,
    dir: Option<&Path>,
) -> Result<BackupSummary, Failure> {
    let directory = match dir {
        Some(dir) => Some(text_path(&absolute(dir)?)?),
        None if setup.bootstrap.backup.directory.is_some() => None,
        None => return Err(no_directory()),
    };
    let client = target.connect().await?;
    match ask(&client, Request::CreateBackup { directory }).await? {
        Response::BackupCreated(summary) => Ok(*summary),
        _ => Err(unexpected()),
    }
}

fn text_path(path: &Path) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::new(format!("{} is not valid text", path.display())))
}

/// A backup without an engine: holds the instance lock, records the request before the
/// copy and the outcome after it, and refuses to start when the request cannot be recorded.
fn create_offline(
    target: &Target,
    setup: &Setup,
    given: Option<&Path>,
) -> Result<BackupSummary, Failure> {
    let dir = offline_dir(
        given,
        setup.bootstrap.backup.directory.as_deref(),
        Timestamp::now(),
    )?;
    let directory = text_path(&dir)?;
    let _lock = service::instance_lock(&target.bootstrap)?;
    let audit = Audit::open(
        setup.layout.audit_dir(),
        Options {
            retention_days: setup.bootstrap.audit.retention_days,
            // A short command: the closing anchor and sync happen when the log is dropped.
            background: false,
            ..Options::default()
        },
    )
    .map_err(|e| refused(&e))?;
    let actor = packie_audit::this_process().map_err(|e| refused(&e))?;
    let entry = Entry::new(actor.clone(), AuditAction::BackupCreate)
        .object(AuditObjectType::Backup, directory);
    audit
        .record(&entry.clone().outcome(AuditOutcome::Requested))
        .map_err(|e| refused(&e))?;
    let started = Instant::now();
    let result = packie_backup::create(&setup.layout, &dir, &actor, None);
    let took = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (outcome, details) = match &result {
        Ok(manifest) => {
            let summary = manifest.summary(&dir, took);
            (
                AuditOutcome::Succeeded,
                serde_json::json!({
                    "files": summary.files,
                    "bytes": summary.bytes,
                    "messages": summary.messages,
                }),
            )
        }
        Err(_) => (
            AuditOutcome::Failed,
            serde_json::json!({ "error_code": "backup_failed" }),
        ),
    };
    if let Err(error) = audit.record(&entry.outcome(outcome).details(details)) {
        crate::output::eprint_line(&format!(
            "packie: warning: the backup's outcome could not be recorded in the audit log ({error})."
        ));
    }
    result
        .map(|manifest| manifest.summary(&dir, took))
        .map_err(|e| Failure::new(format!("the backup failed: {e}")))
}

fn refused(error: &packie_audit::Error) -> Failure {
    Failure::new(format!(
        "the backup was not started: the audit log cannot record it ({error})."
    ))
}

/// `Backup created in <dir>.` and its contents, one per line.
fn summary_lines(summary: &BackupSummary) -> Vec<String> {
    let none = || "-".to_owned();
    vec![
        format!("Backup created in {}.", summary.directory),
        format!(
            "  Files:           {} ({})",
            summary.files,
            ByteSize(summary.bytes)
        ),
        format!(
            "  Messages:        {}",
            summary.messages.map_or_else(none, |n| n.to_string())
        ),
        format!(
            "  Config version:  {}",
            summary.config_version.map_or_else(none, |n| n.to_string())
        ),
        format!(
            "  Audit head:      {}",
            summary
                .audit_head_seq
                .map_or_else(none, |seq| format!("seq {seq}"))
        ),
        format!("  Taken:           {}", short_time(summary.created_at)),
        format!("  Took:            {}", took(summary.duration_ms)),
    ]
}

fn took(milliseconds: u64) -> String {
    if milliseconds < 1000 {
        format!("{milliseconds} ms")
    } else {
        duration(Duration::from_millis(milliseconds))
    }
}

/// What `backup verify --json` prints.
#[derive(Debug, Serialize)]
struct VerifyReport {
    ok: bool,
    created_at: Option<Timestamp>,
    files_checked: u32,
    problems: Vec<String>,
    audit: Option<AuditVerification>,
}

fn verify(args: &VerifyArgs) -> Result<(), Failure> {
    let check = packie_backup::verify(&args.dir)
        .map_err(|e| Failure::new(format!("cannot verify {}: {e}", args.dir.display())))?;
    let report = VerifyReport {
        ok: check.ok,
        created_at: check.manifest.as_ref().map(|m| m.created_at),
        files_checked: check.files_checked,
        problems: check.problems,
        audit: check.audit,
    };
    if args.json {
        print_json(&report)?;
    } else {
        for line in verify_lines(&report, &args.dir) {
            print_line(&line);
        }
    }
    if report.ok {
        Ok(())
    } else {
        Err(Failure::new(format!(
            "{} does not verify: {}.",
            args.dir.display(),
            count(
                u64::try_from(report.problems.len()).unwrap_or(u64::MAX),
                "problem",
                "problems"
            )
        )))
    }
}

fn verify_lines(report: &VerifyReport, dir: &Path) -> Vec<String> {
    let mut lines = vec![if report.ok {
        format!(
            "{} verifies: {} checked, the store copy and the audit chain are intact.",
            dir.display(),
            count(u64::from(report.files_checked), "file", "files")
        )
    } else {
        format!(
            "{} does NOT verify ({} checked).",
            dir.display(),
            count(u64::from(report.files_checked), "file", "files")
        )
    }];
    if let Some(at) = report.created_at {
        lines.push(format!("  Taken: {}", short_time(at)));
    }
    lines.extend(report.problems.iter().map(|p| format!("  Problem: {p}")));
    lines
}

/// What `backup restore --json` prints.
#[derive(Debug, Serialize)]
struct RestoreReport {
    backup_created_at: Timestamp,
    pre_restore: String,
    audit_seq: u64,
}

fn restore(target: &Target, args: &RestoreArgs) -> Result<(), Failure> {
    if !args.yes {
        return Err(Failure::invalid(
            "restore replaces the store, the audit log and the configuration with the backup's, and the engine must be stopped. \
             The replaced state is moved aside, not deleted. Run again with --yes to restore.",
        ));
    }
    let setup = Setup::load(target)?;
    if let Some(pid) = setup.engine_pid() {
        return Err(Failure::new(format!(
            "an engine (pid {pid}) is running on this state directory; stop it first (`packie service stop`, or Ctrl-C in its terminal)."
        )));
    }
    let actor = packie_audit::this_process()
        .map_err(|e| Failure::new(format!("the restore did not happen: {e}")))?;
    let restored = packie_backup::restore(&args.dir, &setup.layout, &actor)
        .map_err(|e| Failure::new(format!("the restore did not happen: {e}")))?;
    let report = RestoreReport {
        backup_created_at: restored.manifest.created_at,
        pre_restore: restored.pre_restore.display().to_string(),
        audit_seq: restored.audit_seq,
    };
    if args.json {
        return print_json(&report);
    }
    for line in restore_lines(&report) {
        print_line(&line);
    }
    Ok(())
}

fn restore_lines(report: &RestoreReport) -> Vec<String> {
    vec![
        format!(
            "Restored the backup taken {}. The audit log records it as event {}.",
            short_time(report.backup_created_at),
            report.audit_seq
        ),
        format!(
            "The previous state was moved to {}; delete it once you no longer need it.",
            report.pre_restore
        ),
        "Next: start the engine (`packie service start`, or `packie run`). On a new computer, then enter the secrets again with `packie secrets set <name>` (secret values are never in a backup; the names are in secrets.names.json). Endpoints that need a missing secret report it and connect once it is set.".to_owned(),
    ]
}

async fn status(target: &Target, args: &StatusArgs) -> Result<(), Failure> {
    let setup = Setup::load(target)?;
    let running = setup.engine_pid().is_some();
    let report = if running {
        let client = target.connect().await?;
        match ask(&client, Request::BackupStatus).await? {
            Response::BackupStatus(report) => *report,
            _ => return Err(unexpected()),
        }
    } else {
        let now = Timestamp::now();
        packie_backup::status(&setup.layout, &setup.bootstrap.backup, now, now)
    };
    if args.json {
        return print_json(&report);
    }
    for line in status_lines(&report, running) {
        print_line(&line);
    }
    Ok(())
}

fn status_lines(report: &BackupStatusReport, engine_running: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if report.stale {
        let limit = report.stale_after_hours.map_or_else(String::new, |h| {
            format!(" than {}", count(u64::from(h), "hour", "hours"))
        });
        lines.push(format!(
            "STALE: the newest successful backup is older{limit}, or there is none. Take one with `packie backup create`."
        ));
    }
    lines.push(format!(
        "Directory:     {}",
        report
            .directory
            .as_deref()
            .unwrap_or("not set (backup.directory)")
    ));
    lines.push(format!(
        "Schedule:      {}",
        report.schedule.as_ref().map_or_else(
            || "none (backup.schedule)".to_owned(),
            |s| format!(
                "daily at {}, keeping {}; next run {}",
                s.daily_at,
                s.keep,
                opt_time(s.next_at)
            )
        )
    ));
    lines.push(format!(
        "Stale limit:   {}",
        report.stale_after_hours.map_or_else(
            || "off".to_owned(),
            |h| count(u64::from(h), "hour", "hours")
        )
    ));
    lines.push(format!(
        "Last attempt:  {}",
        report
            .last_attempt
            .as_ref()
            .map_or_else(|| "none yet".to_owned(), attempt_text)
    ));
    lines.push(format!(
        "Last success:  {}",
        report
            .last_success
            .as_ref()
            .map_or_else(|| "none yet".to_owned(), attempt_text)
    ));
    if !engine_running {
        lines.push(
            "The engine is not running, so no scheduled backup is taken until it starts."
                .to_owned(),
        );
    }
    lines
}

fn attempt_text(attempt: &BackupAttempt) -> String {
    let outcome = if attempt.ok {
        "ok".to_owned()
    } else {
        format!(
            "FAILED ({})",
            attempt.error.as_deref().unwrap_or("no reason recorded")
        )
    };
    format!(
        "{} {outcome}, {}",
        short_time(attempt.at),
        attempt.directory
    )
}

#[cfg(test)]
mod tests {
    use packie_ipc::backup::BackupScheduleInfo;

    use super::*;

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).unwrap()
    }

    #[test]
    fn the_offline_directory_is_given_or_scheduled() {
        let now = at("2026-11-06T12:00:00Z");
        // Absolute on every OS (on Windows "/backups" is relative to the current drive).
        let base = std::env::temp_dir();
        let root = base.join("backups");
        let given = base.join("one");
        assert_eq!(offline_dir(Some(&given), Some(&root), now).unwrap(), given);
        let scheduled = offline_dir(None, Some(&root), now).unwrap();
        assert_eq!(scheduled, root.join("packie-backup-20261106T120000Z"));
        let error = offline_dir(None, None, now).unwrap_err();
        assert!(error.message.contains("backup.directory"));
        assert_eq!(error.code, 2);
    }

    #[test]
    fn restore_needs_confirmation() {
        let target = Target {
            bootstrap: PathBuf::from("does-not-exist.json"),
            ipc: None,
        };
        let args = RestoreArgs {
            dir: PathBuf::from("backup"),
            yes: false,
            json: false,
        };
        let error = restore(&target, &args).unwrap_err();
        assert!(error.message.contains("--yes"));
        assert!(error.message.contains("must be stopped"));
    }

    #[test]
    fn summaries_show_size_and_duration() {
        let summary = BackupSummary {
            directory: "/backups/one".to_owned(),
            created_at: at("2026-11-06T12:00:00Z"),
            files: 12,
            bytes: 3 * 1024 * 1024,
            messages: Some(40),
            config_version: None,
            audit_head_seq: Some(7),
            duration_ms: 850,
        };
        let lines = summary_lines(&summary);
        assert_eq!(lines[0], "Backup created in /backups/one.");
        assert!(lines[1].ends_with("12 (3.0 MiB)"));
        assert!(lines[2].ends_with("40"));
        assert!(lines[3].ends_with('-'));
        assert!(lines[4].ends_with("seq 7"));
        assert!(lines[6].ends_with("850 ms"));
        assert_eq!(took(65_000), "1m 5s");
    }

    #[test]
    fn stale_status_comes_first() {
        let report = BackupStatusReport {
            directory: Some("/backups".to_owned()),
            schedule: Some(BackupScheduleInfo {
                daily_at: "02:00".to_owned(),
                keep: 7,
                next_at: None,
            }),
            stale_after_hours: Some(26),
            last_attempt: Some(BackupAttempt {
                at: at("2026-11-05T02:00:00Z"),
                directory: "/backups/x".to_owned(),
                ok: false,
                error: Some("disk full".to_owned()),
                summary: None,
            }),
            last_success: None,
            stale: true,
        };
        let lines = status_lines(&report, false);
        assert!(lines[0].starts_with("STALE: "));
        assert!(lines[0].contains("26 hours"));
        assert!(lines.contains(&"Schedule:      daily at 02:00, keeping 7; next run -".to_owned()));
        assert!(lines.contains(
            &"Last attempt:  2026-11-05 02:00:00Z FAILED (disk full), /backups/x".to_owned()
        ));
        assert!(lines.contains(&"Last success:  none yet".to_owned()));
        assert!(lines.last().unwrap().contains("not running"));
        assert!(
            !status_lines(&report, true)
                .last()
                .unwrap()
                .contains("not running")
        );
    }

    #[test]
    fn verification_failures_name_each_problem() {
        let report = VerifyReport {
            ok: false,
            created_at: None,
            files_checked: 2,
            problems: vec!["store/messages.sqlite3: hash differs".to_owned()],
            audit: None,
        };
        let lines = verify_lines(&report, Path::new("/b"));
        assert_eq!(lines[0], "/b does NOT verify (2 files checked).");
        assert_eq!(lines[1], "  Problem: store/messages.sqlite3: hash differs");
    }
}
