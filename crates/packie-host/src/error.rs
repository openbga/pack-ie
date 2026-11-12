//! Errors of service management.

use std::path::PathBuf;

/// A service operation failed. Messages are for the operator.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The operation needs administrator rights.
    #[error("{0}")]
    NotElevated(String),
    /// Another engine already runs on this state directory.
    #[error("another engine is already running on {} (pid {pid})", state_dir.display())]
    AlreadyRunning {
        /// The state directory.
        state_dir: PathBuf,
        /// Its process id, when known.
        pid: u32,
    },
    /// The service is not installed.
    #[error("the packie service is not installed; install it with `packie service install`")]
    NotInstalled,
    /// An OS command failed.
    #[error("{program} failed: {message}")]
    Command {
        /// The command.
        program: String,
        /// What it reported.
        message: String,
    },
    /// A file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// This OS has no service support in this build.
    #[error("{0}")]
    Unsupported(String),
}

impl HostError {
    /// An I/O error at `path`.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// A failed OS command (or other service-manager call) named `program`.
    pub(crate) fn command(program: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Command {
            program: program.into(),
            message: message.into(),
        }
    }
}
