//! The engine under test: bootstrap generation, process launch, management interface
//! session, log collection and global checks.
//!
//! The engine runs as a child process (`packie run --bootstrap <file>`). The runner talks to
//! it only through `packie-ipc` and reads the files in the state directory, so the same steps
//! would work against an in-process engine.
//!
//! Test hooks: a scenario that `requires` `test_hooks` gets `"test_hooks": true` in its
//! bootstrap. Whether the engine build has them is probed once per binary
//! ([`probe_capabilities`]) from the `capabilities` of its `hello` answer; without them the
//! scenario is skipped. `crash_engine` kills the process and needs no hook.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use packie_diagnostics::LogFilter;
use packie_ipc::{ClientError, Event, IpcClient, IpcError, Request, Response, Topic};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::actors::conn::lock;
use crate::payload::{encode_base64, encode_hex};

/// How many engine output lines are kept for failure reports.
const TAIL_LINES: usize = 400;
/// Time allowed for one management request. Requests are answered from memory or one
/// SQLite read, so a healthy engine answers in milliseconds; ten seconds covers an engine
/// starved by a loaded CI host and still reports a hung one well inside a scenario's timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Extra time after the configured shutdown drain before a stopping engine is killed: the
/// drain is the engine's own budget for finishing deliveries, the rest is process exit and
/// flushing the log file.
const EXIT_GRACE: Duration = Duration::from_secs(10);
/// How long the log pump may keep reading after the engine ended: it only has to drain
/// events already sent over the local socket.
const PUMP_DRAIN: Duration = Duration::from_secs(2);
/// Interval between connection attempts to a management socket that is not there yet.
const CONNECT_RETRY: Duration = Duration::from_millis(40);
/// Interval at which the log is checked for `node.started` during startup.
const STARTED_POLL: Duration = Duration::from_millis(25);
/// Pause that lets the engine's output pipes and log file settle after a process event
/// (an exit, a kill): the reader tasks and the file system are asynchronous to `wait`.
const SETTLE: Duration = Duration::from_millis(50);
/// Time allowed for the capability probe's engine to open its management socket.
const PROBE_START: Duration = Duration::from_secs(10);

/// Where and how the engine runs.
#[derive(Debug, Clone)]
pub struct EngineSpec {
    /// The `packie` binary.
    pub bin: PathBuf,
    /// The generated bootstrap file.
    pub bootstrap_path: PathBuf,
    /// The state directory.
    pub state_dir: PathBuf,
    /// The log directory.
    pub logs_dir: PathBuf,
    /// The management endpoint (Unix socket path or Windows named pipe).
    pub endpoint: String,
    /// The configured shutdown drain.
    pub drain: Duration,
}

/// A management request failed.
#[derive(Debug)]
pub enum IpcFailure {
    /// The engine answered with an error.
    Server(IpcError),
    /// The engine could not be reached or answered oddly.
    Transport(String),
}

impl std::fmt::Display for IpcFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(e) => write!(f, "{:?}: {}", e.code, e.message),
            Self::Transport(m) => f.write_str(m),
        }
    }
}

/// The sentinel strings of `no_body_leak` with their encodings.
#[derive(Debug, Clone, Default)]
pub struct Needles(Vec<(String, String)>);

impl Needles {
    /// Plain, hex (lower and upper case) and base64 forms of each sentinel.
    #[must_use]
    pub fn new(sentinels: &[String]) -> Self {
        let mut list = Vec::new();
        for s in sentinels {
            let hex = encode_hex(s.as_bytes());
            list.push((format!("\"{s}\""), s.clone()));
            list.push((format!("\"{s}\" (hex)"), hex.to_uppercase()));
            list.push((format!("\"{s}\" (hex)"), hex));
            list.push((format!("\"{s}\" (base64)"), encode_base64(s.as_bytes())));
        }
        Self(list)
    }

    /// The first sentinel form found in `haystack`.
    #[must_use]
    pub fn find(&self, haystack: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(_, form)| haystack.contains(form.as_str()))
            .map(|(label, _)| label.as_str())
    }
}

/// Log events and leaks gathered across every engine start of a run.
#[derive(Debug, Default)]
pub struct Collected {
    events: Mutex<Vec<Value>>,
    seen: Mutex<HashSet<(u32, u64)>>,
    leaks: Mutex<Vec<String>>,
    needles: Needles,
}

impl Collected {
    fn new(needles: Needles) -> Self {
        Self {
            needles,
            ..Self::default()
        }
    }

    /// Records a log event of start number `incarnation` once.
    fn add_event(&self, incarnation: u32, event: &packie_diagnostics::LogEvent) {
        if !lock(&self.seen).insert((incarnation, event.seq)) {
            return;
        }
        let Ok(value) = serde_json::to_value(event) else {
            return;
        };
        self.check_text("a log event", &value.to_string());
        lock(&self.events).push(value);
    }

    /// Records a leak when `text` contains a sentinel.
    pub fn check_text(&self, source: &str, text: &str) {
        if let Some(form) = self.needles.find(text) {
            lock(&self.leaks).push(format!("{form} appears in {source}"));
        }
    }

    /// Log events collected so far, oldest first.
    #[must_use]
    pub fn events(&self) -> Vec<Value> {
        lock(&self.events).clone()
    }

    /// Leaks found so far.
    #[must_use]
    pub fn leaks(&self) -> Vec<String> {
        lock(&self.leaks).clone()
    }
}

/// The name of a log event: `event` in a log file line, `fields.event` in an IPC event.
#[must_use]
pub fn event_name(event: &Value) -> Option<&str> {
    event
        .get("event")
        .or_else(|| event.get("fields")?.get("event"))?
        .as_str()
}

/// A log field by name: top-level keys first (file lines are flat, IPC events lift
/// `channel_id` and friends), then the fields of the enclosing spans of a file line
/// (innermost first; the channel span carries `channel_id`), then `fields`, with text
/// coerced to numbers and booleans.
#[must_use]
pub fn log_field(event: &Value, name: &str) -> Option<Value> {
    if name != "fields"
        && let Some(v) = event.get(name)
    {
        return Some(v.clone());
    }
    let from_spans = event
        .get("spans")
        .and_then(Value::as_array)
        .and_then(|spans| spans.iter().rev().find_map(|s| s.get(name)));
    if let Some(v) = from_spans {
        return Some(v.clone());
    }
    event
        .get("fields")?
        .get(name)
        .and_then(Value::as_str)
        .map(crate::matcher::coerce_text)
}

/// How much of a file's start identifies it, to notice that it was replaced.
const FILE_ID_BYTES: usize = 64;

#[derive(Debug, Default)]
struct FileState {
    offset: u64,
    /// The first bytes seen; a file whose start differs is a new file with the same name.
    prefix: Vec<u8>,
}

/// The JSON log files of the engine (`<logs>/packie.*.log`), read incrementally. They are the
/// durable record: they hold events logged after the management interface closed
/// (`node.stopped`). The engine starts a fresh file on every start, so events are
/// accumulated here across the run (`assert_log` counts "since the engine's first start").
#[derive(Debug, Default)]
struct LogFiles {
    dir: PathBuf,
    files: std::collections::HashMap<PathBuf, FileState>,
    events: Vec<Value>,
    /// Long runs read nothing: they keep no events in memory.
    lean: bool,
}

impl LogFiles {
    fn refresh(&mut self) {
        if self.lean {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("packie.") && n.contains(".log"))
            })
            .collect();
        paths.sort();
        for path in paths {
            if let Some(bytes) = self.read_new(&path) {
                self.add_lines(&bytes);
            }
        }
    }

    /// The complete lines of `path` not read before; `None` when nothing is new.
    fn read_new(&mut self, path: &Path) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).ok()?;
        let mut head = vec![0_u8; FILE_ID_BYTES];
        let got = file.read(&mut head).ok()?;
        head.truncate(got);
        let state = self.files.entry(path.to_owned()).or_default();
        let common = state.prefix.len().min(head.len());
        let replaced =
            state.prefix[..common] != head[..common] || file.metadata().ok()?.len() < state.offset;
        if replaced {
            *state = FileState::default();
        }
        if state.prefix.len() < head.len() {
            state.prefix = head;
        }
        file.seek(SeekFrom::Start(state.offset)).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        // Only complete lines; a partly written one is read again next time.
        let complete = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        state.offset += complete as u64;
        bytes.truncate(complete);
        (!bytes.is_empty()).then_some(bytes)
    }

    fn add_lines(&mut self, bytes: &[u8]) {
        for line in bytes.split(|b| *b == b'\n') {
            if let Ok(mut value) = serde_json::from_slice::<Value>(line) {
                normalize_level(&mut value);
                self.events.push(value);
            }
        }
    }
}

fn normalize_level(event: &mut Value) {
    if let Some(level) = event
        .get("level")
        .and_then(Value::as_str)
        .map(str::to_lowercase)
    {
        event["level"] = Value::String(level);
    }
}

/// Merges `over` into `base`, objects recursively.
fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

/// The bootstrap for a run: generated fields with the scenario's overrides merged over.
#[must_use]
pub fn bootstrap_document(
    scenario_name: &str,
    state: &Path,
    logs: &Path,
    runtime_file: &Path,
    test_hooks: bool,
    overrides: &Map<String, Value>,
) -> Value {
    let mut doc = json!({
        "schema_version": 1,
        "node": { "node_id": format!("sim-{scenario_name}"), "display_name": format!("Simulator {scenario_name}"), "environment": "test" },
        "paths": { "state_directory": state, "log_directory": logs },
        "hosting": { "shutdown_drain_seconds": 5 },
        "logging": { "level": "debug", "retention_days": 1, "ring_buffer_events": 100_000 },
        "runtime": { "file": runtime_file },
        "ipc": { "path": run_ipc_endpoint(state) },
    });
    if test_hooks && let Some(map) = doc.as_object_mut() {
        map.insert("test_hooks".into(), Value::Bool(true));
    }
    merge(&mut doc, &Value::Object(overrides.clone()));
    doc
}

/// A management endpoint isolated from other simulator runs, including concurrent runs
/// of the same scenario. The generated bootstrap persists it across engine restarts.
pub(crate) fn run_ipc_endpoint(state: &Path) -> String {
    if cfg!(windows) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        format!(
            r"\\.\pipe\packie-sim-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    } else {
        state.join("packie.sock").to_string_lossy().into_owned()
    }
}

/// Resolves the endpoint from the generated bootstrap after all scenario overrides.
/// A null `ipc.path` selects the engine's own platform default.
#[must_use]
pub(crate) fn bootstrap_endpoint(bootstrap: &Value) -> String {
    if let Some(path) = bootstrap["ipc"]["path"].as_str() {
        return path.to_owned();
    }
    if cfg!(windows) {
        format!(
            r"\\.\pipe\packie-{}",
            bootstrap["node"]["node_id"].as_str().unwrap_or_default()
        )
    } else {
        Path::new(
            bootstrap["paths"]["state_directory"]
                .as_str()
                .unwrap_or_default(),
        )
        .join("packie.sock")
        .to_string_lossy()
        .into_owned()
    }
}

/// The engine process and its management session.
#[derive(Debug)]
pub struct EngineHost {
    spec: EngineSpec,
    child: Option<Child>,
    tail: Arc<Mutex<VecDeque<String>>>,
    panic_seen: Arc<AtomicBool>,
    client: Option<Arc<IpcClient>>,
    pump: Option<JoinHandle<()>>,
    incarnation: u32,
    collected: Arc<Collected>,
    files: Mutex<LogFiles>,
    problems: Vec<String>,
    /// A long run: no log subscription, no events kept in memory.
    lean: bool,
}

impl EngineHost {
    /// A host for `spec`; nothing is started.
    #[must_use]
    pub fn new(spec: EngineSpec, needles: Needles) -> Self {
        let logs_dir = spec.logs_dir.clone();
        Self {
            spec,
            child: None,
            tail: Arc::new(Mutex::new(VecDeque::new())),
            panic_seen: Arc::new(AtomicBool::new(false)),
            client: None,
            pump: None,
            incarnation: 0,
            collected: Arc::new(Collected::new(needles)),
            files: Mutex::new(LogFiles {
                dir: logs_dir,
                ..LogFiles::default()
            }),
            problems: Vec::new(),
            lean: false,
        }
    }

    /// Makes the host suitable for runs of hours (`packie-sim soak`): the log is not
    /// subscribed to, so the engine serves no log stream and the host keeps no events in
    /// memory. The engine's own log files remain the record; start only waits for the
    /// first snapshot.
    pub fn set_lean(&mut self) {
        self.lean = true;
        lock(&self.files).lean = true;
    }

    /// Whether a `panicked at` line was printed on the engine's console.
    #[must_use]
    pub fn panic_seen(&self) -> bool {
        self.panic_seen.load(Ordering::SeqCst)
    }

    /// Abnormal stops noticed so far (an exit on its own, a stop that needed a kill).
    #[must_use]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The spec.
    #[must_use]
    pub fn spec(&self) -> &EngineSpec {
        &self.spec
    }

    /// Runs `f` over every event in the engine's log files since the first start of the run.
    pub fn with_log_events<R>(&self, f: impl FnOnce(&[Value]) -> R) -> R {
        let mut files = lock(&self.files);
        files.refresh();
        f(&files.events)
    }

    /// Collected log events and leaks.
    #[must_use]
    pub fn collected(&self) -> &Arc<Collected> {
        &self.collected
    }

    /// Whether the engine process is running.
    #[must_use]
    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// The process id of the running engine.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    /// The last `n` log events of the run as JSON lines of at most 400 characters, read
    /// from the engine's log files (they survive restarts and crashes).
    #[must_use]
    pub fn log_tail(&self, n: usize) -> Vec<String> {
        self.with_log_events(|events| {
            events
                .iter()
                .skip(events.len().saturating_sub(n))
                .map(|e| e.to_string().chars().take(400).collect())
                .collect()
        })
    }

    /// The last `n` lines the engine printed.
    #[must_use]
    pub fn recent_output(&self, n: usize) -> Vec<String> {
        let tail = lock(&self.tail);
        tail.iter()
            .skip(tail.len().saturating_sub(n))
            .cloned()
            .collect()
    }

    fn pump_output(&self, stream: impl AsyncRead + Unpin + Send + 'static) {
        tokio::spawn(pump_lines(
            stream,
            Arc::clone(&self.tail),
            Arc::clone(&self.panic_seen),
            Arc::clone(&self.collected),
        ));
    }

    /// Starts the engine and waits until it reports `node.started` and a first snapshot.
    pub async fn start(&mut self, within: Duration) -> Result<(), String> {
        if self.child.is_some() {
            return Err("the engine is already running".into());
        }
        // The previous run's log file is replaced at start; keep what it holds.
        lock(&self.files).refresh();
        self.incarnation += 1;
        let mut command = Command::new(&self.spec.bin);
        command
            .arg("run")
            .arg("--bootstrap")
            .arg(&self.spec.bootstrap_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("NO_COLOR", "1")
            .env_remove("PACKIE_BOOTSTRAP")
            .env_remove("PACKIE_IPC")
            .env_remove("PACKIE_LOG");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // The child keeps our console but has its own group, so Ctrl-Break stops
            // this engine without signalling the simulator or sibling scenarios.
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command
                .as_std_mut()
                .creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        let spawned = {
            let _guard = crate::fdguard::spawning();
            command.spawn()
        };
        let mut child =
            spawned.map_err(|e| format!("cannot start {}: {e}", self.spec.bin.display()))?;
        if let Some(out) = child.stdout.take() {
            self.pump_output(out);
        }
        if let Some(err) = child.stderr.take() {
            self.pump_output(err);
        }
        self.child = Some(child);
        // `await_ready` includes IPC handshakes and requests. Bound the whole startup,
        // including a management socket that accepts but never answers.
        let ready = tokio::time::timeout(within, self.await_ready(within))
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "the engine was not ready within {} ms",
                    within.as_millis()
                ))
            });
        match ready {
            Ok(()) => Ok(()),
            Err(e) => {
                self.kill_quietly().await;
                Err(e)
            }
        }
    }

    async fn await_ready(&mut self, within: Duration) -> Result<(), String> {
        let deadline = Instant::now() + within;
        let client = loop {
            if let Some(status) = self.exit_status()? {
                return Err(format!(
                    "the engine exited ({status}) before its management interface was ready{}",
                    // The pipes may not be drained yet; give them a moment.
                    self.settled_error_hint().await
                ));
            }
            match IpcClient::connect(&self.spec.endpoint).await {
                Ok(c) => break Arc::new(c),
                Err(e) if Instant::now() >= deadline => {
                    return Err(format!(
                        "the engine's management interface did not come up within {} ms ({e})",
                        within.as_millis()
                    ));
                }
                Err(_) => tokio::time::sleep(CONNECT_RETRY).await,
            }
        };
        if self.lean {
            self.client = Some(Arc::clone(&client));
            return self.first_snapshot(deadline).await;
        }
        let mut subscription = client
            .subscribe(Topic::Logs {
                filter: LogFilter::default(),
            })
            .await
            .map_err(|e| format!("cannot subscribe to engine logs: {e}"))?;
        let collected = Arc::clone(&self.collected);
        let incarnation = self.incarnation;
        self.pump = Some(tokio::spawn(async move {
            while let Some(event) = subscription.next().await {
                collect_log_event(&collected, incarnation, event);
            }
        }));
        self.client = Some(Arc::clone(&client));
        let backlog = LogFilter {
            limit: Some(100_000),
            ..LogFilter::default()
        };
        if let Ok(Response::Logs { events }) = self.call(Request::GetLogs { filter: backlog }).await
        {
            for event in &events {
                self.collected.add_event(self.incarnation, event);
            }
        }
        while !self
            .collected
            .events()
            .iter()
            .any(|e| event_name(e) == Some("node.started"))
        {
            if let Some(status) = self.exit_status()? {
                return Err(format!(
                    "the engine exited ({status}) during startup{}",
                    self.settled_error_hint().await
                ));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "the engine did not report node.started within {} ms",
                    within.as_millis()
                ));
            }
            tokio::time::sleep(STARTED_POLL).await;
        }
        self.snapshot()
            .await
            .map_err(|e| format!("no first snapshot: {e}"))?;
        Ok(())
    }

    /// Waits for the engine's first snapshot (a lean host does not wait for `node.started`).
    async fn first_snapshot(&mut self, deadline: Instant) -> Result<(), String> {
        loop {
            match self.snapshot().await {
                Ok(_) => return Ok(()),
                Err(e) if Instant::now() >= deadline => {
                    return Err(format!("no first snapshot: {e}"));
                }
                Err(_) => {}
            }
            if let Some(status) = self.exit_status()? {
                return Err(format!(
                    "the engine exited ({status}) during startup{}",
                    self.settled_error_hint().await
                ));
            }
            tokio::time::sleep(STARTED_POLL).await;
        }
    }

    /// The last lines the engine printed, on one line, for short failure messages.
    fn error_hint(&self) -> String {
        let lines = self.recent_output(2);
        let text = lines
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" / ");
        if text.is_empty() {
            String::new()
        } else {
            let short: String = text.chars().take(300).collect();
            format!(": {short}")
        }
    }

    async fn settled_error_hint(&self) -> String {
        tokio::time::sleep(SETTLE).await;
        self.error_hint()
    }

    fn exit_status(&mut self) -> Result<Option<std::process::ExitStatus>, String> {
        match self.child.as_mut() {
            None => Err("the engine is not running".into()),
            Some(c) => c
                .try_wait()
                .map_err(|e| format!("cannot poll the engine process: {e}")),
        }
    }

    async fn kill_quietly(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        self.drop_session();
    }

    fn drop_session(&mut self) {
        if let Some(pump) = self.pump.take() {
            pump.abort();
        }
        self.client = None;
    }

    /// Sends one management request.
    pub async fn call(&self, request: Request) -> Result<Response, IpcFailure> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| IpcFailure::Transport("the engine is not running".into()))?;
        match tokio::time::timeout(REQUEST_TIMEOUT, client.request(request)).await {
            Err(_) => Err(IpcFailure::Transport(format!(
                "the engine did not answer within {} s",
                REQUEST_TIMEOUT.as_secs()
            ))),
            Ok(Ok(response)) => Ok(response),
            Ok(Err(ClientError::Server(e))) => Err(IpcFailure::Server(e)),
            Ok(Err(e)) => Err(IpcFailure::Transport(e.to_string())),
        }
    }

    /// A fresh snapshot as JSON; leak-checked.
    pub async fn snapshot(&self) -> Result<Value, String> {
        match self.call(Request::GetSnapshot).await {
            Ok(Response::Snapshot { snapshot }) => {
                let value = serde_json::to_value(&*snapshot).map_err(|e| e.to_string())?;
                self.collected.check_text("a snapshot", &value.to_string());
                Ok(value)
            }
            Ok(_) => Err("unexpected response to get_snapshot".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Graceful stop: SIGTERM on Unix, Ctrl-Break on Windows; wait within the drain
    /// budget. An unsent signal or a timeout forces a kill and fails the run.
    pub async fn stop(&mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else {
            return Err("the engine is not running".into());
        };
        if let Ok(Some(status)) = child.try_wait() {
            self.drop_session();
            let note = format!("the engine exited on its own ({status})");
            self.problems.push(note.clone());
            return Err(note);
        }
        let signal_failure = terminate(&mut child).await.err().map(|error| {
            format!("the engine could not be stopped gracefully: {error}; forcing termination")
        });
        if let Some(note) = &signal_failure {
            self.problems.push(note.clone());
            let _ = child.start_kill();
        }
        let budget = self.spec.drain + EXIT_GRACE;
        let status = match tokio::time::timeout(budget, child.wait()).await {
            Ok(status) => status.map_err(|e| format!("waiting for the engine: {e}"))?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                self.problems.push(format!(
                    "the engine did not stop within its drain budget of {} s and was killed",
                    budget.as_secs()
                ));
                self.finish_session().await;
                return Err(format!(
                    "the engine did not stop within {} s",
                    budget.as_secs()
                ));
            }
        };
        self.finish_session().await;
        if let Some(note) = signal_failure {
            return Err(note);
        }
        if status.success() {
            Ok(())
        } else {
            let note = format!("the engine exited abnormally on a graceful stop ({status})");
            self.problems.push(note.clone());
            Err(note)
        }
    }

    /// Kills the process abruptly (`crash_engine`).
    pub async fn crash(&mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else {
            return Err("the engine is not running".into());
        };
        child
            .kill()
            .await
            .map_err(|e| format!("cannot kill the engine: {e}"))?;
        let _ = child.wait().await;
        self.finish_session().await;
        Ok(())
    }

    /// Lets the log pump drain what the engine sent before it ended, then drops it.
    async fn finish_session(&mut self) {
        if let Some(mut pump) = self.pump.take()
            && tokio::time::timeout(PUMP_DRAIN, &mut pump).await.is_err()
        {
            // Dropping a JoinHandle detaches the task; explicitly release the socket
            // when a peer never closes the subscription.
            pump.abort();
            let _ = pump.await;
        }
        self.client = None;
        tokio::time::sleep(SETTLE).await;
        lock(&self.files).refresh();
    }

    /// Stops the engine at the end of a run; leftover problems are recorded.
    pub async fn shutdown(&mut self) {
        if self.child.is_some() {
            let _ = self.stop().await;
        }
    }

    /// Global checks of scenarios.md §9 that the engine side can answer: panics, abnormal
    /// exits, `store.error` events, and body leaks in the log files and collected data.
    pub fn global_failures(&self, store_errors_allowed: bool) -> Vec<String> {
        let mut failures = self.problems.clone();
        if self.panic_seen.load(Ordering::SeqCst) {
            failures.push("the engine panicked (a `panicked at` line was printed)".into());
        }
        if !store_errors_allowed {
            let is_error = |e: &&Value| {
                event_name(e) == Some("store.error")
                    && e.get("level").and_then(Value::as_str) == Some("error")
            };
            let count = self
                .with_log_events(|events| events.iter().filter(is_error).count())
                .max(self.collected.events().iter().filter(is_error).count());
            if count > 0 {
                failures.push(format!(
                    "{count} log event(s) with level error and event store.error"
                ));
            }
        }
        self.scan_log_files();
        failures.extend(
            self.collected
                .leaks()
                .into_iter()
                .map(|l| format!("body leak: {l}")),
        );
        failures
    }

    fn scan_log_files(&self) {
        let Ok(entries) = std::fs::read_dir(&self.spec.logs_dir) else {
            return;
        };
        for entry in entries.flatten() {
            if let Ok(bytes) = std::fs::read(entry.path()) {
                let text = String::from_utf8_lossy(&bytes);
                self.collected.check_text(
                    &format!("log file {}", entry.file_name().to_string_lossy()),
                    &text,
                );
            }
        }
    }
}

/// Keeps the engine's console lines for failure reports and checks them for panics and leaks.
async fn pump_lines(
    stream: impl AsyncRead + Unpin,
    tail: Arc<Mutex<VecDeque<String>>>,
    panic_seen: Arc<AtomicBool>,
    collected: Arc<Collected>,
) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.contains("panicked at") {
            panic_seen.store(true, Ordering::SeqCst);
        }
        collected.check_text("the engine's console output", &line);
        push_tail(&tail, line);
    }
}

fn push_tail(tail: &Mutex<VecDeque<String>>, line: String) {
    let mut tail = lock(tail);
    if tail.len() == TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line);
}

fn collect_log_event(collected: &Collected, incarnation: u32, event: Event) {
    if let Event::Log { event } = event {
        collected.add_event(incarnation, &event);
    }
}

#[cfg(unix)]
async fn terminate(child: &mut Child) -> std::io::Result<()> {
    let pid = child.id().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "the engine has no process id")
    })?;
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "sending SIGTERM failed ({status})"
        )))
    }
}

#[cfg(windows)]
async fn terminate(child: &mut Child) -> std::io::Result<()> {
    // Group zero broadcasts to the console, so only a live child's nonzero group is valid.
    let group = child
        .id()
        .and_then(std::num::NonZeroU32::new)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "the engine has no process id")
        })?;
    send_ctrl_break(group)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn send_ctrl_break(group: std::num::NonZeroU32) -> std::io::Result<()> {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
    // SAFETY: the API takes only integer values, no pointers or borrowed memory. The
    // nonzero group is the pid of our child, spawned with CREATE_NEW_PROCESS_GROUP.
    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group.get()) };
    if sent == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Starts `bin` once with test hooks enabled and returns the capability names its `hello`
/// answer lists (for example `test_hooks`). An engine that does not start, or does not know
/// the `test_hooks` bootstrap field, advertises none.
pub async fn probe_capabilities(bin: &Path) -> Vec<String> {
    let Ok(mut dir) = crate::resources::RunDir::create() else {
        return Vec::new();
    };
    let bootstrap = bootstrap_document(
        "probe",
        &dir.state(),
        &dir.logs(),
        Path::new(""),
        true,
        &Map::new(),
    );
    let mut bootstrap = bootstrap;
    if let Some(runtime) = bootstrap.get_mut("runtime").and_then(Value::as_object_mut) {
        runtime.remove("file");
    }
    let path = dir.root().join("bootstrap.json");
    let written = serde_json::to_string(&bootstrap)
        .ok()
        .and_then(|text| std::fs::write(&path, text).ok());
    let child = written.and_then(|()| {
        let _guard = crate::fdguard::spawning();
        Command::new(bin)
            .arg("run")
            .arg("--bootstrap")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()
    });
    let Some(mut child) = child else {
        dir.remove();
        return Vec::new();
    };
    let endpoint = bootstrap_endpoint(&bootstrap);
    let deadline = Instant::now() + PROBE_START;
    let mut found = Vec::new();
    while Instant::now() < deadline && !matches!(child.try_wait(), Ok(Some(_))) {
        if let Ok(client) = IpcClient::connect(&endpoint).await {
            // The probe's startup budget also covers an unresponsive hello endpoint.
            if let Ok(Ok(Response::Hello { capabilities, .. })) = tokio::time::timeout_at(
                deadline,
                client.request(Request::Hello {
                    client: "packie-sim probe".into(),
                }),
            )
            .await
            {
                found = capabilities
                    .iter()
                    .filter_map(|c| serde_json::to_value(c).ok())
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect();
            }
            break;
        }
        tokio::time::sleep(CONNECT_RETRY).await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
    dir.remove();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstraps_select_isolated_platform_endpoints_and_respect_overrides() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let make = |state: &Path, overrides: &Map<String, Value>| {
            bootstrap_document("same-name", state, state, state, false, overrides)
        };
        let first = make(a.path(), &Map::new());
        let second = make(b.path(), &Map::new());
        let endpoint = bootstrap_endpoint(&first);
        assert_ne!(endpoint, bootstrap_endpoint(&second));
        assert_eq!(first["ipc"]["path"], endpoint);
        if cfg!(windows) {
            assert!(endpoint.starts_with(r"\\.\pipe\packie-sim-"), "{endpoint}");
        } else {
            assert_eq!(Path::new(&endpoint), a.path().join("packie.sock"));
        }
        let mut overrides = Map::new();
        overrides.insert("ipc".into(), json!({"path": "custom-endpoint"}));
        assert_eq!(
            bootstrap_endpoint(&make(a.path(), &overrides)),
            "custom-endpoint"
        );
        overrides.insert("ipc".into(), json!({"path": null}));
        let default = bootstrap_endpoint(&make(a.path(), &overrides));
        if cfg!(windows) {
            assert_eq!(default, r"\\.\pipe\packie-sim-same-name");
        } else {
            assert_eq!(Path::new(&default), a.path().join("packie.sock"));
        }
    }
}
