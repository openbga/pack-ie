//! The service process (Windows only): registers with the dispatcher, handles the SCM's
//! controls and reports the service state while the engine body runs.

use std::ffi::OsString;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use windows_service::define_windows_service;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{
    self, ServiceControlHandlerResult, ServiceStatusHandle,
};
use windows_service::service_dispatcher;

use super::logic::{Event, MAX_STOP, ending, log_event, run_body, scm_error, stop_channel};
use super::scm::host_error;
use super::{ServiceBody, ServiceError};
use crate::error::HostError;
use crate::runner::SystemRunner;
use crate::service::SERVICE_NAME;

/// The body, parked for `service_main`, which the dispatcher calls without arguments
/// of ours.
static BODY: Mutex<Option<ServiceBody>> = Mutex::new(None);
/// What went wrong in `service_main`, for [`run_service`] to return.
static FAILURE: Mutex<Option<HostError>> = Mutex::new(None);

define_windows_service!(ffi_service_main, service_main);

/// Locks `mutex`, ignoring poisoning.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `body` under the dispatcher; blocks until the service ends.
pub(super) fn run_service(body: ServiceBody) -> Result<(), HostError> {
    *lock(&BODY) = Some(body);
    *lock(&FAILURE) = None;
    let dispatched = service_dispatcher::start(SERVICE_NAME, ffi_service_main);
    // A start that never reached `service_main` leaves the body parked.
    drop(lock(&BODY).take());
    if let Err(error) = dispatched {
        return Err(host_error("start service dispatcher", error));
    }
    match lock(&FAILURE).take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// The service entry the dispatcher calls: runs [`serve`] and keeps its failure.
fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = serve() {
        tracing::error!(
            event = "host.service_failed",
            error_summary = %error,
            "the Windows service failed: {error}"
        );
        *lock(&FAILURE) = Some(error);
    }
}

/// Registers with the SCM, reports Running, runs the engine and reports Stopped.
fn serve() -> Result<(), HostError> {
    let Some(body) = lock(&BODY).take() else {
        return Err(scm_error("start service", "no engine to run"));
    };
    let (stop, signal) = stop_channel();
    let status = Arc::new(OnceLock::new());
    let handle =
        service_control_handler::register(SERVICE_NAME, control_handler(stop, Arc::clone(&status)))
            .map_err(|e| host_error("register control handler", e))?;
    let _ = status.set(handle);
    handle
        .set_service_status(running_status())
        .map_err(|e| host_error("report running", e))?;

    let runner = SystemRunner;
    log_event(&runner, &Event::Started);
    let outcome = run_body(body, signal);
    log_event(&runner, &Event::Stopped);

    if let Err(ServiceError::Failed(message) | ServiceError::Configuration(message)) = &outcome {
        tracing::error!(
            event = "host.engine_failed",
            error_summary = %message,
            "the engine ended with an error: {message}"
        );
    }
    let (code, refused) = ending(&outcome);
    if let Some(event) = refused {
        log_event(&runner, &event);
    }
    let exit_code = code.map_or(ServiceExitCode::NO_ERROR, ServiceExitCode::ServiceSpecific);
    handle
        .set_service_status(stopped_status(exit_code))
        .map_err(|e| host_error("report stopped", e))
}

/// Handles SCM controls: Stop and Shutdown report StopPending, then complete the stop
/// signal. Runs on the SCM's thread and never blocks.
fn control_handler(
    stop: tokio::sync::oneshot::Sender<()>,
    status: Arc<OnceLock<ServiceStatusHandle>>,
) -> impl FnMut(ServiceControl) -> ServiceControlHandlerResult + Send + 'static {
    let mut stop = Some(stop);
    move |control| match control {
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(handle) = status.get()
                && let Err(error) = handle.set_service_status(stop_pending_status())
            {
                tracing::warn!(
                    event = "host.status_failed",
                    error_summary = %error,
                    "could not report StopPending: {error}"
                );
            }
            if let Some(sender) = stop.take() {
                let _ = sender.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        _ => ServiceControlHandlerResult::NotImplemented,
    }
}

/// A status report of this service.
fn status_report(
    state: ServiceState,
    accepted: ServiceControlAccept,
    exit_code: ServiceExitCode,
    checkpoint: u32,
    wait_hint: Duration,
) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accepted,
        exit_code,
        checkpoint,
        wait_hint,
        process_id: None,
    }
}

/// Running: accepts Stop and Shutdown.
fn running_status() -> ServiceStatus {
    status_report(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        ServiceExitCode::NO_ERROR,
        0,
        Duration::ZERO,
    )
}

/// StopPending with the longest drain as the wait hint.
fn stop_pending_status() -> ServiceStatus {
    status_report(
        ServiceState::StopPending,
        ServiceControlAccept::empty(),
        ServiceExitCode::NO_ERROR,
        1,
        MAX_STOP,
    )
}

/// Stopped with `exit_code`.
fn stopped_status(exit_code: ServiceExitCode) -> ServiceStatus {
    status_report(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        exit_code,
        0,
        Duration::ZERO,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_pending_waits_long_enough_for_the_drain() {
        let status = stop_pending_status();
        assert_eq!(status.current_state, ServiceState::StopPending);
        assert_eq!(status.wait_hint, Duration::from_secs(330));
        assert!(status.controls_accepted.is_empty());
        let failed = stopped_status(ServiceExitCode::ServiceSpecific(1));
        assert_eq!(failed.exit_code, ServiceExitCode::ServiceSpecific(1));
    }

    #[test]
    fn running_service_accepts_stop_and_shutdown() {
        let status = running_status();
        assert!(
            status
                .controls_accepted
                .contains(ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN)
        );
    }
}
