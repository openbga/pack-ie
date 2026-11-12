//! The bootstrap document (`docs/spec/configuration.md` §3): what the engine needs before
//! anything else. Read at start; edits require a restart. Relative paths are resolved
//! against the directory of the bootstrap file.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::notifications::NotificationSettings;
use crate::tagged;
use crate::validate::{Problem, ProblemCode, ValidationErrors};

/// `bootstrap.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bootstrap {
    /// Must be 1. Default 1.
    #[serde(default = "one")]
    pub schema_version: u32,
    /// Node identity.
    pub node: NodeSettings,
    /// State and log directories.
    pub paths: PathSettings,
    /// Management interface.
    #[serde(default)]
    pub ipc: IpcSettings,
    /// Process behavior.
    #[serde(default)]
    pub hosting: HostingSettings,
    /// The runtime document and its versions.
    #[serde(default)]
    pub runtime: RuntimeSettings,
    /// Log files and the in-memory log ring.
    #[serde(default)]
    pub logging: LoggingSettings,
    /// Snapshot cadence and error retention.
    #[serde(default)]
    pub diagnostics: DiagnosticsSettings,
    /// Where secret values live.
    #[serde(default)]
    pub secrets: SecretsSettings,
    /// The audit log (`docs/spec/audit.md`).
    #[serde(default)]
    pub audit: AuditSettings,
    /// Backups (`docs/spec/backup.md`).
    #[serde(default)]
    pub backup: BackupSettings,
    /// Alert messages by e-mail and webhook; off without targets.
    #[serde(default)]
    pub notifications: NotificationSettings,
    /// Enables the scenario test hooks over IPC (hold store commits, crash). Never set in
    /// production. Default `false`.
    #[serde(default)]
    pub test_hooks: bool,
}

/// Node identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSettings {
    /// Id pattern; names the Windows pipe. Never generated at runtime.
    pub node_id: String,
    /// Operator-facing name, 1–100 characters.
    pub display_name: String,
    /// Default `prod`.
    #[serde(default)]
    pub environment: Environment,
    /// Optional grouping label, ≤ 100 characters.
    #[serde(default)]
    pub site_id: Option<String>,
}

/// Deployment environment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    /// Development.
    Dev,
    /// Test.
    Test,
    /// Production.
    #[default]
    Prod,
}

/// Directories. The state directory holds the message store, saved configuration and the
/// IPC socket; both directories are created owner-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathSettings {
    /// State directory.
    pub state_directory: PathBuf,
    /// Log directory. Default `<state_directory>/logs`.
    #[serde(default)]
    pub log_directory: Option<PathBuf>,
}

/// Management interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcSettings {
    /// Socket path (Unix, ≤ 100 bytes) or pipe name (Windows). Default
    /// `<state_directory>/packie.sock` or `\\.\pipe\packie-<node_id>`.
    #[serde(default)]
    pub path: Option<String>,
    /// OS group whose members may use the management interface besides the service account.
    /// Default `packie` (on Windows the local group `PACK-IE Operators`); `null` leaves only the
    /// service account and administrators.
    #[serde(default = "ipc_group")]
    pub group: Option<String>,
}

impl Default for IpcSettings {
    fn default() -> Self {
        Self {
            path: None,
            group: ipc_group(),
        }
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "serde default of an optional field"
)]
fn ipc_group() -> Option<String> {
    Some("packie".to_owned())
}

/// Process behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostingSettings {
    /// Time channels get to drain at shutdown, 1–300. Default 15.
    #[serde(default = "secs_15")]
    pub shutdown_drain_seconds: u32,
}

impl Default for HostingSettings {
    fn default() -> Self {
        Self {
            shutdown_drain_seconds: secs_15(),
        }
    }
}

/// The runtime document and its saved versions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSettings {
    /// The active document. Default `<state_directory>/runtime.json`.
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// Saved versions kept, at least 1; `null` (the default) keeps every version. Versions
    /// younger than `audit.retention_days` are never removed.
    #[serde(default)]
    pub versions_kept: Option<u32>,
}

/// Log files and the in-memory log ring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingSettings {
    /// `error`, `warn`, `info`, `debug`, `trace`. Default `info`; the `PACKIE_LOG` environment
    /// variable overrides it with a full filter.
    #[serde(default)]
    pub level: LogLevelName,
    /// Days of rotated files to keep, 1–3650. Default 30.
    #[serde(default = "days_30")]
    pub retention_days: u32,
    /// Size at which files rotate, 1–1024. Default 64. Files also rotate at local midnight.
    #[serde(default = "mb_64")]
    pub rotate_at_megabytes: u32,
    /// Events kept in memory for the console and CLI, 1000–1000000. Default 20000.
    #[serde(default = "events_20000")]
    pub ring_buffer_events: u32,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: LogLevelName::default(),
            retention_days: days_30(),
            rotate_at_megabytes: mb_64(),
            ring_buffer_events: events_20000(),
        }
    }
}

/// Log level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevelName {
    /// Failures only.
    Error,
    /// And unexpected situations.
    Warn,
    /// And normal operation.
    #[default]
    Info,
    /// And details.
    Debug,
    /// Everything.
    Trace,
}

impl LogLevelName {
    /// The `tracing` filter directive.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// Snapshot cadence and error retention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsSettings {
    /// 1–60. Default 1.
    #[serde(default = "one")]
    pub snapshot_interval_seconds: u32,
    /// Recent errors kept per channel, 10–10000. Default 100.
    #[serde(default = "errors_100")]
    pub retain_recent_errors: u32,
}

impl Default for DiagnosticsSettings {
    fn default() -> Self {
        Self {
            snapshot_interval_seconds: one(),
            retain_recent_errors: errors_100(),
        }
    }
}

/// Where secret values live.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretsSettings {
    /// Default `keystore` on macOS and Windows, `file` on Linux and other Unix systems
    /// (whose key store, the Secret Service, needs a desktop session).
    #[serde(default)]
    pub provider: SecretsProvider,
    /// With `provider: file`; default `<state_directory>/secrets.json`. Readable by its owner
    /// only; a missing file holds no secrets and is created owner-only on the first write.
    #[serde(default)]
    pub file: Option<PathBuf>,
}

/// Secret provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretsProvider {
    /// The OS key store, service `packie`.
    Keystore,
    /// A JSON file of name to value.
    File,
}

impl Default for SecretsProvider {
    /// The key store where every computer has one (macOS Keychain, Windows Credential
    /// Manager); the secrets file elsewhere.
    fn default() -> Self {
        if cfg!(any(target_os = "macos", windows)) {
            Self::Keystore
        } else {
            Self::File
        }
    }
}

/// The audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditSettings {
    /// Sealed yearly audit files whose newest event is older than this are removed,
    /// 365–36500. Default 2190 (six years).
    #[serde(default = "days_2190")]
    pub retention_days: u32,
}

impl Default for AuditSettings {
    fn default() -> Self {
        Self {
            retention_days: days_2190(),
        }
    }
}

/// Backups.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupSettings {
    /// Where scheduled backups (and backups taken without a directory) go, each in its
    /// own `packie-backup-<UTC time>` directory. Required with `schedule`.
    #[serde(default)]
    pub directory: Option<PathBuf>,
    /// A daily backup.
    #[serde(default)]
    pub schedule: Option<BackupSchedule>,
    /// `backup_stale` fires when the newest successful backup is older than this, 1–8760.
    /// Default 26 with a schedule, otherwise off.
    #[serde(default)]
    pub stale_after_hours: Option<u32>,
}

impl BackupSettings {
    /// The `backup_stale` limit in hours, when it applies.
    #[must_use]
    pub fn stale_limit_hours(&self) -> Option<u32> {
        self.stale_after_hours
            .or_else(|| self.schedule.as_ref().map(|_| 26))
    }
}

/// A daily backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupSchedule {
    /// Local time of day, `HH:MM` (24-hour).
    pub daily_at: String,
    /// Scheduled backups kept in `backup.directory`, 1–365. Default 7.
    #[serde(default = "keep_7")]
    pub keep: u32,
}

impl BackupSchedule {
    /// `daily_at` as hour and minute, when valid.
    #[must_use]
    pub fn time_of_day(&self) -> Option<(i8, i8)> {
        let (hour, minute) = self.daily_at.split_once(':')?;
        if hour.len() != 2 || minute.len() != 2 {
            return None;
        }
        let (hour, minute): (i8, i8) = (hour.parse().ok()?, minute.parse().ok()?);
        ((0..24).contains(&hour) && (0..60).contains(&minute)).then_some((hour, minute))
    }
}

impl Bootstrap {
    /// Reads, validates and resolves the bootstrap file at `path`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Read`] when the file cannot be read; [`ConfigError::Invalid`] with
    /// every problem when it is malformed or invalid.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        let invalid = |problems: Vec<Problem>| ConfigError::Invalid {
            path: path.to_owned(),
            errors: ValidationErrors::from(problems),
        };
        let mut deserializer = serde_json::Deserializer::from_str(&text);
        let mut bootstrap: Self =
            serde_path_to_error::deserialize(&mut deserializer).map_err(|e| {
                let at = e.path().to_string();
                invalid(vec![tagged::into_problem(&at, &e.into_inner().to_string())])
            })?;
        deserializer.end().map_err(|e| {
            invalid(vec![tagged::into_problem(
                "",
                &format!("unexpected text after the document ({e})"),
            )])
        })?;
        let base = std::path::absolute(path)
            .ok()
            .and_then(|p| p.parent().map(Path::to_owned))
            .unwrap_or_default();
        bootstrap.resolve(&base);
        let problems = bootstrap.validate();
        if !problems.is_empty() {
            return Err(invalid(problems));
        }
        Ok(bootstrap)
    }

    fn validate(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut schema = |path: &str, message: &str| {
            problems.push(Problem::error(ProblemCode::Schema, path, message));
        };
        if self.schema_version != 1 {
            schema("schema_version", "must be 1");
        }
        if !packie_core::is_valid_name(&self.node.node_id) {
            schema("node.node_id", "must match ^[a-z0-9][a-z0-9._-]{0,63}$");
        }
        if !(1..=100).contains(&self.node.display_name.trim().chars().count()) {
            schema("node.display_name", "must be 1 to 100 characters");
        }
        if self
            .node
            .site_id
            .as_ref()
            .is_some_and(|s| s.chars().count() > 100)
        {
            schema("node.site_id", "must be at most 100 characters");
        }
        let ranges = [
            (
                "hosting.shutdown_drain_seconds",
                self.hosting.shutdown_drain_seconds,
                1..=300,
            ),
            (
                "logging.retention_days",
                self.logging.retention_days,
                1..=3650,
            ),
            (
                "logging.rotate_at_megabytes",
                self.logging.rotate_at_megabytes,
                1..=1024,
            ),
            (
                "logging.ring_buffer_events",
                self.logging.ring_buffer_events,
                1000..=1_000_000,
            ),
            (
                "diagnostics.snapshot_interval_seconds",
                self.diagnostics.snapshot_interval_seconds,
                1..=60,
            ),
            (
                "diagnostics.retain_recent_errors",
                self.diagnostics.retain_recent_errors,
                10..=10_000,
            ),
        ];
        for (field, value, range) in ranges {
            if !range.contains(&value) {
                schema(
                    field,
                    &format!("must be from {} to {}", range.start(), range.end()),
                );
            }
        }
        if let Some((field, message)) = self.ipc_problem() {
            schema(field, message);
        }
        if self.runtime.versions_kept == Some(0) {
            schema(
                "runtime.versions_kept",
                "must be at least 1, or null to keep every version",
            );
        }
        if let Some(message) = self.secrets_problem() {
            schema("secrets.file", &message);
        }
        for (field, message) in self.audit_backup_problems() {
            schema(field, message);
        }
        problems.extend(self.notifications.problems());
        problems
    }

    fn audit_backup_problems(&self) -> Vec<(&'static str, &'static str)> {
        let mut problems = Vec::new();
        if !(365..=36_500).contains(&self.audit.retention_days) {
            problems.push(("audit.retention_days", "must be from 365 to 36500"));
        }
        let backup = &self.backup;
        if backup
            .stale_after_hours
            .is_some_and(|hours| !(1..=8760).contains(&hours))
        {
            problems.push(("backup.stale_after_hours", "must be from 1 to 8760"));
        }
        if let Some(schedule) = &backup.schedule {
            if schedule.time_of_day().is_none() {
                problems.push(("backup.schedule.daily_at", "must be a time of day HH:MM"));
            }
            if !(1..=365).contains(&schedule.keep) {
                problems.push(("backup.schedule.keep", "must be from 1 to 365"));
            }
            if backup.directory.is_none() {
                problems.push(("backup.directory", "required with backup.schedule"));
            }
        }
        problems
    }

    fn ipc_problem(&self) -> Option<(&'static str, &'static str)> {
        if cfg!(unix) && self.ipc_endpoint().len() > 100 {
            return Some((
                "ipc.path",
                "the socket path must be at most 100 bytes; set a shorter ipc.path",
            ));
        }
        self.ipc
            .group
            .as_deref()
            .is_some_and(str::is_empty)
            .then_some((
                "ipc.group",
                "must be a group name, or null for an owner-only socket",
            ))
    }

    fn secrets_problem(&self) -> Option<String> {
        if self.secrets.provider != SecretsProvider::File {
            return None;
        }
        let file = self.secrets.file.as_ref()?;
        // A client outside the service's group cannot see the file; only the engine (at
        // start, `secret_store_problem`) insists on reaching it.
        crate::secrets::secrets_file_problem(file, false)
    }

    fn resolve(&mut self, base: &Path) {
        let resolve = |p: &mut PathBuf| {
            if p.is_relative() {
                *p = normalize(&base.join(&*p));
            }
        };
        resolve(&mut self.paths.state_directory);
        for path in [
            self.paths.log_directory.as_mut(),
            self.runtime.file.as_mut(),
            self.secrets.file.as_mut(),
            self.backup.directory.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            resolve(path);
        }
        if self.secrets.provider == SecretsProvider::File && self.secrets.file.is_none() {
            self.secrets.file = Some(self.paths.state_directory.join("secrets.json"));
        }
        // An empty CA file stays empty, for validation to name.
        if let Some(ca_file) = self.notifications.ca_file.as_mut()
            && !ca_file.as_os_str().is_empty()
        {
            resolve(ca_file);
        }
    }

    /// The state directory.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.paths.state_directory
    }

    /// The log directory.
    #[must_use]
    pub fn log_dir(&self) -> PathBuf {
        self.paths
            .log_directory
            .clone()
            .unwrap_or_else(|| self.paths.state_directory.join("logs"))
    }

    /// The active runtime document.
    #[must_use]
    pub fn runtime_file(&self) -> PathBuf {
        self.runtime
            .file
            .clone()
            .unwrap_or_else(|| self.paths.state_directory.join("runtime.json"))
    }

    /// Where saved runtime versions live: `<state>/config`.
    #[must_use]
    pub fn config_dir(&self) -> PathBuf {
        self.paths.state_directory.join("config")
    }

    /// The IPC endpoint: the configured one, or the platform default.
    #[must_use]
    pub fn ipc_endpoint(&self) -> String {
        if let Some(endpoint) = &self.ipc.path {
            return endpoint.clone();
        }
        if cfg!(windows) {
            format!(r"\\.\pipe\packie-{}", self.node.node_id)
        } else {
            self.paths
                .state_directory
                .join("packie.sock")
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// Removes `.` and `name/..` pairs without touching the file system, so paths read well
/// in logs and the Unix socket path stays short.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

const fn one() -> u32 {
    1
}
const fn secs_15() -> u32 {
    15
}
const fn days_30() -> u32 {
    30
}
const fn mb_64() -> u32 {
    64
}
const fn events_20000() -> u32 {
    20_000
}
const fn errors_100() -> u32 {
    100
}
const fn days_2190() -> u32 {
    2190
}
const fn keep_7() -> u32 {
    7
}
