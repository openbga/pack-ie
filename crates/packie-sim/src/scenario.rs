//! The scenario file model (scenarios.md §2, §5, §6, §7).
//!
//! Loading is two-stage so that placeholders can be resolved per run:
//!
//! 1. [`Scenario::load`] parses the file into [`Scenario`]; `engine`, `actors` and `steps`
//!    stay raw JSON because they contain placeholders.
//! 2. [`Scenario::resolve`] substitutes placeholders and parses the result into typed
//!    [`Actor`] and [`Step`] values. Unknown fields, unknown steps and unresolved
//!    placeholders are errors here, which is why `check` simply resolves with dummy values.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::error::ScenarioError;
use crate::placeholders::Placeholders;
use crate::steps::{Action, parse_step};

/// A host capability a scenario may require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Virtual serial port pairs.
    SerialPair,
    /// Engine test hooks (store commit gate and crash).
    TestHooks,
    /// A second loopback address such as 127.0.0.2.
    LoopbackAlias,
}

impl Capability {
    /// The name used in scenario files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SerialPair => "serial_pair",
            Self::TestHooks => "test_hooks",
            Self::LoopbackAlias => "loopback_alias",
        }
    }
}

/// Default timeouts.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Default `timeout_ms` of waiting steps.
    #[serde(default = "five_seconds")]
    pub step_timeout_ms: u64,
    /// Default `within_ms` of assertion steps.
    #[serde(default = "five_seconds")]
    pub assert_within_ms: u64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            step_timeout_ms: five_seconds(),
            assert_within_ms: five_seconds(),
        }
    }
}

const fn five_seconds() -> u64 {
    5000
}

/// Named allocations.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    /// Free loopback TCP ports the engine will bind.
    #[serde(default)]
    pub ports: Vec<String>,
    /// Virtual serial pairs.
    #[serde(default)]
    pub serial_pairs: Vec<String>,
    /// Temporary folders.
    #[serde(default)]
    pub folders: Vec<String>,
}

/// A scenario file as written, before placeholder substitution.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Format version, `1`.
    pub format: u32,
    /// `^[a-z0-9-]+$`, equal to the file name.
    pub name: String,
    /// One sentence.
    pub description: String,
    /// Requirement ids proven.
    pub requirements: Vec<String>,
    /// Capabilities the host must provide.
    #[serde(default)]
    pub requires: Vec<Capability>,
    /// Whole-scenario limit.
    pub timeout_ms: u64,
    /// Timeout defaults.
    #[serde(default)]
    pub defaults: Defaults,
    /// Resources.
    #[serde(default)]
    pub resources: Resources,
    /// Engine section, raw until resolved.
    pub engine: Value,
    /// Actors, raw until resolved.
    #[serde(default)]
    pub actors: BTreeMap<String, Value>,
    /// Steps, raw until resolved.
    pub steps: Vec<Value>,
    /// Sentinels that must appear nowhere in diagnostics.
    #[serde(default)]
    pub no_body_leak: Vec<String>,
}

/// How the engine starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    /// Before step 1.
    #[default]
    Auto,
    /// At the first `start_engine` step.
    Manual,
}

/// The engine section after substitution.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSection {
    /// Fields merged over the generated bootstrap.
    #[serde(default)]
    pub bootstrap: Map<String, Value>,
    /// The runtime document.
    #[serde(default)]
    pub runtime: Option<Value>,
    /// A runtime document file relative to the scenario file.
    #[serde(default)]
    pub runtime_file: Option<String>,
    /// Start mode.
    #[serde(default)]
    pub start: StartMode,
}

/// An actor definition after substitution.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// Accepts connections.
    TcpServer(TcpServerDef),
    /// Connects out.
    TcpClient(TcpClientDef),
    /// Drives the peer end of a serial pair.
    SerialPeer(SerialPeerDef),
    /// Owns a folder.
    Folder(FolderDef),
}

/// `tcp_server` fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TcpServerDef {
    /// Bind address.
    #[serde(default = "loopback")]
    pub bind: String,
    /// `auto` binds at scenario start, `manual` at a `listen` step.
    #[serde(default)]
    pub listen: ListenMode,
    /// Listen backlog.
    #[serde(default = "sixteen")]
    pub backlog: u32,
    /// Receive buffer size option.
    #[serde(default)]
    pub recv_buffer_bytes: Option<u32>,
}

/// When a server binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    /// At scenario start.
    #[default]
    Auto,
    /// At a `listen` step.
    Manual,
}

fn loopback() -> String {
    "127.0.0.1".to_owned()
}

const fn sixteen() -> u32 {
    16
}

/// `tcp_client` fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TcpClientDef {
    /// Local address to connect from.
    #[serde(default)]
    pub bind_source: Option<String>,
    /// Receive buffer size option.
    #[serde(default)]
    pub recv_buffer_bytes: Option<u32>,
}

/// `serial_peer` fields. Line settings are validated and recorded; a virtual pair has no
/// line rate, so they do not change behavior.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SerialPeerDef {
    /// A `serial_pairs` name.
    pub pair: String,
    /// Baud rate.
    #[serde(default = "baud_9600")]
    pub baud_rate: u32,
    /// Data bits.
    #[serde(default = "eight")]
    pub data_bits: u8,
    /// `none`, `even` or `odd`.
    #[serde(default = "none_text")]
    pub parity: String,
    /// `one` or `two`.
    #[serde(default = "one_text")]
    pub stop_bits: String,
    /// `none`, `rts_cts` or `xon_xoff`.
    #[serde(default = "none_text")]
    pub flow_control: String,
}

const fn baud_9600() -> u32 {
    9600
}
const fn eight() -> u8 {
    8
}
fn none_text() -> String {
    "none".to_owned()
}
fn one_text() -> String {
    "one".to_owned()
}

/// `folder` fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderDef {
    /// A `folders` name.
    pub resource: String,
}

impl Actor {
    /// The kind name used in messages.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::TcpServer(_) => "tcp_server",
            Self::TcpClient(_) => "tcp_client",
            Self::SerialPeer(_) => "serial_peer",
            Self::Folder(_) => "folder",
        }
    }

    /// Whether the actor has a connection (TCP or serial).
    #[must_use]
    pub const fn has_connection(&self) -> bool {
        !matches!(self, Self::Folder(_))
    }
}

/// The reserved pseudo-actor of engine steps.
pub const ENGINE_ACTOR: &str = "engine";

/// One step after substitution.
#[derive(Debug, Clone)]
pub struct Step {
    /// Position in the file, from 0.
    pub index: usize,
    /// Optional label.
    pub id: Option<String>,
    /// Actor name or `engine`.
    pub actor: String,
    /// What to do.
    pub action: Action,
}

/// A scenario with placeholders substituted and everything typed.
#[derive(Debug, Clone)]
pub struct ResolvedScenario {
    /// The scenario's name.
    pub name: String,
    /// Whole-scenario limit.
    pub timeout_ms: u64,
    /// Timeout defaults.
    pub defaults: Defaults,
    /// Capabilities required.
    pub requires: BTreeSet<Capability>,
    /// Engine section.
    pub engine: EngineSection,
    /// Actors by name.
    pub actors: BTreeMap<String, Actor>,
    /// Steps in order.
    pub steps: Vec<Step>,
    /// Sentinels for the no-body-leak check.
    pub no_body_leak: Vec<String>,
}

impl Scenario {
    /// Reads and statically validates the file at `path`.
    pub fn load(path: &Path) -> Result<Self, ScenarioError> {
        let text = std::fs::read_to_string(path).map_err(|source| ScenarioError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&text, path)
    }

    /// Parses `text`; `path` names the file in messages and supplies the expected name.
    pub fn parse(text: &str, path: &Path) -> Result<Self, ScenarioError> {
        let deserializer = &mut serde_json::Deserializer::from_str(text);
        let scenario: Self = serde_path_to_error::deserialize(deserializer)
            .map_err(|e| ScenarioError::invalid(path, format!("{}: {}", e.path(), e.inner())))?;
        scenario.check_header(path)?;
        scenario.resolve(&Placeholders::dry(&scenario), path)?;
        Ok(scenario)
    }

    fn check_header(&self, path: &Path) -> Result<(), ScenarioError> {
        let bad = |m: String| ScenarioError::invalid(path, m);
        if self.format != 1 {
            return Err(bad(format!("format must be 1, found {}", self.format)));
        }
        let name_ok = !self.name.is_empty()
            && self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !name_ok {
            return Err(bad(format!("name `{}` must match ^[a-z0-9-]+$", self.name)));
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && !stem.is_empty()
            && stem != self.name
        {
            return Err(bad(format!(
                "name `{}` must equal the file name `{stem}`",
                self.name
            )));
        }
        if self.steps.is_empty() {
            return Err(bad("steps must not be empty".into()));
        }
        if self.timeout_ms == 0 {
            return Err(bad("timeout_ms must be greater than 0".into()));
        }
        let mut seen = BTreeSet::new();
        let resource_names = self
            .resources
            .ports
            .iter()
            .map(|n| ("ports", n))
            .chain(
                self.resources
                    .serial_pairs
                    .iter()
                    .map(|n| ("serial_pairs", n)),
            )
            .chain(self.resources.folders.iter().map(|n| ("folders", n)));
        for (kind, name) in resource_names {
            if !is_identifier(name) {
                return Err(bad(format!(
                    "resources.{kind}: `{name}` is not a valid name"
                )));
            }
            if !seen.insert((kind, name.as_str())) {
                return Err(bad(format!("resources.{kind}: `{name}` is declared twice")));
            }
        }
        for name in self.actors.keys() {
            if name == ENGINE_ACTOR || !is_identifier(name) {
                return Err(bad(format!(
                    "actor name `{name}` must match ^[a-z][a-z0-9_]*$ and not be `engine`"
                )));
            }
        }
        Ok(())
    }

    /// Substitutes placeholders and types the engine section, actors and steps.
    pub fn resolve(
        &self,
        placeholders: &Placeholders,
        path: &Path,
    ) -> Result<ResolvedScenario, ScenarioError> {
        let bad = |m: String| ScenarioError::invalid(path, m);
        let engine_value = placeholders
            .substitute(&self.engine, "engine")
            .map_err(&bad)?;
        let engine: EngineSection =
            from_json(engine_value).map_err(|e| bad(format!("engine: {e}")))?;
        if engine.runtime.is_some() == engine.runtime_file.is_some() {
            return Err(bad(
                "engine: exactly one of `runtime` and `runtime_file` is required".into(),
            ));
        }

        let mut actors = BTreeMap::new();
        for (name, raw) in &self.actors {
            let value = placeholders
                .substitute(raw, &format!("actors.{name}"))
                .map_err(&bad)?;
            let actor: Actor = from_json(value).map_err(|e| bad(format!("actors.{name}: {e}")))?;
            actors.insert(name.clone(), actor);
        }

        let mut steps = Vec::with_capacity(self.steps.len());
        for (index, raw) in self.steps.iter().enumerate() {
            let value = placeholders
                .substitute(raw, &format!("steps[{index}]"))
                .map_err(&bad)?;
            steps.push(parse_step(index, value).map_err(&bad)?);
        }

        let resolved = ResolvedScenario {
            name: self.name.clone(),
            timeout_ms: self.timeout_ms,
            defaults: self.defaults,
            requires: self.requires.iter().copied().collect(),
            engine,
            actors,
            steps,
            no_body_leak: self.no_body_leak.clone(),
        };
        crate::validate::check(self, &resolved, path.parent().unwrap_or(Path::new(".")))
            .map_err(bad)?;
        Ok(resolved)
    }
}

/// Deserializes `value`, naming the failing field path in the error.
pub(crate) fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_path_to_error::deserialize(value).map_err(|e| {
        let path = e.path().to_string();
        if path == "." {
            e.inner().to_string()
        } else {
            format!("{path}: {}", e.inner())
        }
    })
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
