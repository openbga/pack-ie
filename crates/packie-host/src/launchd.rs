//! macOS: a launchd system daemon (`/Library/LaunchDaemons`, needs root) or, when not
//! root, a user agent (`~/Library/LaunchAgents`) for development and evaluation.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use crate::error::HostError;
use crate::runner::Runner;
use crate::service::{
    DISPLAY_NAME, IPC_GROUP, MAX_STOP_SECONDS, Scope, ServiceManager, ServiceSpec, ServiceStatus,
    bootstrap_argument, property, require_elevated,
};

/// The launchd label.
pub(crate) const LABEL: &str = "org.openbga.positiveack";

/// The launchd back end.
pub(crate) struct Launchd {
    runner: Box<dyn Runner>,
}

/// Escapes text for an XML string element.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// The property list of the service: runs at load (boot or login), restarts when the
/// engine fails (not after a clean stop), works in the state directory and sends stdout
/// and stderr to the log directory (the engine writes its own log files there too).
#[must_use]
pub(crate) fn plist(spec: &ServiceSpec) -> String {
    let mut arguments = String::new();
    for argument in
        std::iter::once(spec.executable.to_string_lossy().into_owned()).chain(spec.run_arguments())
    {
        let _ = writeln!(arguments, "        <string>{}</string>", escape(&argument));
    }
    let log = |name: &str| escape(&spec.log_dir.join(name).to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{arguments}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ExitTimeOut</key>
    <integer>{exit_timeout}</integer>
    <key>WorkingDirectory</key>
    <string>{state}</string>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
</dict>
</plist>
"#,
        exit_timeout = spec.stop_timeout_seconds(),
        state = escape(&spec.state_dir.to_string_lossy()),
        stdout = log("launchd.stdout.log"),
        stderr = log("launchd.stderr.log"),
    )
}

/// The `ProgramArguments` of a plist written by [`plist`].
#[must_use]
pub(crate) fn program_arguments(plist: &str) -> Vec<String> {
    let Some(start) = plist.find("<key>ProgramArguments</key>") else {
        return Vec::new();
    };
    let rest = &plist[start..];
    let end = rest.find("</array>").unwrap_or(rest.len());
    rest[..end]
        .split("<string>")
        .skip(1)
        .filter_map(|part| {
            part.split_once("</string>")
                .map(|(value, _)| unescape(value))
        })
        .collect()
}

/// The launchd target of the service in `domain`: `<domain>/<label>`.
fn job(domain: &str) -> String {
    format!("{domain}/{LABEL}")
}

impl Launchd {
    /// launchd through `runner`.
    #[must_use]
    pub(crate) fn new(runner: Box<dyn Runner>) -> Self {
        Self { runner }
    }

    fn plist_path(&self, scope: Scope) -> Result<PathBuf, HostError> {
        let file = format!("{LABEL}.plist");
        match scope {
            Scope::System => Ok(PathBuf::from("/Library/LaunchDaemons").join(file)),
            Scope::User => self
                .runner
                .home()
                .map(|home| home.join("Library/LaunchAgents").join(file))
                .ok_or_else(|| HostError::Unsupported("HOME is not set".into())),
        }
    }

    /// The launchd domain: `system` or `gui/<uid>`.
    fn domain(&self, scope: Scope) -> Result<String, HostError> {
        match scope {
            Scope::System => Ok("system".to_owned()),
            Scope::User => Ok(format!("gui/{}", self.runner.run_stdout("id", &["-u"])?)),
        }
    }

    /// The job of the service in `scope`.
    fn target(&self, scope: Scope) -> Result<String, HostError> {
        Ok(job(&self.domain(scope)?))
    }

    fn check_scope(&self, scope: Scope) -> Result<(), HostError> {
        match scope {
            Scope::System => require_elevated(self.runner.as_ref()),
            Scope::User => Ok(()),
        }
    }

    /// Creates the IPC group, adds the installing user to it and lets the group reach the
    /// socket in the state directory (the files in it stay owner-only).
    fn share_with_group(&self, spec: &ServiceSpec) -> Result<(), HostError> {
        if !self
            .runner
            .run("dseditgroup", &["-o", "read", IPC_GROUP])?
            .success()
        {
            self.runner.run_ok(
                "dseditgroup",
                &["-o", "create", "-r", DISPLAY_NAME, IPC_GROUP],
            )?;
        }
        if let Some(operator) = &spec.operator {
            self.runner.run_ok(
                "dseditgroup",
                &["-o", "edit", "-a", operator, "-t", "user", IPC_GROUP],
            )?;
        }
        for dir in [&spec.state_dir, &spec.log_dir] {
            let dir = dir.to_string_lossy();
            self.runner
                .run_ok("chown", &[&format!("root:{IPC_GROUP}"), &dir])?;
            self.runner.run_ok("chmod", &["0750", &dir])?;
        }
        Ok(())
    }

    fn loaded(&self, target: &str) -> Result<bool, HostError> {
        Ok(self.runner.run("launchctl", &["print", target])?.success())
    }

    /// Unloads the job and waits until launchd has removed it: `bootout` returns while the
    /// engine may still be draining, and loading again before it is gone fails quietly.
    fn bootout(&self, target: &str) -> Result<(), HostError> {
        self.runner.run_ok("launchctl", &["bootout", target])?;
        let poll = Duration::from_millis(200);
        let polls = u64::from(MAX_STOP_SECONDS) * 1000 / 200;
        for _ in 0..polls {
            if !self.loaded(target)? {
                return Ok(());
            }
            self.runner.pause(poll);
        }
        Err(HostError::command(
            format!("launchctl bootout {target}"),
            format!("the engine did not stop within {MAX_STOP_SECONDS} seconds"),
        ))
    }

    fn bootstrap(&self, domain: &str, plist: &str) -> Result<(), HostError> {
        self.runner
            .run_ok("launchctl", &["bootstrap", domain, plist])
    }
}

impl ServiceManager for Launchd {
    fn default_scope(&self) -> Scope {
        if self.runner.is_elevated() {
            Scope::System
        } else {
            Scope::User
        }
    }

    fn install(&self, spec: &ServiceSpec) -> Result<(), HostError> {
        self.check_scope(spec.scope)?;
        let path = self.plist_path(spec.scope)?;
        let domain = self.domain(spec.scope)?;
        let target = job(&domain);
        self.runner.create_dir_all(&spec.state_dir)?;
        self.runner.create_dir_all(&spec.log_dir)?;
        if spec.scope == Scope::System {
            self.share_with_group(spec)?;
        }
        if self.loaded(&target)? {
            self.bootout(&target)?;
        }
        self.runner.write_file(&path, &plist(spec))?;
        // Clears a "disabled" override left by an earlier `launchctl disable`.
        self.runner.run_ok("launchctl", &["enable", &target])?;
        self.bootstrap(&domain, &path.to_string_lossy())
    }

    fn uninstall(&self, scope: Scope) -> Result<(), HostError> {
        self.check_scope(scope)?;
        let path = self.plist_path(scope)?;
        let target = self.target(scope)?;
        let loaded = self.loaded(&target)?;
        if !loaded && self.runner.read_file(&path)?.is_none() {
            return Err(HostError::NotInstalled);
        }
        if loaded {
            self.bootout(&target)?;
        }
        self.runner.remove_file(&path)
    }

    fn start(&self, scope: Scope) -> Result<(), HostError> {
        self.check_scope(scope)?;
        let path = self.plist_path(scope)?;
        if self.runner.read_file(&path)?.is_none() {
            return Err(HostError::NotInstalled);
        }
        let domain = self.domain(scope)?;
        let target = job(&domain);
        if !self.loaded(&target)? {
            // Loading runs it (RunAtLoad).
            return self.bootstrap(&domain, &path.to_string_lossy());
        }
        self.runner.run_ok("launchctl", &["kickstart", &target])
    }

    fn stop(&self, scope: Scope) -> Result<(), HostError> {
        self.check_scope(scope)?;
        let target = self.target(scope)?;
        if !self.loaded(&target)? {
            return Ok(());
        }
        // Unloading stops the engine (SIGTERM, then the exit timeout) and keeps KeepAlive
        // from restarting it; `start` loads it again.
        self.bootout(&target)
    }

    fn status(&self, scope: Scope) -> Result<ServiceStatus, HostError> {
        let path = self.plist_path(scope)?;
        let Some(definition) = self.runner.read_file(&path)? else {
            return Ok(ServiceStatus::default());
        };
        let target = self.target(scope)?;
        let print = self.runner.run("launchctl", &["print", &target])?;
        let field = |name: &str| property(&print.stdout, name);
        let arguments = program_arguments(&definition);
        Ok(ServiceStatus {
            installed: true,
            running: print.success() && field("state").as_deref() == Some("running"),
            pid: field("pid").and_then(|p| p.parse().ok()),
            definition: Some(path),
            bootstrap: bootstrap_argument(arguments.iter().map(String::as_str)),
        })
    }
}

// launchd and systemd exist on Unix only: these tests check Unix paths and the `sudo` hint.
#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::fake::{self, FakeRunner};

    fn spec(scope: Scope) -> ServiceSpec {
        ServiceSpec {
            scope,
            ..fake::spec(
                "/usr/local/bin/packie",
                "/Library/Application Support/PACK-IE/bootstrap & co.json",
            )
        }
    }

    #[test]
    fn plist_runs_the_engine_and_round_trips_arguments() {
        let text = plist(&spec(Scope::System));
        assert!(text.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(text.contains("<key>SuccessfulExit</key>\n        <false/>"));
        assert!(text.contains("<string>/var/lib/packie</string>"));
        assert!(text.contains("<string>/var/log/packie/launchd.stderr.log</string>"));
        assert!(text.contains("bootstrap &amp; co.json"));
        assert_eq!(
            program_arguments(&text),
            [
                "/usr/local/bin/packie",
                "run",
                "--bootstrap",
                "/Library/Application Support/PACK-IE/bootstrap & co.json",
                "--service"
            ]
        );
    }

    #[test]
    fn system_install_needs_root() {
        let launchd = Launchd::new(Box::new(FakeRunner::new(false)));
        let error = launchd.install(&spec(Scope::System)).unwrap_err();
        assert!(error.to_string().contains("sudo"), "{error}");
    }

    #[test]
    fn user_agent_install_loads_into_the_gui_domain() {
        let runner = FakeRunner::new(false);
        runner.push_output(0, "501\n", ""); // id -u
        runner.push_output(113, "", "Could not find service"); // print: not loaded
        let launchd = Launchd::new(Box::new(runner.clone()));
        assert_eq!(launchd.default_scope(), Scope::User);
        launchd.install(&spec(Scope::User)).unwrap();
        let plist = "/Users/test/Library/LaunchAgents/org.openbga.positiveack.plist";
        let calls = runner.calls();
        assert!(calls.contains(&format!("write {plist}")), "{calls:?}");
        assert!(
            calls.contains(&format!("launchctl bootstrap gui/501 {plist}")),
            "{calls:?}"
        );
        assert!(!calls.iter().any(|c| c.contains("bootout")), "{calls:?}");
    }

    #[test]
    fn system_install_shares_the_socket_with_the_ipc_group() {
        let runner = FakeRunner::new(true);
        runner.push_output(64, "", "Group not found"); // dseditgroup read
        for _ in 0..6 {
            runner.push_output(0, "", ""); // create, edit, chown and chmod twice
        }
        runner.push_output(113, "", ""); // launchctl print: not loaded
        let launchd = Launchd::new(Box::new(runner.clone()));
        launchd.install(&spec(Scope::System)).unwrap();
        let calls = runner.calls();
        for expected in [
            "dseditgroup -o create -r Positive ACK packie",
            "dseditgroup -o edit -a operator -t user packie",
            "chown root:packie /var/lib/packie",
            "chmod 0750 /var/lib/packie",
            "write /Library/LaunchDaemons/org.openbga.positiveack.plist",
            "launchctl bootstrap system /Library/LaunchDaemons/org.openbga.positiveack.plist",
        ] {
            assert!(calls.iter().any(|c| c == expected), "{expected}: {calls:?}");
        }
        let plist = runner
            .file(Path::new(
                "/Library/LaunchDaemons/org.openbga.positiveack.plist",
            ))
            .unwrap();
        assert!(plist.contains("<key>ExitTimeOut</key>\n    <integer>45</integer>"));
    }

    #[test]
    fn stop_waits_until_launchd_has_removed_the_job() {
        let runner = FakeRunner::new(true);
        runner.push_output(0, "", ""); // print: loaded
        runner.push_output(0, "", ""); // bootout
        runner.push_output(0, "", ""); // print: still draining
        runner.push_output(113, "", ""); // print: gone
        let launchd = Launchd::new(Box::new(runner.clone()));
        launchd.stop(Scope::System).unwrap();
        let target = "system/org.openbga.positiveack";
        assert_eq!(
            runner.calls(),
            [
                format!("launchctl print {target}"),
                format!("launchctl bootout {target}"),
                format!("launchctl print {target}"),
                format!("launchctl print {target}"),
            ]
        );
    }

    #[test]
    fn status_reads_the_plist_and_launchctl_print() {
        let runner = FakeRunner::new(true);
        let launchd = Launchd::new(Box::new(runner.clone()));
        assert_eq!(
            launchd.status(Scope::System).unwrap(),
            ServiceStatus::default()
        );
        let path = "/Library/LaunchDaemons/org.openbga.positiveack.plist";
        runner
            .write_file(Path::new(path), &plist(&spec(Scope::System)))
            .unwrap();
        runner.push_output(0, "service = {\n\tstate = running\n\tpid = 4242\n}", "");
        let status = launchd.status(Scope::System).unwrap();
        assert!(status.installed && status.running);
        assert_eq!(status.pid, Some(4242));
        assert_eq!(
            status.bootstrap.as_deref(),
            Some(Path::new(
                "/Library/Application Support/PACK-IE/bootstrap & co.json"
            ))
        );
        // Installed but not loaded: launchctl print fails.
        runner.push_output(113, "", "Could not find service");
        assert!(!launchd.status(Scope::System).unwrap().running);
    }
}
