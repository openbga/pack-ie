//! Versioned, atomic persistence of runtime documents and their change-control record
//! (`docs/spec/configuration.md` §10, decision 16).
//!
//! Each saved version is one file, `runtime.v<version>.json`, in the store directory, with
//! its record beside it in `runtime.v<version>.meta.json`: who applied it, when, the
//! required note, and any verification sign-offs. The record is written before the document,
//! and the newest valid file with its record is the active configuration. Files are written
//! to a new temporary file, flushed and renamed, so a crash leaves either the old or the new
//! version, never a partial one or one without its record. Versions are kept without
//! limit unless a limit is configured, and never removed while younger than the protection
//! period (the audit retention).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use packie_core::{Actor, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::diff::ChannelOutcome;
use crate::error::ConfigError;
use crate::runtime::RuntimeConfig;

const PREFIX: &str = "runtime.v";
const SUFFIX: &str = ".json";
const META: &str = "meta.json";

/// A saved version, as listed in the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct VersionInfo {
    /// The version.
    #[ts(type = "number")]
    pub config_version: u64,
    /// When the document was generated.
    pub generated_at: Timestamp,
    /// Number of endpoints.
    pub endpoints: u32,
    /// Number of channels.
    pub channels: u32,
    /// Why it was applied.
    pub note: Option<String>,
    /// Who applied it; absent for versions saved before the record was kept.
    pub applied_by: Option<Actor>,
    /// When it was saved.
    pub applied_at: Option<Timestamp>,
    /// Verification sign-offs, oldest first; empty means "not verified since the change".
    pub verifications: Vec<Verification>,
    /// What applying the version did to each channel; empty for versions saved before the
    /// outcomes were kept, and for the startup import (applied with the engine's start).
    pub outcomes: Vec<ChannelOutcome>,
}

/// A sign-off that a version was verified after the change (CAP GEN.48500). What verified
/// means is the laboratory's procedure; PACK-IE records who, when and their reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Verification {
    /// The OS account that signed off.
    pub verified_by: Actor,
    /// When.
    pub verified_at: Timestamp,
    /// The operator's note, such as a validation record number.
    pub note: String,
}

/// The change-control record of one version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionRecord {
    applied_by: Actor,
    applied_at: Timestamp,
    note: String,
    #[serde(default)]
    verifications: Vec<Verification>,
    #[serde(default)]
    outcomes: Vec<ChannelOutcome>,
}

/// The directory of saved runtime versions.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
    keep: Option<usize>,
    protect: Duration,
}

impl ConfigStore {
    /// Opens (creating if needed) the store in `dir`, keeping every version, or the newest
    /// `keep` (at least one) when set; versions younger than `protect` are never removed.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Write`] when the directory cannot be created.
    pub fn open(
        dir: impl Into<PathBuf>,
        keep: Option<usize>,
        protect: Duration,
    ) -> Result<Self, ConfigError> {
        let dir = dir.into();
        create_private_dir(&dir)?;
        Ok(Self {
            dir,
            keep: keep.map(|k| k.max(1)),
            protect,
        })
    }

    /// The store directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The newest version that parses, validates and has its change-control record. Newer
    /// files that fail are skipped with a warning, so the engine starts from its last valid,
    /// attributed configuration. A version without a record is used only in a store that
    /// keeps none (versions saved before records were kept).
    ///
    /// # Errors
    ///
    /// [`ConfigError::Read`] when the directory cannot be listed.
    pub fn latest(&self) -> Result<Option<RuntimeConfig>, ConfigError> {
        let versions = self.versions()?;
        let keeps_records = versions.iter().any(|(_, path)| meta_path(path).exists());
        for (version, path) in versions {
            if keeps_records && self.record(&path).is_none() {
                tracing::warn!(
                    event = "config.version_skipped",
                    config_version = version,
                    error = "the version has no change-control record (who applied it and why)",
                    "Skipping a saved configuration version without its record"
                );
                continue;
            }
            match load_runtime_file(&path) {
                Ok(config) => return Ok(Some(config)),
                Err(error) => tracing::warn!(
                    event = "config.version_skipped",
                    config_version = version,
                    %error,
                    "Skipping unusable saved configuration version"
                ),
            }
        }
        Ok(None)
    }

    /// Saves `config` as version `config.config_version`, applied by `actor` with `note`
    /// (the change-control record), then prunes versions beyond the limit.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Serialize`] or [`ConfigError::Write`] when the version cannot be written.
    pub fn save(
        &self,
        config: &RuntimeConfig,
        note: &str,
        actor: &Actor,
    ) -> Result<(), ConfigError> {
        let path = self.path_of(config.config_version);
        let mut text = config.to_pretty_json()?;
        text.push('\n');
        let record = VersionRecord {
            applied_by: actor.clone(),
            applied_at: Timestamp::now(),
            note: note.to_owned(),
            verifications: Vec::new(),
            outcomes: Vec::new(),
        };
        // The record first: a version is never published without who applied it and why. A
        // record without its document is not a version and is removed on failure.
        self.write_record(config.config_version, &record)?;
        if let Err(error) = write_atomic(&path, text.as_bytes()) {
            let _ = fs::remove_file(meta_path(&path));
            return Err(error);
        }
        self.prune();
        Ok(())
    }

    /// Records what applying `version` did to each channel.
    pub fn record_outcomes(
        &self,
        version: u64,
        outcomes: &[ChannelOutcome],
    ) -> Result<(), ConfigError> {
        let path = self.path_of(version);
        let Some(mut record) = self.record(&path) else {
            return Err(ConfigError::UnknownVersion(version));
        };
        record.outcomes = outcomes.to_vec();
        self.write_record(version, &record)
    }

    /// Records that `actor` verified `version`, with their `note`; returns the version's
    /// history entry.
    ///
    /// # Errors
    ///
    /// [`ConfigError::UnknownVersion`] when `version` is not saved; [`ConfigError::Write`]
    /// when the record cannot be written.
    pub fn verify(
        &self,
        version: u64,
        actor: &Actor,
        note: &str,
    ) -> Result<VersionInfo, ConfigError> {
        let path = self.path_of(version);
        if !path.exists() {
            return Err(ConfigError::UnknownVersion(version));
        }
        let mut record = self.record(&path).unwrap_or_else(|| VersionRecord {
            applied_by: Actor::system(),
            applied_at: Timestamp::now(),
            note: legacy_note(&path).unwrap_or_default(),
            verifications: Vec::new(),
            outcomes: Vec::new(),
        });
        record.verifications.push(Verification {
            verified_by: actor.clone(),
            verified_at: Timestamp::now(),
            note: note.to_owned(),
        });
        self.write_record(version, &record)?;
        self.info(&path).ok_or(ConfigError::UnknownVersion(version))
    }

    /// Loads one saved version.
    ///
    /// # Errors
    ///
    /// [`ConfigError::UnknownVersion`] when `version` is not saved; otherwise as
    /// [`load_runtime_file`].
    pub fn load(&self, version: u64) -> Result<RuntimeConfig, ConfigError> {
        let path = self.path_of(version);
        if !path.exists() {
            return Err(ConfigError::UnknownVersion(version));
        }
        load_runtime_file(&path)
    }

    /// Saved versions, newest first. Unreadable files are left out.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Read`] when the directory cannot be listed.
    pub fn history(&self) -> Result<Vec<VersionInfo>, ConfigError> {
        Ok(self
            .versions()?
            .into_iter()
            .filter_map(|(_, path)| self.info(&path))
            .collect())
    }

    fn info(&self, path: &Path) -> Option<VersionInfo> {
        let config = load_runtime_file(path).ok()?;
        let record = self.record(path);
        Some(VersionInfo {
            config_version: config.config_version,
            generated_at: config.generated_at,
            endpoints: u32::try_from(config.endpoints.len()).unwrap_or(u32::MAX),
            channels: u32::try_from(config.channels.len()).unwrap_or(u32::MAX),
            note: record
                .as_ref()
                .map(|r| r.note.clone())
                .or_else(|| legacy_note(path)),
            applied_by: record.as_ref().map(|r| r.applied_by.clone()),
            applied_at: record.as_ref().map(|r| r.applied_at),
            outcomes: record
                .as_ref()
                .map(|r| r.outcomes.clone())
                .unwrap_or_default(),
            verifications: record.map(|r| r.verifications).unwrap_or_default(),
        })
    }

    fn record(&self, path: &Path) -> Option<VersionRecord> {
        let text = fs::read_to_string(meta_path(path)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_record(&self, version: u64, record: &VersionRecord) -> Result<(), ConfigError> {
        let mut text = serde_json::to_string_pretty(record).map_err(|e| ConfigError::Write {
            path: meta_path(&self.path_of(version)),
            source: std::io::Error::other(e),
        })?;
        text.push('\n');
        write_atomic(&meta_path(&self.path_of(version)), text.as_bytes())
    }

    /// Removes versions beyond the limit that are older than the protection period.
    fn prune(&self) {
        let Some(keep) = self.keep else {
            return;
        };
        let Ok(versions) = self.versions() else {
            return;
        };
        let horizon = Timestamp::now().checked_sub(self.protect);
        for (_, old) in versions.into_iter().skip(keep) {
            let applied = self.record(&old).map(|r| r.applied_at);
            if applied.zip(horizon).is_some_and(|(at, h)| at >= h) {
                continue;
            }
            let _ = fs::remove_file(meta_path(&old));
            let _ = fs::remove_file(old.with_extension("note"));
            if let Err(source) = fs::remove_file(&old) {
                tracing::warn!(
                    event = "config.prune_failed",
                    path = %old.display(),
                    error = %source,
                    "Could not remove an old configuration version"
                );
            }
        }
    }

    fn path_of(&self, version: u64) -> PathBuf {
        self.dir.join(format!("{PREFIX}{version:010}{SUFFIX}"))
    }

    /// Version files, newest first.
    fn versions(&self) -> Result<Vec<(u64, PathBuf)>, ConfigError> {
        let entries = fs::read_dir(&self.dir).map_err(|source| ConfigError::Read {
            path: self.dir.clone(),
            source,
        })?;
        let mut versions: Vec<(u64, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name();
                let version = name
                    .to_str()?
                    .strip_prefix(PREFIX)?
                    .strip_suffix(SUFFIX)?
                    .parse()
                    .ok()?;
                Some((version, entry.path()))
            })
            .collect();
        versions.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
        Ok(versions)
    }
}

/// `runtime.v<N>.meta.json` beside `runtime.v<N>.json`.
fn meta_path(path: &Path) -> PathBuf {
    path.with_extension(META)
}

/// The `.note` file of a version saved before the change-control record existed.
fn legacy_note(path: &Path) -> Option<String> {
    fs::read_to_string(path.with_extension("note")).ok()
}

/// Reads, parses and validates a runtime document from `path`.
///
/// # Errors
///
/// [`ConfigError::Read`] or [`ConfigError::Invalid`].
pub fn load_runtime_file(path: &Path) -> Result<RuntimeConfig, ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_owned(),
        source,
    })?;
    RuntimeConfig::parse(&text).map_err(|errors| ConfigError::Invalid {
        path: path.to_owned(),
        errors,
    })
}

/// Writes `bytes` to `path` atomically: a new temporary file in the same directory, created
/// exclusively (never an existing file or symlink) and owner-only on Unix, is written,
/// flushed to disk and renamed over `path`.
///
/// # Errors
///
/// [`ConfigError::Write`] when any step fails.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let write_err = |source| ConfigError::Write {
        path: path.to_owned(),
        source,
    };
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut prefix = std::ffi::OsString::from(".");
    prefix.push(path.file_name().unwrap_or_default());
    prefix.push(".");
    // Unique name, O_EXCL, mode 0600; removed again if anything below fails.
    let mut temp = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(write_err)?;
    temp.write_all(bytes).map_err(write_err)?;
    temp.as_file().sync_all().map_err(write_err)?;
    persist_replacing(temp, path).map_err(write_err)?;
    // Persist the rename itself; directories cannot be opened for sync on Windows.
    #[cfg(unix)]
    fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(write_err)?;
    Ok(())
}

/// Moves a written temporary file over `target`, retrying while Windows reports the name
/// busy ([`retry_while_busy`]). The temporary is removed when the move fails.
///
/// # Errors
///
/// The move's last error.
pub fn persist_replacing(temp: tempfile::NamedTempFile, target: &Path) -> io::Result<()> {
    let mut pending = Some(temp);
    retry_while_busy(|| {
        let temp = pending
            .take()
            .ok_or_else(|| io::Error::other("the temporary file is gone"))?;
        temp.persist(target).map(drop).map_err(|e| {
            pending = Some(e.file);
            e.error
        })
    })
}

/// Retries a rename (or another operation on a path) while Windows reports the path busy:
/// a virus scanner or the search indexer opening a new file, or another writer replacing
/// the same name, makes Windows refuse a rename for a moment with "access denied" or a
/// sharing or lock violation. Up to 10 retries, waiting 25 ms longer before each (about
/// 1.4 s in all), as SQLite's Windows file layer does. Elsewhere the operation runs once.
///
/// # Errors
///
/// The operation's last error.
pub fn retry_while_busy<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    const RETRIES: u32 = 10;
    const STEP: Duration = Duration::from_millis(25);
    let mut retries = 0;
    loop {
        match operation() {
            Err(error) if cfg!(windows) && retries < RETRIES && is_busy(&error) => {
                retries += 1;
                std::thread::sleep(STEP * retries);
            }
            result => return result,
        }
    }
}

/// Access denied, or `ERROR_SHARING_VIOLATION` (32) / `ERROR_LOCK_VIOLATION` (33).
fn is_busy(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || matches!(error.raw_os_error(), Some(32 | 33))
}

/// Creates `dir` and its parents; on Unix a new directory is accessible by the owner only.
///
/// # Errors
///
/// [`ConfigError::Write`] when the directory cannot be created.
pub fn create_private_dir(dir: &Path) -> Result<(), ConfigError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir).map_err(|source| ConfigError::Write {
        path: dir.to_owned(),
        source,
    })
}
