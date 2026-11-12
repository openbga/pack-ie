//! The state of a running soak: the engine and how to start, kill and call it, the
//! peers, and the journals everything is written to.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use packie_ipc::{Request, Response};
use serde_json::{Value, json};
use tokio::sync::RwLock;

use super::chan::{ChannelRt, Env};
use super::snap::SnapshotWatch;
use super::topology::{DocOptions, Knobs, runtime_document};
use super::util::{unix_ms, utc};
use crate::engine::{EngineHost, IpcFailure};

/// How long the engine may take to start.
pub(crate) const START_WITHIN: Duration = Duration::from_secs(90);

/// Who should look at an anomaly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    /// The message pipeline, delivery and the store.
    RoutingStore,
    /// The engine core: configuration, diagnostics, hosting.
    Core,
    /// The soak runner itself, not the engine.
    Runner,
}

impl Owner {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::RoutingStore => "routing/store",
            Self::Core => "core",
            Self::Runner => "soak runner",
        }
    }
}

/// Something the soak saw that deserves a line in the report.
#[derive(Debug, Clone)]
pub(crate) struct Anomaly {
    /// Run time of the first occurrence, milliseconds.
    pub(crate) at_ms: u64,
    pub(crate) unix_ms: u64,
    pub(crate) category: &'static str,
    pub(crate) owner: Owner,
    pub(crate) summary: String,
    /// How often it was seen (identical anomalies are merged).
    pub(crate) count: u64,
    /// Whether it fails the run.
    pub(crate) fatal: bool,
}

/// Where the run keeps its files.
#[derive(Debug, Clone)]
pub(crate) struct Paths {
    pub(crate) out: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) logs: PathBuf,
    pub(crate) runtime: PathBuf,
    pub(crate) ipc: String,
}

/// The line-oriented files of a run.
#[derive(Debug)]
pub(crate) struct Journal {
    chaos: Mutex<File>,
    reconcile: Mutex<File>,
    alerts: Mutex<File>,
    anomalies: Mutex<Vec<Anomaly>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn create(path: PathBuf) -> Result<Mutex<File>, String> {
    File::create(&path)
        .map(Mutex::new)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))
}

fn append(file: &Mutex<File>, value: &Value) {
    let mut file = lock(file);
    let _ = writeln!(file, "{value}");
    let _ = file.flush();
}

impl Journal {
    pub(crate) fn create(dir: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            chaos: create(dir.join("chaos.jsonl"))?,
            reconcile: create(dir.join("reconcile.jsonl"))?,
            alerts: create(dir.join("alerts.jsonl"))?,
            anomalies: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn chaos(&self, value: &Value) {
        append(&self.chaos, value);
    }

    pub(crate) fn reconcile(&self, value: &Value) {
        append(&self.reconcile, value);
    }

    pub(crate) fn alert(&self, value: &Value) {
        append(&self.alerts, value);
    }

    pub(crate) fn anomalies(&self) -> Vec<Anomaly> {
        lock(&self.anomalies).clone()
    }

    /// Records an anomaly; one that repeats (same category and summary) is counted.
    pub(crate) fn note(&self, at_ms: u64, mut anomaly: Anomaly) {
        let mut list = lock(&self.anomalies);
        if let Some(same) = list
            .iter_mut()
            .find(|a| a.category == anomaly.category && a.summary == anomaly.summary)
        {
            same.count += 1;
            return;
        }
        anomaly.at_ms = at_ms;
        list.push(anomaly);
    }
}

/// Everything the tasks of a run share.
#[derive(Debug)]
pub(crate) struct Rig {
    pub(crate) env: Arc<Env>,
    pub(crate) paths: Paths,
    pub(crate) channels: Vec<Arc<ChannelRt>>,
    pub(crate) engine: RwLock<EngineHost>,
    pub(crate) knobs: Mutex<BTreeMap<String, Knobs>>,
    pub(crate) doc_options: DocOptions,
    pub(crate) watch: Arc<SnapshotWatch>,
    pub(crate) journal: Arc<Journal>,
    /// Which start of the engine is running; the first start is 1.
    pub(crate) incarnation: AtomicU32,
    /// The engine is down on purpose (a kill episode is under way).
    pub(crate) down_on_purpose: AtomicBool,
    /// Chaos episodes in progress.
    pub(crate) active_episodes: AtomicU32,
    /// Run time at which the last episode ended, milliseconds.
    pub(crate) last_episode_end_ms: std::sync::atomic::AtomicU64,
    /// What the monitor recorded.
    pub(crate) monitor: Mutex<super::monitor::MonitorData>,
    /// Finished chaos episodes.
    pub(crate) episodes: Mutex<Vec<super::chaos::Episode>>,
    /// The engine's state of every message found missing at a peer, by channel and sequence.
    pub(crate) verdicts: Mutex<std::collections::HashMap<(String, u64), String>>,
    /// Every reconciliation done.
    pub(crate) recons: Mutex<Vec<super::reconcile::Summary>>,
    /// Which channels episodes worked on, and when.
    pub(crate) targets: Mutex<super::chaos::Targets>,
    /// What the store held at the end.
    pub(crate) census: Mutex<Option<super::census::Census>>,
}

impl Rig {
    pub(crate) fn now_ms(&self) -> u64 {
        self.env.now_ms()
    }

    /// Records an anomaly that fails the run.
    pub(crate) fn fail(&self, category: &'static str, owner: Owner, summary: String) {
        self.anomaly(category, owner, summary, true, unix_ms());
    }

    /// Records an anomaly that fails the run and happened at `unix_ms`, which is where the
    /// report looks in the engine log.
    pub(crate) fn fail_at(
        &self,
        category: &'static str,
        owner: Owner,
        summary: String,
        unix_ms: u64,
    ) {
        self.anomaly(category, owner, summary, true, unix_ms);
    }

    /// Records an anomaly that is reported but does not fail the run.
    pub(crate) fn warn(&self, category: &'static str, owner: Owner, summary: String) {
        self.anomaly(category, owner, summary, false, unix_ms());
    }

    fn anomaly(
        &self,
        category: &'static str,
        owner: Owner,
        summary: String,
        fatal: bool,
        at_unix_ms: u64,
    ) {
        let at = self
            .now_ms()
            .saturating_sub(unix_ms().saturating_sub(at_unix_ms));
        self.journal.note(
            at,
            Anomaly {
                at_ms: at,
                unix_ms: at_unix_ms,
                category,
                owner,
                summary,
                count: 1,
                fatal,
            },
        );
    }

    /// One management request to the running engine.
    pub(crate) async fn call(&self, request: Request) -> Result<Response, String> {
        let engine = self.engine.read().await;
        engine.call(request).await.map_err(|e| match e {
            IpcFailure::Server(error) => format!("{:?}: {}", error.code, error.message),
            IpcFailure::Transport(why) => why,
        })
    }

    /// A snapshot: the latest published one when recent, otherwise a fresh request.
    pub(crate) async fn snapshot(&self) -> Option<Arc<Value>> {
        if let Some(value) = self.watch.fresh() {
            return Some(value);
        }
        let engine = self.engine.read().await;
        engine.snapshot().await.ok().map(Arc::new)
    }

    /// The process id of the engine, when it runs and nothing is changing it.
    pub(crate) fn pid(&self) -> Option<u32> {
        self.engine.try_read().ok().and_then(|e| e.pid())
    }

    /// Whether the engine process is alive; `None` while it is being started or stopped.
    pub(crate) async fn alive(&self) -> Option<bool> {
        let mut engine = self.engine.try_write().ok()?;
        Some(engine.is_running())
    }

    /// Messages retention may leave over its limit because their delivery is unresolved.
    pub(crate) fn unresolved_allowance(&self) -> u64 {
        self.verdicts
            .lock()
            .map(|v| {
                v.values()
                    .filter(|s| matches!(s.as_str(), "outcome_unknown" | "timed_out" | "failed"))
                    .count() as u64
            })
            .unwrap_or(0)
    }

    /// The current runtime document with the current knobs.
    pub(crate) fn document(&self) -> Value {
        runtime_document(&self.plans(), &lock(&self.knobs), &self.doc_options)
    }

    pub(crate) fn plans(&self) -> Vec<super::topology::Plan> {
        self.channels.iter().map(|c| c.plan.clone()).collect()
    }

    /// Starts the engine and waits until it answers. Returns how long that took.
    pub(crate) async fn start_engine(&self) -> Result<Duration, String> {
        let started = std::time::Instant::now();
        let mut engine = self.engine.write().await;
        engine.start(START_WITHIN).await?;
        self.incarnation.fetch_add(1, Ordering::SeqCst);
        self.down_on_purpose.store(false, Ordering::SeqCst);
        Ok(started.elapsed())
    }

    /// Kills the engine with SIGKILL.
    pub(crate) async fn kill_engine(&self) -> Result<(), String> {
        self.down_on_purpose.store(true, Ordering::SeqCst);
        let mut engine = self.engine.write().await;
        let result = engine.crash().await;
        self.watch.clear();
        result
    }

    /// Stops the engine with SIGTERM at the end of the run.
    pub(crate) async fn stop_engine(&self) -> Result<(), String> {
        self.down_on_purpose.store(true, Ordering::SeqCst);
        let mut engine = self.engine.write().await;
        engine.stop().await
    }

    /// Applies `document` over the active version and returns the engine's answer.
    pub(crate) async fn put_config(&self, document: Value, note: &str) -> Result<Value, String> {
        let Response::Snapshot { snapshot } = self.call(Request::GetSnapshot).await? else {
            return Err("unexpected response to get_snapshot".to_owned());
        };
        let base = snapshot.node.config.active_version.unwrap_or(0);
        let request = Request::PutConfig {
            document,
            base_version: base,
            note: Some(note.to_owned()),
        };
        match self.call(request).await? {
            Response::ConfigApplied {
                config_version,
                result,
                outcomes,
                ..
            } => Ok(json!({ "config_version": config_version, "result": result,
                            "outcomes": outcomes })),
            _ => Err("unexpected response to put_config".to_owned()),
        }
    }

    /// A line for the chaos journal with the time of both clocks.
    pub(crate) fn log_chaos(&self, kind: &str, detail: Value) {
        let mut line = json!({ "t_ms": self.now_ms(), "at": utc(unix_ms()), "event": kind });
        if let (Some(map), Some(extra)) = (line.as_object_mut(), detail.as_object()) {
            map.extend(extra.clone());
        }
        self.journal.chaos(&line);
    }
}
