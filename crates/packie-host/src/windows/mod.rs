//! Windows: a service of the service control manager (SCM), running as `LocalSystem`,
//! started with the computer and restarted on failure.
//!
//! - `WindowsServices` is the [`ServiceManager`] back end: it talks to the SCM through
//!   the `windows-service` crate (install, update, start, stop, delete, status).
//! - [`run_service`] is the other end: the SCM starts `packie run --bootstrap <path>
//!   --service`, and the engine calls it to register with the SCM, report its state and
//!   learn when to stop.
//!
//! Installing also creates the local group `PACK-IE Operators` (the engine's management pipe
//! admits its members besides administrators) and adds the installing account to it: the
//! account named by [`ServiceSpec::operator`], else the elevated installer process's
//! `USERDOMAIN\USERNAME`.
//!
//! The module compiles on every OS so callers need no `cfg`; elsewhere every operation
//! returns [`HostError::Unsupported`]. The SCM code lives in the `scm` module (management)
//! and the `process` module (the service process), which only exist on Windows. Everything
//! that does not need the SCM is tested on every OS: `commandline` (the configured command
//! line), `logic` (error classification, status mapping, the Application event-log
//! entries, running the engine body) and `setup` (private directories, the operators
//! group).

use crate::error::HostError;
use crate::runner::Runner;
use crate::service::{Scope, ServiceManager, ServiceSpec, ServiceStatus};

#[cfg(any(windows, test))]
mod commandline;
#[cfg(any(windows, test))]
mod logic;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod scm;
#[cfg(any(windows, test))]
mod setup;

/// Stands in for the SCM back end on other operating systems.
#[cfg(not(windows))]
mod scm {
    use super::ServiceBody;
    use crate::error::HostError;
    use crate::runner::Runner;
    use crate::service::{ServiceSpec, ServiceStatus};

    fn unsupported() -> HostError {
        HostError::Unsupported("the Windows service is available on Windows only".into())
    }

    pub(super) fn run_service(_body: ServiceBody) -> Result<(), HostError> {
        Err(unsupported())
    }

    pub(super) fn install(_runner: &dyn Runner, _spec: &ServiceSpec) -> Result<(), HostError> {
        Err(unsupported())
    }

    pub(super) fn uninstall(_runner: &dyn Runner) -> Result<(), HostError> {
        Err(unsupported())
    }

    pub(super) fn start(_runner: &dyn Runner) -> Result<(), HostError> {
        Err(unsupported())
    }

    pub(super) fn stop(_runner: &dyn Runner) -> Result<(), HostError> {
        Err(unsupported())
    }

    pub(super) fn status() -> Result<ServiceStatus, HostError> {
        Err(unsupported())
    }
}

/// The engine, run as the service: gets the [`StopSignal`] and returns when the engine
/// has ended. [`ServiceError::Failed`] is reported to the SCM as a failure, which restarts
/// the service; [`ServiceError::Configuration`] is not.
pub type ServiceBody = Box<dyn FnOnce(StopSignal) -> Result<(), ServiceError> + Send>;

/// Why the engine, run as the service, ended other than by a stop request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceError {
    /// A failure the SCM answers with its recovery actions (a restart after 5 seconds).
    Failed(String),
    /// The configuration must be fixed first (the engine's exit code 2, such as an invalid
    /// bootstrap or an unusable secret store). Reported as a clean stop, so the SCM does not
    /// restart the service into the same error, with an Application event-log error that
    /// names the cause.
    Configuration(String),
}

/// Completes when the SCM asks the service to stop (Stop or system Shutdown).
#[derive(Debug)]
pub struct StopSignal {
    receiver: tokio::sync::oneshot::Receiver<()>,
}

impl StopSignal {
    /// Waits for the stop request. A control handler that went away counts as a stop.
    pub async fn stopped(self) {
        let _ = self.receiver.await;
    }
}

/// The Windows service control manager.
pub(crate) struct WindowsServices {
    runner: Box<dyn Runner>,
}

impl WindowsServices {
    /// Service management through `runner`.
    pub(crate) fn new(runner: Box<dyn Runner>) -> Self {
        Self { runner }
    }
}

/// Runs `body` as the Windows service: registers the control handler, reports `Running`,
/// calls `body(stop)`, and reports `Stopped` with exit code 0, or a service-specific code 1
/// when `body` returns an error, when it returns. `stop` completes when the SCM sends
/// `Stop` or `Shutdown`; the service then reports `StopPending` with a 330 second wait hint
/// (the longest drain, 300 seconds, plus the 30 second margin) until `body` returns.
/// Blocks until the service ends. Writes one Application event-log entry on start and
/// one on stop.
///
/// # Errors
/// This process was not started by the SCM, or the service could not register or report
/// its state; [`HostError::Unsupported`] off Windows.
pub fn run_service(body: ServiceBody) -> Result<(), HostError> {
    scm::run_service(body)
}

impl ServiceManager for WindowsServices {
    fn default_scope(&self) -> Scope {
        Scope::System
    }

    fn install(&self, spec: &ServiceSpec) -> Result<(), HostError> {
        scm::install(self.runner.as_ref(), spec)
    }

    fn uninstall(&self, _scope: Scope) -> Result<(), HostError> {
        scm::uninstall(self.runner.as_ref())
    }

    fn start(&self, _scope: Scope) -> Result<(), HostError> {
        scm::start(self.runner.as_ref())
    }

    fn stop(&self, _scope: Scope) -> Result<(), HostError> {
        scm::stop(self.runner.as_ref())
    }

    fn status(&self, _scope: Scope) -> Result<ServiceStatus, HostError> {
        scm::status()
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use crate::fake::{self, FakeRunner};

    #[test]
    fn everything_is_unsupported_off_windows() {
        let services = WindowsServices::new(Box::new(FakeRunner::new(true)));
        let spec = fake::spec("packie.exe", "b.json");
        assert!(matches!(
            services.install(&spec),
            Err(HostError::Unsupported(_))
        ));
        for result in [
            services.uninstall(Scope::System),
            services.start(Scope::System),
            services.stop(Scope::System),
        ] {
            assert!(matches!(result, Err(HostError::Unsupported(_))));
        }
        assert!(matches!(
            services.status(Scope::System),
            Err(HostError::Unsupported(_))
        ));
        assert!(matches!(
            run_service(Box::new(|_| Ok(()))),
            Err(HostError::Unsupported(_))
        ));
    }
}
