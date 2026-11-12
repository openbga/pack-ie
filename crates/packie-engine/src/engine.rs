//! Engine start, run and shutdown.

use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use packie_config::{
    ApplyOutcome, ApplyReport, Bootstrap, ConfigDiff, ConfigError, ConfigStore, RuntimeConfig,
    SecretResolver,
};
use packie_core::{Actor, ChannelId, Severity, Timestamp};
use packie_diagnostics::{
    Alert, AlertBook, AlertCounts, AlertState, ApplyResult, ConfigStatus, EndpointCounts,
    EngineSnapshot, HostingMode, LastApply, LogHandle, LogInitError, LogOptions, LogRing,
    NodeConditions, NodeInputs, NodeSnapshot, NodeState, StateCounts, StoreState, StoreStatus,
    Totals, TrafficTap,
};
use packie_ipc::audit::{AuditAction, AuditObjectType, AuditOutcome};
use packie_ipc::{IpcServer, TransportError};
use tokio::sync::{Notify, broadcast};

use crate::audit::{AuditLog, AuditOpener, AuditRecord, AuditUnavailable, MemoryAudit};
use crate::backups::Backups;
use crate::handler::EngineHandler;
use crate::registry::{ChannelRegistry, EngineContext, RegistrySnapshot, TestHooks};

/// The engine version, as reported over IPC.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Snapshots and alerts buffered per subscriber before it misses some.
const BROADCAST_CAPACITY: usize = 64;
/// Wait after a change notification so a burst of changes publishes once.
const CHANGE_DEBOUNCE: Duration = Duration::from_millis(100);

/// How to run.
#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    /// Also write human-readable logs to stderr (foreground runs).
    pub console_logs: bool,
    /// How the engine is hosted, as reported in the node snapshot.
    pub hosting: HostingMode,
    /// Opens the audit log under the state directory. Default: [`MemoryAudit`], which
    /// keeps nothing; the `packie` binary passes the persistent log.
    pub audit: AuditOpener,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            console_logs: false,
            hosting: HostingMode::Console,
            audit: MemoryAudit::open,
        }
    }
}

/// The engine could not start.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The bootstrap document is missing or invalid.
    #[error("bootstrap: {0}")]
    Bootstrap(#[source] ConfigError),
    /// Logging could not be set up.
    #[error("logging: {0}")]
    Logging(#[from] LogInitError),
    /// The state directory or saved configuration could not be used.
    #[error("runtime configuration: {0}")]
    Config(#[source] ConfigError),
    /// The management interface could not be bound.
    #[error("management interface: {0}")]
    Ipc(#[from] TransportError),
    /// The secret store the bootstrap selects cannot be used on this computer.
    #[error("secret store: {0}")]
    Secrets(String),
}

/// Apply state shown in the node snapshot and fed to the alert rules.
#[derive(Debug, Default)]
pub(crate) struct ApplyStatus {
    /// A runtime configuration is in effect (not a zero-channel fallback).
    pub(crate) config_active: bool,
    pub(crate) pending: Option<u64>,
    pub(crate) last_apply: Option<LastApply>,
    pub(crate) config_rejected: bool,
    pub(crate) apply_failures: Vec<(ChannelId, String)>,
}

/// State shared by the IPC handler and the snapshot publisher.
pub(crate) struct Shared {
    pub(crate) bootstrap: Arc<Bootstrap>,
    pub(crate) hosting: HostingMode,
    pub(crate) started_at: Timestamp,
    pub(crate) registry: Arc<dyn ChannelRegistry>,
    pub(crate) store: ConfigStore,
    pub(crate) secrets: Arc<dyn SecretResolver>,
    active: RwLock<Arc<RuntimeConfig>>,
    pub(crate) status: Mutex<ApplyStatus>,
    /// Serializes configuration applies and operator commands.
    pub(crate) apply_lock: tokio::sync::Mutex<()>,
    pub(crate) alerts: Mutex<AlertBook>,
    pub(crate) snapshots: broadcast::Sender<Arc<EngineSnapshot>>,
    pub(crate) alert_events: broadcast::Sender<Alert>,
    latest: RwLock<Arc<EngineSnapshot>>,
    pub(crate) ring: LogRing,
    pub(crate) tap: TrafficTap,
    pub(crate) test_hooks: Option<TestHooks>,
    pub(crate) snapshot_now: Arc<Notify>,
    /// Where operator actions and content views are recorded (fail closed).
    pub(crate) audit: Arc<dyn AuditLog>,
    /// Backups on request and on schedule, and their staleness.
    pub(crate) backups: Arc<Backups>,
    /// Sends alert changes to the bootstrap's notification targets.
    pub(crate) notifier: packie_notify::Notifier,
    /// Exports whose audited first chunk was answered.
    pub(crate) exports: crate::exports::ExportSessions,
}

impl Shared {
    pub(crate) fn active(&self) -> Arc<RuntimeConfig> {
        Arc::clone(&self.active.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn set_active(&self, config: Arc<RuntimeConfig>) {
        *self.active.write().unwrap_or_else(PoisonError::into_inner) = config;
    }

    pub(crate) fn status(&self) -> std::sync::MutexGuard<'_, ApplyStatus> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn alerts(&self) -> std::sync::MutexGuard<'_, AlertBook> {
        self.alerts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn free_disk(&self) -> Option<u64> {
        packie_diagnostics::free_space(self.bootstrap.state_dir())
    }

    pub(crate) fn latest(&self) -> Arc<EngineSnapshot> {
        Arc::clone(&self.latest.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Saves `config` as a version and as the active `runtime.file`.
    pub(crate) fn save(
        &self,
        config: &RuntimeConfig,
        note: &str,
        actor: &Actor,
    ) -> Result<(), ConfigError> {
        self.store.save(config, note, actor)?;
        write_runtime_file(&self.bootstrap.runtime_file(), config)
    }

    /// Records the outcome of an apply.
    pub(crate) fn applied(&self, report: &ApplyReport) -> ApplyResult {
        let result = if report.partial() {
            ApplyResult::Partial
        } else {
            ApplyResult::Applied
        };
        let mut status = self.status();
        status.config_active = true;
        status.pending = None;
        status.config_rejected = false;
        status.last_apply = Some(LastApply {
            at: Timestamp::now(),
            result,
        });
        status.apply_failures = report
            .channels
            .iter()
            .filter_map(|c| match &c.outcome {
                ApplyOutcome::Failed { reason } => Some((c.channel_id, reason.clone())),
                _ => None,
            })
            .collect();
        result
    }

    /// Records a rejected document.
    pub(crate) fn rejected(&self) {
        let mut status = self.status();
        status.config_rejected = true;
        status.last_apply = Some(LastApply {
            at: Timestamp::now(),
            result: ApplyResult::Rejected,
        });
    }

    /// A snapshot of now, with the alerts as last evaluated.
    pub(crate) fn current(&self) -> EngineSnapshot {
        let alerts = self.alerts().active();
        self.build(self.registry.snapshot(), alerts, false)
    }

    fn build(
        &self,
        registry: RegistrySnapshot,
        alerts: Vec<Alert>,
        stopped: bool,
    ) -> EngineSnapshot {
        let now = Timestamp::now();
        let free = self.free_disk();
        let store = self.registry.store_status();
        let config = self.active();
        let critical_disk = u64::from(config.alerts.disk_critical_megabytes) * 1024 * 1024;
        let (config_status, config_active) = {
            let status = self.status();
            let config_status = ConfigStatus {
                active_version: status.config_active.then_some(config.config_version),
                pending_version: status.pending,
                last_apply: status.last_apply.clone(),
            };
            (config_status, status.config_active)
        };
        let inputs = NodeInputs {
            config_active,
            store_ok: store.is_none_or(|s| s.state != StoreState::Unavailable),
            disk_critical: free.is_some_and(|f| f < critical_disk),
            stopped,
        };
        let count = |severity| {
            u32::try_from(alerts.iter().filter(|a| a.severity == severity).count())
                .unwrap_or(u32::MAX)
        };
        let totals = Totals {
            bytes_in: registry.endpoints.iter().map(|e| e.traffic.bytes_in).sum(),
            bytes_out: registry.endpoints.iter().map(|e| e.traffic.bytes_out).sum(),
            messages_stored: store.and_then(|s| s.messages_stored),
            deliveries_unresolved: store.and_then(|s| s.deliveries_unresolved),
            deliveries_queued: store.and_then(|s| s.deliveries_queued),
            alerts_active: AlertCounts {
                warning: count(Severity::Warning),
                critical: count(Severity::Critical),
            },
        };
        EngineSnapshot {
            node: NodeSnapshot {
                node_id: self.bootstrap.node.node_id.clone(),
                display_name: self.bootstrap.node.display_name.clone(),
                version: ENGINE_VERSION.to_owned(),
                hosting_mode: self.hosting,
                started_at: self.started_at,
                captured_at: now,
                uptime_seconds: now.saturating_duration_since(self.started_at).as_secs(),
                node_state: NodeState::derive(&registry.channels, inputs),
                config: config_status,
                channel_counts: StateCounts::of(registry.channels.iter().map(|c| c.state)),
                endpoint_counts: EndpointCounts::of(registry.endpoints.iter().map(|e| e.state)),
                totals,
                store: StoreStatus {
                    state: store.map_or(StoreState::NotConfigured, |s| s.state),
                    size_bytes: store.and_then(|s| s.size_bytes),
                    free_disk_bytes: free,
                    total_disk_bytes: packie_diagnostics::total_space(self.bootstrap.state_dir()),
                },
            },
            channels: registry.channels,
            endpoints: registry.endpoints,
            alerts,
        }
    }

    /// Evaluates alerts, builds a snapshot and publishes both.
    fn publish(&self, stopped: bool) {
        let registry = self.registry.snapshot();
        let config = self.active();
        // Everything that can block is read before the status lock is taken.
        let free_disk_bytes = self.free_disk();
        let store_unavailable = self
            .registry
            .store_status()
            .is_some_and(|s| s.state == StoreState::Unavailable);
        let audit_unavailable = !self.audit.available();
        let backup_stale = self.backups.stale();
        let node = {
            let status = self.status();
            NodeConditions {
                free_disk_bytes,
                store_unavailable,
                config_rejected: status.config_rejected,
                apply_failures: status.apply_failures.clone(),
                audit_unavailable,
                backup_stale,
                notifications_failing: crate::checks::failing(&self.notifier),
            }
        };
        let (changes, active) = {
            let mut book = self.alerts();
            let changes = book.evaluate(
                &config,
                &registry.channels,
                &registry.endpoints,
                &node,
                Timestamp::now(),
            );
            (changes, book.active())
        };
        for alert in changes {
            log_alert(&alert);
            crate::checks::notify(&self.notifier, &config, &alert);
            // No subscribers is normal.
            let _ = self.alert_events.send(alert);
        }
        let snapshot = Arc::new(self.build(registry, active, stopped));
        *self.latest.write().unwrap_or_else(PoisonError::into_inner) = Arc::clone(&snapshot);
        let _ = self.snapshots.send(snapshot);
    }

    fn snapshot_interval(&self) -> Duration {
        let seconds = self
            .active()
            .diagnostics
            .snapshot_interval_seconds
            .unwrap_or(self.bootstrap.diagnostics.snapshot_interval_seconds);
        Duration::from_secs(u64::from(seconds.max(1)))
    }
}

/// The catalog code of `alert` as logs and the spec name it (`endpoint_down`).
fn alert_code(alert: &Alert) -> String {
    serde_json::to_value(alert.kind)
        .ok()
        .and_then(|code| code.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn log_alert(alert: &Alert) {
    let channel_id = alert.channel_id.map(|id| id.to_string());
    let endpoint_id = alert.endpoint_id.map(|id| id.to_string());
    if alert.state == AlertState::Cleared {
        tracing::info!(
            event = "alert.cleared",
            alert_id = %alert.id,
            code = alert_code(alert),
            channel_id,
            endpoint_id,
            "Alert cleared: {}",
            alert.summary
        );
    } else {
        tracing::warn!(
            event = "alert.raised",
            alert_id = %alert.id,
            code = alert_code(alert),
            severity = %alert.severity,
            channel_id,
            endpoint_id,
            "Alert: {}. {}",
            alert.summary,
            alert.next_action
        );
    }
}

/// Runs the engine from the bootstrap file at `bootstrap_path` until `shutdown`
/// completes. `registry` builds the channel registry once the context exists.
///
/// Start: load bootstrap, start logging, load the runtime document (the newest valid saved
/// version, or the runtime file imported as the next one), apply it, bind IPC, then publish
/// snapshots every `snapshot_interval_seconds` and on change. Shutdown: stop channels
/// within `shutdown_drain_seconds`, publish a final `stopped` snapshot, close IPC.
///
/// # Errors
///
/// [`EngineError`] when the bootstrap, logging, the saved configuration or the management
/// interface cannot be set up; the engine has not started then.
pub async fn run<F>(
    bootstrap_path: &Path,
    options: RunOptions,
    registry: F,
    shutdown: impl Future<Output = ()>,
) -> Result<(), EngineError>
where
    F: FnOnce(&EngineContext) -> Arc<dyn ChannelRegistry>,
{
    let bootstrap = Arc::new(Bootstrap::load(bootstrap_path).map_err(EngineError::Bootstrap)?);
    let logs: LogHandle = packie_diagnostics::init(&LogOptions {
        dir: bootstrap.log_dir(),
        filter: bootstrap.logging.level.as_str().to_owned(),
        retention_days: bootstrap.logging.retention_days,
        max_file_bytes: u64::from(bootstrap.logging.rotate_at_megabytes) * 1024 * 1024,
        ring_capacity: usize::try_from(bootstrap.logging.ring_buffer_events).unwrap_or(usize::MAX),
        console: options.console_logs,
    })?;
    let result = run_logged(
        (&bootstrap, bootstrap_path),
        options,
        logs.ring().clone(),
        registry,
        shutdown,
    )
    .await;
    if let Err(error) = &result {
        tracing::error!(event = "node.start_failed", %error, "Engine stopped: {error}");
    }
    result
}

async fn run_logged<F>(
    (bootstrap, bootstrap_path): (&Arc<Bootstrap>, &Path),
    options: RunOptions,
    ring: LogRing,
    registry: F,
    shutdown: impl Future<Output = ()>,
) -> Result<(), EngineError>
where
    F: FnOnce(&EngineContext) -> Arc<dyn ChannelRegistry>,
{
    prepare(bootstrap)?;
    let store = open_versions(bootstrap)?;
    let start = initial_config(bootstrap, &store)?;
    let secrets: Arc<dyn SecretResolver> = Arc::from(packie_config::secrets_for(bootstrap));
    let audit = open_audit(options.audit, bootstrap);

    let context = EngineContext {
        bootstrap: Arc::clone(bootstrap),
        state_dir: bootstrap.state_dir().to_owned(),
        tap: TrafficTap::new(),
        snapshot_now: Arc::new(Notify::new()),
        test_hooks: bootstrap.test_hooks.then(TestHooks::new),
        secrets: Arc::clone(&secrets),
    };
    let registry = registry(&context);
    let report = registry
        .apply(
            &start.config,
            &ConfigDiff::between(&RuntimeConfig::empty(), &start.config),
        )
        .await;

    let endpoint = bootstrap.ipc_endpoint();
    let server = IpcServer::bind_shared(&endpoint, bootstrap.ipc.group.as_deref())?;
    let shared = Arc::new(Shared {
        bootstrap: Arc::clone(bootstrap),
        hosting: options.hosting,
        started_at: Timestamp::now(),
        registry,
        store,
        notifier: crate::checks::notifier(bootstrap, &secrets),
        exports: crate::exports::ExportSessions::default(),
        secrets,
        active: RwLock::new(Arc::new(start.config)),
        status: Mutex::new(ApplyStatus::default()),
        apply_lock: tokio::sync::Mutex::new(()),
        alerts: Mutex::new(AlertBook::new()),
        snapshots: broadcast::channel(BROADCAST_CAPACITY).0,
        alert_events: broadcast::channel(BROADCAST_CAPACITY).0,
        latest: RwLock::new(Arc::new(EngineSnapshot {
            node: placeholder_node(bootstrap, options.hosting),
            channels: Vec::new(),
            endpoints: Vec::new(),
            alerts: Vec::new(),
        })),
        ring,
        tap: context.tap.clone(),
        test_hooks: context.test_hooks.clone(),
        snapshot_now: Arc::clone(&context.snapshot_now),
        audit,
        backups: Arc::new(Backups::new(bootstrap, bootstrap_path, Timestamp::now())),
    });
    if start.active {
        shared.applied(&report);
    }
    shared.status().config_rejected = start.rejected;
    shared.publish(false);
    if shared.test_hooks.is_some() {
        tracing::warn!(
            event = "test_hooks.enabled",
            "Test hooks are enabled; never use this in production"
        );
    }

    let ipc = tokio::spawn(server.serve(Arc::new(EngineHandler::new(Arc::clone(&shared)))));
    let publisher = tokio::spawn(publish_loop(Arc::clone(&shared)));
    let scheduler = tokio::spawn(Arc::clone(&shared.backups).run(Arc::clone(&shared.audit)));
    tracing::info!(
        event = "node.started",
        node_id = %bootstrap.node.node_id,
        config_version = shared.active().config_version,
        ipc = %endpoint,
        "Engine {ENGINE_VERSION} started; management interface at {endpoint}"
    );

    record_engine(&shared, AuditAction::EngineStart);

    shutdown.await;
    scheduler.abort();
    stop(&shared, ipc, publisher).await;
    record_engine(&shared, AuditAction::EngineStop);
    Ok(())
}

/// Checks that the selected secret store can be used here, then creates the state directory.
fn prepare(bootstrap: &Bootstrap) -> Result<(), EngineError> {
    if let Some(problem) = packie_config::secret_store_problem(bootstrap) {
        return Err(EngineError::Secrets(problem));
    }
    packie_config::create_private_dir(bootstrap.state_dir()).map_err(EngineError::Config)
}

/// The saved configuration versions: kept without limit unless `runtime.versions_kept`
/// is set, and never pruned while the audit log may still refer to them.
fn open_versions(bootstrap: &Bootstrap) -> Result<ConfigStore, EngineError> {
    let keep = bootstrap
        .runtime
        .versions_kept
        .map(|k| usize::try_from(k).unwrap_or(usize::MAX));
    let protect = Duration::from_secs(u64::from(bootstrap.audit.retention_days) * 86_400);
    ConfigStore::open(bootstrap.config_dir(), keep, protect).map_err(EngineError::Config)
}

/// Opens the audit log; when it cannot be opened the engine still runs (message traffic
/// never waits on the audit log), but every audited command is refused until a restart.
fn open_audit(open: AuditOpener, bootstrap: &Bootstrap) -> Arc<dyn AuditLog> {
    match open(bootstrap) {
        Ok(audit) => audit,
        Err(error) => {
            tracing::error!(
                event = "audit.unavailable",
                error_summary = %error,
                "The audit log cannot be opened; audited commands are refused: {error}"
            );
            Arc::new(UnavailableAudit(error))
        }
    }
}

/// Records the engine's own start or stop; a failure is logged, not fatal.
fn record_engine(shared: &Shared, action: AuditAction) {
    let record = AuditRecord {
        actor: Actor::system(),
        action,
        object: Some((
            AuditObjectType::Engine,
            shared.bootstrap.node.node_id.clone(),
        )),
        outcome: AuditOutcome::Succeeded,
        details: serde_json::json!({ "version": ENGINE_VERSION }),
    };
    record_logged(shared.audit.as_ref(), &record);
}

/// Records `record`; a failure is logged, not returned (for records of actions that
/// already happened).
pub(crate) fn record_logged(audit: &dyn AuditLog, record: &AuditRecord) {
    if let Err(error) = audit.record(record) {
        tracing::error!(event = "audit.write_failed", error_summary = %error, "{error}");
    }
}

/// The log that could not be opened: every write fails, so audited commands are refused.
struct UnavailableAudit(AuditUnavailable);

impl AuditLog for UnavailableAudit {
    fn record(&self, _record: &AuditRecord) -> Result<u64, AuditUnavailable> {
        Err(self.0.clone())
    }

    fn query(
        &self,
        _query: &packie_ipc::audit::AuditQuery,
    ) -> Result<packie_ipc::audit::AuditPage, AuditUnavailable> {
        Err(self.0.clone())
    }

    fn export(
        &self,
        _request: &packie_ipc::audit::AuditExportRequest,
    ) -> Result<packie_ipc::audit::AuditExportChunk, AuditUnavailable> {
        Err(self.0.clone())
    }

    fn verify(&self) -> Result<packie_ipc::audit::AuditVerification, AuditUnavailable> {
        Err(self.0.clone())
    }

    fn available(&self) -> bool {
        false
    }
}

/// Stops channels within the drain budget, publishes the final snapshot and closes IPC.
async fn stop(
    shared: &Arc<Shared>,
    ipc: tokio::task::JoinHandle<Result<(), TransportError>>,
    publisher: tokio::task::JoinHandle<()>,
) {
    tracing::info!(event = "node.stopping", "Engine stopping");
    publisher.abort();
    let drain = Duration::from_secs(u64::from(shared.bootstrap.hosting.shutdown_drain_seconds));
    {
        let _guard = shared.apply_lock.lock().await;
        shared.registry.shutdown(drain).await;
    }
    shared.publish(true);
    // Let subscribers receive the final snapshot before connections close.
    tokio::time::sleep(CHANGE_DEBOUNCE).await;
    ipc.abort();
    let _ = ipc.await;
    tracing::info!(
        event = "node.stopped",
        config_version = shared.active().config_version,
        "Engine stopped"
    );
}

fn placeholder_node(bootstrap: &Bootstrap, hosting: HostingMode) -> NodeSnapshot {
    let now = Timestamp::now();
    NodeSnapshot {
        node_id: bootstrap.node.node_id.clone(),
        display_name: bootstrap.node.display_name.clone(),
        version: ENGINE_VERSION.to_owned(),
        hosting_mode: hosting,
        started_at: now,
        captured_at: now,
        uptime_seconds: 0,
        node_state: NodeState::Healthy,
        config: ConfigStatus {
            active_version: None,
            pending_version: None,
            last_apply: None,
        },
        channel_counts: StateCounts::default(),
        endpoint_counts: EndpointCounts::default(),
        totals: Totals::default(),
        store: StoreStatus {
            state: StoreState::NotConfigured,
            size_bytes: None,
            free_disk_bytes: None,
            total_disk_bytes: None,
        },
    }
}

/// The configuration the engine starts from.
struct Start {
    config: RuntimeConfig,
    /// A configuration is in effect (false for the zero-channel fallback).
    active: bool,
    /// `runtime.file` was unreadable or invalid.
    rejected: bool,
}

/// `docs/spec/configuration.md` §10 Startup: a valid `runtime.file` (saved as a new
/// version when it differs from the newest stored one); else the newest valid stored
/// version, with `config_rejected` when the file was invalid; else zero channels.
fn initial_config(bootstrap: &Bootstrap, store: &ConfigStore) -> Result<Start, EngineError> {
    let latest = store.latest().map_err(EngineError::Config)?;
    let file = bootstrap.runtime_file();
    let from_file = if file.exists() {
        match packie_config::load_runtime_file(&file) {
            Ok(config) => Some(config),
            Err(error) => {
                tracing::warn!(event = "config.rejected", file = %file.display(), %error, "The runtime file is invalid; using the newest stored version");
                let config = latest.unwrap_or_else(RuntimeConfig::empty);
                let active = config.config_version > 0;
                return Ok(Start {
                    config,
                    active,
                    rejected: true,
                });
            }
        }
    } else {
        None
    };
    let config = match (from_file, latest) {
        (Some(file_config), Some(stored)) if same_document(&file_config, &stored) => stored,
        (Some(mut file_config), stored) => {
            file_config.config_version = stored.map_or(0, |s| s.config_version) + 1;
            file_config.generated_at = Timestamp::now();
            store
                .save(
                    &file_config,
                    "imported from the runtime file",
                    &Actor::system(),
                )
                .map_err(EngineError::Config)?;
            write_runtime_file(&file, &file_config).map_err(EngineError::Config)?;
            tracing::info!(event = "config.imported", file = %file.display(), config_version = file_config.config_version, "Imported the runtime file");
            file_config
        }
        (None, Some(stored)) => stored,
        (None, None) => {
            tracing::warn!(
                event = "config.empty",
                "No runtime configuration; running with zero channels"
            );
            return Ok(Start {
                config: RuntimeConfig::empty(),
                active: false,
                rejected: false,
            });
        }
    };
    Ok(Start {
        config,
        active: true,
        rejected: false,
    })
}

/// Writes `config` to the runtime file at `file`, as pretty JSON with a final newline.
fn write_runtime_file(file: &Path, config: &RuntimeConfig) -> Result<(), ConfigError> {
    let mut text = config.to_pretty_json()?;
    text.push('\n');
    packie_config::write_atomic(file, text.as_bytes())
}

/// Equal apart from the assigned version and time.
fn same_document(a: &RuntimeConfig, b: &RuntimeConfig) -> bool {
    let strip = |c: &RuntimeConfig| RuntimeConfig {
        config_version: 0,
        generated_at: b.generated_at,
        ..c.clone()
    };
    strip(a) == strip(b) && a.config_version == b.config_version
}

async fn publish_loop(shared: Arc<Shared>) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(shared.snapshot_interval()) => {}
            () = shared.snapshot_now.notified() => tokio::time::sleep(CHANGE_DEBOUNCE).await,
        }
        shared.publish(false);
    }
}
