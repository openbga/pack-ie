//! `packie service ...`: installs and controls the engine as an OS service through
//! `packie-host`, and `packie run --service`, the entry point the service manager starts.

use std::path::{Path, PathBuf};

use packie_config::Bootstrap;
use packie_host::{InstanceLock, Scope, ServiceManager, ServiceSpec, ServiceStatus, SystemRunner};

use crate::Failure;
use crate::output::print_line;

/// One `packie service` subcommand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
}

/// Runs `action` against this OS's service manager. `user` selects the macOS user agent;
/// otherwise the scope follows elevation (system service as root or administrator).
pub(crate) fn service(action: Action, bootstrap: &Path, user: bool) -> Result<(), Failure> {
    if user && !cfg!(target_os = "macos") {
        return Err(Failure::new(
            "--user is available on macOS only; install the system service instead",
        ));
    }
    let manager = packie_host::platform(Box::new(SystemRunner)).map_err(host)?;
    let scope = if user {
        Scope::User
    } else {
        manager.default_scope()
    };
    match action {
        Action::Install => install(manager.as_ref(), bootstrap, scope),
        Action::Uninstall => {
            manager.uninstall(scope).map_err(host)?;
            print_line("Service removed. State and logs are kept.");
            Ok(())
        }
        Action::Start => {
            manager.start(scope).map_err(host)?;
            print_line("Service started. `packie service status` shows whether it runs.");
            Ok(())
        }
        Action::Stop => {
            manager.stop(scope).map_err(host)?;
            print_line(match scope {
                Scope::System => "Service stopped. It starts again with the computer.",
                Scope::User => "Service stopped. It starts again at your next login.",
            });
            Ok(())
        }
        Action::Status => status(manager.as_ref(), bootstrap, scope, user),
    }
}

fn host(error: packie_host::HostError) -> Failure {
    Failure::new(error.to_string())
}

fn install(manager: &dyn ServiceManager, bootstrap: &Path, scope: Scope) -> Result<(), Failure> {
    let spec = spec(bootstrap, scope)?;
    manager.install(&spec).map_err(host)?;
    print_line(&format!(
        "Service installed and started ({}). Bootstrap {}; logs in {}.",
        scope_name(scope),
        spec.bootstrap.display(),
        spec.log_dir.display()
    ));
    if let Some(operator) = &spec.operator {
        print_line(&format!(
            "{operator} was added to the `{}` group; sign out and in again to use the \
             console with the service.",
            packie_host::IPC_GROUP
        ));
    } else if cfg!(windows) {
        print_line(
            "The installing account was added to the local group `PACK-IE Operators`; add every \
             operator with `net localgroup \"PACK-IE Operators\" <user> /add`, then sign out and \
             in again.",
        );
    } else if let Some(command) = add_member_command(scope) {
        print_line(&format!(
            "No user was added to the `{group}` group (no sudo user is known). To use the \
             console or `packie` with the service as <user>, run `{command}` and sign in again.",
            group = packie_host::IPC_GROUP
        ));
    }
    Ok(())
}

/// The service definition for `bootstrap`, with absolute paths: the service manager does
/// not start the engine in the current directory.
fn spec(bootstrap: &Path, scope: Scope) -> Result<ServiceSpec, Failure> {
    let bootstrap = std::fs::canonicalize(bootstrap).map_err(|error| {
        Failure::new(format!(
            "bootstrap file {}: {error}; pass --bootstrap with the file the service should use",
            bootstrap.display()
        ))
    })?;
    let loaded = Bootstrap::load(&bootstrap).map_err(|error| Failure::new(error.to_string()))?;
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|error| Failure::new(format!("cannot find the packie executable: {error}")))?;
    // The user running `sudo packie service install`, so their console can attach.
    let operator = (scope == Scope::System)
        .then(|| std::env::var("SUDO_USER").ok())
        .flatten()
        .filter(|name| !name.is_empty() && name != "root");
    Ok(ServiceSpec {
        executable,
        state_dir: loaded.state_dir().to_owned(),
        log_dir: loaded.log_dir(),
        drain_seconds: loaded.hosting.shutdown_drain_seconds,
        bootstrap,
        scope,
        operator,
    })
}

/// How to add a user to the IPC group on this OS, for a system service on Unix.
fn add_member_command(scope: Scope) -> Option<String> {
    let group = packie_host::IPC_GROUP;
    match scope {
        Scope::System if cfg!(target_os = "linux") => {
            Some(format!("sudo usermod -aG {group} <user>"))
        }
        Scope::System if cfg!(target_os = "macos") => Some(format!(
            "sudo dseditgroup -o edit -a <user> -t user {group}"
        )),
        Scope::System | Scope::User => None,
    }
}

fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::System => "system service",
        Scope::User => "user agent",
    }
}

/// `service status`. Without `--user` and without elevation macOS defaults to the user
/// agent; when none is installed but the system service is (the packaged daemon), that is
/// what is reported, as far as it can be read without administrator rights.
fn status(
    manager: &dyn ServiceManager,
    bootstrap: &Path,
    scope: Scope,
    user: bool,
) -> Result<(), Failure> {
    for line in status_lines(manager, bootstrap, scope, user)? {
        print_line(&line);
    }
    Ok(())
}

fn status_lines(
    manager: &dyn ServiceManager,
    bootstrap: &Path,
    scope: Scope,
    user: bool,
) -> Result<Vec<String>, Failure> {
    let status = manager.status(scope).map_err(host)?;
    if status.installed {
        return Ok(report(status, bootstrap, scope, false));
    }
    if scope == Scope::User && !user {
        let system = manager.status(Scope::System).map_err(host)?;
        if system.installed {
            return Ok(report(system, bootstrap, Scope::System, true));
        }
        return Ok(vec![
            "Not installed: neither the system service nor a user agent. `sudo packie service \
             install` installs the system service, `packie service install --user` a user agent."
                .to_owned(),
        ]);
    }
    Ok(vec![format!(
        "Not installed ({}). `packie service install` installs it.",
        scope_name(scope)
    )])
}

/// The lines describing an installed service. `unelevated` is a system service read
/// without administrator rights: when the service manager does not say it runs, the
/// instance file (readable by the operators' group) and the life of the process it names do.
fn report(
    mut status: ServiceStatus,
    bootstrap: &Path,
    scope: Scope,
    unelevated: bool,
) -> Vec<String> {
    // The installed definition names the bootstrap the service really uses.
    let bootstrap = status
        .bootstrap
        .clone()
        .unwrap_or_else(|| bootstrap.to_owned());
    let loaded = Bootstrap::load(&bootstrap).ok();
    if unelevated && !status.running {
        let recorded = loaded
            .as_ref()
            .and_then(|b| InstanceLock::last_info(b.state_dir()))
            .filter(|info| process_alive(info.pid));
        if let Some(info) = recorded {
            status.running = true;
            status.pid = Some(info.pid);
        }
    }
    let state = if status.running { "running" } else { "stopped" };
    // The lock outlives the service manager's view while an engine drains; report it only
    // for a running service. Operators in the service's group cannot open the owner-only
    // lock, so for them the instance file stands in when it names the running process.
    let holder = loaded.as_ref().filter(|_| status.running).and_then(|b| {
        InstanceLock::holder(b.state_dir()).or_else(|| {
            InstanceLock::last_info(b.state_dir())
                .filter(|info| status.pid.is_none_or(|pid| pid == info.pid))
        })
    });
    let pid = status.pid.or(holder.as_ref().map(|h| h.pid));
    let rows = [
        ("Service", format!("{state} ({})", scope_name(scope))),
        ("PID", pid.map_or_else(|| "-".to_owned(), |p| p.to_string())),
        (
            "Since",
            holder.map_or_else(|| "-".to_owned(), |h| h.started_at.to_string()),
        ),
        ("Bootstrap", bootstrap.display().to_string()),
        (
            "IPC",
            loaded.map_or_else(
                || "unknown (bootstrap unreadable)".to_owned(),
                |b| b.ipc_endpoint(),
            ),
        ),
        (
            "Definition",
            status
                .definition
                .map_or_else(|| "-".to_owned(), |d| d.display().to_string()),
        ),
    ];
    let mut lines: Vec<String> = rows
        .iter()
        .map(|(name, value)| format!("{name:<11}{value}"))
        .collect();
    if unelevated {
        lines.push(format!(
            "Read without administrator rights; `sudo packie --bootstrap \"{}\" service status` \
             shows the full status.",
            bootstrap.display()
        ));
    }
    lines
}

/// Whether process `pid` exists, also when it belongs to another account (the signal
/// check is refused then, which still proves it exists).
fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::errno::Errno;
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        let Ok(raw) = i32::try_from(pid) else {
            return false;
        };
        matches!(kill(Pid::from_raw(raw), None), Ok(()) | Err(Errno::EPERM))
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Holds the single-instance lock for `bootstrap`'s state directory while the engine runs.
/// An unreadable bootstrap is left to the engine, which reports it.
pub(crate) fn instance_lock(bootstrap: &Path) -> Result<Option<InstanceLock>, Failure> {
    let Ok(loaded) = Bootstrap::load(bootstrap) else {
        return Ok(None);
    };
    let state_dir = loaded.state_dir();
    packie_config::create_private_dir(state_dir).map_err(|e| Failure::new(e.to_string()))?;
    let absolute = std::path::absolute(bootstrap).unwrap_or_else(|_| PathBuf::from(bootstrap));
    InstanceLock::acquire(state_dir, &absolute)
        .map(Some)
        .map_err(host)
}

/// `packie run --service` on Windows: runs the engine under the service control manager.
#[cfg(windows)]
pub(crate) fn run_windows_service(bootstrap: PathBuf) -> Result<(), Failure> {
    use packie_host::windows::ServiceError;
    let body: packie_host::windows::ServiceBody = Box::new(move |stop| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                ServiceError::Failed(format!("cannot start the async runtime: {error}"))
            })?;
        // An exit-code-2 failure (invalid bootstrap, unusable secret store) is the
        // operator's to fix: the SCM must not restart the service into it.
        runtime
            .block_on(crate::run(&bootstrap, true, stop.stopped()))
            .map_err(|failure| match failure.code {
                2 => ServiceError::Configuration(failure.message),
                _ => ServiceError::Failed(failure.message),
            })
    });
    packie_host::windows::run_service(body).map_err(host)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use packie_host::{HostError, Scope, ServiceManager, ServiceSpec, ServiceStatus};

    use super::status_lines;

    /// A service manager with a fixed status per scope.
    struct Fixed {
        system: ServiceStatus,
        user: ServiceStatus,
    }

    impl ServiceManager for Fixed {
        fn default_scope(&self) -> Scope {
            Scope::User
        }
        fn install(&self, _spec: &ServiceSpec) -> Result<(), HostError> {
            Ok(())
        }
        fn uninstall(&self, _scope: Scope) -> Result<(), HostError> {
            Ok(())
        }
        fn start(&self, _scope: Scope) -> Result<(), HostError> {
            Ok(())
        }
        fn stop(&self, _scope: Scope) -> Result<(), HostError> {
            Ok(())
        }
        fn status(&self, scope: Scope) -> Result<ServiceStatus, HostError> {
            Ok(match scope {
                Scope::System => self.system.clone(),
                Scope::User => self.user.clone(),
            })
        }
    }

    fn daemon(running: bool) -> ServiceStatus {
        ServiceStatus {
            installed: true,
            running,
            pid: running.then_some(4242),
            definition: Some("/Library/LaunchDaemons/org.openbga.positiveack.plist".into()),
            bootstrap: Some("/nonexistent/bootstrap.json".into()),
        }
    }

    /// Without elevation and without a user agent, the installed system daemon is reported
    /// with a pointer to the full, elevated status; a user agent wins when there is one.
    #[test]
    fn unelevated_status_reports_the_installed_system_service() {
        let bootstrap = Path::new("/nonexistent/bootstrap.json");
        let only_daemon = Fixed {
            system: daemon(true),
            user: ServiceStatus::default(),
        };
        let lines = status_lines(&only_daemon, bootstrap, Scope::User, false).unwrap();
        assert_eq!(lines[0], "Service    running (system service)");
        assert_eq!(lines[1], "PID        4242");
        assert!(
            lines.last().unwrap().contains("sudo packie --bootstrap"),
            "{lines:?}"
        );

        let with_agent = Fixed {
            system: daemon(true),
            user: daemon(false),
        };
        let lines = status_lines(&with_agent, bootstrap, Scope::User, false).unwrap();
        assert_eq!(lines[0], "Service    stopped (user agent)");
        assert!(!lines.iter().any(|l| l.contains("sudo")), "{lines:?}");

        // `--user` asks for the user agent only.
        let lines = status_lines(&only_daemon, bootstrap, Scope::User, true).unwrap();
        assert_eq!(
            lines,
            ["Not installed (user agent). `packie service install` installs it."]
        );

        let nothing = Fixed {
            system: ServiceStatus::default(),
            user: ServiceStatus::default(),
        };
        let lines = status_lines(&nothing, bootstrap, Scope::User, false).unwrap();
        assert!(
            lines[0].starts_with("Not installed: neither the system service"),
            "{lines:?}"
        );
    }
}
