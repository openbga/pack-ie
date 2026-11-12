//! Linux: a systemd system unit running as the dedicated `packie` user.

use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::runner::Runner;
use crate::service::{
    DISPLAY_NAME, IPC_GROUP, SERVICE_NAME, Scope, ServiceManager, ServiceSpec, ServiceStatus,
    bootstrap_argument, property, require_elevated,
};

/// Where the unit is installed.
pub(crate) const UNIT_PATH: &str = "/etc/systemd/system/packie.service";
/// The service account and group; members of the group may use the IPC socket.
pub(crate) const ACCOUNT: &str = IPC_GROUP;
/// The group that owns serial devices on Debian-family systems.
pub(crate) const SERIAL_GROUP: &str = "dialout";

/// The systemd back end.
pub(crate) struct Systemd {
    runner: Box<dyn Runner>,
}

/// Quotes one `ExecStart` word when it needs it.
fn quote(word: &str) -> String {
    if word.contains([' ', '"', '\\', '\'']) {
        format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        word.to_owned()
    }
}

/// The unit: a simple service restarted on failure, running as `packie` with the serial group
/// (when it exists) as a supplementary group, its state and logs writable and the rest of
/// the system read-only. Device access is not restricted, so serial ports keep working.
#[must_use]
pub(crate) fn unit(spec: &ServiceSpec, serial_group: Option<&str>) -> String {
    let exec: Vec<String> = std::iter::once(spec.executable.to_string_lossy().into_owned())
        .chain(spec.run_arguments())
        .map(|w| quote(&w))
        .collect();
    let groups = serial_group.map_or_else(String::new, |g| format!("SupplementaryGroups={g}\n"));
    let writable = format!(
        "{} {}",
        quote(&spec.state_dir.to_string_lossy()),
        quote(&spec.log_dir.to_string_lossy())
    );
    format!(
        "[Unit]
Description={DISPLAY_NAME}
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={ACCOUNT}
Group={ACCOUNT}
{groups}ExecStart={exec}
Restart=on-failure
RestartSec=5
RestartPreventExitStatus=2
TimeoutStopSec={stop_timeout}
StateDirectory={SERVICE_NAME}
StateDirectoryMode=2750
LogsDirectory={SERVICE_NAME}
LogsDirectoryMode=0750
UMask=0007
ReadWritePaths={writable}
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
LockPersonality=yes

[Install]
WantedBy=multi-user.target
",
        exec = exec.join(" "),
        stop_timeout = spec.stop_timeout_seconds(),
    )
}

/// The words of `ExecStart` in a unit written by [`unit()`].
#[must_use]
pub(crate) fn exec_words(unit: &str) -> Vec<String> {
    let Some(line) = unit.lines().find_map(|l| l.strip_prefix("ExecStart=")) else {
        return Vec::new();
    };
    let mut words = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut escaped, mut started) = (false, false, false);
    for c in line.chars() {
        match (escaped, quoted, c) {
            (true, _, c) => {
                current.push(c);
                escaped = false;
            }
            (false, true, '\\') => escaped = true,
            (false, _, '"') => {
                quoted = !quoted;
                started = true;
            }
            (false, false, ' ') => {
                if started || !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                started = false;
            }
            (false, _, c) => current.push(c),
        }
    }
    if started || !current.is_empty() {
        words.push(current);
    }
    words
}

impl Systemd {
    /// systemd through `runner`.
    #[must_use]
    pub(crate) fn new(runner: Box<dyn Runner>) -> Self {
        Self { runner }
    }

    fn exists(&self, database: &str, name: &str) -> Result<bool, HostError> {
        Ok(self.runner.run("getent", &[database, name])?.success())
    }

    /// Creates the `packie` group and user when missing.
    fn ensure_account(&self, state_dir: &Path) -> Result<(), HostError> {
        if !self.exists("group", ACCOUNT)? {
            self.runner.run_ok("groupadd", &["--system", ACCOUNT])?;
        }
        if !self.exists("passwd", ACCOUNT)? {
            let home = state_dir.to_string_lossy();
            self.runner.run_ok(
                "useradd",
                &[
                    "--system",
                    "--gid",
                    ACCOUNT,
                    "--home-dir",
                    &home,
                    "--no-create-home",
                    "--shell",
                    "/usr/sbin/nologin",
                    ACCOUNT,
                ],
            )?;
        }
        Ok(())
    }
}

impl ServiceManager for Systemd {
    fn default_scope(&self) -> Scope {
        Scope::System
    }

    fn install(&self, spec: &ServiceSpec) -> Result<(), HostError> {
        require_elevated(self.runner.as_ref())?;
        self.ensure_account(&spec.state_dir)?;
        if let Some(operator) = &spec.operator {
            // The installing user may then use the console against the service.
            self.runner
                .run_ok("usermod", &["--append", "--groups", ACCOUNT, operator])?;
        }
        for dir in [&spec.state_dir, &spec.log_dir] {
            self.runner.create_dir_all(dir)?;
            let dir = dir.to_string_lossy();
            self.runner
                .run_ok("chown", &["-R", &format!("{ACCOUNT}:{ACCOUNT}"), &dir])?;
            // Group members may reach the socket, not read the files (they are 0600).
            self.runner.run_ok("chmod", &["2750", &dir])?;
        }
        let serial = self.exists("group", SERIAL_GROUP)?.then_some(SERIAL_GROUP);
        self.runner
            .write_file(Path::new(UNIT_PATH), &unit(spec, serial))?;
        self.runner.run_ok("systemctl", &["daemon-reload"])?;
        self.runner
            .run_ok("systemctl", &["enable", "--now", SERVICE_NAME])
    }

    fn uninstall(&self, _scope: Scope) -> Result<(), HostError> {
        require_elevated(self.runner.as_ref())?;
        if self.runner.read_file(Path::new(UNIT_PATH))?.is_none() {
            return Err(HostError::NotInstalled);
        }
        self.runner
            .run_ok("systemctl", &["disable", "--now", SERVICE_NAME])?;
        self.runner.remove_file(Path::new(UNIT_PATH))?;
        self.runner.run_ok("systemctl", &["daemon-reload"])
    }

    fn start(&self, _scope: Scope) -> Result<(), HostError> {
        require_elevated(self.runner.as_ref())?;
        self.runner.run_ok("systemctl", &["start", SERVICE_NAME])
    }

    fn stop(&self, _scope: Scope) -> Result<(), HostError> {
        require_elevated(self.runner.as_ref())?;
        self.runner.run_ok("systemctl", &["stop", SERVICE_NAME])
    }

    fn status(&self, _scope: Scope) -> Result<ServiceStatus, HostError> {
        let Some(definition) = self.runner.read_file(Path::new(UNIT_PATH))? else {
            return Ok(ServiceStatus::default());
        };
        let show = self.runner.run(
            "systemctl",
            &["show", SERVICE_NAME, "--property=ActiveState,MainPID"],
        )?;
        let field = |name: &str| property(&show.stdout, name);
        let words = exec_words(&definition);
        Ok(ServiceStatus {
            installed: true,
            running: field("ActiveState").as_deref() == Some("active"),
            pid: field("MainPID")
                .and_then(|p| p.parse().ok())
                .filter(|p| *p > 0),
            definition: Some(PathBuf::from(UNIT_PATH)),
            bootstrap: bootstrap_argument(words.iter().map(String::as_str)),
        })
    }
}

// launchd and systemd exist on Unix only: these tests check Unix paths and the `sudo` hint.
#[cfg(all(test, unix))]
mod tests {

    use super::*;
    use crate::fake::{self, FakeRunner};

    fn spec() -> ServiceSpec {
        fake::spec("/usr/bin/packie", "/etc/packie/my bootstrap.json")
    }

    #[test]
    fn unit_runs_as_the_service_account_and_keeps_serial_access() {
        let text = unit(&spec(), Some(SERIAL_GROUP));
        for line in [
            "Type=simple",
            "Restart=on-failure",
            // Exit code 2 is a configuration the operator must fix: no restart loop.
            "RestartPreventExitStatus=2",
            "User=packie",
            "Group=packie",
            "SupplementaryGroups=dialout",
            "StateDirectory=packie",
            "LogsDirectory=packie",
            "ReadWritePaths=/var/lib/packie /var/log/packie",
            "TimeoutStopSec=45",
            "WantedBy=multi-user.target",
        ] {
            assert!(text.lines().any(|l| l == line), "missing {line}:\n{text}");
        }
        // Device access stays open for serial ports.
        assert!(!text.contains("PrivateDevices"));
        assert!(!text.contains("DevicePolicy"));
        assert_eq!(
            exec_words(&text),
            [
                "/usr/bin/packie",
                "run",
                "--bootstrap",
                "/etc/packie/my bootstrap.json",
                "--service"
            ]
        );
        assert!(!unit(&spec(), None).contains("SupplementaryGroups"));
    }

    #[test]
    fn install_needs_root() {
        let systemd = Systemd::new(Box::new(FakeRunner::new(false)));
        let error = systemd.install(&spec()).unwrap_err();
        assert!(error.to_string().contains("sudo"), "{error}");
    }

    #[test]
    fn install_creates_the_account_and_enables_the_unit() {
        let runner = FakeRunner::new(true);
        runner.push_output(2, "", ""); // getent group packie
        runner.push_output(0, "", ""); // groupadd
        runner.push_output(2, "", ""); // getent passwd packie
        let systemd = Systemd::new(Box::new(runner.clone()));
        systemd.install(&spec()).unwrap();
        let calls = runner.calls();
        for expected in [
            "groupadd --system packie",
            "usermod --append --groups packie operator",
            "chown -R packie:packie /var/lib/packie",
            "write /etc/systemd/system/packie.service",
            "systemctl daemon-reload",
            "systemctl enable --now packie",
        ] {
            assert!(calls.iter().any(|c| c == expected), "{expected}: {calls:?}");
        }
        assert!(calls.iter().any(|c| c.starts_with("useradd --system")));
        let unit = runner.file(Path::new(UNIT_PATH)).unwrap();
        assert!(unit.contains("SupplementaryGroups=dialout"));
    }

    #[test]
    fn status_reads_systemctl_show() {
        let runner = FakeRunner::new(false);
        let systemd = Systemd::new(Box::new(runner.clone()));
        assert_eq!(
            systemd.status(Scope::System).unwrap(),
            ServiceStatus::default()
        );
        runner
            .write_file(Path::new(UNIT_PATH), &unit(&spec(), None))
            .unwrap();
        runner.push_output(0, "ActiveState=active\nMainPID=4242\n", "");
        let status = systemd.status(Scope::System).unwrap();
        assert!(status.installed && status.running);
        assert_eq!(status.pid, Some(4242));
        assert_eq!(
            status.bootstrap.as_deref(),
            Some(Path::new("/etc/packie/my bootstrap.json"))
        );
    }

    #[test]
    fn uninstall_without_unit_is_not_installed() {
        let systemd = Systemd::new(Box::new(FakeRunner::new(true)));
        assert!(matches!(
            systemd.uninstall(Scope::System),
            Err(HostError::NotInstalled)
        ));
    }
}
