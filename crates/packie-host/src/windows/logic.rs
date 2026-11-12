//! The parts of the Windows service that need no SCM: error classification, status mapping,
//! polling, the Application event-log entries and running the engine body.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::commandline::bootstrap_from_command_line;
use super::{ServiceBody, ServiceError, StopSignal};
use crate::error::HostError;
use crate::runner::Runner;
use crate::service::{DISPLAY_NAME, MAX_STOP_SECONDS, SERVICE_NAME, ServiceStatus};

/// The account the service runs as.
pub(super) const ACCOUNT: &str = "LocalSystem";

/// What the SCM shows for the service.
pub(super) const DESCRIPTION: &str = "Positive ACK laboratory interface engine";
/// The delay before the SCM restarts a failed service.
pub(super) const RESTART_DELAY: Duration = Duration::from_secs(5);
/// Failure actions configured, all restarts; the SCM repeats the last action for any
/// further failure, so the service keeps being restarted.
pub(super) const RESTART_ACTIONS: usize = 3;
/// The failure count restarts from zero after this long without failures.
pub(super) const FAILURE_RESET: Duration = Duration::from_hours(24);
/// The longest stop: the largest drain (`hosting.shutdown_drain_seconds`) plus the margin.
pub(super) const MAX_STOP: Duration = Duration::from_secs(MAX_STOP_SECONDS as u64);
/// How often the stop and start waits look at the service state.
pub(super) const POLL: Duration = Duration::from_millis(250);

/// `ERROR_ACCESS_DENIED`.
const ACCESS_DENIED: i32 = 5;
/// `ERROR_SERVICE_ALREADY_RUNNING`.
const ALREADY_RUNNING: i32 = 1056;
/// `ERROR_SERVICE_DOES_NOT_EXIST`.
const DOES_NOT_EXIST: i32 = 1060;
/// `ERROR_SERVICE_CANNOT_ACCEPT_CTRL`: the service is already stopping.
const CANNOT_ACCEPT_CONTROL: i32 = 1061;
/// `ERROR_SERVICE_NOT_ACTIVE`.
const NOT_ACTIVE: i32 = 1062;
/// `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT`: not started by the SCM.
const NOT_UNDER_SCM: i32 = 1063;
/// `ERROR_SERVICE_MARKED_FOR_DELETE`.
const MARKED_FOR_DELETE: i32 = 1072;

/// What an SCM error code means for us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    NotInstalled,
    AccessDenied,
    AlreadyRunning,
    AlreadyStopping,
    NotActive,
    NotUnderScm,
    MarkedForDelete,
    Other,
}

/// Classifies the Win32 error code of a failed SCM call.
pub(super) fn classify(code: Option<i32>) -> Failure {
    match code {
        Some(DOES_NOT_EXIST) => Failure::NotInstalled,
        Some(ACCESS_DENIED) => Failure::AccessDenied,
        Some(ALREADY_RUNNING) => Failure::AlreadyRunning,
        Some(CANNOT_ACCEPT_CONTROL) => Failure::AlreadyStopping,
        Some(NOT_ACTIVE) => Failure::NotActive,
        Some(NOT_UNDER_SCM) => Failure::NotUnderScm,
        Some(MARKED_FOR_DELETE) => Failure::MarkedForDelete,
        _ => Failure::Other,
    }
}

/// The operator-facing error for a classified failure of `action`, or `None` when the
/// caller decides (already running, not active).
pub(super) fn explain(failure: Failure, action: &str) -> Option<HostError> {
    match failure {
        Failure::NotInstalled => Some(HostError::NotInstalled),
        Failure::AccessDenied => Some(HostError::NotElevated(
            "access denied by the service control manager: run the terminal as administrator and try again".to_owned(),
        )),
        Failure::NotUnderScm => Some(scm_error(
            action,
            "this process was not started by the service control manager; `packie run --service` is only for the installed service, use `packie run` in a terminal",
        )),
        Failure::MarkedForDelete => Some(scm_error(
            action,
            "the service is marked for deletion; close the Services console and any tool that has it open (or restart the computer), then install again",
        )),
        _ => None,
    }
}

/// A failed call of the service control manager.
pub(super) fn scm_error(action: &str, message: impl Into<String>) -> HostError {
    HostError::command(format!("service control manager ({action})"), message)
}

/// What the SCM reported about an installed service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Observed {
    /// The service state is Stopped.
    pub(super) stopped: bool,
    /// The process id while it runs.
    pub(super) pid: Option<u32>,
    /// The configured command line (executable and arguments).
    pub(super) command_line: String,
}

/// Where the SCM keeps the definition.
pub(super) fn definition() -> PathBuf {
    PathBuf::from(format!(
        r"HKLM\SYSTEM\CurrentControlSet\Services\{SERVICE_NAME}"
    ))
}

/// The status of an installed service.
pub(super) fn status_from(observed: &Observed) -> ServiceStatus {
    ServiceStatus {
        installed: true,
        running: !observed.stopped,
        pid: observed.pid.filter(|pid| *pid > 0),
        definition: Some(definition()),
        bootstrap: bootstrap_from_command_line(&observed.command_line),
    }
}

/// Polls `done` every `interval` until it is true (returns `Ok(true)`) or `limit`
/// has passed (`Ok(false)`).
pub(super) fn wait_for(
    limit: Duration,
    interval: Duration,
    mut done: impl FnMut() -> Result<bool, HostError>,
) -> Result<bool, HostError> {
    let started = Instant::now();
    loop {
        if done()? {
            return Ok(true);
        }
        if started.elapsed() >= limit {
            return Ok(false);
        }
        std::thread::sleep(interval);
    }
}

/// The entries the service writes to the Application event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Event {
    Started,
    Stopped,
    /// The engine refused to start until its configuration is fixed; the cause.
    Refused(String),
}

/// Longest cause written into an event-log entry.
const EVENT_TEXT_LIMIT: usize = 900;

/// Writes `event` to the Application log with `eventcreate`. A failure is logged and
/// ignored; the engine does not depend on the event log.
pub(super) fn log_event(runner: &dyn Runner, event: &Event) {
    let (id, kind, text) = match event {
        Event::Started => ("1", "INFORMATION", format!("{DISPLAY_NAME} started")),
        Event::Stopped => ("2", "INFORMATION", format!("{DISPLAY_NAME} stopped")),
        Event::Refused(cause) => {
            let cause: String = cause.chars().take(EVENT_TEXT_LIMIT).collect();
            let text = format!(
                "{DISPLAY_NAME} did not start and will not be restarted until the configuration is fixed: {cause}"
            );
            ("3", "ERROR", text)
        }
    };
    let args = [
        "/L",
        "APPLICATION",
        "/T",
        kind,
        "/SO",
        SERVICE_NAME,
        "/ID",
        id,
        "/D",
        &text,
    ];
    let failure = match runner.run("eventcreate", &args) {
        Ok(output) if output.success() => return,
        Ok(output) => format!("eventcreate exited with {:?}", output.code),
        Err(error) => error.to_string(),
    };
    tracing::warn!(
        event = "host.eventlog_failed",
        error_summary = %failure,
        "could not write the Application event-log entry: {failure}"
    );
}

/// Runs the engine body, turning a panic into an error so the service still reports
/// its end.
pub(super) fn run_body(body: ServiceBody, signal: StopSignal) -> Result<(), ServiceError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || body(signal))) {
        Ok(result) => result,
        Err(_) => Err(ServiceError::Failed("the engine panicked".to_owned())),
    }
}

/// How the end of the engine is reported to the SCM: the service-specific exit code (`None`
/// for a clean stop) and the event-log entry naming a configuration error. Only a non-zero
/// code makes the SCM run its recovery actions (restart on non-crash failures).
pub(super) fn ending(outcome: &Result<(), ServiceError>) -> (Option<u32>, Option<Event>) {
    match outcome {
        Ok(()) => (None, None),
        Err(ServiceError::Failed(_)) => (Some(1), None),
        Err(ServiceError::Configuration(cause)) => (None, Some(Event::Refused(cause.clone()))),
    }
}

/// A stop channel: the sender fires when the SCM asks the service to stop.
pub(super) fn stop_channel() -> (tokio::sync::oneshot::Sender<()>, StopSignal) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    (sender, StopSignal { receiver })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::fake::FakeRunner;

    #[test]
    fn scm_error_codes_are_classified() {
        for (code, expected) in [
            (Some(1060), Failure::NotInstalled),
            (Some(5), Failure::AccessDenied),
            (Some(1056), Failure::AlreadyRunning),
            (Some(1061), Failure::AlreadyStopping),
            (Some(1062), Failure::NotActive),
            (Some(1063), Failure::NotUnderScm),
            (Some(1072), Failure::MarkedForDelete),
            (Some(2), Failure::Other),
            (None, Failure::Other),
        ] {
            assert_eq!(classify(code), expected, "{code:?}");
        }
    }

    #[test]
    fn recovery_and_stop_limits_are_the_documented_ones() {
        assert_eq!(RESTART_DELAY, Duration::from_secs(5));
        assert_eq!(RESTART_ACTIONS, 3);
        assert_eq!(FAILURE_RESET, Duration::from_hours(24));
        assert_eq!(MAX_STOP, Duration::from_secs(330));
        assert_eq!(ACCOUNT, "LocalSystem");
        assert!(!DESCRIPTION.is_empty());
        assert!(POLL < Duration::from_secs(1));
    }

    #[test]
    fn access_denied_tells_the_operator_to_elevate() {
        let Some(HostError::NotElevated(message)) =
            explain(Failure::AccessDenied, "create service")
        else {
            panic!("expected a not-elevated error");
        };
        assert!(message.contains("administrator"));
        assert!(matches!(
            explain(Failure::NotInstalled, "open service"),
            Some(HostError::NotInstalled)
        ));
        assert!(explain(Failure::AlreadyRunning, "start service").is_none());
        assert!(explain(Failure::Other, "start service").is_none());
    }

    #[test]
    fn status_maps_what_the_scm_reports() {
        let running = status_from(&Observed {
            stopped: false,
            pid: Some(4242),
            command_line: r"packie.exe run --bootstrap C:\b.json --service".into(),
        });
        assert!(running.installed && running.running);
        assert_eq!(running.pid, Some(4242));
        assert_eq!(running.bootstrap.as_deref(), Some(Path::new(r"C:\b.json")));
        assert!(
            running
                .definition
                .unwrap()
                .to_string_lossy()
                .ends_with(r"Services\packie")
        );

        let stopped = status_from(&Observed {
            stopped: true,
            pid: Some(0),
            command_line: String::new(),
        });
        assert!(stopped.installed && !stopped.running);
        assert_eq!(stopped.pid, None);
        assert_eq!(stopped.bootstrap, None);
    }

    #[test]
    fn wait_for_polls_until_done_or_the_limit() {
        let mut polls = 0;
        let done = wait_for(Duration::from_secs(5), Duration::from_millis(1), || {
            polls += 1;
            Ok(polls == 3)
        })
        .unwrap();
        assert!(done);
        assert_eq!(polls, 3);

        let done = wait_for(Duration::from_millis(5), Duration::from_millis(1), || {
            Ok(false)
        })
        .unwrap();
        assert!(!done);

        let failed = wait_for(Duration::from_secs(5), Duration::from_millis(1), || {
            Err(HostError::NotInstalled)
        });
        assert!(matches!(failed, Err(HostError::NotInstalled)));
    }

    #[test]
    fn start_and_stop_write_one_event_each() {
        let runner = FakeRunner::new(true);
        log_event(&runner, &Event::Started);
        log_event(&runner, &Event::Stopped);
        log_event(&runner, &Event::Refused("bootstrap: secret store".into()));
        assert_eq!(
            runner.calls(),
            [
                "eventcreate /L APPLICATION /T INFORMATION /SO packie /ID 1 /D Positive ACK started",
                "eventcreate /L APPLICATION /T INFORMATION /SO packie /ID 2 /D Positive ACK stopped",
                "eventcreate /L APPLICATION /T ERROR /SO packie /ID 3 /D Positive ACK did not start and will not be restarted until the configuration is fixed: bootstrap: secret store",
            ]
        );
    }

    #[test]
    fn a_failed_event_write_is_ignored() {
        let runner = FakeRunner::new(true);
        runner.push_output(1, "", "access denied");
        log_event(&runner, &Event::Started);
    }

    #[tokio::test]
    async fn stop_signal_completes_when_the_sender_fires_or_goes_away() {
        let (sender, signal) = stop_channel();
        sender.send(()).unwrap();
        signal.stopped().await;

        let (sender, signal) = stop_channel();
        drop(sender);
        signal.stopped().await;
    }

    #[test]
    fn the_body_result_and_panics_are_reported() {
        let (_sender, signal) = stop_channel();
        assert_eq!(run_body(Box::new(|_| Ok(())), signal), Ok(()));
        let (_sender, signal) = stop_channel();
        assert_eq!(
            run_body(
                Box::new(|_| Err(ServiceError::Failed("boom".into()))),
                signal
            ),
            Err(ServiceError::Failed("boom".to_owned()))
        );
        let (_sender, signal) = stop_channel();
        let panicked = run_body(
            Box::new(|_| {
                panic!("engine bug");
            }),
            signal,
        );
        assert!(matches!(panicked, Err(ServiceError::Failed(_))));
    }

    /// A configuration error stops cleanly, so the recovery actions do not restart the
    /// service into it again; a failure is restarted.
    #[test]
    fn only_failures_are_reported_for_a_restart() {
        assert_eq!(ending(&Ok(())), (None, None));
        assert_eq!(
            ending(&Err(ServiceError::Failed("boom".into()))),
            (Some(1), None)
        );
        assert_eq!(
            ending(&Err(ServiceError::Configuration("secret store".into()))),
            (None, Some(Event::Refused("secret store".into())))
        );
    }
}
