//! One engine per state directory.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use packie_core::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::HostError;

const LOCK_FILE: &str = "packie.lock";
/// Written beside the lock, because Windows blocks reading a locked file. Readable by the
/// state directory's group (mode 0640; the directory's set-group-id bit makes it the
/// operators' group), so `packie service status` can show it to them: it holds no secrets.
const INFO_FILE: &str = "packie.instance.json";

/// What the running engine wrote into its lock file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    /// Process id.
    pub pid: u32,
    /// When it started.
    pub started_at: Timestamp,
    /// Its bootstrap document.
    pub bootstrap: PathBuf,
}

/// An exclusive lock on `<state>/packie.lock`, held while the engine runs; the OS releases it
/// when the process ends, however it ends. `<state>/packie.instance.json` says who holds it.
///
/// The lock belongs to the open file (`flock` on Unix): a child process forked while it is
/// held shares it until the child execs (the descriptor is close-on-exec), so a holder that
/// spawns processes may keep it a moment past its drop. The engine spawns none on Unix.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock for this process, or fails with the pid of the engine holding it.
    ///
    /// # Errors
    /// [`HostError::AlreadyRunning`] when another engine holds the lock;
    /// [`HostError::Io`] when the lock or information file cannot be written.
    pub fn acquire(state_dir: &Path, bootstrap: &Path) -> Result<Self, HostError> {
        let path = state_dir.join(LOCK_FILE);
        let file = private_options()
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| HostError::io(&path, e))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let pid = read_info(state_dir).map_or(0, |info| info.pid);
                return Err(HostError::AlreadyRunning {
                    state_dir: state_dir.to_owned(),
                    pid,
                });
            }
            Err(TryLockError::Error(e)) => return Err(HostError::io(&path, e)),
        }
        let info = LockInfo {
            pid: std::process::id(),
            started_at: Timestamp::now(),
            bootstrap: bootstrap.to_owned(),
        };
        let info_path = state_dir.join(INFO_FILE);
        let text = serde_json::to_vec(&info).map_err(|e| HostError::io(&info_path, e.into()))?;
        write_group_readable(&info_path, &text).map_err(|e| HostError::io(&info_path, e))?;
        Ok(Self { _file: file })
    }

    /// What `<state>/packie.instance.json` says about the engine that last took the lock,
    /// without checking that it still holds it (for callers that cannot open the owner-only
    /// lock and know from elsewhere, such as the service manager, that the engine runs).
    #[must_use]
    pub fn last_info(state_dir: &Path) -> Option<LockInfo> {
        read_info(state_dir)
    }

    /// The running engine's lock information, or `None` when no engine holds the lock.
    #[must_use]
    pub fn holder(state_dir: &Path) -> Option<LockInfo> {
        let file = File::open(state_dir.join(LOCK_FILE)).ok()?;
        match file.try_lock_shared() {
            Err(TryLockError::WouldBlock) => read_info(state_dir),
            // Nobody holds it (whatever the info file says is stale), or it cannot be tried.
            Ok(()) | Err(TryLockError::Error(_)) => None,
        }
    }
}

/// Options that create the file owner-only (mode 0600) on Unix, like the rest of the state
/// directory.
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

/// Writes `bytes` to `path` with mode 0640 on Unix (owner writes, the directory's group
/// reads), also when an older file had other bits.
fn write_group_readable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = private_options().write(true).truncate(true).open(path)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o640))?;
    file.write_all(bytes)
}

fn read_info(state_dir: &Path) -> Option<LockInfo> {
    let text = std::fs::read_to_string(state_dir.join(INFO_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}
