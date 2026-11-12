//! Folder and SFTP settings (`docs/spec/configuration.md` §5.4).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::defaults::{ms_30000, secs_30, secs_60, yes};
use crate::secret::SecretRef;

/// How a folder source finds new files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FolderMode {
    /// Change notifications plus a safety poll every `poll_interval_seconds`.
    #[default]
    Watch,
    /// Poll every `poll_interval_seconds` only (for shares where notifications are
    /// unreliable).
    Poll,
}

/// What happens to a source file after its content is committed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum OnConsumed {
    /// Move it to `processed_directory`.
    #[default]
    Move,
    /// Delete it.
    Delete,
    /// Leave it; duplicate detection prevents reprocessing.
    Leave,
}

/// How a destination file is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    /// Write a uniquely named `.<random>.part` file, then rename it to the final name.
    #[default]
    AtomicRename,
    /// Write the final name directly.
    Direct,
}

/// Folder settings (`docs/spec/configuration.md` §5.4). A source reads complete files
/// matching `file_pattern` (size and modified time unchanged for `stable_seconds`, no
/// ignored suffix, and openable exclusively when `exclusive_open_check`); a destination
/// writes per `write_mode`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct FolderSettings {
    /// The directory (a UNC path on Windows for shares).
    #[ts(as = "String")]
    pub path: PathBuf,
    /// Default `watch`.
    #[serde(default)]
    pub mode: FolderMode,
    /// Poll interval, and the safety poll in `watch` mode. Default 30.
    #[serde(default = "secs_30")]
    pub poll_interval_seconds: u32,
    /// Glob on the file name with `*` and `?`. Default `*`.
    #[serde(default = "star")]
    pub file_pattern: String,
    /// Include sub-directories (except `processed_directory`). Default `false`.
    #[serde(default)]
    pub recursive: bool,
    /// Files ending in any of these are never read.
    #[serde(default = "ignored_suffixes")]
    pub ignore_suffixes: Vec<String>,
    /// Seconds a file's size and modified time must stay unchanged. Default 2.
    #[serde(default = "two")]
    pub stable_seconds: u32,
    /// Also require that the file can be opened exclusively. Default `true`.
    #[serde(default = "yes")]
    pub exclusive_open_check: bool,
    /// Default `move`.
    #[serde(default)]
    pub on_consumed: OnConsumed,
    /// Relative to `path`; created when needed. Default `processed`.
    #[serde(default = "processed")]
    pub processed_directory: String,
    /// Default `atomic_rename`.
    #[serde(default)]
    pub write_mode: WriteMode,
}

/// SFTP settings (`docs/spec/configuration.md` §5.4). The server's host key is pinned by
/// its SHA-256 fingerprint; files are discovered by polling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SftpSettings {
    /// Host name or IP address.
    pub host: String,
    /// Default 22.
    #[serde(default = "port_22")]
    pub port: u16,
    /// Account name.
    pub username: String,
    /// How to authenticate.
    pub auth: SftpAuth,
    /// `SHA256:<base64>` as printed by `ssh-keygen -lf`.
    pub host_key_fingerprint: String,
    /// Absolute remote directory.
    pub remote_directory: String,
    /// 1–120000. Default 10000.
    #[serde(default = "ms_10000")]
    pub connect_timeout_ms: u32,
    /// One list, read or write operation. Default 30000.
    #[serde(default = "ms_30000")]
    pub operation_timeout_ms: u32,
    /// Default 60.
    #[serde(default = "secs_60")]
    pub poll_interval_seconds: u32,
    /// Glob on the file name with `*` and `?`. Default `*`.
    #[serde(default = "star")]
    pub file_pattern: String,
    /// Include sub-directories (except `processed_directory`). Default `false`.
    #[serde(default)]
    pub recursive: bool,
    /// Files ending in any of these are never read.
    #[serde(default = "ignored_suffixes")]
    pub ignore_suffixes: Vec<String>,
    /// Seconds size and modified time must stay unchanged, across two polls. Default 2.
    #[serde(default = "two")]
    pub stable_seconds: u32,
    /// Default `move`.
    #[serde(default)]
    pub on_consumed: OnConsumed,
    /// Relative to `remote_directory`. Default `processed`.
    #[serde(default = "processed")]
    pub processed_directory: String,
    /// Default `atomic_rename`.
    #[serde(default)]
    pub write_mode: WriteMode,
}

/// SFTP authentication; secrets are references, never values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum SftpAuth {
    /// Password from the secret store.
    Password {
        /// The password secret.
        password: SecretRef,
    },
    /// Private key (OpenSSH format) from the secret store.
    Key {
        /// The key secret.
        private_key: SecretRef,
        /// The key's passphrase secret, if it has one.
        #[serde(default)]
        passphrase: Option<SecretRef>,
    },
}

impl SftpAuth {
    /// The secret references, each with the name of the field that holds it.
    pub(crate) fn secret_refs(&self) -> Vec<(&'static str, &SecretRef)> {
        match self {
            Self::Password { password } => vec![("password", password)],
            Self::Key {
                private_key,
                passphrase,
            } => std::iter::once(("private_key", private_key))
                .chain(passphrase.iter().map(|p| ("passphrase", p)))
                .collect(),
        }
    }
}

const fn two() -> u32 {
    2
}
const fn port_22() -> u16 {
    22
}
const fn ms_10000() -> u32 {
    10_000
}
fn star() -> String {
    "*".to_owned()
}
fn processed() -> String {
    "processed".to_owned()
}
fn ignored_suffixes() -> Vec<String> {
    [".tmp", ".part", ".partial", ".crdownload"]
        .map(str::to_owned)
        .to_vec()
}
