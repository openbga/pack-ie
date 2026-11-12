//! Restoring a backup into a state directory whose engine is stopped.

use std::fs;
use std::path::{Path, PathBuf};

use packie_audit::{Audit, Entry};
use packie_core::{Actor, Timestamp};
use packie_host::{HostError, InstanceLock};
use packie_ipc::audit::{AuditAction, AuditObjectType};
use serde_json::json;

use crate::fsutil::{self, copy_file, private_dir, remove_tree, sync_path, tree};
use crate::manifest::{FileEntry, Manifest};
use crate::{AUDIT, BOOTSTRAP, CONFIG, Error, Layout, RUNTIME, Result, STORE};

/// What [`restore`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    /// The verified manifest of the restored backup.
    pub manifest: Manifest,
    /// Where the replaced state was moved (`<state>/pre-restore-<UTC time>`).
    pub pre_restore: PathBuf,
    /// The `backup_restore` audit event.
    pub audit_seq: u64,
}

/// Restores the backup in `backup` into `layout`:
///
/// 1. takes the instance lock, so it refuses while an engine runs and no engine can start
///    meanwhile;
/// 2. verifies the backup completely;
/// 3. moves the current `store/`, `audit/` and `config/` into `pre-restore-<time>/` (and
///    copies the runtime document there), then checks they are all there;
/// 4. copies the backup's store, audit and configuration in: only the regular files its
///    manifest lists, opened without following links and re-hashed; then writes its
///    `runtime.json` to `runtime.file`. A secrets file the bootstrap keeps inside one of
///    these directories stays at its path: it is moved back after the swap and never taken
///    from the backup;
/// 5. records `backup_restore` in the restored audit log.
///
/// A failure at any step after the first move undoes exactly what was done: directories
/// copied in are removed, moved originals go back, and directories the restore never
/// touched stay as they are.
///
/// The bootstrap document and secret values are not restored.
///
/// # Errors
///
/// [`Error::EngineRunning`] while an engine holds the state directory;
/// [`Error::NotVerified`] when the backup fails verification (nothing is touched); any other
/// error after the state was undone as described above.
pub fn restore(backup: &Path, layout: &Layout, actor: &Actor) -> Result<Restored> {
    let bootstrap = layout
        .bootstrap_file
        .clone()
        .unwrap_or_else(|| layout.state_dir.join(BOOTSTRAP));
    private_dir(&layout.state_dir)?;
    let _lock =
        InstanceLock::acquire(&layout.state_dir, &bootstrap).map_err(|error| match error {
            HostError::AlreadyRunning { pid, .. } => Error::EngineRunning { pid },
            other => Error::Host(other),
        })?;
    let check = crate::verify(backup)?;
    let Some(manifest) = check.manifest.filter(|_| check.ok) else {
        return Err(Error::NotVerified(check.problems));
    };
    let stamp = crate::utc_stamp(Timestamp::now());
    let pre_restore = layout.state_dir.join(format!("pre-restore-{stamp}"));
    private_dir(&pre_restore)?;
    let mut swap = Swap::new(layout, &pre_restore);
    if let Err(error) = swap.put_in_place(backup, &manifest) {
        swap.undo();
        return Err(error);
    }
    let audit_seq = match record(layout, actor, backup, &manifest, &pre_restore) {
        Ok(seq) => seq,
        Err(error) => {
            swap.undo();
            return Err(error);
        }
    };
    give_to_owner(layout)?;
    tracing::info!(
        event = "backup.restored",
        backup = %backup.display(),
        pre_restore = %pre_restore.display(),
        "Backup restored; the previous state is in {}",
        pre_restore.display()
    );
    Ok(Restored {
        manifest,
        pre_restore,
        audit_seq,
    })
}

/// Moves the current state aside and the backup in, remembering exactly what it changed so
/// that a failure at any step puts back what was moved and removes only what was placed.
struct Swap<'a> {
    layout: &'a Layout,
    pre_restore: &'a Path,
    /// Originals moved into `pre_restore`, in order.
    moved: Vec<&'static str>,
    /// Directories created in the state directory (backup content, or a kept secret's
    /// parent).
    placed: Vec<&'static str>,
    /// Secret files inside a swapped directory (`config/secrets.json`): moved back to their
    /// configured path after the backup is in, never taken from the backup.
    kept: Vec<String>,
    /// The kept secrets already moved back.
    kept_back: Vec<String>,
    /// The original runtime document was saved into `pre_restore`.
    runtime_saved: bool,
    /// The runtime document was replaced (or created) from the backup.
    runtime_written: bool,
}

const SWAPPED: [&str; 3] = [STORE, AUDIT, CONFIG];

impl<'a> Swap<'a> {
    fn new(layout: &'a Layout, pre_restore: &'a Path) -> Self {
        Self {
            layout,
            pre_restore,
            moved: Vec::new(),
            placed: Vec::new(),
            kept: kept_secrets(layout),
            kept_back: Vec::new(),
            runtime_saved: false,
            runtime_written: false,
        }
    }

    /// Moves the originals aside, checks they are there, copies the manifest's regular
    /// files in (never following links, re-checking each hash), then moves kept secrets
    /// back to their configured paths.
    fn put_in_place(&mut self, backup: &Path, manifest: &Manifest) -> Result<()> {
        self.move_aside()?;
        self.check_aside()?;
        for name in SWAPPED {
            let prefix = format!("{name}/");
            let entries: Vec<&FileEntry> = manifest
                .files
                .iter()
                .filter(|e| e.path.starts_with(&prefix) && !self.kept.contains(&e.path))
                .collect();
            if entries.is_empty() {
                continue;
            }
            // Recorded before copying: a partial copy is removed by `undo`.
            self.placed.push(name);
            private_dir(&self.layout.state_dir.join(name))?;
            for entry in entries {
                place_file(&self.layout.state_dir, backup, entry)?;
            }
            sync_tree(&self.layout.state_dir.join(name))?;
        }
        self.keep_secrets()?;
        let runtime = &self.layout.runtime_file;
        if let Some(entry) = manifest.files.iter().find(|e| e.path == RUNTIME)
            && !self.layout.is_secret(runtime)
        {
            let bytes = fsutil::read_listed(backup, entry)?;
            self.runtime_written = true;
            packie_config::write_atomic(runtime, &bytes)?;
        }
        sync_path(&self.layout.state_dir)
    }

    /// Moves each kept secret from `pre_restore` back to its configured path.
    fn keep_secrets(&mut self) -> Result<()> {
        for relative in self.kept.clone() {
            let saved = self.pre_restore.join(&relative);
            if !saved.is_file() {
                continue;
            }
            let target = self.layout.state_dir.join(&relative);
            if let Some(root) = SWAPPED
                .iter()
                .find(|n| relative.starts_with(&format!("{n}/")))
                && !self.placed.contains(root)
            {
                self.placed.push(root);
            }
            if let Some(parent) = target.parent() {
                private_dir(parent)?;
            }
            rename(&saved, &target).map_err(Error::io(&target))?;
            self.kept_back.push(relative);
            if let Some(parent) = target.parent() {
                sync_path(parent)?;
            }
        }
        Ok(())
    }

    fn move_aside(&mut self) -> Result<()> {
        for name in SWAPPED {
            let current = self.layout.state_dir.join(name);
            if current.exists() {
                rename(&current, &self.pre_restore.join(name)).map_err(Error::io(&current))?;
                self.moved.push(name);
            }
        }
        let runtime = &self.layout.runtime_file;
        if runtime.is_file() {
            copy_file(runtime, &self.pre_restore.join(RUNTIME))?;
            self.runtime_saved = true;
        }
        sync_path(&self.layout.state_dir)?;
        sync_path(self.pre_restore)
    }

    /// Every moved original is in `pre_restore` and gone from the state directory.
    fn check_aside(&self) -> Result<()> {
        for name in &self.moved {
            let saved = self.pre_restore.join(name);
            if !saved.is_dir() || self.layout.state_dir.join(name).exists() {
                return Err(Error::Invalid(format!(
                    "{} was not moved aside completely; nothing was restored",
                    self.layout.state_dir.join(name).display()
                )));
            }
        }
        Ok(())
    }

    /// Puts the previous state back after a failed restore: removes only what was placed,
    /// moves back only what was moved, and leaves every other directory alone (best
    /// effort, logged).
    fn undo(&self) {
        self.undo_directories();
        self.undo_runtime();
        if let Err(error) = sync_path(&self.layout.state_dir) {
            tracing::warn!(event = "backup.undo_failed", %error, "Could not sync the state directory");
        }
    }

    /// Kept secrets aside again, placed directories removed, moved originals back.
    fn undo_directories(&self) {
        for relative in self.kept_back.iter().rev() {
            let target = self.layout.state_dir.join(relative);
            if let Err(error) = rename(&target, &self.pre_restore.join(relative)) {
                tracing::error!(event = "backup.undo_failed", path = %target.display(), %error, "Could not move a kept secrets file back aside");
            }
        }
        for name in &self.placed {
            let restored = self.layout.state_dir.join(name);
            if restored.exists()
                && let Err(error) = remove_tree(&restored)
            {
                tracing::error!(event = "backup.undo_failed", path = %restored.display(), %error, "Could not remove a partly restored directory");
            }
        }
        for name in self.moved.iter().rev() {
            let saved = self.pre_restore.join(name);
            if let Err(error) = rename(&saved, &self.layout.state_dir.join(name)) {
                tracing::error!(event = "backup.undo_failed", path = %saved.display(), %error, "Could not put the previous state back");
            }
        }
    }

    /// The previous runtime document back, or the one the restore created removed.
    fn undo_runtime(&self) {
        if self.runtime_written {
            let runtime = &self.layout.runtime_file;
            let put_back = if self.runtime_saved {
                replace_file(&self.pre_restore.join(RUNTIME), runtime)
            } else {
                fs::remove_file(runtime).map_err(Error::io(runtime))
            };
            if let Err(error) = put_back {
                tracing::error!(event = "backup.undo_failed", %error, "Could not put the previous runtime document back");
            }
        }
    }
}

/// Copies one listed backup file to its place under the state directory.
fn place_file(state_dir: &Path, backup: &Path, entry: &FileEntry) -> Result<()> {
    let target = fsutil::inside(state_dir, &entry.path)
        .ok_or_else(|| Error::Invalid(format!("{} is not a backup path", entry.path)))?;
    if let Some(parent) = target.parent() {
        private_dir(parent)?;
    }
    fsutil::copy_listed(backup, entry, &target)
}

fn record(
    layout: &Layout,
    actor: &Actor,
    backup: &Path,
    manifest: &Manifest,
    pre_restore: &Path,
) -> Result<u64> {
    let audit = Audit::open(
        layout.audit_dir(),
        packie_audit::Options {
            background: false,
            ..packie_audit::Options::default()
        },
    )?;
    let pre_restore_name = pre_restore
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    let entry = Entry::new(actor.clone(), AuditAction::BackupRestore)
        .object(AuditObjectType::Backup, backup.display().to_string())
        .details(json!({
            "created_at": manifest.created_at,
            "node_id": manifest.node_id,
            "config_version": manifest.config_version,
            "messages": manifest.store.as_ref().map(|s| s.messages),
            "audit_head_seq": manifest.audit.as_ref().and_then(|a| a.head_seq),
            "pre_restore": pre_restore_name,
        }));
    Ok(audit.record(&entry)?)
}

/// Syncs every directory of a restored tree, children before parents.
fn sync_tree(root: &Path) -> Result<()> {
    let entries = tree(root)?;
    for (relative, _) in entries.iter().filter(|(_, kind)| kind.is_dir()).rev() {
        sync_path(&root.join(relative))?;
    }
    sync_path(root)
}

/// Secret files (`secrets.file`) that live inside a swapped directory, as backup paths
/// (`config/secrets.json`).
fn kept_secrets(layout: &Layout) -> Vec<String> {
    let mut kept = Vec::new();
    for secret in &layout.secret_files {
        let Ok(secret) = fs::canonicalize(secret) else {
            continue;
        };
        for name in SWAPPED {
            let Ok(root) = fs::canonicalize(layout.state_dir.join(name)) else {
                continue;
            };
            if let Ok(relative) = secret.strip_prefix(&root) {
                kept.push(format!("{name}/{}", fsutil::slashed(relative)));
            }
        }
    }
    kept
}

/// Renames `from` to `to`. On Windows a directory cannot be renamed while any file in it is
/// open, and a virus scanner or the search indexer briefly opens newly written files, so a
/// busy rename is retried for a moment there.
fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    packie_config::retry_while_busy(|| fs::rename(from, to))
}

/// Writes `src`'s bytes to `dest` through a temporary file and a rename.
fn replace_file(src: &Path, dest: &Path) -> Result<()> {
    let bytes = fs::read(src).map_err(Error::io(src))?;
    packie_config::write_atomic(dest, &bytes)?;
    Ok(())
}

/// When run as root for a service account, hands the restored files to the state
/// directory's owner so the engine can open them.
#[cfg(unix)]
fn give_to_owner(layout: &Layout) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let state = &layout.state_dir;
    let owner = fs::metadata(state).map_err(Error::io(state))?;
    if nix::unistd::geteuid().as_raw() == owner.uid() {
        return Ok(());
    }
    let roots = SWAPPED
        .map(|name| state.join(name))
        .into_iter()
        .chain([layout.runtime_file.clone()])
        .filter(|path| path.exists());
    for root in roots {
        let below = if root.is_dir() {
            tree(&root)?
        } else {
            Vec::new()
        };
        let paths =
            std::iter::once(root.clone()).chain(below.iter().map(|(rel, _)| root.join(rel)));
        for path in paths {
            std::os::unix::fs::chown(&path, Some(owner.uid()), Some(owner.gid()))
                .map_err(Error::io(&path))?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn give_to_owner(_layout: &Layout) -> Result<()> {
    // Windows: the state directory's inherited ACL (SYSTEM and Administrators) applies.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A state directory with every swapped directory and a runtime document, a backup with
    /// different content, and the pre-restore directory.
    struct Rig {
        _dir: tempfile::TempDir,
        layout: Layout,
        backup: PathBuf,
        pre_restore: PathBuf,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let backup = dir.path().join("backup");
        for name in SWAPPED {
            fs::create_dir_all(state.join(name)).unwrap();
            fs::write(state.join(name).join("original"), name).unwrap();
            fs::create_dir_all(backup.join(name)).unwrap();
            fs::write(backup.join(name).join("from-backup"), name).unwrap();
        }
        fs::write(state.join(RUNTIME), b"current").unwrap();
        fs::write(backup.join(RUNTIME), b"backup").unwrap();
        let pre_restore = state.join("pre-restore-test");
        fs::create_dir(&pre_restore).unwrap();
        Rig {
            layout: Layout {
                runtime_file: state.join(RUNTIME),
                state_dir: state,
                bootstrap_file: None,
                node_id: "lab-1".into(),
                secret_files: Vec::new(),
            },
            _dir: dir,
            backup,
            pre_restore,
        }
    }

    /// The manifest of a backup directory as it is now.
    fn listing(backup: &Path) -> Manifest {
        let mut manifest = Manifest::new(Timestamp::now(), Actor::system(), "lab-1".into());
        for path in crate::manifest::walk(backup).unwrap() {
            let (bytes, sha256) = fsutil::hash_file(&backup.join(&path)).unwrap();
            manifest.files.push(FileEntry {
                path,
                bytes,
                sha256,
            });
        }
        manifest
    }

    /// Every original is in place with its content, and nothing came from the backup.
    fn assert_untouched(rig: &Rig) {
        for name in SWAPPED {
            let dir = rig.layout.state_dir.join(name);
            assert_eq!(fs::read_to_string(dir.join("original")).unwrap(), name);
            assert!(
                !dir.join("from-backup").exists(),
                "{name} has backup content"
            );
            assert!(!rig.pre_restore.join(name).exists(), "{name} still aside");
        }
        assert_eq!(fs::read(&rig.layout.runtime_file).unwrap(), b"current");
    }

    #[test]
    fn a_failed_move_puts_back_what_moved_and_leaves_the_rest_alone() {
        let rig = rig();
        // The second move (audit) fails: its place in pre-restore is taken.
        fs::create_dir(rig.pre_restore.join(AUDIT)).unwrap();
        fs::write(rig.pre_restore.join(AUDIT).join("occupied"), b"x").unwrap();
        let mut swap = Swap::new(&rig.layout, &rig.pre_restore);
        assert!(
            swap.put_in_place(&rig.backup, &listing(&rig.backup))
                .is_err()
        );
        assert_eq!(swap.moved, [STORE], "only the store was moved");
        swap.undo();
        fs::remove_dir_all(rig.pre_restore.join(AUDIT)).unwrap();
        assert_untouched(&rig);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_copy_removes_what_was_placed_and_moves_every_original_back() {
        use std::os::unix::fs::PermissionsExt;
        if nix::unistd::geteuid().is_root() {
            return; // root reads unreadable files, so the copy cannot be made to fail
        }
        let rig = rig();
        let unreadable = rig.backup.join(CONFIG).join("unreadable");
        fs::write(&unreadable, b"x").unwrap();
        let manifest = listing(&rig.backup);
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        let mut swap = Swap::new(&rig.layout, &rig.pre_restore);
        assert!(swap.put_in_place(&rig.backup, &manifest).is_err());
        assert_eq!(swap.moved, SWAPPED);
        assert_eq!(swap.placed, SWAPPED, "config was placed partly");
        swap.undo();
        assert_untouched(&rig);
    }

    #[test]
    fn a_runtime_document_the_restore_created_is_removed_again() {
        let rig = rig();
        fs::remove_file(&rig.layout.runtime_file).unwrap();
        let mut swap = Swap::new(&rig.layout, &rig.pre_restore);
        swap.put_in_place(&rig.backup, &listing(&rig.backup))
            .unwrap();
        assert!(swap.runtime_written && !swap.runtime_saved);
        swap.undo();
        assert!(!rig.layout.runtime_file.exists());
        for name in SWAPPED {
            let dir = rig.layout.state_dir.join(name);
            assert!(dir.join("original").is_file() && !dir.join("from-backup").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_listed_file_replaced_by_a_link_after_verification_is_refused_and_undone() {
        let rig = rig();
        let manifest = listing(&rig.backup);
        // Same bytes outside the backup, so only the link itself is wrong.
        let outside = rig.layout.state_dir.parent().unwrap().join("outside");
        fs::write(&outside, CONFIG).unwrap();
        let listed = rig.backup.join(CONFIG).join("from-backup");
        fs::remove_file(&listed).unwrap();
        std::os::unix::fs::symlink(&outside, &listed).unwrap();
        let mut swap = Swap::new(&rig.layout, &rig.pre_restore);
        let error = swap.put_in_place(&rig.backup, &manifest).unwrap_err();
        assert!(error.to_string().contains("symbolic link"), "{error}");
        swap.undo();
        assert_untouched(&rig);
    }

    #[test]
    fn a_secrets_file_kept_in_config_stays_through_restore_and_undo() {
        let mut rig = rig();
        let secrets = rig.layout.state_dir.join(CONFIG).join("secrets.json");
        fs::write(&secrets, b"current values").unwrap();
        // An older backup that still carries a secrets file at the same path.
        fs::write(rig.backup.join(CONFIG).join("secrets.json"), b"old values").unwrap();
        rig.layout.secret_files = vec![secrets.clone()];
        let mut swap = Swap::new(&rig.layout, &rig.pre_restore);
        assert_eq!(swap.kept, ["config/secrets.json"]);
        swap.put_in_place(&rig.backup, &listing(&rig.backup))
            .unwrap();
        assert_eq!(fs::read(&secrets).unwrap(), b"current values");
        assert!(
            rig.layout
                .state_dir
                .join(CONFIG)
                .join("from-backup")
                .is_file()
        );
        assert!(!rig.pre_restore.join(CONFIG).join("secrets.json").exists());
        swap.undo();
        assert_untouched(&rig);
        assert_eq!(fs::read(&secrets).unwrap(), b"current values");
    }
}
