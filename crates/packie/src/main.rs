//! `packie`: hosts the Positive ACK engine and talks to a running one.
//!
//! `packie run` runs the engine in the foreground until Ctrl-C (or SIGTERM; Ctrl-Break on
//! Windows). `packie status`,
//! `packie channels ...`, `packie messages ...`, `packie config ...`, `packie secrets ...`, `packie logs`,
//! `packie audit ...`, `packie backup ...`, `packie endpoints ...` and `packie notify test` are clients of the engine's local management
//! interface (`packie audit verify --dir`, `packie backup verify` and, with the engine stopped,
//! `packie backup create`, `restore` and `status` work on files); they find it through the
//! bootstrap file (`--bootstrap`, `PACKIE_BOOTSTRAP`, or `config/bootstrap.json` in the
//! current directory) or `--ipc`.
//! `packie service ...` installs and controls the engine as an OS service, which the service
//! manager starts as `packie run --bootstrap <path> --service`.

mod args;
mod audit;
mod backup;
mod channels;
mod checks;
mod client;
mod config_diff;
mod logs;
mod message_view;
mod messages;
mod output;
mod reports;
mod secrets;
mod service;
mod status;
mod stop;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::channels::ChannelsCommand;
use crate::logs::LogsArgs;
use crate::messages::MessagesCommand;

use crate::output::{eprint_line, print_line};

/// Positive ACK: a laboratory interface engine.
#[derive(Debug, Parser)]
#[command(name = "packie", version, about, propagate_version = true)]
struct Cli {
    /// Bootstrap file of the engine.
    #[arg(
        long,
        global = true,
        env = "PACKIE_BOOTSTRAP",
        default_value = "config/bootstrap.json"
    )]
    bootstrap: PathBuf,

    /// Management interface to connect to (socket path or pipe name), instead of the one
    /// named by the bootstrap file.
    #[arg(long, global = true, env = "PACKIE_IPC")]
    ipc: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the engine in the foreground until Ctrl-C.
    Run {
        /// Run as the OS service: log to the log directory only (the service manager
        /// passes this).
        #[arg(long)]
        service: bool,
    },
    /// Show the engine, its channels and endpoints.
    Status {
        /// Print the complete snapshot as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Pause, resume or restart a channel.
    #[command(subcommand)]
    Channels(ChannelsCommand),
    /// Store, remove and list the secrets endpoints reference (values are never shown).
    #[command(subcommand)]
    Secrets(secrets::SecretsCommand),
    /// List and inspect stored messages; resend them and close deliveries.
    #[command(subcommand)]
    Messages(MessagesCommand),
    /// Show, check and change the runtime configuration.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Show recent log events, optionally following new ones.
    Logs(LogsArgs),
    /// Install and control the engine as an OS service.
    Service(ServiceArgs),
    /// Review, export and verify the audit log of operator actions.
    #[command(subcommand)]
    Audit(audit::AuditCommand),
    /// Create, verify and restore backups of the engine's state.
    #[command(subcommand)]
    Backup(backup::BackupCommand),
    /// List serial ports and test endpoints without sending data.
    #[command(subcommand)]
    Endpoints(checks::EndpointsCommand),
    /// Test the alert notification targets.
    #[command(subcommand)]
    Notify(checks::NotifyCommand),
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print the active runtime document, with secrets as references by name (redirect it to
    /// a file to export it).
    #[command(alias = "export")]
    Show,
    /// Check a runtime document without applying it (works without an engine).
    Validate {
        /// The document.
        file: PathBuf,
    },
    /// Validate, save as the next version and apply a runtime document.
    Apply {
        /// The document.
        file: PathBuf,
        /// Apply even if the document was edited from an older version than the active
        /// one.
        #[arg(long)]
        force: bool,
        /// Why (1 to 500 characters; required), shown in the version history.
        #[arg(long)]
        note: String,
    },
    /// List saved versions with who applied them and their verification sign-offs.
    History,
    /// Sign off a saved version as verified after the change.
    Verify {
        /// The version.
        #[arg(value_name = "VERSION")]
        config_version: u64,
        /// Your reference, such as a validation record number (1 to 500 characters).
        #[arg(long)]
        note: String,
    },
    /// Show what changes from one document to another, field by field.
    Diff {
        /// The old side: a saved version number, `active`, or a runtime document file.
        a: String,
        /// The new side, written the same way.
        b: String,
        /// Print the comparison as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
    /// macOS: a user agent for this account (starts at login) instead of the system
    /// daemon. Without it the scope follows elevation: system service with sudo, user
    /// agent without.
    #[arg(long, global = true)]
    user: bool,
}

#[derive(Debug, Clone, Copy, Subcommand)]
enum ServiceCommand {
    /// Install the engine as a service that starts with the computer, and start it
    /// (needs sudo or an administrator terminal, except a macOS user agent).
    Install,
    /// Remove the service.
    Uninstall,
    /// Start the installed service.
    Start,
    /// Stop the service.
    Stop,
    /// Show whether the service is installed and running.
    Status,
}

/// A command failed; the message is for the operator.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct Failure {
    pub(crate) message: String,
    /// The process exit code: 1, or 2 for invalid input the operator must fix first.
    pub(crate) code: u8,
}

impl Failure {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 1,
        }
    }

    /// Input that cannot work as given, such as an invalid bootstrap document (exit code 2).
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 2,
        }
    }
}

fn main() -> ExitCode {
    // Before anything else: a stop request during startup must wait for the handlers.
    stop::hold();
    stop::held_for_test();
    let cli = Cli::parse();
    if !matches!(cli.command, Command::Run { .. }) {
        stop::release();
    }
    // The service control manager talks to the process before any async runtime exists.
    #[cfg(windows)]
    if matches!(cli.command, Command::Run { service: true }) {
        return finish(service::run_windows_service(cli.bootstrap));
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprint_line(&format!("packie: cannot start the async runtime: {error}"));
            return ExitCode::FAILURE;
        }
    };
    finish(runtime.block_on(dispatch(cli)))
}

fn finish(result: Result<(), Failure>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure { message, code }) => {
            eprint_line(&format!("packie: {message}"));
            ExitCode::from(code)
        }
    }
}

async fn dispatch(cli: Cli) -> Result<(), Failure> {
    let target = client::Target {
        bootstrap: cli.bootstrap.clone(),
        ipc: cli.ipc.clone(),
    };
    match cli.command {
        Command::Run { service } => {
            let stop = stop::install()
                .map_err(|e| Failure::new(format!("cannot install the stop handlers: {e}")))?;
            let result = run(&cli.bootstrap, service, stop).await;
            if service {
                launchd_outcome(result)
            } else {
                result
            }
        }
        Command::Status { json } => status::status(&target, json).await,
        Command::Config(command) => config(&target, command).await,
        Command::Channels(command) => channels::run(&target, command).await,
        Command::Secrets(command) => secrets::run(&target, command).await,
        Command::Messages(command) => messages::run(&target, command).await,
        Command::Logs(args) => logs::logs(&target, &args).await,
        Command::Audit(command) => audit::run(&target, command).await,
        Command::Backup(command) => backup::run(&target, command).await,
        Command::Endpoints(command) => checks::endpoints(&target, command).await,
        Command::Notify(command) => checks::notify(&target, command).await,
        Command::Service(args) => {
            let action = match args.command {
                ServiceCommand::Install => service::Action::Install,
                ServiceCommand::Uninstall => service::Action::Uninstall,
                ServiceCommand::Start => service::Action::Start,
                ServiceCommand::Stop => service::Action::Stop,
                ServiceCommand::Status => service::Action::Status,
            };
            service::service(action, &cli.bootstrap, args.user)
        }
    }
}

/// launchd relaunches a job after any non-zero exit (`KeepAlive` has no per-code
/// exception), so on macOS the service ends a configuration error (exit code 2: invalid
/// bootstrap, unusable secret store) with status 0 after printing it to its error log;
/// launchd then leaves it stopped until `packie service start`. Elsewhere the result stands:
/// systemd's unit prevents a restart on exit code 2, and Windows maps it in the service.
fn launchd_outcome(result: Result<(), Failure>) -> Result<(), Failure> {
    match result {
        Err(Failure { message, code: 2 }) if cfg!(target_os = "macos") => {
            eprint_line(&format!(
                "packie: {message}\npackie: the service is not restarted until this is fixed; then run \
                 `sudo packie service start`."
            ));
            Ok(())
        }
        other => other,
    }
}

async fn config(target: &client::Target, command: ConfigCommand) -> Result<(), Failure> {
    match command {
        ConfigCommand::Show => client::config_show(target).await,
        ConfigCommand::Validate { file } => client::config_validate(&file),
        ConfigCommand::Apply { file, force, note } => {
            client::config_apply(target, &file, force, note).await
        }
        ConfigCommand::History => client::config_history(target).await,
        ConfigCommand::Verify {
            config_version,
            note,
        } => client::config_verify(target, config_version, note).await,
        ConfigCommand::Diff { a, b, json } => config_diff::config_diff(target, &a, &b, json).await,
    }
}

/// Runs the engine until `shutdown`, holding the single-instance lock. As a service it logs
/// to the log directory only.
pub(crate) async fn run(
    bootstrap: &std::path::Path,
    as_service: bool,
    shutdown: impl Future<Output = ()>,
) -> Result<(), Failure> {
    let _lock = service::instance_lock(bootstrap)?;
    let registry = packie_routing::registry_with_store;
    let options = packie_engine::RunOptions {
        console_logs: !as_service,
        hosting: if as_service {
            packie_diagnostics::HostingMode::Service
        } else {
            packie_diagnostics::HostingMode::Console
        },
        audit: crate::audit::open_persistent,
    };
    packie_engine::run(bootstrap, options, registry, shutdown)
        .await
        .map_err(|error| match error {
            packie_engine::EngineError::Bootstrap(_) | packie_engine::EngineError::Secrets(_) => {
                Failure::invalid(error.to_string())
            }
            _ => Failure::new(error.to_string()),
        })?;
    if !as_service {
        print_line("Engine stopped.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    /// clap checks argument ids, conflicts and help only when a command is parsed; this
    /// checks every subcommand at once.
    #[test]
    fn the_command_tree_is_consistent() {
        Cli::command().debug_assert();
    }
}
