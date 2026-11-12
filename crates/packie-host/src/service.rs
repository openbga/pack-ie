//! The service contract shared by the OS back ends.

use std::path::PathBuf;

use crate::error::HostError;
use crate::runner::Runner;

/// The service (and unit, and launchd label stem) name.
pub(crate) const SERVICE_NAME: &str = "packie";
/// The name operators see in service managers.
pub(crate) const DISPLAY_NAME: &str = "Positive ACK";
/// Time beyond the drain for closing the store and the IPC endpoint.
pub(crate) const STOP_MARGIN_SECONDS: u32 = 30;
/// The longest stop: the largest `shutdown_drain_seconds` (300) plus the margin.
pub(crate) const MAX_STOP_SECONDS: u32 = 300 + STOP_MARGIN_SECONDS;
/// The group whose members may use the management socket (`ipc.group` default) on Unix.
pub const IPC_GROUP: &str = "packie";

/// Who the service runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// A system service started with the computer (needs administrator rights).
    System,
    /// A per-user agent started at login (macOS only; for development and evaluation).
    User,
}

/// What to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    /// Absolute path of the `packie` executable.
    pub executable: PathBuf,
    /// Absolute path of the bootstrap document.
    pub bootstrap: PathBuf,
    /// The engine's state directory (from the bootstrap).
    pub state_dir: PathBuf,
    /// The engine's log directory (from the bootstrap).
    pub log_dir: PathBuf,
    /// System service or user agent.
    pub scope: Scope,
    /// The user installing it (from `sudo`), added to the IPC group so their console can
    /// attach to the service.
    pub operator: Option<String>,
    /// The bootstrap's `hosting.shutdown_drain_seconds`; the service manager waits this
    /// long plus a 30 second margin for the engine to stop before killing it.
    pub drain_seconds: u32,
}

impl ServiceSpec {
    /// How long the service manager waits for the engine to stop.
    #[must_use]
    pub fn stop_timeout_seconds(&self) -> u32 {
        self.drain_seconds.saturating_add(STOP_MARGIN_SECONDS)
    }

    /// The arguments the service manager starts the engine with.
    #[must_use]
    pub fn run_arguments(&self) -> Vec<String> {
        vec![
            "run".to_owned(),
            "--bootstrap".to_owned(),
            self.bootstrap.to_string_lossy().into_owned(),
            "--service".to_owned(),
        ]
    }
}

/// What the service manager reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceStatus {
    /// The definition is installed.
    pub installed: bool,
    /// The engine process runs.
    pub running: bool,
    /// Its process id, when running.
    pub pid: Option<u32>,
    /// Where the definition lives (plist, unit file), when installed.
    pub definition: Option<PathBuf>,
    /// The bootstrap path in the installed definition.
    pub bootstrap: Option<PathBuf>,
}

/// One OS's service manager.
///
/// Every operation fails with a [`HostError`] whose message is for the operator: missing
/// administrator rights, a service that is not installed, or the failed OS command.
pub trait ServiceManager {
    /// The scope used when the operator does not choose one.
    fn default_scope(&self) -> Scope;

    /// Writes and registers the service definition, set to start with the computer
    /// (or at login for a user agent) and restart on failure, and starts it.
    ///
    /// # Errors
    /// Not elevated, or any step of the installation failed.
    fn install(&self, spec: &ServiceSpec) -> Result<(), HostError>;

    /// Stops and removes the service definition.
    ///
    /// # Errors
    /// Not elevated, not installed, or the engine did not stop.
    fn uninstall(&self, scope: Scope) -> Result<(), HostError>;

    /// Starts the installed service.
    ///
    /// # Errors
    /// Not elevated, not installed, or the service manager refused.
    fn start(&self, scope: Scope) -> Result<(), HostError>;

    /// Stops the service without uninstalling it.
    ///
    /// # Errors
    /// Not elevated, or the engine did not stop.
    fn stop(&self, scope: Scope) -> Result<(), HostError>;

    /// Whether it is installed and running.
    ///
    /// # Errors
    /// The service manager could not be queried.
    fn status(&self, scope: Scope) -> Result<ServiceStatus, HostError>;
}

/// The service manager of this OS, running commands through `runner`.
///
/// # Errors
/// [`HostError::Unsupported`] on an OS other than Windows, macOS and Linux.
pub fn platform(runner: Box<dyn Runner>) -> Result<Box<dyn ServiceManager>, HostError> {
    if cfg!(target_os = "macos") {
        Ok(Box::new(crate::launchd::Launchd::new(runner)))
    } else if cfg!(target_os = "linux") {
        Ok(Box::new(crate::systemd::Systemd::new(runner)))
    } else if cfg!(windows) {
        Ok(Box::new(crate::windows::WindowsServices::new(runner)))
    } else {
        Err(HostError::Unsupported(
            "service management is available on Windows, macOS and Linux".into(),
        ))
    }
}

/// Fails with the elevation hint when not elevated.
pub(crate) fn require_elevated(runner: &dyn Runner) -> Result<(), HostError> {
    if runner.is_elevated() {
        return Ok(());
    }
    let hint = if cfg!(windows) {
        "this needs administrator rights: run the terminal as administrator and try again"
    } else {
        "this needs root: run it again with sudo"
    };
    Err(HostError::NotElevated(hint.into()))
}

/// The value after `--bootstrap` in a list of arguments.
pub(crate) fn bootstrap_argument<'a>(mut args: impl Iterator<Item = &'a str>) -> Option<PathBuf> {
    args.find(|a| *a == "--bootstrap")?;
    args.next().map(PathBuf::from)
}

/// The value of `name` in the `name = value` (`launchctl print`) or `name=value`
/// (`systemctl show`) lines of service-manager output.
pub(crate) fn property(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(name)?.trim_start();
        Some(rest.strip_prefix('=')?.trim().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_are_read_with_or_without_spaces() {
        let output = "ActiveState=active\n\tstate = running\n\tpid = 77\npidfile = x\n";
        for (name, expected) in [
            ("ActiveState", Some("active")),
            ("state", Some("running")),
            ("pid", Some("77")),
            ("pi", None),
            ("missing", None),
        ] {
            assert_eq!(property(output, name).as_deref(), expected, "{name}");
        }
    }

    #[test]
    fn the_bootstrap_follows_its_flag() {
        for (args, expected) in [
            (
                &["run", "--bootstrap", "b.json", "--service"][..],
                Some("b.json"),
            ),
            (&["run", "--bootstrap"], None),
            (&["run"], None),
        ] {
            assert_eq!(
                bootstrap_argument(args.iter().copied()),
                expected.map(PathBuf::from)
            );
        }
    }

    #[test]
    fn the_stop_timeout_adds_the_margin_without_overflow() {
        let mut spec = crate::fake::spec("packie", "b.json");
        assert_eq!(spec.stop_timeout_seconds(), 45);
        spec.drain_seconds = u32::MAX;
        assert_eq!(spec.stop_timeout_seconds(), u32::MAX);
    }
}
