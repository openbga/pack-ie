//! Consistent snapshots of the store for backups (`docs/spec/backup.md`).
//!
//! The database is copied with the SQLite online backup API in one step, which reads one
//! WAL snapshot while the writer keeps committing. Spilled body files referenced by that
//! snapshot are then copied while body-file collection is held off, so a backup never
//! references a file it lacks. Files spilled after the snapshot are not needed by it.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

use crate::Cached;
use crate::{Error, Result, Store, private_dir, private_file};

/// The database file name in a store directory and in a backup's `store` directory.
pub const DATABASE_FILE: &str = "messages.sqlite3";
/// The body-file directory name beside it.
pub const BODIES_DIR: &str = "bodies";

/// What a snapshot wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreCopy {
    /// `<dest>/messages.sqlite3`, a standalone file in rollback-journal mode.
    pub database: PathBuf,
    /// The body files copied, absolute.
    pub bodies: Vec<PathBuf>,
    /// The store schema version of the copy.
    pub schema_version: i64,
    /// Messages in the copy.
    pub messages: u64,
}

impl Store {
    /// Writes a consistent snapshot of this open store into `dest` (created owner-only):
    /// `messages.sqlite3` and `bodies/<xx>/<hash>` for every spilled body it references.
    /// Commits continue meanwhile; body-file collection waits until the copy is done.
    pub fn backup_into(&self, dest: &Path) -> Result<StoreCopy> {
        let root = self.0.bodies.root().to_owned();
        self.read(|conn| {
            let _files = self.0.bodies.hold_files();
            snapshot(conn, &root, dest)
        })
    }
}

/// Like [`Store::backup_into`] through its own read-only connection to `store_dir`
/// (`<state>/store`): for a store nobody has open, or one an engine is writing through
/// another handle. In the second case retention may remove a body file between the
/// snapshot and its copy; the snapshot is then taken again, once.
pub fn backup_closed(store_dir: &Path, dest: &Path) -> Result<StoreCopy> {
    let database = store_dir.join(DATABASE_FILE);
    if !database.is_file() {
        return Err(Error::NotFound);
    }
    let attempt = || -> Result<StoreCopy> {
        let source = Connection::open_with_flags(
            &database,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        source.busy_timeout(std::time::Duration::from_secs(5))?;
        snapshot(&source, &store_dir.join(BODIES_DIR), dest)
    };
    match attempt() {
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::remove_file(dest.join(DATABASE_FILE))?;
            let bodies = dest.join(BODIES_DIR);
            if bodies.exists() {
                fs::remove_dir_all(bodies)?;
            }
            attempt()
        }
        other => other,
    }
}

fn snapshot(source: &Connection, bodies_root: &Path, dest: &Path) -> Result<StoreCopy> {
    private_dir(dest)?;
    let database = dest.join(DATABASE_FILE);
    if database.exists() {
        return Err(Error::Invalid(format!(
            "{} already exists; a backup goes into a new directory",
            database.display()
        )));
    }
    private_file(&database)?;
    let mut copy = Connection::open(&database)?;
    match Backup::new(source, &mut copy)?.step(-1)? {
        StepResult::Done => {}
        other => {
            return Err(Error::Invalid(format!(
                "the store snapshot did not complete ({other:?}); try again"
            )));
        }
    }
    copy.pragma_update(None, "journal_mode", "DELETE")?;
    let schema_version: i64 = copy.query_row_cached(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    let messages: u64 =
        copy.query_row_cached("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?;
    let files: Vec<String> = copy
        .prepare("SELECT DISTINCT file_path FROM body_contents WHERE file_path IS NOT NULL")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    copy.close().map_err(|(_, error)| error)?;
    let mut bodies = Vec::with_capacity(files.len());
    for relative in files {
        // Stored paths are `<xx>/<hash>`; anything else is not a body file of ours.
        if relative.split('/').count() != 2 || relative.contains("..") {
            return Err(Error::Invalid(
                "the store references an invalid body path".into(),
            ));
        }
        let target = dest.join(BODIES_DIR).join(&relative);
        if let Some(parent) = target.parent() {
            private_dir(parent)?;
        }
        fs::copy(bodies_root.join(&relative), &target)?;
        private_file(&target)?;
        bodies.push(target);
    }
    Ok(StoreCopy {
        database,
        bodies,
        schema_version,
        messages,
    })
}
