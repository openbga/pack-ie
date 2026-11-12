//! Errors of the scenario format and of the simulator itself.

use std::path::PathBuf;

/// A scenario file is invalid. Reported before anything starts (exit code 2).
#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    /// The file could not be read.
    #[error("{path}: cannot read: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// The file is malformed or breaks a rule of the format.
    #[error("{path}: {message}")]
    Invalid {
        /// The file.
        path: PathBuf,
        /// What is wrong, with the location inside the file.
        message: String,
    },
}

impl ScenarioError {
    pub(crate) fn invalid(path: &std::path::Path, message: impl Into<String>) -> Self {
        Self::Invalid {
            path: path.to_owned(),
            message: message.into(),
        }
    }
}

/// A failure while executing a scenario (resources, actors, engine process).
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    /// An operating-system resource could not be allocated or used.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The cause.
        source: std::io::Error,
    },
    /// Anything else, already phrased for the report.
    #[error("{0}")]
    Other(String),
}

impl SimError {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub(crate) fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }
}
