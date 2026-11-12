//! Starting the engine service: runs `packie service start` and reports what it printed.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::error::CommandError;

/// The only subcommand the console ever runs.
const SERVICE_ARGS: [&str; 2] = ["service", "start"];
/// How long `packie service start` may run before it is killed.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The arguments of `packie service start`: with `--bootstrap <file>` when the console found the
/// engine's bootstrap file, so the CLI manages the same installation the console looks for.
fn service_args(bootstrap: Option<&Path>) -> Vec<OsString> {
    let mut args = Vec::new();
    if let Some(path) = bootstrap {
        args.push(OsString::from("--bootstrap"));
        args.push(path.as_os_str().to_owned());
    }
    args.extend(SERVICE_ARGS.iter().map(OsString::from));
    args
}

/// Runs `packie [--bootstrap <file>] service start` and returns its standard output.
pub(crate) async fn start_service(bootstrap: Option<PathBuf>) -> Result<String, CommandError> {
    let bin = engine_binary(
        std::env::var_os("PACKIE_BIN"),
        std::env::current_exe().ok(),
        |path| path.is_file(),
    );
    run(&bin, &service_args(bootstrap.as_deref()), TIMEOUT).await
}

/// `PACKIE_BIN`, else `packie` next to the console executable, else `packie` on the `PATH`.
fn engine_binary(
    env_bin: Option<OsString>,
    current_exe: Option<PathBuf>,
    exists: impl Fn(&PathBuf) -> bool,
) -> OsString {
    if let Some(bin) = env_bin.filter(|bin| !bin.is_empty()) {
        return bin;
    }
    let name = format!("packie{}", std::env::consts::EXE_SUFFIX);
    current_exe
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(exists)
        .map_or_else(|| OsString::from("packie"), PathBuf::into_os_string)
}

async fn run<S: AsRef<OsStr>>(
    bin: &OsStr,
    args: &[S],
    timeout: Duration,
) -> Result<String, CommandError> {
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up.

    let child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CommandError::internal(
                "the packie program was not found. Install the engine next to the console, put packie \
                 on the PATH, or set PACKIE_BIN to its location.",
            )
        } else {
            CommandError::internal(format!("cannot start {}: {error}", bin.to_string_lossy()))
        }
    })?;
    // Dropping the wait on timeout drops the child, which kills it.
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return Err(CommandError::internal(format!(
                "waiting for packie failed: {error}"
            )));
        }
        Err(_) => {
            return Err(CommandError::internal(format!(
                "packie did not finish within {} seconds and was stopped.",
                timeout.as_secs()
            )));
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    Err(CommandError::internal(if message.is_empty() {
        format!("packie exited with {}", output.status)
    } else {
        message.to_owned()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_lookup_prefers_env_then_sibling_then_path() {
        let exe = Some(PathBuf::from("/app/packie-console"));
        let sibling = PathBuf::from("/app").join(format!("packie{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(
            engine_binary(Some("/custom/packie".into()), exe.clone(), |_| true),
            OsString::from("/custom/packie")
        );
        assert_eq!(
            engine_binary(Some(OsString::new()), exe.clone(), |p| *p == sibling),
            sibling.clone().into_os_string()
        );
        assert_eq!(
            engine_binary(None, exe.clone(), |_| false),
            OsString::from("packie")
        );
        assert_eq!(engine_binary(None, None, |_| true), OsString::from("packie"));
    }

    #[test]
    fn the_found_bootstrap_is_passed_to_the_cli() {
        assert_eq!(
            service_args(None),
            vec![OsString::from("service"), OsString::from("start")]
        );
        assert_eq!(
            service_args(Some(Path::new("/etc/packie/bootstrap.json"))),
            vec![
                OsString::from("--bootstrap"),
                OsString::from("/etc/packie/bootstrap.json"),
                OsString::from("service"),
                OsString::from("start"),
            ]
        );
    }

    #[tokio::test]
    async fn a_missing_binary_gets_an_operator_sentence() {
        let error = run(
            OsStr::new("/nonexistent/packie-for-tests"),
            &SERVICE_ARGS,
            TIMEOUT,
        )
        .await
        .unwrap_err();
        let CommandError::Internal { message } = error else {
            panic!("expected an internal error");
        };
        assert!(message.contains("not found"), "{message}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn success_returns_stdout_and_failure_returns_trimmed_stderr() {
        let ok = run(OsStr::new("sh"), &["-c", "echo started"], TIMEOUT)
            .await
            .unwrap();
        assert_eq!(ok.trim(), "started");

        let error = run(
            OsStr::new("sh"),
            &["-c", "echo '  not implemented; use packie run  ' >&2; exit 3"],
            TIMEOUT,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            CommandError::internal("not implemented; use packie run")
        );

        let error = run(OsStr::new("sh"), &["-c", "exit 4"], TIMEOUT)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("exited"), "{error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_hung_child_is_stopped_at_the_timeout() {
        let error = run(
            OsStr::new("sh"),
            &["-c", "sleep 30"],
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("did not finish"), "{error}");
    }
}
