//! The shipped instrument profiles: connector setting patches and mapping presets
//! (`docs/spec/configuration.md` §7.6). They are data in `profiles/profiles.json`; applying
//! one copies explicit values into a connector or mapping, and nothing in the engine knows
//! a profile by name.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const CATALOG: &str = include_str!("../../profiles/profiles.json");

/// Which side of a message channel a connector profile fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRole {
    /// The source connector.
    Source,
    /// A destination connector.
    Destination,
}

/// A patch of one protocol's connector settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorProfile {
    /// Stable id.
    pub id: String,
    /// Short name for a picker.
    pub label: String,
    /// `astm`, `au` or `hl7_mllp`.
    pub protocol: String,
    /// The connector roles it fits.
    pub roles: Vec<ConnectorRole>,
    /// What it changes, in a sentence or two.
    pub summary: String,
    /// The settings to set; other settings stay as they are.
    pub settings: Value,
    /// Where the behavior is documented.
    pub evidence: String,
}

/// A synthetic message and the output the mapping must produce for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Header fields the pipeline generates for an HL7 destination (`MSH.3` and so on);
    /// the output below was produced with these.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub header: BTreeMap<String, String>,
    /// Source connector settings needed to read `input` (the AU sheet, the ASTM specimen
    /// path), as a patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_settings: Option<Value>,
    /// The source message; records and segments end with CR.
    pub input: String,
    /// The expected destination message.
    pub output: String,
}

/// A mapping for a pair of protocols.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingProfile {
    /// Stable id.
    pub id: String,
    /// Short name for a picker.
    pub label: String,
    /// Source protocol (`astm`, `au`, `hl7_mllp`).
    pub from: String,
    /// Destination protocol.
    pub to: String,
    /// What it does, in a sentence or two.
    pub summary: String,
    /// Limits and assumptions shown next to the preset.
    #[serde(default)]
    pub notes: Vec<String>,
    /// The destination connector's `mapping` object (`configuration.md` §8).
    pub mapping: Value,
    /// Settings of the destination connector to set with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_settings: Option<Value>,
    /// An example for the console's dry run and for the tests.
    pub sample: Sample,
    /// Where the field positions come from.
    pub evidence: String,
}

/// All shipped profiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Layout version of the file; 1.
    pub format: u32,
    /// What the profiles are and are not: built from documentation and synthetic fixtures,
    /// not qualified against hardware. The console shows it next to the picker.
    pub qualification: String,
    /// Connector setting patches.
    pub connectors: Vec<ConnectorProfile>,
    /// Mapping presets.
    pub mappings: Vec<MappingProfile>,
}

impl Catalog {
    /// The connector profile with `id`.
    #[must_use]
    pub fn connector(&self, id: &str) -> Option<&ConnectorProfile> {
        self.connectors.iter().find(|p| p.id == id)
    }

    /// The mapping preset with `id`.
    #[must_use]
    pub fn mapping(&self, id: &str) -> Option<&MappingProfile> {
        self.mappings.iter().find(|p| p.id == id)
    }
}

/// The shipped catalog.
///
/// # Errors
/// The embedded file does not parse; a test keeps that from happening.
pub fn catalog() -> Result<Catalog, serde_json::Error> {
    serde_json::from_str(CATALOG)
}
