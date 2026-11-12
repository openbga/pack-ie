//! The SCM back end (Windows only): installs, controls and queries the service through the
//! `windows-service` crate. The service process itself is in `process`.

use std::ffi::OsString;
use std::time::Duration;

use windows_service::Error as ScmError;
use windows_service::service::{
    Service, ServiceAccess, ServiceAction, ServiceActionType, ServiceErrorControl,
    ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState,
    ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use super::logic::{
    ACCOUNT, DESCRIPTION, FAILURE_RESET, Failure, MAX_STOP, Observed, POLL, RESTART_ACTIONS,
    RESTART_DELAY, classify, explain, scm_error, status_from, wait_for,
};
use super::setup::{ensure_operators_group, operator_account, restrict_dirs};
use crate::error::HostError;
use crate::runner::Runner;
use crate::service::{
    DISPLAY_NAME, SERVICE_NAME, ServiceSpec, ServiceStatus as HostStatus, require_elevated,
};

pub(super) use super::process::run_service;

/// The Win32 error code behind a failed SCM call, when there is one.
fn code_of(error: &ScmError) -> Option<i32> {
    match error {
        ScmError::Winapi(source) => source.raw_os_error(),
        _ => None,
    }
}

/// What a failed SCM call means for us.
fn failure_of(error: &ScmError) -> Failure {
    classify(code_of(error))
}

/// The operator-facing error for a failed SCM call.
pub(super) fn host_error(action: &str, error: ScmError) -> HostError {
    explain(failure_of(&error), action).unwrap_or_else(|| {
        let message = match &error {
            ScmError::Winapi(source) => source.to_string(),
            other => other.to_string(),
        };
        scm_error(action, message)
    })
}

/// Connects to the SCM of this computer.
fn connect(access: ServiceManagerAccess) -> Result<ServiceManager, HostError> {
    ServiceManager::local_computer(None::<&str>, access).map_err(|e| host_error("connect", e))
}

/// Connects to the SCM and opens the service with `access`. The service handle stays
/// valid after the connection is closed.
fn open(access: ServiceAccess) -> Result<Service, HostError> {
    connect(ServiceManagerAccess::CONNECT)?
        .open_service(SERVICE_NAME, access)
        .map_err(|e| host_error("open service", e))
}

/// [`open`] for the operations that need administrator rights.
fn open_elevated(runner: &dyn Runner, access: ServiceAccess) -> Result<Service, HostError> {
    require_elevated(runner)?;
    open(access)
}

/// The current state of `service`.
fn query_state(service: &Service) -> Result<ServiceState, HostError> {
    service
        .query_status()
        .map(|status| status.current_state)
        .map_err(|e| host_error("query service status", e))
}

/// The service definition: own process, auto start, `LocalSystem`.
fn service_info(spec: &ServiceSpec) -> ServiceInfo {
    ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: spec.executable.clone(),
        launch_arguments: spec
            .run_arguments()
            .into_iter()
            .map(OsString::from)
            .collect(),
        dependencies: Vec::new(),
        account_name: Some(OsString::from(ACCOUNT)),
        account_password: None,
    }
}

/// Restart after [`RESTART_DELAY`] on each failure; the failure count resets after a day.
fn failure_actions() -> ServiceFailureActions {
    let restart = ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: RESTART_DELAY,
    };
    ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(FAILURE_RESET),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart; RESTART_ACTIONS]),
    }
}

/// Description, recovery actions, and recovery also for a non-zero exit code.
fn configure(service: &Service) -> Result<(), HostError> {
    service
        .set_description(DESCRIPTION)
        .map_err(|e| host_error("set description", e))?;
    service
        .update_failure_actions(failure_actions())
        .map_err(|e| host_error("set recovery actions", e))?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(|e| host_error("set recovery on failed exit", e))
}

/// Sends Stop and waits up to `limit` for Stopped. Already stopped is fine.
fn stop_and_wait(service: &Service, limit: Duration) -> Result<(), HostError> {
    if let Err(error) = service.stop() {
        match failure_of(&error) {
            Failure::NotActive => return Ok(()),
            Failure::AlreadyStopping => {}
            _ => return Err(host_error("stop service", error)),
        }
    }
    let stopped = wait_for(limit, POLL, || {
        Ok(query_state(service)? == ServiceState::Stopped)
    })?;
    if stopped {
        Ok(())
    } else {
        Err(scm_error(
            "stop service",
            format!(
                "the service did not stop within {} seconds",
                limit.as_secs()
            ),
        ))
    }
}

/// Starts the service; already running is fine.
fn start_service(service: &Service) -> Result<(), HostError> {
    match service.start::<&str>(&[]) {
        Ok(()) => Ok(()),
        Err(error) if failure_of(&error) == Failure::AlreadyRunning => Ok(()),
        Err(error) => Err(host_error("start service", error)),
    }
}

/// Registers (or updates) the service with the private directories and the operators
/// group in place, and starts it.
pub(super) fn install(runner: &dyn Runner, spec: &ServiceSpec) -> Result<(), HostError> {
    require_elevated(runner)?;
    runner.create_dir_all(&spec.state_dir)?;
    runner.create_dir_all(&spec.log_dir)?;
    restrict_dirs(runner, &[&spec.state_dir, &spec.log_dir])?;
    // Before the service starts: the engine resolves the group when it binds the pipe.
    let operator = operator_account(
        spec.operator.as_deref(),
        std::env::var("USERDOMAIN").ok().as_deref(),
        std::env::var("USERNAME").ok().as_deref(),
    );
    ensure_operators_group(runner, operator.as_deref())?;
    let manager = connect(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    let access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::START
        | ServiceAccess::STOP
        | ServiceAccess::CHANGE_CONFIG;
    let info = service_info(spec);
    // An existing service keeps its identity and gets the new definition, so a
    // reinstall after moving the files or changing the bootstrap just works.
    let service = match manager.open_service(SERVICE_NAME, access) {
        Ok(service) => {
            service
                .change_config(&info)
                .map_err(|e| host_error("update service", e))?;
            if query_state(&service)? != ServiceState::Stopped {
                // The new command line applies at the next start.
                let limit = Duration::from_secs(spec.stop_timeout_seconds().into());
                stop_and_wait(&service, limit)?;
            }
            service
        }
        Err(error) if failure_of(&error) == Failure::NotInstalled => manager
            .create_service(&info, access)
            .map_err(|e| host_error("create service", e))?,
        Err(error) => return Err(host_error("open service", error)),
    };
    configure(&service)?;
    start_service(&service)
}

/// Stops and deletes the service.
pub(super) fn uninstall(runner: &dyn Runner) -> Result<(), HostError> {
    let service = open_elevated(
        runner,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    stop_and_wait(&service, MAX_STOP)?;
    service
        .delete()
        .map_err(|e| host_error("delete service", e))
}

/// Starts the installed service.
pub(super) fn start(runner: &dyn Runner) -> Result<(), HostError> {
    let service = open_elevated(runner, ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
    start_service(&service)
}

/// Stops the service and waits for it.
pub(super) fn stop(runner: &dyn Runner) -> Result<(), HostError> {
    let service = open_elevated(runner, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?;
    stop_and_wait(&service, MAX_STOP)
}

/// Works without administrator rights: only connects and queries.
pub(super) fn status() -> Result<HostStatus, HostError> {
    let service = match open(ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG) {
        Ok(service) => service,
        Err(HostError::NotInstalled) => return Ok(HostStatus::default()),
        Err(error) => return Err(error),
    };
    let status = service
        .query_status()
        .map_err(|e| host_error("query service status", e))?;
    let config = service
        .query_config()
        .map_err(|e| host_error("query service configuration", e))?;
    Ok(status_from(&Observed {
        stopped: status.current_state == ServiceState::Stopped,
        pid: status.process_id,
        command_line: config.executable_path.to_string_lossy().into_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    #[test]
    fn definition_is_an_auto_start_own_process_service() {
        let spec = fake::spec(
            r"C:\Program Files\packie\packie.exe",
            r"C:\ProgramData\Positive ACK\bootstrap.json",
        );
        let info = service_info(&spec);
        assert_eq!(info.name, "packie");
        assert_eq!(info.display_name, "Positive ACK");
        assert_eq!(info.service_type, ServiceType::OWN_PROCESS);
        assert_eq!(info.start_type, ServiceStartType::AutoStart);
        assert_eq!(info.error_control, ServiceErrorControl::Normal);
        assert_eq!(info.account_name.as_deref(), Some("LocalSystem".as_ref()));
        let arguments: Vec<_> = info
            .launch_arguments
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            arguments,
            [
                "run",
                "--bootstrap",
                r"C:\ProgramData\Positive ACK\bootstrap.json",
                "--service"
            ]
        );
    }

    #[test]
    fn recovery_restarts_after_five_seconds_and_resets_daily() {
        let actions = failure_actions();
        assert_eq!(
            actions.reset_period,
            ServiceFailureResetPeriod::After(Duration::from_secs(86_400))
        );
        let list = actions.actions.unwrap();
        assert_eq!(list.len(), 3);
        for action in list {
            assert_eq!(action.action_type, ServiceActionType::Restart);
            assert_eq!(action.delay, Duration::from_secs(5));
        }
    }

    #[test]
    fn win32_errors_map_to_host_errors() {
        let denied = ScmError::Winapi(std::io::Error::from_raw_os_error(5));
        assert!(matches!(
            host_error("create service", denied),
            HostError::NotElevated(_)
        ));
        let missing = ScmError::Winapi(std::io::Error::from_raw_os_error(1060));
        assert!(matches!(
            host_error("open service", missing),
            HostError::NotInstalled
        ));
        let other = ScmError::Winapi(std::io::Error::from_raw_os_error(1053));
        let HostError::Command { program, .. } = host_error("start service", other) else {
            panic!("expected a command error");
        };
        assert_eq!(program, "service control manager (start service)");
    }
}
