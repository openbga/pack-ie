//! Copying the audit directory into a backup.

use std::path::{Path, PathBuf};

use crate::Result;
use crate::files;

/// What [`copy_dir`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    /// The files written, absolute.
    pub files: Vec<PathBuf>,
    /// The newest event copied: seq and hash.
    pub head: Option<(u64, String)>,
}

/// Copies every audit file of `src` into `dest`: sealed files as they are, the current
/// file through the `SQLite` online backup API (a consistent snapshot, rollback-journal
/// mode). Used with the engine stopped; [`Audit::backup_into`](crate::Audit::backup_into) while it runs.
///
/// # Errors
///
/// `dest` cannot be created, or a file cannot be listed, copied or read.
pub fn copy_dir(src: &Path, dest: &Path) -> Result<Copied> {
    files::private_dir(dest)?;
    let mut written = Vec::new();
    let mut head = None;
    for file in files::list(src)? {
        let target = dest.join(file.name());
        let copied = if file.is_current() {
            snapshot_current(&file.path, &target)?
        } else {
            std::fs::copy(&file.path, &target)?;
            files::restrict(&target, true)?;
            files::last_event(&files::read_only(&target)?)?
        };
        if let Some((seq, hash, _)) = copied {
            head = Some((seq, hash));
        }
        written.push(target);
    }
    Ok(Copied {
        files: written,
        head,
    })
}

/// Snapshots the live current file into `target` as a standalone file; returns its newest event.
fn snapshot_current(source: &Path, target: &Path) -> Result<Option<(u64, String, String)>> {
    let source = files::read_only(source)?;
    let mut copy = rusqlite::Connection::open(target)?;
    rusqlite::backup::Backup::new(&source, &mut copy)?.step(-1)?;
    copy.pragma_update(None, "journal_mode", "DELETE")?;
    let last = files::last_event(&copy)?;
    copy.close().map_err(|(_, error)| error)?;
    files::restrict(target, false)?;
    Ok(last)
}
