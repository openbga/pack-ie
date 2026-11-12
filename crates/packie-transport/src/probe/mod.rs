//! Read-only probes of endpoint definitions, for "test this endpoint" in the console.
//!
//! A probe walks through the steps a channel would take to use the endpoint (resolve,
//! connect, open, bind, authenticate, list) and reports each with its outcome and duration.
//! It never sends a byte to a peer or serial device, never writes to an SFTP server and
//! leaves nothing behind: sockets are dropped, a folder gets one hidden empty file that is
//! deleted at once. Details are operator text with counts only: file names can carry
//! identifiers, secrets never appear.
//!
//! Every step is bounded by what is left of the overall time limit; a step that cannot run
//! because an earlier one failed is omitted.

mod folder;
mod net;
mod serial;
// Linux only; its tests also run on other Unixes (the fake sysfs tree needs `:` in names).
#[cfg(any(target_os = "linux", all(test, unix)))]
mod sysfs;

use std::future::Future;
use std::time::{Duration, Instant};

use packie_config::{EndpointConfig, SecretResolver, Transport};

use crate::link::LinkError;
pub(crate) use crate::serial::PortUse;
pub(crate) use net::resolve;
pub use serial::{SerialPortInfo, SerialPortKind, list_serial_ports};

/// The result of a read-only probe of an endpoint definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// Every step that ran succeeded.
    pub ok: bool,
    /// The steps that ran, in order.
    pub steps: Vec<ProbeStep>,
    /// One sentence for the operator.
    pub summary: String,
}

/// One step of a probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeStep {
    /// `resolve`, `connect`, `open`, `bind`, `read directory`, `write test`,
    /// `authenticate`, `list`, ...
    pub name: String,
    /// The step succeeded.
    pub ok: bool,
    /// What was found or why it failed; no secrets and no file names.
    pub detail: String,
    /// How long the step took.
    pub millis: u64,
}

/// Probes `endpoint` without changing anything, spending at most `timeout` in total.
///
/// Secrets are looked up through `secrets` (a blocking call, made directly on the calling
/// task). A serial port is opened without asking whether this process already uses it, so
/// a port a running channel holds is reported by the operating system as in use by another
/// program; [`SystemTransports::probe`](crate::SystemTransports::probe) knows the channels
/// and says so precisely.
pub async fn probe(
    endpoint: &EndpointConfig,
    secrets: &dyn SecretResolver,
    timeout: Duration,
) -> ProbeReport {
    run(endpoint, secrets, timeout, PortUse::Free).await
}

/// Probes `endpoint`; `serial` says whether this process uses the serial port it names.
pub(crate) async fn run(
    endpoint: &EndpointConfig,
    secrets: &dyn SecretResolver,
    timeout: Duration,
    serial: PortUse,
) -> ProbeReport {
    let mut run = Run::new(timeout);
    match &endpoint.transport {
        Transport::Serial(settings) => serial::probe(&mut run, settings, serial).await,
        Transport::TcpClient(settings) => net::tcp_client(&mut run, settings).await,
        Transport::TcpListener(settings) => net::tcp_listener(&mut run, settings).await,
        Transport::Folder(settings) => folder::probe(&mut run, settings).await,
        Transport::Sftp(settings) => crate::sftp::probe(&mut run, settings, secrets).await,
    }
    run.into_report()
}

/// The steps taken so far and the time left.
pub(crate) struct Run {
    limit: Duration,
    deadline: Instant,
    steps: Vec<ProbeStep>,
    success: Option<String>,
}

impl Run {
    fn new(limit: Duration) -> Self {
        Self {
            limit,
            deadline: Instant::now() + limit,
            steps: Vec::new(),
            success: None,
        }
    }

    /// Runs one step, bounded by the time left, and records it. `Some` with the step's value
    /// when it succeeded; its detail is the second element of the result.
    pub(crate) async fn step<T>(
        &mut self,
        name: &str,
        work: impl Future<Output = Result<(T, String), LinkError>>,
    ) -> Option<T> {
        let started = Instant::now();
        let left = self.deadline.saturating_duration_since(started);
        let outcome = match tokio::time::timeout(left, work).await {
            Ok(result) => result.map_err(|error| error.cause),
            Err(_) => Err(format!(
                "did not finish within the probe limit of {} ms",
                self.limit.as_millis()
            )),
        };
        let (value, ok, detail) = match outcome {
            Ok((value, detail)) => (Some(value), true, detail),
            Err(detail) => (None, false, detail),
        };
        self.note(name, ok, detail, started.elapsed());
        value
    }

    /// Records a step that was decided without waiting.
    pub(crate) fn note(
        &mut self,
        name: &str,
        ok: bool,
        detail: impl Into<String>,
        elapsed: Duration,
    ) {
        self.steps.push(ProbeStep {
            name: name.to_owned(),
            ok,
            detail: detail.into(),
            millis: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        });
    }

    /// The sentence to report when every step succeeded.
    pub(crate) fn succeeded(&mut self, summary: impl Into<String>) {
        self.success = Some(summary.into());
    }

    fn into_report(self) -> ProbeReport {
        let failed = self.steps.iter().find(|step| !step.ok);
        let (ok, summary) = match failed {
            Some(step) => (
                false,
                format!(
                    "The {} step failed: {}.",
                    step.name,
                    step.detail.trim_end_matches('.')
                ),
            ),
            None => (
                !self.steps.is_empty(),
                self.success
                    .unwrap_or_else(|| "The endpoint could not be probed.".to_owned()),
            ),
        };
        ProbeReport {
            ok,
            steps: self.steps,
            summary,
        }
    }
}
