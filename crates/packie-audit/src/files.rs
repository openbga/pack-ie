//! The audit directory: the current file and the sealed yearly files.
//!
//! `<state>/audit/audit.sqlite3` takes new events. At the first event of a new (UTC) year
//! it is sealed: renamed to `audit-<year>-<first seq, 12 digits>.sqlite3` and made
//! read-only, and a new current file continues the chain. Sealed files are only ever read,
//! copied into backups, or removed whole by retention.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use packie_core::Timestamp;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// The file that takes new events.
pub(crate) const CURRENT: &str = "audit.sqlite3";

/// One audit file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuditFile {
    pub(crate) path: PathBuf,
    /// The first seq the name states (sealed files only).
    pub(crate) first_seq: Option<u64>,
}

impl AuditFile {
    pub(crate) fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub(crate) fn is_current(&self) -> bool {
        self.first_seq.is_none()
    }
}

/// The sealed name for a file of `year` starting at `first_seq`.
pub(crate) fn sealed_name(year: i16, first_seq: u64) -> String {
    format!("audit-{year:04}-{first_seq:012}.sqlite3")
}

/// The first seq of a sealed file name (see [`sealed_name`]).
fn parse_sealed(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("audit-")?.strip_suffix(".sqlite3")?;
    let (year, seq) = rest.split_once('-')?;
    if year.len() != 4 || seq.len() != 12 {
        return None;
    }
    year.parse::<i16>().ok()?;
    seq.parse().ok()
}

/// The audit files of `dir`, sealed ones by first seq, then the current one if it exists.
pub(crate) fn list(dir: &Path) -> std::io::Result<Vec<AuditFile>> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(first_seq) = parse_sealed(name) {
            files.push(AuditFile {
                path: entry.path(),
                first_seq: Some(first_seq),
            });
        }
    }
    files.sort_by_key(|file| file.first_seq);
    let current = dir.join(CURRENT);
    if current.is_file() {
        files.push(AuditFile {
            path: current,
            first_seq: None,
        });
    }
    Ok(files)
}

/// A read-only connection.
pub(crate) fn read_only(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

/// The oldest event's seq and time in one file.
pub(crate) fn first_event(connection: &Connection) -> rusqlite::Result<Option<(u64, Timestamp)>> {
    connection
        .query_row(
            "SELECT seq, at FROM events ORDER BY seq LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
}

/// The newest event's seq, hash and time in one file.
pub(crate) fn last_event(
    connection: &Connection,
) -> rusqlite::Result<Option<(u64, String, String)>> {
    connection
        .query_row(
            "SELECT seq, hash, at FROM events ORDER BY seq DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
}

/// Creates `path` empty when missing, readable by the owner only.
pub(crate) fn create_private(path: &Path) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?;
    restrict(path, false)
}

/// Creates `path` (and parents) readable by the owner only.
pub(crate) fn private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path)?;
    Ok(())
}

/// Makes `path` owner-only (`0600`), or read-only for the owner (`0400`) when sealed.
pub(crate) fn restrict(path: &Path, read_only: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if read_only { 0o400 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_readonly(read_only);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// Flushes a directory entry change (rename, create, remove) to disk where the OS allows.
pub(crate) fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_names_round_trip_and_sort_by_first_seq() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            sealed_name(2025, 1001),
            sealed_name(2024, 1),
            "audit.sqlite3".to_owned(),
            "audit-20x4-000000000001.sqlite3".to_owned(),
            "notes.txt".to_owned(),
        ] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        let names: Vec<String> = list(dir.path())
            .unwrap()
            .iter()
            .map(AuditFile::name)
            .collect();
        assert_eq!(
            names,
            [
                "audit-2024-000000000001.sqlite3",
                "audit-2025-000000001001.sqlite3",
                "audit.sqlite3"
            ]
        );
    }
}
