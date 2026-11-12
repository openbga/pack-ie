//! [`RoutingRegistry`]: the engine's [`ChannelRegistry`] (`docs/spec/bridge-channels.md`
//! §12–§14).
//!
//! Every operation that starts, stops or replaces channels (apply, pause, resume,
//! restart, shutdown) runs under one async lock, so they never interleave. Snapshots
//! read an immutable [`View`] swapped in after each operation plus the instances'
//! atomics, so they never wait for an operation.
//!
//! Slot operations (start, stop, replace, commands, shutdown) live in `slots`, the
//! store's lifecycle and the operator's message commands in `store`, the read model in
//! `view`.

mod probes;
mod slots;
mod store;
mod view;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use packie_config::{
    ApplyOutcome, ApplyReport, Change, ChannelConfig, ChannelOutcome, ConfigDiff, EndpointConfig,
    RuntimeConfig,
};
use packie_core::{ChannelId, EndpointId, MessageId, Timestamp};
use packie_diagnostics::{StoreFigures, StoreState};
use packie_engine::{BoxFuture, ChannelRegistry, EngineContext, RegistryError, RegistrySnapshot};
use packie_ipc::ChannelCommand;
use packie_ipc::checks::{ProbeReport, SerialPort};
use packie_ipc::messages::{
    BodyRequest, CloseRequest, DeliveryRecord, MessageBody, MessageList, MessageQuery,
    MessageRecord, RejectionCode, ResendRequest, ResendResult,
};
use packie_transport::{SystemTransports, Transports};
use tokio::task::JoinHandle;

use crate::TapSink;
use crate::bridge::BridgeRunner;
use crate::message::{MessageRunner, MessageStore, Wakeups, transform::Compiled};
use crate::paused::PausedSet;
use crate::runner::{ChannelRunner, EndpointCounters};
use crate::status::ChannelShared;
use crate::stop::StopTx;
use crate::store_open::{DeferredTapSink, StoreOpener, StoreRetry, sqlite_store_opener};
use slots::refresh_endpoints;
use store::store_error;
use view::View;

/// How long a stop without drain may take before the instance task is aborted.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// Upper bound for a make-before-break replacement to become ready.
const TAKEOVER_LIMIT: Duration = Duration::from_secs(15);

/// What a [`RoutingRegistry`] is built with.
#[derive(Clone)]
pub struct RegistryOptions {
    /// Transport adapters for endpoints.
    pub transports: Arc<dyn Transports>,
    /// Receives messages observed by bridge taps, for recording in the store.
    pub tap_sink: Option<Arc<dyn TapSink>>,
    /// The message store. Without one (and without `store_opener`), message channels
    /// report `stopped` with reason `not_implemented`.
    pub store: Option<Arc<dyn MessageStore>>,
    /// Opens the store when `store` is absent because opening failed: until it succeeds,
    /// message channels report `stopped` (`store_unavailable`), the node snapshot reports
    /// the store `unavailable`, and the registry tries again by `store_retry`.
    pub store_opener: Option<StoreOpener>,
    /// The waits between attempts of `store_opener`.
    pub store_retry: StoreRetry,
}

impl RegistryOptions {
    /// `transports`, no tap sink, no store.
    #[must_use]
    pub fn new(transports: Arc<dyn Transports>) -> Self {
        Self {
            transports,
            tap_sink: None,
            store: None,
            store_opener: None,
            store_retry: StoreRetry::default(),
        }
    }

    /// The host's transports, resolving secrets as the engine does.
    #[must_use]
    pub fn for_host(context: &EngineContext) -> Self {
        Self::new(Arc::new(SystemTransports::new(Arc::clone(
            &context.secrets,
        ))))
    }
}

/// The channel registry that runs bridge channels. Message channels report `stopped`
/// with reason `not_implemented` until a message runner exists.
pub struct RoutingRegistry {
    inner: Arc<Inner>,
}

struct Inner {
    context: EngineContext,
    options: RegistryOptions,
    bridge_runner: Arc<dyn ChannelRunner>,
    /// The message store, once open.
    store: RwLock<Option<Arc<dyn MessageStore>>>,
    /// Runs message channels once the store is open.
    message_runner: RwLock<Option<Arc<dyn ChannelRunner>>>,
    /// The store could not be opened yet; the registry keeps trying.
    store_unavailable: AtomicBool,
    /// What bridges record their taps into until the store opens.
    deferred_tap: Option<Arc<DeferredTapSink>>,
    wakeups: Arc<Wakeups>,
    /// Startup reconciliation completed; message channels start only after it.
    reconciled: AtomicBool,
    /// The last reconciliation attempt failed; it is tried again with the store-reopen
    /// backoff and the store reports unavailable meanwhile.
    reconcile_failed: AtomicBool,
    view: RwLock<Arc<View>>,
    control: tokio::sync::Mutex<Control>,
    shutting_down: AtomicBool,
}

struct Control {
    config: Arc<RuntimeConfig>,
    slots: HashMap<ChannelId, Slot>,
    endpoint_counters: EndpointCounters,
    endpoint_since: HashMap<EndpointId, Timestamp>,
    paused: PausedSet,
}

/// The current instance of one configured channel.
struct Slot {
    shared: Arc<ChannelShared>,
    running: Option<Running>,
}

struct Running {
    stop: StopTx,
    task: JoinHandle<()>,
}

/// A command precondition failed; `message` is operator-facing.
fn rejected(code: RejectionCode, message: &str) -> RegistryError {
    RegistryError::Rejected {
        code,
        message: message.to_owned(),
    }
}

impl RoutingRegistry {
    /// A registry with no channels, persisting pause flags under the state directory.
    #[must_use]
    pub fn new(context: &EngineContext, options: RegistryOptions) -> Self {
        let mut options = options;
        let waiting = options.store.is_none() && options.store_opener.is_some();
        let deferred_tap = (waiting && options.tap_sink.is_none()).then(|| {
            let sink = Arc::new(DeferredTapSink::default());
            options.tap_sink = Some(Arc::clone(&sink) as Arc<dyn TapSink>);
            sink
        });
        let paused = PausedSet::load(paused_path(context));
        let empty = Arc::new(RuntimeConfig::empty());
        let view = View {
            config: Arc::clone(&empty),
            channels: HashMap::new(),
            endpoint_counters: Arc::default(),
            endpoint_since: HashMap::new(),
            store: options.store.clone(),
            tap: context.tap.clone(),
        };
        let wakeups = Arc::new(Wakeups::default());
        let store = options.store.clone();
        let message_runner = store.as_ref().map(|store| message_runner(store, &wakeups));
        let registry = Self {
            inner: Arc::new(Inner {
                context: context.clone(),
                options,
                bridge_runner: Arc::new(BridgeRunner),
                store: RwLock::new(store),
                message_runner: RwLock::new(message_runner),
                store_unavailable: AtomicBool::new(waiting),
                deferred_tap,
                wakeups,
                reconciled: AtomicBool::new(false),
                reconcile_failed: AtomicBool::new(false),
                view: RwLock::new(Arc::new(view)),
                control: tokio::sync::Mutex::new(Control {
                    config: empty,
                    slots: HashMap::new(),
                    endpoint_counters: Arc::default(),
                    endpoint_since: HashMap::new(),
                    paused,
                }),
                shutting_down: AtomicBool::new(false),
            }),
        };
        registry.schedule_retention();
        registry.schedule_store_reopen();
        registry
    }
}

fn message_runner(store: &Arc<dyn MessageStore>, wakeups: &Arc<Wakeups>) -> Arc<dyn ChannelRunner> {
    Arc::new(MessageRunner {
        store: Arc::clone(store),
        wakeups: Arc::clone(wakeups),
    })
}

/// The registry with the durable message store under `<state>/store`: message channels
/// run, retention is scheduled, and the store's test hook gate is wired. If the store
/// cannot be opened the engine still runs bridges; message channels report `stopped`
/// (`store_unavailable`), the node snapshot reports the store unavailable, and the registry
/// retries opening it (10 s doubling to 5 min), then starts them.
#[must_use]
pub fn registry_with_store(context: &EngineContext) -> Arc<dyn ChannelRegistry> {
    let mut options = RegistryOptions::for_host(context);
    let opener = sqlite_store_opener(context);
    match opener() {
        Ok(opened) => {
            options.tap_sink = opened.tap_sink;
            options.store = Some(opened.store);
        }
        Err(error) => {
            tracing::error!(
                event = "store.error",
                error_code = "open_failed",
                "The message store could not be opened: {error}; message channels wait \
                 while it is opened again"
            );
            options.store_opener = Some(opener);
        }
    }
    Arc::new(RoutingRegistry::new(context, options))
}

/// The factory the `packie` binary passes to `packie_engine::run`: the host's transports, no
/// tap recording, no message store (message channels report `not_implemented`).
#[must_use]
pub fn registry(context: &EngineContext) -> Arc<dyn ChannelRegistry> {
    Arc::new(RoutingRegistry::new(
        context,
        RegistryOptions::for_host(context),
    ))
}

/// Why a message channel's filters or mappings do not compile, with each problem's JSON
/// path; `None` when they compile (or the channel is a bridge).
fn compile_problems(config: &RuntimeConfig, id: ChannelId) -> Option<String> {
    let (index, channel) = config
        .channels
        .iter()
        .enumerate()
        .find(|(_, c)| c.id() == id)?;
    let ChannelConfig::Message(channel) = channel else {
        return None;
    };
    let at = format!("channels[{index}]");
    let problems = Compiled::compile(channel, config.config_version, &at).err()?;
    let listed: Vec<String> = problems
        .iter()
        .map(|p| format!("{}: {}", p.path, p.message))
        .collect();
    Some(format!(
        "The filters or mappings do not compile: {}",
        listed.join("; ")
    ))
}

/// The outcome of a start or restart: `stopped` when the new instance does not run.
fn started(control: &Control, id: ChannelId, running: ApplyOutcome) -> ApplyOutcome {
    match control.slots.get(&id) {
        Some(slot) if slot.running.is_some() => running,
        _ => ApplyOutcome::Stopped,
    }
}

fn paused_path(context: &EngineContext) -> PathBuf {
    context.state_dir.join("routing").join("paused.json")
}

impl ChannelRegistry for RoutingRegistry {
    fn apply<'a>(
        &'a self,
        config: &'a RuntimeConfig,
        diff: &'a ConfigDiff,
    ) -> BoxFuture<'a, ApplyReport> {
        Box::pin(self.inner.apply(config, diff))
    }

    fn snapshot(&self) -> RegistrySnapshot {
        self.inner.view().snapshot()
    }

    fn command(
        &self,
        channel: ChannelId,
        command: ChannelCommand,
    ) -> BoxFuture<'_, Result<(), RegistryError>> {
        Box::pin(self.inner.command(channel, command))
    }

    fn shutdown(&self, drain: Duration) -> BoxFuture<'_, ()> {
        Box::pin(self.inner.shutdown(drain))
    }

    fn store_status(&self) -> Option<StoreFigures> {
        let inner = &self.inner;
        if inner.store_unavailable.load(Ordering::Relaxed)
            || inner.reconcile_failed.load(Ordering::Relaxed)
        {
            return Some(StoreFigures {
                state: StoreState::Unavailable,
                size_bytes: None,
                messages_stored: None,
                deliveries_unresolved: None,
                deliveries_queued: None,
            });
        }
        let store = inner.store_handle()?;
        let failing = inner.view().channels.values().any(|c| {
            c.messages
                .as_ref()
                .is_some_and(|m| m.store_unavailable.load(Ordering::Relaxed))
        });
        let totals = store.totals();
        Some(StoreFigures {
            state: if failing {
                StoreState::Unavailable
            } else {
                StoreState::Ok
            },
            size_bytes: totals.map(|t| t.size_bytes),
            messages_stored: totals.map(|t| t.messages),
            deliveries_unresolved: totals.map(|t| t.deliveries_unresolved),
            deliveries_queued: totals.map(|t| t.deliveries_queued),
        })
    }

    fn resend(&self, request: ResendRequest) -> BoxFuture<'_, Result<ResendResult, RegistryError>> {
        Box::pin(self.inner.resend(request))
    }

    fn close_delivery(
        &self,
        request: CloseRequest,
    ) -> BoxFuture<'_, Result<DeliveryRecord, RegistryError>> {
        Box::pin(self.inner.close_delivery(request))
    }

    fn serial_ports(&self) -> BoxFuture<'_, Result<Vec<SerialPort>, RegistryError>> {
        Box::pin(probes::serial_ports())
    }

    fn probe_endpoint<'a>(
        &'a self,
        endpoint: &'a EndpointConfig,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<ProbeReport, RegistryError>> {
        Box::pin(probes::probe(
            self.inner.options.transports.as_ref(),
            endpoint,
            timeout,
        ))
    }

    fn transmission_totals(
        &self,
        filter: packie_ipc::messages::MessageFilter,
    ) -> BoxFuture<'_, Result<Vec<packie_ipc::reports::DestinationTransmission>, RegistryError>>
    {
        Box::pin(async move {
            self.inner
                .store()?
                .transmission_totals(filter)
                .await
                .map_err(store_error)
        })
    }

    fn query_messages(
        &self,
        query: MessageQuery,
    ) -> BoxFuture<'_, Result<MessageList, RegistryError>> {
        Box::pin(async move {
            self.inner
                .store()?
                .query_messages(query)
                .await
                .map_err(store_error)
        })
    }

    fn message(&self, id: MessageId) -> BoxFuture<'_, Result<MessageRecord, RegistryError>> {
        Box::pin(async move {
            self.inner
                .store()?
                .message_record(id)
                .await
                .map_err(store_error)
        })
    }

    fn message_body(
        &self,
        request: BodyRequest,
    ) -> BoxFuture<'_, Result<MessageBody, RegistryError>> {
        Box::pin(async move {
            self.inner
                .store()?
                .message_body(request)
                .await
                .map_err(store_error)
        })
    }
}

impl Inner {
    fn view(&self) -> Arc<View> {
        Arc::clone(&self.view.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn publish(&self, control: &Control) {
        let view = View {
            config: Arc::clone(&control.config),
            channels: control
                .slots
                .iter()
                .map(|(id, slot)| (*id, Arc::clone(&slot.shared)))
                .collect(),
            endpoint_counters: Arc::clone(&control.endpoint_counters),
            endpoint_since: control.endpoint_since.clone(),
            store: self.store_handle(),
            tap: self.context.tap.clone(),
        };
        *self.view.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(view);
        self.context.snapshot_now.notify_one();
    }

    async fn apply(&self, config: &RuntimeConfig, diff: &ConfigDiff) -> ApplyReport {
        let mut control = self.control.lock().await;
        self.reconcile_once().await;
        let old = Arc::clone(&control.config);
        let new = Arc::new(config.clone());
        refresh_endpoints(&mut control, &new);
        control.config = Arc::clone(&new);
        let mut report = ApplyReport::default();
        let mut failed = BTreeSet::new();
        for change in &diff.channels {
            let problems = match change.change {
                Change::Added | Change::Changed | Change::Updated | Change::Stopped => {
                    compile_problems(&new, change.id)
                }
                Change::Removed | Change::Unchanged => None,
            };
            let outcome = match (problems, change.change) {
                (Some(reason), _) => {
                    failed.insert(change.id);
                    ApplyOutcome::Failed { reason }
                }
                (None, change_kind) => {
                    self.apply_change(&mut control, &old, change.id, change_kind)
                        .await
                }
            };
            report.channels.push(ChannelOutcome {
                channel_id: change.id,
                outcome,
            });
            self.publish(&control);
        }
        // Robustness: every configured channel has a slot, whatever the diff said.
        let missing: Vec<ChannelId> = new
            .channels
            .iter()
            .map(ChannelConfig::id)
            .filter(|id| !control.slots.contains_key(id) && !failed.contains(id))
            .collect();
        for id in missing {
            self.add(&mut control, id);
        }
        let configured: BTreeSet<ChannelId> = new.channels.iter().map(ChannelConfig::id).collect();
        control.paused.retain(&configured);
        // Channels updated in place or unchanged keep running with the document's limit.
        let kept = new.retain_recent_errors(&self.context.bootstrap);
        for slot in control.slots.values() {
            slot.shared.keep_recent_errors(kept);
        }
        self.publish(&control);
        report
    }

    /// Carries out one channel's change and names its outcome.
    async fn apply_change(
        &self,
        control: &mut Control,
        old: &RuntimeConfig,
        id: ChannelId,
        change: Change,
    ) -> ApplyOutcome {
        match change {
            Change::Removed => {
                self.remove(control, id).await;
                ApplyOutcome::Removed
            }
            Change::Added => {
                self.add(control, id);
                started(control, id, ApplyOutcome::Started)
            }
            Change::Changed => {
                self.change(control, old, id).await;
                started(control, id, ApplyOutcome::Restarted)
            }
            Change::Updated => {
                self.update_in_place(control, old, id).await;
                ApplyOutcome::Updated
            }
            // Disabled by the document: a new slot, or the old instance replaced, not started.
            Change::Stopped => {
                if old.channel(id).is_some() {
                    self.change(control, old, id).await;
                } else {
                    self.add(control, id);
                }
                started(control, id, ApplyOutcome::Stopped)
            }
            Change::Unchanged => ApplyOutcome::Unchanged,
        }
    }
}
