//! OS commands and environment queries, behind a trait so tests can fake them.

use std::path::{Path, PathBuf};

use crate::error::HostError;

/// What a command printed and how it ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit code; `None` when killed by a signal.
    pub code: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

impl CommandOutput {
    /// Whether it exited with 0.
    #[must_use]
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runs OS commands and answers questions about the environment.
pub trait Runner: Send + Sync {
    /// Runs `program` with `args` and waits for it.
    ///
    /// # Errors
    /// The command could not be started. A non-zero exit is reported in the output.
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, HostError>;

    /// Whether the process runs as root / administrator.
    fn is_elevated(&self) -> bool;

    /// The current user's home directory.
    fn home(&self) -> Option<PathBuf>;

    /// Writes a file (creating parent directories).
    ///
    /// # Errors
    /// The directory or file could not be written.
    fn write_file(&self, path: &Path, contents: &str) -> Result<(), HostError>;

    /// Removes a file; a missing file is not an error.
    ///
    /// # Errors
    /// The file exists and could not be removed.
    fn remove_file(&self, path: &Path) -> Result<(), HostError>;

    /// Reads a file, or `None` when it does not exist.
    ///
    /// # Errors
    /// The file exists and could not be read.
    fn read_file(&self, path: &Path) -> Result<Option<String>, HostError>;

    /// Creates a directory and its parents.
    ///
    /// # Errors
    /// The directory could not be created.
    fn create_dir_all(&self, path: &Path) -> Result<(), HostError>;

    /// Waits `duration` between polls of the service manager.
    fn pause(&self, duration: std::time::Duration) {
        std::thread::sleep(duration);
    }

    /// Runs `program` and returns its trimmed standard output.
    ///
    /// # Errors
    /// The command could not be started, or exited non-zero ([`HostError::Command`] with
    /// its standard error, or its standard output when that is empty).
    fn run_stdout(&self, program: &str, args: &[&str]) -> Result<String, HostError> {
        let output = self.run(program, args)?;
        if output.success() {
            return Ok(output.stdout.trim().to_owned());
        }
        let message = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        Err(HostError::command(
            format!("{program} {}", args.join(" ")),
            message,
        ))
    }

    /// Runs `program` and fails with its output when it exits non-zero.
    ///
    /// # Errors
    /// As [`run_stdout`](Self::run_stdout).
    fn run_ok(&self, program: &str, args: &[&str]) -> Result<(), HostError> {
        self.run_stdout(program, args).map(drop)
    }
}

/// The real OS.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, HostError> {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|source| HostError::io(program, source))?;
        Ok(CommandOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn is_elevated(&self) -> bool {
        #[cfg(unix)]
        {
            self.run("id", &["-u"])
                .is_ok_and(|o| o.stdout.trim() == "0")
        }
        #[cfg(windows)]
        {
            // `net session` succeeds only in an elevated process.
            self.run("net", &["session"]).is_ok_and(|o| o.success())
        }
        #[cfg(not(any(unix, windows)))]
        {
            false
        }
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| HostError::io(parent, e))?;
        }
        std::fs::write(path, contents).map_err(|e| HostError::io(path, e))
    }

    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(HostError::io(path, e)),
        }
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HostError> {
        std::fs::create_dir_all(path).map_err(|e| HostError::io(path, e))
    }

    fn read_file(&self, path: &Path) -> Result<Option<String>, HostError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(HostError::io(path, e)),
        }
    }
}
