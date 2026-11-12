//! Execution of single steps (scenarios.md §7). Each step returns `Ok(note)` or an `Err`
//! phrased as expected versus observed.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use packie_ipc::messages::{CloseRequest, ResendMode, ResendRequest, ResendTargets};
use packie_ipc::{ChannelCommand, ErrorCode, IpcError, Request, Response, TestHook};
use serde_json::Value;
use tokio::time::Instant;

use crate::actors::RtActor;
use crate::actors::conn::{Conn, Waited};
use crate::actors::folder::{Selector, content_rule};
use crate::actors::tcp::{ConnectOutcome, TcpServer};
use crate::engine::{IpcFailure, event_name, log_field};
use crate::matcher::{Matcher, check_all};
use crate::payload::preview;
use crate::placeholders::Placeholders;
use crate::protocol;
use crate::runner::Run;
use crate::scenario::{ENGINE_ACTOR, Step};
use crate::snapshot_view::{ViewTarget, alerts_view, view};
use crate::steps::{
    AcceptArgs, AcceptExpect, Action, ApplyConfigArgs, ApplyExpect, AssertAlertArgs, AssertLogArgs,
    AssertSnapshotArgs, AssertStoreArgs, AwaitExpect, ChannelCommandArgs, CloseDeliveryArgs,
    CommandExpect, ConnectExpect, Destinations, ResendArgs, ResendMode as StepResendMode, Table,
    Target, ValidateConfigArgs, ValidateExpect,
};
use crate::store;

/// A step's result: an optional note on success, the failure text otherwise.
pub(crate) type StepResult = Result<Option<String>, String>;

/// Interval at which assertion steps re-read the engine's state: short enough that an
/// assertion passes soon after its condition holds, long enough not to flood the engine
/// with snapshot requests from many parallel scenarios.
const POLL: Duration = Duration::from_millis(50);

fn ms(value: Option<u64>, default: u64) -> Duration {
    Duration::from_millis(value.unwrap_or(default))
}

/// Executes `step` against the run.
pub(crate) async fn run_step(run: &mut Run, step: &Step) -> StepResult {
    if step.actor == ENGINE_ACTOR && !matches!(step.action, Action::Wait(_)) {
        engine_step(run, step).await
    } else {
        actor_step(run, step).await
    }
}

/// Steps addressed to a scripted actor.
async fn actor_step(run: &mut Run, step: &Step) -> StepResult {
    let step_timeout = run.scenario.defaults.step_timeout_ms;
    match &step.action {
        Action::Listen(_) | Action::StopListening(_) | Action::Accept(_) => {
            server_step(run, step).await
        }
        Action::Connect(_) => connect_step(run, step).await,
        Action::Wait(a) => {
            tokio::time::sleep(Duration::from_millis(a.ms)).await;
            Ok(None)
        }
        Action::WriteFile(_)
        | Action::ExpectFile(_)
        | Action::ExpectNoFile(_)
        | Action::DeleteFile(_) => folder_step(run, step, step_timeout).await,
        Action::AwaitSend(a) => await_send(run, a, step_timeout).await,
        _ => connection_step(run, step, step_timeout).await,
    }
}

/// Steps addressed to the engine pseudo-actor.
async fn engine_step(run: &mut Run, step: &Step) -> StepResult {
    match &step.action {
        Action::StartEngine(_) => {
            run.engine.start(crate::runner::ENGINE_START).await?;
            Ok(None)
        }
        Action::StopEngine(_) => {
            let stopped = run.engine.stop().await;
            global_checks(run)?;
            stopped.map(|()| None)
        }
        Action::CrashEngine(_) => {
            run.engine.crash().await?;
            global_checks(run)?;
            Ok(None)
        }
        Action::PauseChannel(a) => channel_command(run, ChannelCommand::Pause, a).await,
        Action::ResumeChannel(a) => channel_command(run, ChannelCommand::Resume, a).await,
        Action::RestartChannel(a) => channel_command(run, ChannelCommand::Restart, a).await,
        Action::Resend(a) => resend(run, a).await,
        Action::CloseDelivery(a) => close_delivery(run, a).await,
        Action::HoldStoreCommits(_) => test_hook(run, TestHook::HoldStoreCommits).await,
        Action::ReleaseStoreCommits(_) => test_hook(run, TestHook::ReleaseStoreCommits).await,
        Action::ValidateConfig(a) => validate_config(run, a).await,
        Action::ApplyConfig(a) => apply_config(run, a).await,
        _ => assertion_step(run, step).await,
    }
}

/// The assertion steps.
async fn assertion_step(run: &Run, step: &Step) -> StepResult {
    let within = run.scenario.defaults.assert_within_ms;
    match &step.action {
        Action::AssertSnapshot(a) => assert_snapshot(run, a, within).await,
        Action::AssertAlert(a) => assert_alert(run, a, within).await,
        Action::AssertLog(a) => assert_log(run, a, within).await,
        Action::AssertStore(a) => assert_store(run, a, within).await,
        other => Err(format!("`{}` is not an engine step", other.name())),
    }
}

fn global_checks(run: &Run) -> Result<(), String> {
    let failures = run.engine.global_failures(run.store_errors_allowed);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("global assertion failed: {}", failures.join("; ")))
    }
}

fn conn_of(run: &Run, actor: &str) -> Result<Arc<Conn>, String> {
    run.actors
        .get(actor)
        .and_then(RtActor::conn)
        .ok_or_else(|| {
            format!("actor `{actor}` has no current connection (use accept or connect first)")
        })
}

// ------------------------------------------------------------------------- servers

async fn server_step(run: &Run, step: &Step) -> StepResult {
    let Some(RtActor::Server(server)) = run.actors.get(&step.actor) else {
        return Err(format!("`{}` is not a tcp_server", step.actor));
    };
    match &step.action {
        Action::Listen(_) => server
            .listen()
            .map(|()| Some(format!("listening on {}:{}", server.host(), server.port())))
            .map_err(|e| e.to_string()),
        Action::StopListening(_) => server
            .stop_listening()
            .await
            .map(|()| None)
            .map_err(|e| e.to_string()),
        Action::Accept(a) => {
            let wait = ms(a.timeout_ms, run.scenario.defaults.step_timeout_ms);
            accept_step(server, a, wait).await
        }
        _ => Err("not a server step".into()),
    }
}

async fn accept_step(server: &TcpServer, a: &AcceptArgs, wait: Duration) -> StepResult {
    let accepted = server.accept(wait).await.map_err(|e| e.to_string())?;
    let Some((_, peer)) = accepted else {
        return match a.expect {
            AcceptExpect::None => Ok(None),
            AcceptExpect::Connection => Err(format!(
                "expected a connection within {} ms, none arrived (listening on {}:{})",
                wait.as_millis(),
                server.host(),
                server.port()
            )),
        };
    };
    if a.expect == AcceptExpect::None {
        return Err(format!(
            "expected no connection, but one arrived from {peer}"
        ));
    }
    if let Some(want) = &a.expect_peer {
        let observed = if want.contains(':') {
            peer.to_string()
        } else {
            peer.ip().to_string()
        };
        if &observed != want {
            return Err(format!(
                "expected a connection from {want}, observed {observed}"
            ));
        }
    }
    Ok(Some(format!("connection from {peer}")))
}

async fn connect_step(run: &Run, step: &Step) -> StepResult {
    let Action::Connect(a) = &step.action else {
        return Err("not a connect step".into());
    };
    match run.actors.get(&step.actor) {
        Some(RtActor::Client(client)) => {
            let (Some(host), Some(port)) = (&a.host, a.port) else {
                return Err("connect needs host and port".into());
            };
            let wait = Duration::from_millis(run.scenario.defaults.step_timeout_ms);
            match (
                client
                    .connect(host, port, wait)
                    .await
                    .map_err(|e| e.to_string())?,
                a.expect,
            ) {
                (ConnectOutcome::Connected, ConnectExpect::Ok)
                | (ConnectOutcome::Refused, ConnectExpect::Refused) => Ok(None),
                (ConnectOutcome::Refused, ConnectExpect::Ok) => Err(format!(
                    "expected {host}:{port} to accept the connection, it was refused"
                )),
                (ConnectOutcome::Connected, ConnectExpect::Refused) => {
                    if let Some(conn) = client.current() {
                        conn.close(true).await;
                    }
                    Err(format!(
                        "expected {host}:{port} to refuse the connection, it was accepted"
                    ))
                }
            }
        }
        Some(RtActor::Serial(peer)) => peer.connect().map(|()| None).map_err(|e| e.to_string()),
        _ => Err(format!("`{}` cannot connect", step.actor)),
    }
}

// ---------------------------------------------------------------------- connections

#[allow(clippy::too_many_lines)]
async fn connection_step(run: &mut Run, step: &Step, step_timeout: u64) -> StepResult {
    let wait = |t: Option<u64>| ms(t, step_timeout);
    // `close` on a serial peer unplugs; it does not need a current connection.
    if let (Action::Close(_), Some(RtActor::Serial(peer))) =
        (&step.action, run.actors.get(&step.actor))
    {
        peer.unplug().await;
        return Ok(None);
    }
    let conn = conn_of(run, &step.actor)?;
    match &step.action {
        Action::Send(a) => {
            let bytes = a.payload().bytes().map_err(|e| e.to_string())?;
            let delay = Duration::from_millis(a.chunk_delay_ms.unwrap_or(0));
            let chunk = a.chunk_size;
            if a.is_async {
                let name = a.name.clone().unwrap_or_default();
                let task = tokio::spawn(async move {
                    conn.write(&bytes, chunk, delay)
                        .await
                        .map_err(|e| format!("write failed: {e}"))
                });
                run.ops.insert(name, task);
                Ok(None)
            } else {
                conn.write(&bytes, chunk, delay)
                    .await
                    .map_err(|e| format!("write failed: {e}"))?;
                Ok(Some(format!("sent {} bytes", bytes.len())))
            }
        }
        Action::SendMessage(a) => {
            let default = Duration::from_millis(step_timeout);
            if a.is_async {
                let (args, name) = (a.clone(), a.name.clone().unwrap_or_default());
                run.ops.insert(
                    name,
                    tokio::spawn(
                        async move { protocol::send_message(&conn, &args, default).await },
                    ),
                );
                Ok(None)
            } else {
                protocol::send_message(&conn, a, default)
                    .await
                    .map(|()| None)
            }
        }
        Action::Expect(a) => {
            let bytes = a.payload().bytes().map_err(|e| e.to_string())?;
            conn.expect_bytes(&bytes, wait(a.timeout_ms))
                .await
                .map(|()| None)
        }
        Action::ExpectMessage(a) => protocol::expect_message(&conn, a, wait(a.timeout_ms))
            .await
            .map(|()| None),
        Action::ExpectClosed(a) => expect_closed(&conn, wait(a.timeout_ms)).await,
        Action::ExpectOpen(a) => {
            expect_open(&conn, Duration::from_millis(a.for_ms.unwrap_or(0))).await
        }
        Action::ExpectNoData(a) => {
            expect_no_data(&conn, Duration::from_millis(a.for_ms.unwrap_or(0))).await
        }
        Action::PauseReading(_) => {
            conn.set_reading(false);
            Ok(None)
        }
        Action::ResumeReading(_) => {
            conn.set_reading(true);
            Ok(None)
        }
        Action::Close(a) => {
            conn.close(a.abort).await;
            Ok(None)
        }
        _ => Err("not a connection step".into()),
    }
}

async fn expect_closed(conn: &Conn, wait: Duration) -> StepResult {
    match conn
        .wait_until(Instant::now() + wait, |data, state| {
            (!state.is_open()).then(|| (data.len(), state.clone()))
        })
        .await
    {
        Waited::Ready((0, state)) => Ok(Some(state.to_string())),
        Waited::Ready((n, state)) => Err(format!(
            "{n} bytes before close ({state}): {}",
            preview(&conn.peek(120), 120)
        )),
        Waited::TimedOut => Err(format!(
            "expected the connection to end within {} ms, it is still open ({} bytes buffered)",
            wait.as_millis(),
            conn.buffered()
        )),
    }
}

async fn expect_open(conn: &Conn, window: Duration) -> StepResult {
    if !conn.state().is_open() {
        return Err(format!(
            "expected the connection to be open, it is {}",
            conn.state()
        ));
    }
    match conn
        .wait_until(Instant::now() + window, |_, state| {
            (!state.is_open()).then(|| state.clone())
        })
        .await
    {
        Waited::Ready(state) => Err(format!(
            "expected the connection to stay open for {} ms, it became {state}",
            window.as_millis()
        )),
        Waited::TimedOut => Ok(None),
    }
}

async fn expect_no_data(conn: &Conn, window: Duration) -> StepResult {
    match conn
        .wait_until(Instant::now() + window, |data, state| {
            (!data.is_empty() || !state.is_open()).then(|| (data.len(), state.clone()))
        })
        .await
    {
        Waited::Ready((0, state)) => Err(format!(
            "expected the connection to stay open and quiet, it became {state}"
        )),
        Waited::Ready((n, _)) => Err(format!(
            "expected no data for {} ms, {n} bytes arrived: {}",
            window.as_millis(),
            preview(&conn.peek(120), 120)
        )),
        Waited::TimedOut => Ok(None),
    }
}

async fn await_send(
    run: &mut Run,
    a: &crate::steps::AwaitSendArgs,
    step_timeout: u64,
) -> StepResult {
    let Some(op) = run.ops.get_mut(&a.name) else {
        return Err(format!(
            "no asynchronous operation `{}` is outstanding",
            a.name
        ));
    };
    match a.expect {
        AwaitExpect::Complete => {
            let limit = ms(a.timeout_ms, step_timeout);
            match tokio::time::timeout(limit, &mut *op).await {
                Err(_) => Err(format!(
                    "`{}` has not completed after {} ms",
                    a.name,
                    limit.as_millis()
                )),
                Ok(joined) => {
                    run.ops.remove(&a.name);
                    joined
                        .map_err(|e| format!("`{}` failed: {e}", a.name))?
                        .map(|()| None)
                }
            }
        }
        AwaitExpect::Pending => {
            let window = Duration::from_millis(a.for_ms.unwrap_or(0));
            match tokio::time::timeout(window, &mut *op).await {
                Err(_) => Ok(None),
                Ok(joined) => {
                    run.ops.remove(&a.name);
                    let result = joined.map_err(|e| e.to_string()).and_then(|r| r);
                    Err(format!(
                        "expected `{}` to stay pending for {} ms, but it finished ({})",
                        a.name,
                        window.as_millis(),
                        result.map_or_else(|e| format!("failed: {e}"), |()| "completed".to_owned())
                    ))
                }
            }
        }
    }
}

// ----------------------------------------------------------------------- folders

async fn folder_step(run: &Run, step: &Step, step_timeout: u64) -> StepResult {
    let Some(RtActor::Folder(folder)) = run.actors.get(&step.actor) else {
        return Err(format!("`{}` is not a folder", step.actor));
    };
    match &step.action {
        Action::WriteFile(a) => {
            let bytes = a.payload().bytes().map_err(|e| e.to_string())?;
            folder.write_file(&a.name, &bytes, a.atomic)?;
            Ok(Some(format!("wrote {} bytes to {}", bytes.len(), a.name)))
        }
        Action::DeleteFile(a) => folder.delete_file(&a.name).map(|()| None),
        Action::ExpectNoFile(a) => folder
            .expect_no_file(&a.pattern, Duration::from_millis(a.for_ms.unwrap_or(0)))
            .await
            .map(|()| None),
        Action::ExpectFile(a) => {
            let selector = match (&a.name, &a.pattern) {
                (Some(n), _) => Selector::Name(n.clone()),
                (None, Some(p)) => Selector::Pattern(p.clone()),
                (None, None) => return Err("expect_file needs name or pattern".into()),
            };
            let rule = content_rule(a.equals.as_ref(), a.contains.as_ref())?;
            folder
                .expect_file(&selector, &rule, ms(a.timeout_ms, step_timeout))
                .await
                .map(|found| Some(format!("found {found}")))
        }
        _ => Err("not a folder step".into()),
    }
}

// ----------------------------------------------------------------- engine commands

/// Reads a runtime document from an inline value or a file next to the scenario, with
/// placeholders substituted.
pub(crate) fn load_runtime_doc(
    inline: Option<&Value>,
    file: Option<&str>,
    base: &Path,
    placeholders: &Placeholders,
) -> Result<Value, String> {
    let doc = match (inline, file) {
        (Some(v), None) => v.clone(),
        (None, Some(f)) => {
            let path = base.join(f);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let value: Value =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            placeholders.substitute(&value, f)?
        }
        _ => return Err("exactly one of runtime and runtime_file is required".into()),
    };
    Ok(doc)
}

/// The stable reason code of a rejection, as text.
fn reason_of(error: &IpcError) -> Option<String> {
    match serde_json::to_value(error.reason_code) {
        Ok(Value::String(code)) => Some(code),
        _ => None,
    }
}

/// Whether a rejection carries `code`: as its reason code, or (for engines that only
/// phrase it) in its text.
fn names_code(error: &IpcError, code: &str) -> bool {
    if let Some(reason) = reason_of(error) {
        return reason == code;
    }
    let spaced = code.replace('_', " ").to_lowercase();
    error.message.to_lowercase().contains(&spaced) || error.message.contains(code)
}

fn describe_rejection(error: &IpcError) -> String {
    format!(
        "{:?} reason {}: {}",
        error.code,
        reason_of(error).unwrap_or_else(|| "<none>".into()),
        error.message
    )
}

async fn channel_command(run: &Run, command: ChannelCommand, a: &ChannelCommandArgs) -> StepResult {
    let channel_id = a.channel.parse::<packie_core::ChannelId>().map_err(|_| {
        format!(
            "channel id `{}` cannot be sent: the engine's IPC DTOs use UUIDv7 channel ids, the scenario uses a string id",
            a.channel
        )
    })?;
    let outcome = run
        .engine
        .call(Request::ChannelCommand {
            channel_id,
            command,
        })
        .await;
    match (outcome, a.expect) {
        (Ok(_), CommandExpect::Accepted) => Ok(None),
        (Ok(_), CommandExpect::Rejected) => Err(format!(
            "expected {command:?} of `{}` to be rejected{}, the engine accepted it",
            a.channel,
            a.reason_code
                .as_ref()
                .map(|c| format!(" with {c}"))
                .unwrap_or_default()
        )),
        (Err(IpcFailure::Server(e)), CommandExpect::Rejected) => match &a.reason_code {
            Some(code) if !names_code(&e, code) => Err(format!(
                "rejected, but not with reason `{code}`: observed {}",
                describe_rejection(&e)
            )),
            _ => Ok(Some(format!("rejected: {}", e.message))),
        },
        (Err(e), CommandExpect::Accepted) => Err(format!(
            "expected {command:?} of `{}` to be accepted, observed: {e}",
            a.channel
        )),
        (Err(e), CommandExpect::Rejected) => Err(format!("expected a rejection, observed: {e}")),
    }
}

/// A problem of a document as `(severity, code, path, message)`.
type ProblemView = (String, String, String, String);

fn problem_view(problem: &impl serde::Serialize) -> ProblemView {
    let value = serde_json::to_value(problem).unwrap_or(Value::Null);
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    (
        text("severity"),
        text("code"),
        text("path"),
        text("message"),
    )
}

fn show_problems(problems: &[ProblemView]) -> String {
    problems
        .iter()
        .map(|(sev, code, path, message)| format!("{sev} {code} {path}: {message}"))
        .collect::<Vec<_>>()
        .join("\n    ")
}

/// The codes in `wanted` that no problem carries.
fn missing_problems(wanted: &[String], problems: &[ProblemView]) -> StepResult {
    let missing: Vec<&String> = wanted
        .iter()
        .filter(|code| !problems.iter().any(|(_, c, _, _)| c == *code))
        .collect();
    if missing.is_empty() {
        Ok(None)
    } else {
        Err(format!(
            "expected problems {missing:?}; observed:\n    {}",
            show_problems(problems)
        ))
    }
}

async fn validate_config(run: &Run, a: &ValidateConfigArgs) -> StepResult {
    let document = load_runtime_doc(
        a.runtime.as_ref(),
        a.runtime_file.as_deref(),
        &run.base_dir,
        &run.alloc.placeholders,
    )?;
    let response = run
        .engine
        .call(Request::ValidateConfig { document })
        .await
        .map_err(|e| e.to_string())?;
    let Response::Validation { problems, .. } = response else {
        return Err("unexpected response to validate_config".into());
    };
    let problems: Vec<ProblemView> = problems.iter().map(problem_view).collect();
    let errors: Vec<ProblemView> = problems
        .iter()
        .filter(|p| p.0 == "error")
        .cloned()
        .collect();
    match a.expect {
        ValidateExpect::Valid if errors.is_empty() => Ok(None),
        ValidateExpect::Valid => Err(format!(
            "expected the document to be valid, observed {} error(s):\n    {}",
            errors.len(),
            show_problems(&errors)
        )),
        ValidateExpect::Invalid if errors.is_empty() => {
            Err("expected the document to be invalid, the engine found no error".into())
        }
        ValidateExpect::Invalid => missing_problems(&a.problems, &errors),
    }
}

async fn apply_config(run: &Run, a: &ApplyConfigArgs) -> StepResult {
    let document = load_runtime_doc(
        a.runtime.as_ref(),
        a.runtime_file.as_deref(),
        &run.base_dir,
        &run.alloc.placeholders,
    )?;
    let snapshot = run.engine.snapshot().await?;
    let base_version = snapshot["node"]["active_config_version"]
        .as_u64()
        .or_else(|| snapshot["node"]["config"]["active_version"].as_u64())
        .unwrap_or(0);
    let outcome = run
        .engine
        .call(Request::PutConfig {
            document,
            base_version,
            note: Some("scenario apply".to_owned()),
        })
        .await;
    match (outcome, a.expect) {
        (
            Ok(Response::ConfigApplied {
                result, outcomes, ..
            }),
            expected,
        ) => {
            let result = serde_json::to_value(result).unwrap_or(Value::Null);
            let outcomes = serde_json::to_value(&outcomes).unwrap_or(Value::Null);
            check_applied(expected, &result, &outcomes, &a.outcomes)
        }
        (Ok(_), _) => Err("unexpected response to apply_config".into()),
        (Err(IpcFailure::Server(e)), ApplyExpect::Rejected)
            if e.code == ErrorCode::InvalidConfig =>
        {
            let problems: Vec<ProblemView> = e.field_errors.iter().map(problem_view).collect();
            missing_problems(&a.problems, &problems).map_err(|m| format!("rejected, but {m}"))
        }
        (Err(e), expected) => Err(format!(
            "expected the apply to be {expected:?}, observed: {e}"
        )),
    }
}

/// Compares the engine's apply result and per-channel outcomes with the step.
fn check_applied(
    expected: ApplyExpect,
    result: &Value,
    outcomes: &Value,
    wanted: &std::collections::BTreeMap<String, String>,
) -> StepResult {
    let want = match expected {
        ApplyExpect::Applied => "applied",
        ApplyExpect::Rejected => "rejected",
        ApplyExpect::Partial => "partial",
    };
    if result.as_str() != Some(want) {
        return Err(format!(
            "expected the apply to be {want}, observed {result}"
        ));
    }
    let list = outcomes.as_array().cloned().unwrap_or_default();
    let mut problems = Vec::new();
    for (channel, outcome) in wanted {
        let found = list
            .iter()
            .find(|c| c["channel_id"].as_str() == Some(channel.as_str()));
        match found.and_then(|c| c["outcome"].as_str()) {
            None => problems.push(format!(
                "{channel}: expected {outcome}, the engine reports no outcome for it"
            )),
            Some(got) if got != outcome => {
                problems.push(format!("{channel}: expected {outcome}, observed {got}"))
            }
            Some(_) => {}
        }
    }
    if problems.is_empty() {
        Ok(None)
    } else {
        Err(format!(
            "channel outcomes differ:\n    {}",
            problems.join("\n    ")
        ))
    }
}

// ---------------------------------------------------------------------- assertions

/// Polls `check` every 50 ms until it returns `Ok` or `within` passes; on timeout the
/// last failure is returned with the window.
async fn poll<F, Fut>(within: Duration, mut check: F) -> StepResult
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let deadline = Instant::now() + within;
    loop {
        match check().await {
            Ok(()) => return Ok(None),
            Err(why) if Instant::now() >= deadline => {
                return Err(if within.is_zero() {
                    why
                } else {
                    format!("{why}\n    (polled for {} ms)", within.as_millis())
                });
            }
            Err(_) => tokio::time::sleep(POLL).await,
        }
    }
}

async fn assert_snapshot(run: &Run, a: &AssertSnapshotArgs, default_within: u64) -> StepResult {
    let target = match &a.target {
        Target::Channel { channel } => ViewTarget::Channel(channel),
        Target::Endpoint { endpoint } => ViewTarget::Endpoint(endpoint),
        Target::Node { .. } => ViewTarget::Node,
    };
    poll(ms(a.within_ms, default_within), || async {
        let snapshot = run.engine.snapshot().await?;
        let document = view(&snapshot, &target)?;
        let failures = check_all(&document, a.expect.iter());
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n    "))
        }
    })
    .await
}

async fn assert_alert(run: &Run, a: &AssertAlertArgs, default_within: u64) -> StepResult {
    poll(ms(a.within_ms, default_within), || async {
        let snapshot = run.engine.snapshot().await?;
        let alerts = alerts_view(&snapshot);
        let scoped: Vec<&Value> = alerts
            .iter()
            .filter(|al| al["code"] == a.code.as_str() && al["state"] == "active")
            .filter(|al| scope_matches(al, "channel_id", a.scope.channel.as_deref()))
            .filter(|al| scope_matches(al, "endpoint_id", a.scope.endpoint.as_deref()))
            .filter(|al| scope_matches(al, "destination_id", a.scope.destination.as_deref()))
            .collect();
        let active: Vec<String> = alerts
            .iter()
            .map(|al| {
                format!(
                    "{} {}",
                    al["code"].as_str().unwrap_or("?"),
                    al["channel_id"].as_str().unwrap_or("")
                )
            })
            .collect();
        if !a.present {
            return if scoped.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "expected no active alert `{}`, one is active",
                    a.code
                ))
            };
        }
        let Some(first) = scoped
            .iter()
            .find(|al| check_all(al, a.expect.iter()).is_empty())
        else {
            let detail = scoped
                .first()
                .map(|al| check_all(al, a.expect.iter()).join("; "))
                .unwrap_or_default();
            return Err(format!(
                "expected an active alert `{}` with scope {:?}{}; active alerts: [{}]",
                a.code,
                (&a.scope.channel, &a.scope.endpoint, &a.scope.destination),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" matching the checks, but {detail}")
                },
                active.join(", ")
            ));
        };
        let _ = first;
        Ok(())
    })
    .await
}

fn scope_matches(alert: &Value, field: &str, want: Option<&str>) -> bool {
    want.is_none_or(|w| alert[field].as_str() == Some(w))
}

async fn assert_log(run: &Run, a: &AssertLogArgs, default_within: u64) -> StepResult {
    let count = a
        .count
        .clone()
        .unwrap_or_else(|| Matcher::Gte(Value::from(1)));
    poll(ms(a.within_ms, default_within), || async {
        run.engine.with_log_events(|events| {
            let named: Vec<&Value> = events
                .iter()
                .filter(|e| event_name(e) == Some(a.event.as_str()))
                .collect();
            let matching = named
                .iter()
                .filter(|e| {
                    a.fields
                        .iter()
                        .all(|(f, m)| m.check(log_field(e, f).as_ref()).is_ok())
                })
                .count();
            match count.check(Some(&Value::from(matching))) {
                Ok(()) => Ok(()),
                Err(why) => {
                    let sample = named
                        .iter()
                        .take(2)
                        .map(|e| (*e).to_string())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    Err(format!(
                        "event `{}` with the given fields: count {why}; {} event(s) of that name{}",
                        a.event,
                        named.len(),
                        if sample.is_empty() {
                            String::new()
                        } else {
                            format!(", for example {sample}")
                        }
                    ))
                }
            }
        })
    })
    .await
}

async fn assert_store(run: &Run, a: &AssertStoreArgs, default_within: u64) -> StepResult {
    let count = a
        .count
        .clone()
        .unwrap_or_else(|| Matcher::Gte(Value::from(1)));
    let count = &count;
    let with_bodies = store::needs_bodies(a.where_.keys().chain(a.each.keys()));
    poll(ms(a.within_ms, default_within), || async move {
        let rows = store::load_rows(&run.engine, a.table, with_bodies).await?;
        store::evaluate(&rows, &a.where_, count, &a.each)
    })
    .await
}

async fn test_hook(run: &Run, hook: TestHook) -> StepResult {
    match run.engine.call(Request::TestHook { hook }).await {
        Ok(Response::Done) => Ok(None),
        Ok(_) => Err("unexpected response to the test hook".into()),
        Err(e) => Err(format!(
            "test hook {hook:?} failed: {e} (does the scenario list `test_hooks` in `requires`?)"
        )),
    }
}

/// The single row of `table` in `channel` that satisfies `selector`.
async fn select_one(
    run: &Run,
    table: Table,
    channel: &str,
    selector: &std::collections::BTreeMap<String, Matcher>,
) -> Result<store::Row, String> {
    let rows = store::load_rows(&run.engine, table, store::needs_bodies(selector.keys())).await?;
    let mut hits: Vec<store::Row> = rows
        .into_iter()
        .filter(|r| r.get("channel_id").and_then(Value::as_str) == Some(channel))
        .filter(|r| store::row_mismatches(r, selector).is_empty())
        .collect();
    match hits.len() {
        1 => Ok(hits.remove(0)),
        n => Err(format!(
            "the selector must match exactly one row in channel `{channel}`, it matches {n}"
        )),
    }
}

fn id_of<T: std::str::FromStr>(row: &store::Row, field: &str) -> Result<T, String> {
    let text = row.get(field).and_then(Value::as_str).unwrap_or_default();
    text.parse()
        .map_err(|_| format!("`{field}` value `{text}` is not an id this client can send"))
}

async fn resend(run: &Run, a: &ResendArgs) -> StepResult {
    let row = select_one(run, Table::Messages, &a.channel, &a.message).await?;
    let message_id: packie_core::MessageId = id_of(&row, "message_id")?;
    let destinations = match &a.destinations {
        Destinations::All => ResendTargets::All,
        Destinations::List(names) => ResendTargets::Destinations(
            names
                .iter()
                .map(|n| n.parse::<packie_core::ConnectorId>().map_err(|_| format!("destination id `{n}` cannot be sent: the engine's IPC DTOs use UUIDv7 ids")))
                .collect::<Result<_, _>>()?,
        ),
    };
    let mode = match a.mode {
        StepResendMode::SameMessage => ResendMode::SameMessage,
        StepResendMode::NewMessage => ResendMode::NewMessage,
    };
    let outcome = run
        .engine
        .call(Request::Resend(ResendRequest {
            message_id,
            destinations,
            mode,
            confirm_duplicate_risk: a.confirm_duplicate_risk,
            // The `resend` step has no reason argument.
            reason: None,
            confirm_superseded: false,
        }))
        .await;
    let rejected_codes: Vec<String> = match &outcome {
        Ok(Response::Resent(result)) => serde_json::to_value(result)
            .ok()
            .and_then(|v| v["destinations"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|d| d["rejected"]["code"].as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    match (outcome, a.expect) {
        (Ok(_), CommandExpect::Accepted) if rejected_codes.is_empty() => Ok(None),
        (Ok(_), CommandExpect::Accepted) => Err(format!(
            "expected the resend to be accepted, destinations were rejected: {rejected_codes:?}"
        )),
        (Ok(_), CommandExpect::Rejected) => match &a.reason_code {
            Some(code) if rejected_codes.contains(code) => Ok(None),
            None if !rejected_codes.is_empty() => Ok(None),
            _ => Err(format!(
                "expected a rejection{}, the engine accepted it (rejected destinations: {rejected_codes:?})",
                a.reason_code
                    .as_ref()
                    .map(|c| format!(" with {c}"))
                    .unwrap_or_default()
            )),
        },
        (Err(IpcFailure::Server(e)), CommandExpect::Rejected) => match &a.reason_code {
            Some(code) if !names_code(&e, code) => Err(format!(
                "rejected, but not with reason `{code}`: observed {}",
                describe_rejection(&e)
            )),
            _ => Ok(None),
        },
        (Err(e), CommandExpect::Accepted) => {
            Err(format!("expected the resend to be accepted, observed: {e}"))
        }
        (Err(e), CommandExpect::Rejected) => Err(format!("expected a rejection, observed: {e}")),
    }
}

async fn close_delivery(run: &Run, a: &CloseDeliveryArgs) -> StepResult {
    let row = select_one(run, Table::Deliveries, &a.channel, &a.delivery).await?;
    let delivery_id: packie_core::DeliveryId = id_of(&row, "delivery_id")?;
    let outcome = run
        .engine
        .call(Request::CloseDelivery(CloseRequest {
            delivery_id,
            reason: a.reason.clone(),
        }))
        .await;
    match (outcome, a.expect) {
        (Ok(_), CommandExpect::Accepted) => Ok(None),
        (Ok(_), CommandExpect::Rejected) => {
            Err("expected the close to be rejected, the engine accepted it".into())
        }
        (Err(IpcFailure::Server(e)), CommandExpect::Rejected) => match &a.reason_code {
            Some(code) if !names_code(&e, code) => Err(format!(
                "rejected, but not with reason `{code}`: observed {}",
                describe_rejection(&e)
            )),
            _ => Ok(None),
        },
        (Err(e), CommandExpect::Accepted) => {
            Err(format!("expected the close to be accepted, observed: {e}"))
        }
        (Err(e), CommandExpect::Rejected) => Err(format!("expected a rejection, observed: {e}")),
    }
}
