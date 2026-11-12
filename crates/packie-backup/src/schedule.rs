//! The daily backup of `backup.schedule`.

use std::fs;
use std::path::{Path, PathBuf};

use jiff::tz::TimeZone;
use packie_config::{BackupSchedule, BackupSettings};
use packie_core::{Actor, Timestamp};
use packie_store::Store;

use crate::fsutil::{private_dir, remove_tree};
use crate::manifest::{MANIFEST, Manifest};
use crate::{Error, Layout, Result};

const PREFIX: &str = "packie-backup-";

/// The first scheduled time after `after`, in the computer's time zone.
#[must_use]
pub fn next_run(schedule: &BackupSchedule, after: Timestamp) -> Option<Timestamp> {
    next_run_in(schedule, after, &TimeZone::system())
}

/// [`next_run`] in `zone`. A time that a daylight-saving change skips runs at the
/// equivalent later instant of that day.
#[must_use]
pub fn next_run_in(
    schedule: &BackupSchedule,
    after: Timestamp,
    zone: &TimeZone,
) -> Option<Timestamp> {
    let (hour, minute) = schedule.time_of_day()?;
    let local = after.as_jiff().to_zoned(zone.clone());
    let mut day = local.date();
    for _ in 0..3 {
        let at = day.at(hour, minute, 0, 0).to_zoned(zone.clone()).ok()?;
        if at.timestamp() > after.as_jiff() {
            return Some(Timestamp::from(at.timestamp()));
        }
        day = day.tomorrow().ok()?;
    }
    None
}

/// `<root>/packie-backup-<UTC time>` for a scheduled backup taken at `at`.
#[must_use]
pub fn scheduled_dir(root: &Path, at: Timestamp) -> PathBuf {
    root.join(format!("{PREFIX}{}", crate::utc_stamp(at)))
}

/// Removes complete scheduled backups (`packie-backup-*` with a manifest) beyond the newest
/// `keep`; other directories in `root` are never touched. Returns what was removed.
///
/// # Errors
///
/// [`Error::Io`] when `root` cannot be read or a backup cannot be removed.
pub fn prune_scheduled(root: &Path, keep: u32) -> Result<Vec<PathBuf>> {
    let mut complete: Vec<PathBuf> = fs::read_dir(root)
        .map_err(Error::io(root))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(PREFIX))
                && path.join(MANIFEST).is_file()
        })
        .collect();
    // The UTC time in the name sorts lexically.
    complete.sort();
    let surplus = complete
        .len()
        .saturating_sub(usize::try_from(keep).unwrap_or(usize::MAX));
    let mut removed = Vec::new();
    for path in complete.into_iter().take(surplus) {
        remove_tree(&path).map_err(Error::io(&path))?;
        tracing::info!(event = "backup.pruned", directory = %path.display(), "Old scheduled backup removed");
        removed.push(path);
    }
    Ok(removed)
}

/// Takes the scheduled backup now into `backup.directory`, then keeps the newest
/// `schedule.keep`. The caller records `backup_create` in the audit log.
///
/// # Errors
///
/// [`Error::Invalid`] without a configured directory and schedule; otherwise the errors of
/// [`create`](crate::create) and [`prune_scheduled`].
pub fn run_scheduled(
    layout: &Layout,
    settings: &BackupSettings,
    actor: &Actor,
    store: Option<&Store>,
) -> Result<Manifest> {
    let (Some(root), Some(schedule)) = (&settings.directory, &settings.schedule) else {
        return Err(Error::Invalid("no backup schedule is configured".into()));
    };
    private_dir(root)?;
    let manifest = crate::create(layout, &scheduled_dir(root, Timestamp::now()), actor, store)?;
    prune_scheduled(root, schedule.keep)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daily(at: &str) -> BackupSchedule {
        BackupSchedule {
            daily_at: at.into(),
            keep: 3,
        }
    }

    #[test]
    fn the_next_run_is_today_or_tomorrow_in_local_time() {
        let zone = TimeZone::get("America/Chicago").unwrap();
        let before = Timestamp::parse("2026-11-06T07:00:00Z").unwrap(); // 01:00 local
        let after = Timestamp::parse("2026-11-06T09:00:00Z").unwrap(); // 03:00 local
        let schedule = daily("02:30");
        assert_eq!(
            next_run_in(&schedule, before, &zone).unwrap().to_string(),
            "2026-11-06T08:30:00.000000000Z"
        );
        assert_eq!(
            next_run_in(&schedule, after, &zone).unwrap().to_string(),
            "2026-11-07T08:30:00.000000000Z"
        );
        assert!(next_run_in(&daily("25:00"), after, &zone).is_none());
    }

    #[test]
    fn pruning_keeps_the_newest_complete_scheduled_backups_only() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "packie-backup-20261101T023000Z",
            "packie-backup-20261102T023000Z",
            "packie-backup-20261103T023000Z",
            "packie-backup-20261104T023000Z",
            "manual-copy",
        ] {
            fs::create_dir(root.path().join(name)).unwrap();
            fs::write(root.path().join(name).join(MANIFEST), b"{}").unwrap();
        }
        fs::create_dir(root.path().join("packie-backup-20261031T023000Z")).unwrap();
        let removed = prune_scheduled(root.path(), 2).unwrap();
        let names: Vec<_> = removed
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "packie-backup-20261101T023000Z",
                "packie-backup-20261102T023000Z"
            ]
        );
        assert!(root.path().join("manual-copy").exists());
        assert!(
            root.path().join("packie-backup-20261031T023000Z").exists(),
            "incomplete kept"
        );
    }
}
