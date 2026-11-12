//! A scripted [`Runner`] for the unit tests of the service back ends.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::error::HostError;
use crate::runner::{CommandOutput, Runner};
use crate::service::{Scope, ServiceSpec};

/// A system-scope spec for `executable` and `bootstrap`, installed by `operator`, with
/// state in `/var/lib/packie`, logs in `/var/log/packie` and a 15 second drain.
pub(crate) fn spec(executable: &str, bootstrap: &str) -> ServiceSpec {
    ServiceSpec {
        executable: executable.into(),
        bootstrap: bootstrap.into(),
        state_dir: "/var/lib/packie".into(),
        log_dir: "/var/log/packie".into(),
        scope: Scope::System,
        operator: Some("operator".into()),
        drain_seconds: 15,
    }
}

/// Records every command and file operation, answers commands from a queue of outputs
/// (success with empty output when the queue is empty), and keeps files in memory.
///
/// Clones share their state, so a test keeps a handle on a runner it hands to a back end.
#[derive(Debug, Clone)]
pub(crate) struct FakeRunner {
    elevated: bool,
    calls: Arc<Mutex<Vec<String>>>,
    outputs: Arc<Mutex<VecDeque<CommandOutput>>>,
    files: Arc<Mutex<BTreeMap<PathBuf, String>>>,
}

impl FakeRunner {
    /// A runner reporting `elevated`, with `/Users/test` as home.
    pub(crate) fn new(elevated: bool) -> Self {
        Self {
            elevated,
            calls: Arc::default(),
            outputs: Arc::default(),
            files: Arc::default(),
        }
    }

    /// Queues the output of the next command.
    pub(crate) fn push_output(&self, code: i32, stdout: &str, stderr: &str) {
        lock(&self.outputs).push_back(CommandOutput {
            code: Some(code),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        });
    }

    /// Commands run so far, as `program arg arg`.
    pub(crate) fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    /// The content of a written file.
    #[cfg(unix)]
    pub(crate) fn file(&self, path: &Path) -> Option<String> {
        lock(&self.files).get(path).cloned()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Runner for FakeRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, HostError> {
        lock(&self.calls).push(
            std::iter::once(program)
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" "),
        );
        Ok(lock(&self.outputs).pop_front().unwrap_or(CommandOutput {
            code: Some(0),
            ..CommandOutput::default()
        }))
    }

    fn is_elevated(&self) -> bool {
        self.elevated
    }

    fn home(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/Users/test"))
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        lock(&self.calls).push(format!("write {}", path.display()));
        lock(&self.files).insert(path.to_owned(), contents.to_owned());
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        lock(&self.calls).push(format!("remove {}", path.display()));
        lock(&self.files).remove(path);
        Ok(())
    }

    fn read_file(&self, path: &Path) -> Result<Option<String>, HostError> {
        Ok(lock(&self.files).get(path).cloned())
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HostError> {
        lock(&self.calls).push(format!("mkdir {}", path.display()));
        Ok(())
    }

    fn pause(&self, _duration: std::time::Duration) {}
}
