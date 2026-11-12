//! Taking a backup.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use packie_core::{Actor, Timestamp};
use packie_ipc::backup::{BackupAttempt, BackupSummary};
use packie_store::Store;

use crate::fsutil::{self, copy_file, has_extension, private_dir, remove_children, sync_path};
use crate::manifest::{self, AuditInfo, FileEntry, Manifest, StoreInfo};
use crate::{AUDIT, BOOTSTRAP, CONFIG, Error, Layout, RUNTIME, Result, SECRET_NAMES, STORE};

/// A backup in progress: its directory exists and is empty or holds only the store
/// snapshot the caller wrote into [`Pending::store_dir`].
#[derive(Debug)]
pub struct Pending {
    dest: PathBuf,
    created_dir: bool,
    at: Timestamp,
    started: Instant,
}

/// Starts a backup into `dest`, an absolute directory that does not exist yet or is
/// empty.
///
/// # Errors
///
/// [`Error::Invalid`] when `dest` is relative or not empty; [`Error::Io`] when it cannot be
/// read or created.
pub fn begin(dest: &Path) -> Result<Pending> {
    if !dest.is_absolute() {
        return Err(Error::Invalid(format!(
            "the backup directory must be an absolute path, not {}",
            dest.display()
        )));
    }
    let created_dir = match fs::read_dir(dest) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(Error::Invalid(format!(
                    "{} is not empty; a backup goes into a new or empty directory",
                    dest.display()
                )));
            }
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(Error::io(dest)(error)),
    };
    private_dir(dest)?;
    Ok(Pending {
        dest: dest.to_owned(),
        created_dir,
        at: Timestamp::now(),
        started: Instant::now(),
    })
}

/// Takes a complete backup of `layout` into `dest` and records the attempt in the backup
/// status. With `store`, the open store is snapshotted while it runs; without, the store
/// on disk is read directly, which needs the engine stopped (hold the instance lock).
///
/// # Errors
///
/// Whatever [`begin`] and [`Pending::finish`] report, or the store's snapshot error. A
/// failed backup leaves nothing behind in `dest`.
pub fn create(
    layout: &Layout,
    dest: &Path,
    actor: &Actor,
    store: Option<&Store>,
) -> Result<Manifest> {
    let pending = begin(dest)?;
    let snapshot = match store {
        Some(store) => store.backup_into(&pending.store_dir()).map(drop),
        None => Ok(()),
    };
    match snapshot {
        Ok(()) => pending.finish(layout, actor),
        Err(error) => {
            let error = Error::from(error);
            pending.fail(layout, &error);
            Err(error)
        }
    }
}

impl Pending {
    /// The backup directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.dest
    }

    /// Where the caller writes the store snapshot (`packie_store::Store::backup_into`).
    #[must_use]
    pub fn store_dir(&self) -> PathBuf {
        self.dest.join(STORE)
    }

    /// Copies everything else, hashes every file, writes the manifest and records the
    /// attempt. On failure the partial directory is removed and the failure recorded.
    ///
    /// # Errors
    ///
    /// Any copy, hash, database or audit failure while assembling the backup.
    pub fn finish(self, layout: &Layout, actor: &Actor) -> Result<Manifest> {
        match self.complete(layout, actor) {
            Ok(manifest) => {
                let elapsed = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let summary = manifest.summary(&self.dest, elapsed);
                tracing::info!(
                    event = "backup.created",
                    directory = %self.dest.display(),
                    files = summary.files,
                    bytes = summary.bytes,
                    "Backup created in {}",
                    self.dest.display()
                );
                crate::record_attempt(layout, self.attempt(Ok(summary)));
                Ok(manifest)
            }
            Err(error) => {
                self.fail(layout, &error);
                Err(error)
            }
        }
    }

    /// Abandons the backup: removes what was written and records the failure.
    pub fn fail(self, layout: &Layout, error: &Error) {
        tracing::error!(event = "backup.failed", directory = %self.dest.display(), %error, "Backup failed");
        let removed = if self.created_dir {
            fs::remove_dir_all(&self.dest)
        } else {
            remove_children(&self.dest)
        };
        if let Err(cleanup) = removed {
            tracing::warn!(event = "backup.cleanup_failed", directory = %self.dest.display(), error = %cleanup, "The incomplete backup could not be removed");
        }
        crate::record_attempt(layout, self.attempt(Err(error)));
    }

    /// The status record of this backup's outcome.
    fn attempt(&self, outcome: std::result::Result<BackupSummary, &Error>) -> BackupAttempt {
        BackupAttempt {
            at: self.at,
            directory: self.dest.display().to_string(),
            ok: outcome.is_ok(),
            error: outcome.as_ref().err().map(ToString::to_string),
            summary: outcome.ok(),
        }
    }

    fn complete(&self, layout: &Layout, actor: &Actor) -> Result<Manifest> {
        let mut manifest = Manifest::new(self.at, actor.clone(), layout.node_id.clone());
        manifest.store = self.store(layout)?;
        manifest.audit = copy_audit(&layout.audit_dir(), &self.dest.join(AUDIT))?;
        manifest.config_version =
            copy_config(layout, &layout.config_dir(), &self.dest.join(CONFIG))?;
        copy_if_present(layout, &layout.runtime_file, &self.dest.join(RUNTIME))?;
        if let Some(bootstrap) = &layout.bootstrap_file {
            copy_if_present(layout, bootstrap, &self.dest.join(BOOTSTRAP))?;
        }
        copy_if_present(
            layout,
            &layout.state_dir.join(SECRET_NAMES),
            &self.dest.join(SECRET_NAMES),
        )?;
        for path in manifest::walk(&self.dest)? {
            let absolute = self.dest.join(&path);
            sync_path(&absolute)?;
            let (bytes, sha256) = fsutil::hash_file(&absolute)?;
            manifest.files.push(FileEntry {
                path,
                bytes,
                sha256,
            });
        }
        manifest.write(&self.dest)?;
        Ok(manifest)
    }

    /// The store snapshot the caller wrote, or one read from a closed store.
    fn store(&self, layout: &Layout) -> Result<Option<StoreInfo>> {
        let target = self.store_dir();
        let database = target.join(packie_store::DATABASE_FILE);
        if !database.is_file() {
            match packie_store::backup_closed(&layout.store_dir(), &target) {
                Ok(_) => {}
                Err(packie_store::Error::NotFound) => return Ok(None),
                Err(error) => return Err(error.into()),
            }
        }
        store_info(&database).map(Some)
    }
}

/// Counts what the store snapshot holds.
fn store_info(database: &Path) -> Result<StoreInfo> {
    let connection = rusqlite::Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let count = |sql: &str| connection.query_row(sql, [], |row| row.get::<_, i64>(0));
    Ok(StoreInfo {
        schema_version: count("SELECT COALESCE(MAX(version), 0) FROM schema_version")?,
        messages: u64::try_from(count("SELECT COUNT(*) FROM messages")?).unwrap_or(0),
        body_files: u32::try_from(count(
            "SELECT COUNT(DISTINCT file_path) FROM body_contents WHERE file_path IS NOT NULL",
        )?)
        .unwrap_or(u32::MAX),
    })
}

/// Copies the audit files. A seal or retention removal running at the same moment
/// changes the file set; the copy is then taken again (once).
fn copy_audit(src: &Path, dest: &Path) -> Result<Option<AuditInfo>> {
    if !src.is_dir() {
        return Ok(None);
    }
    if let Ok(Some(info)) = copy_audit_once(src, dest) {
        return Ok(Some(info));
    }
    remove_children(dest).map_err(Error::io(dest))?;
    copy_audit_once(src, dest)?
        .map(Some)
        .ok_or_else(|| Error::Invalid("the audit log changed during the backup; try again".into()))
}

/// One copy of the audit files; `None` when the file set changed while it ran.
fn copy_audit_once(src: &Path, dest: &Path) -> Result<Option<AuditInfo>> {
    let before = database_names(src)?;
    let copied = packie_audit::copy_dir(src, dest)?;
    if database_names(src)? != before {
        return Ok(None);
    }
    Ok(Some(AuditInfo {
        files: u32::try_from(copied.files.len()).unwrap_or(u32::MAX),
        head_seq: copied.head.as_ref().map(|(seq, _)| *seq),
        head_hash: copied.head.as_ref().map(|(_, hash)| hash.clone()),
    }))
}

/// The sorted names of the audit database files in `dir`.
fn database_names(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut names: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(Error::io(dir))?
        .filter_map(std::result::Result::ok)
        .map(|entry| PathBuf::from(entry.file_name()))
        .filter(|name| has_extension(name, "sqlite3"))
        .collect();
    names.sort();
    Ok(names)
}

/// Copies every saved version and note (never a secret file placed there); returns the
/// newest version number.
fn copy_config(layout: &Layout, src: &Path, dest: &Path) -> Result<Option<u64>> {
    if !src.is_dir() {
        return Ok(None);
    }
    private_dir(dest)?;
    let mut newest = None;
    for entry in fs::read_dir(src).map_err(Error::io(src))? {
        let entry = entry.map_err(Error::io(src))?;
        if !entry
            .file_type()
            .map_err(Error::io(&entry.path()))?
            .is_file()
        {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Temporary files of an apply in progress are not part of the history.
        if has_extension(&entry.path(), "tmp") || layout.is_secret(&entry.path()) {
            continue;
        }
        copy_file(&entry.path(), &dest.join(&*name))?;
        let version = name
            .strip_prefix("runtime.v")
            .and_then(|rest| rest.strip_suffix(".json"))
            .and_then(|digits| digits.parse::<u64>().ok());
        newest = newest.max(version);
    }
    Ok(newest)
}

fn copy_if_present(layout: &Layout, src: &Path, dest: &Path) -> Result<()> {
    if src.is_file() && !layout.is_secret(src) {
        copy_file(src, dest)?;
    }
    Ok(())
}
