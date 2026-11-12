//! The runtime document (`docs/spec/configuration.md` §4): endpoints, channels, alert
//! thresholds and diagnostics overrides.

use packie_core::{ChannelId, EndpointId, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::channel::ChannelConfig;
use crate::defaults::{secs_10, secs_60};
use crate::endpoint::EndpointConfig;
use crate::tagged;
use crate::validate::{Problem, ValidationErrors, validate};

/// The supported `schema_version`.
pub const SCHEMA_VERSION: u32 = 1;

/// `runtime.json`: everything the engine runs. Versioned; changed through IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// Must be 1. Default 1.
    #[serde(default = "schema_1")]
    pub schema_version: u32,
    /// Assigned by the engine at every successful apply (previous + 1); a value in an
    /// incoming document is ignored.
    #[serde(default)]
    #[ts(type = "number")]
    pub config_version: u64,
    /// When this version was saved; assigned. Informational.
    #[serde(default = "Timestamp::now")]
    pub generated_at: Timestamp,
    /// Overrides of bootstrap diagnostics settings.
    #[serde(default)]
    pub diagnostics: DiagnosticsOverrides,
    /// Alert thresholds.
    #[serde(default)]
    pub alerts: AlertThresholds,
    /// Configured endpoints.
    #[serde(default)]
    pub endpoints: Vec<EndpointConfig>,
    /// Configured channels.
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
}

impl RuntimeConfig {
    /// Recent errors to keep per channel: the document's `diagnostics.retain_recent_errors`,
    /// else the bootstrap's.
    #[must_use]
    pub fn retain_recent_errors(&self, bootstrap: &crate::Bootstrap) -> usize {
        let kept = self
            .diagnostics
            .retain_recent_errors
            .unwrap_or(bootstrap.diagnostics.retain_recent_errors);
        usize::try_from(kept).unwrap_or(usize::MAX)
    }

    /// A document with no endpoints or channels, version 0.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            config_version: 0,
            generated_at: Timestamp::now(),
            diagnostics: DiagnosticsOverrides::default(),
            alerts: AlertThresholds::default(),
            endpoints: Vec::new(),
            channels: Vec::new(),
        }
    }

    /// Parses and validates a document. On success returns it with its warnings; on any
    /// error returns every problem (errors and warnings) with its field path.
    ///
    /// A document that is not well-formed JSON or does not match the schema yields one
    /// `schema` problem at the failing path.
    ///
    /// # Errors
    ///
    /// [`ValidationErrors`] with every problem when the document is rejected.
    pub fn parse(text: &str) -> Result<Self, ValidationErrors> {
        let config = Self::from_text(text)?;
        validate(&config)?;
        Ok(config)
    }

    /// Deserializes one JSON document from `text`; anything but whitespace after it is a
    /// `schema` problem.
    fn from_text(text: &str) -> Result<Self, ValidationErrors> {
        let mut deserializer = serde_json::Deserializer::from_str(text);
        let config = Self::deserialize_checked(&mut deserializer)?;
        deserializer.end().map_err(|e| {
            ValidationErrors::from(vec![tagged::into_problem(
                "",
                &format!("unexpected text after the document ({e})"),
            )])
        })?;
        Ok(config)
    }

    /// Like [`parse`](Self::parse), from an already parsed JSON value (as received over
    /// IPC); problems carry field paths but no line numbers.
    ///
    /// # Errors
    ///
    /// As for [`parse`](Self::parse).
    pub fn from_value(value: serde_json::Value) -> Result<Self, ValidationErrors> {
        Self::checked(value)
    }

    /// Like [`parse`](Self::parse), also returning the warnings of a valid document.
    ///
    /// # Errors
    ///
    /// As for [`parse`](Self::parse).
    pub fn parse_with_warnings(text: &str) -> Result<(Self, Vec<Problem>), ValidationErrors> {
        let config = Self::from_text(text)?;
        let warnings = validate(&config)?;
        Ok((config, warnings))
    }

    fn checked<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, ValidationErrors> {
        let config = Self::deserialize_checked(deserializer)?;
        validate(&config)?;
        Ok(config)
    }

    fn deserialize_checked<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, ValidationErrors> {
        serde_path_to_error::deserialize(deserializer).map_err(|err| {
            let path = err.path().to_string();
            let message = err.into_inner().to_string();
            ValidationErrors::from(vec![tagged::into_problem(&path, &message)])
        })
    }

    /// The document as pretty JSON, as written to disk and shown to operators.
    ///
    /// # Errors
    ///
    /// The serialization error, which a valid document does not produce.
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// The endpoint with `id`.
    #[must_use]
    pub fn endpoint(&self, id: EndpointId) -> Option<&EndpointConfig> {
        self.endpoints.iter().find(|e| e.id == id)
    }

    /// The channel with `id`.
    #[must_use]
    pub fn channel(&self, id: ChannelId) -> Option<&ChannelConfig> {
        self.channels.iter().find(|c| c.id() == id)
    }
}

/// Runtime overrides of the bootstrap `diagnostics` settings (§4.2); absent fields inherit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsOverrides {
    /// 1–60.
    #[serde(default)]
    pub snapshot_interval_seconds: Option<u32>,
    /// 10–10000.
    #[serde(default)]
    pub retain_recent_errors: Option<u32>,
}

/// Alert thresholds (§4.3). `0` disables an alert where noted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AlertThresholds {
    /// `endpoint_down` warning after this long; 1–86400. Default 60.
    #[serde(default = "secs_60")]
    pub endpoint_down_seconds: u32,
    /// `endpoint_down` critical; ≥ `endpoint_down_seconds`. Default 900.
    #[serde(default = "secs_900")]
    pub endpoint_down_critical_seconds: u32,
    /// `no_peer_connected` after this long without a peer; 0 (default) disables.
    #[serde(default)]
    pub no_peer_seconds: u32,
    /// `bridge_buffer_pressure` above this fill; 1–100. Default 80.
    #[serde(default = "percent_80")]
    pub buffer_high_percent: u8,
    /// ... for this long; 1–3600. Default 10.
    #[serde(default = "secs_10")]
    pub buffer_high_seconds: u32,
    /// `delivery_backlog` above this many queued; 0 disables. Default 1000.
    #[serde(default = "count_1000")]
    pub delivery_backlog_count: u32,
    /// `delivery_backlog` when the oldest queued is older; 0 disables. Default 300.
    #[serde(default = "secs_300")]
    pub delivery_backlog_age_seconds: u32,
    /// `delivery_needs_attention` turns critical at this many. Default 10.
    #[serde(default = "count_10")]
    pub delivery_attention_critical_count: u32,
    /// `invalid_messages` at this many in the window; 0 disables. Default 5.
    #[serde(default = "count_5")]
    pub invalid_messages_count: u32,
    /// Window of `invalid_messages`; 1–86400. Default 600.
    #[serde(default = "secs_600")]
    pub invalid_messages_window_seconds: u32,
    /// `disk_low` warning below this free space. Default 1024.
    #[serde(default = "mib_1024")]
    pub disk_low_megabytes: u32,
    /// `disk_low` critical; ≤ `disk_low_megabytes`. Default 256.
    #[serde(default = "mib_256")]
    pub disk_critical_megabytes: u32,
}

impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            endpoint_down_seconds: secs_60(),
            endpoint_down_critical_seconds: secs_900(),
            no_peer_seconds: 0,
            buffer_high_percent: percent_80(),
            buffer_high_seconds: secs_10(),
            delivery_backlog_count: count_1000(),
            delivery_backlog_age_seconds: secs_300(),
            delivery_attention_critical_count: count_10(),
            invalid_messages_count: count_5(),
            invalid_messages_window_seconds: secs_600(),
            disk_low_megabytes: mib_1024(),
            disk_critical_megabytes: mib_256(),
        }
    }
}

const fn schema_1() -> u32 {
    SCHEMA_VERSION
}
const fn secs_300() -> u32 {
    300
}
const fn secs_600() -> u32 {
    600
}
const fn secs_900() -> u32 {
    900
}
const fn percent_80() -> u8 {
    80
}
const fn count_5() -> u32 {
    5
}
const fn count_10() -> u32 {
    10
}
const fn count_1000() -> u32 {
    1000
}
const fn mib_256() -> u32 {
    256
}
const fn mib_1024() -> u32 {
    1024
}
