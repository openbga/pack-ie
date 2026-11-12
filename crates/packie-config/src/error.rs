//! Errors loading and saving configuration documents.

use std::path::PathBuf;

use crate::validate::ValidationErrors;

/// A configuration document could not be read, written or accepted.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// The file or directory could not be written.
    #[error("cannot write {}: {source}", path.display())]
    Write {
        /// The file or directory.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// The document is malformed or invalid.
    #[error("{} is invalid: {errors}", path.display())]
    Invalid {
        /// The file.
        path: PathBuf,
        /// Every problem found.
        errors: ValidationErrors,
    },
    /// The document could not be serialized.
    #[error("cannot serialize configuration: {0}")]
    Serialize(#[from] serde_json::Error),
    /// The requested version is not in the history.
    #[error("configuration version {0} is not in the history")]
    UnknownVersion(u64),
}
