//! `manifest.json`: what a backup contains and the SHA-256 of every file.

use std::fs;
use std::io::Write;
use std::path::Path;

use packie_core::{Actor, Timestamp};
use packie_ipc::backup::BackupSummary;
use serde::{Deserialize, Serialize};

use crate::fsutil::{self, private_create};
use crate::{Error, Result};

/// The manifest file name; it is written last, so a backup without it is incomplete.
pub const MANIFEST: &str = "manifest.json";
/// The manifest format this build writes and reads.
const FORMAT: u32 = 1;

/// A backup's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest format, 1.
    pub format: u32,
    /// The engine version that wrote it.
    pub engine_version: String,
    /// When the snapshot was taken.
    pub created_at: Timestamp,
    /// Who took it.
    pub actor: Actor,
    /// The node id of the engine.
    pub node_id: String,
    /// The store snapshot, when the state had a store.
    pub store: Option<StoreInfo>,
    /// The audit copy, when the state had an audit log.
    pub audit: Option<AuditInfo>,
    /// The newest saved configuration version included.
    pub config_version: Option<u64>,
    /// Every file except the manifest, sorted by path.
    pub files: Vec<FileEntry>,
}

/// The store snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreInfo {
    /// Store schema version.
    pub schema_version: i64,
    /// Messages in the snapshot.
    pub messages: u64,
    /// Spilled body files copied with it.
    pub body_files: u32,
}

/// The audit copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditInfo {
    /// Audit files copied.
    pub files: u32,
    /// The newest event copied.
    pub head_seq: Option<u64>,
    /// Its hash.
    pub head_hash: Option<String>,
}

/// One file of the backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative to the backup directory, `/`-separated.
    pub path: String,
    /// Size.
    pub bytes: u64,
    /// SHA-256, lowercase hex.
    pub sha256: String,
}

impl Manifest {
    /// An empty manifest for a snapshot taken at `created_at`.
    pub(crate) fn new(created_at: Timestamp, actor: Actor, node_id: String) -> Self {
        Self {
            format: FORMAT,
            engine_version: crate::ENGINE_VERSION.to_owned(),
            created_at,
            actor,
            node_id,
            store: None,
            audit: None,
            config_version: None,
            files: Vec::new(),
        }
    }

    /// Reads `<dir>/manifest.json`.
    ///
    /// # Errors
    ///
    /// Fails when the file is missing, is not a manifest, or has an unsupported format.
    pub fn read(dir: &Path) -> Result<Self> {
        let path = dir.join(MANIFEST);
        let text = fs::read_to_string(&path).map_err(Error::io(&path))?;
        let manifest: Self = serde_json::from_str(&text)?;
        if manifest.format != FORMAT {
            return Err(Error::Invalid(format!(
                "manifest format {} is not supported by this version",
                manifest.format
            )));
        }
        Ok(manifest)
    }

    /// Writes `<dir>/manifest.json` through a synced temporary file and a rename.
    pub(crate) fn write(&self, dir: &Path) -> Result<()> {
        let path = dir.join(MANIFEST);
        let temporary = dir.join("manifest.json.tmp");
        let text = serde_json::to_vec_pretty(self)?;
        let mut file = private_create(&temporary)?;
        file.write_all(&text)
            .and_then(|()| file.sync_all())
            .map_err(Error::io(&temporary))?;
        fs::rename(&temporary, &path).map_err(Error::io(&path))?;
        fsutil::sync_path(dir)
    }

    /// The IPC summary.
    #[must_use]
    pub fn summary(&self, dir: &Path, duration_ms: u64) -> BackupSummary {
        BackupSummary {
            directory: dir.display().to_string(),
            created_at: self.created_at,
            files: u32::try_from(self.files.len()).unwrap_or(u32::MAX),
            bytes: self.files.iter().map(|f| f.bytes).sum(),
            messages: self.store.as_ref().map(|s| s.messages),
            config_version: self.config_version,
            audit_head_seq: self.audit.as_ref().and_then(|a| a.head_seq),
            duration_ms,
        }
    }
}

/// Every regular file under `dir` except the manifest, relative and `/`-separated, sorted.
pub(crate) fn walk(dir: &Path) -> Result<Vec<String>> {
    let mut found: Vec<String> = fsutil::tree(dir)?
        .into_iter()
        .filter(|(_, kind)| kind.is_file())
        .map(|(path, _)| fsutil::slashed(&path))
        .filter(|path| path != MANIFEST)
        .collect();
    found.sort();
    Ok(found)
}
