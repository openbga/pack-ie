//! Protocol settings of connectors (`docs/spec/configuration.md` §7). Names, defaults and
//! ranges follow that document; [`crate::validate`] enforces the ranges.

mod delimited;
mod hl7;
mod instrument;
pub mod profiles;

use packie_core::{AckLevel, ProtocolKind};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::tagged;

pub use delimited::{Column, ColumnType, DelimitedSettings, LineEnding, OnExists, RecordMode};
pub use hl7::{AckMode, Hl7Settings, ProcessingId, ReplyType};
pub use instrument::{AstmRole, AstmSettings, AuClass, AuFormat, AuFraming, AuProfile, AuSettings};

/// A connector's protocol and its settings, written as `protocol` and `settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "protocol", content = "settings", rename_all = "snake_case")]
pub enum ProtocolSettings {
    /// ASTM E1381/E1394.
    Astm(AstmSettings),
    /// AU400/AU640.
    Au(AuSettings),
    /// HL7 v2 over MLLP.
    #[serde(rename = "hl7_mllp")]
    Hl7(Hl7Settings),
    /// Delimited files.
    #[serde(rename = "delimited_file")]
    Delimited(DelimitedSettings),
    /// Raw bytes; only bridge taps use it, validation rejects it on connectors.
    Raw,
}

impl ProtocolSettings {
    /// The names its messages have, against which filter and mapping paths are checked:
    /// HL7 (also for `raw`), ASTM, AU, or the delimited columns.
    #[must_use]
    pub fn schema(&self) -> packie_transform::Schema {
        match self {
            Self::Hl7(_) | Self::Raw => packie_transform::Schema::hl7(),
            Self::Astm(_) => packie_transform::Schema::astm(),
            Self::Au(_) => packie_transform::Schema::au(),
            Self::Delimited(settings) => {
                packie_transform::Schema::delimited(settings.columns.iter().map(|c| c.name.clone()))
            }
        }
    }

    /// Builds the settings of `protocol` from its `settings` value (an empty object when
    /// the document has none), reporting failures under `settings`.
    pub(crate) fn from_parts<'de, D: serde::Deserializer<'de>>(
        protocol: &str,
        settings: serde_json::Value,
    ) -> Result<Self, D::Error> {
        Ok(match protocol {
            "astm" => Self::Astm(tagged::at::<D, _>(settings, "settings")?),
            "au" => Self::Au(tagged::at::<D, _>(settings, "settings")?),
            "hl7_mllp" => Self::Hl7(tagged::at::<D, _>(settings, "settings")?),
            "delimited_file" => Self::Delimited(tagged::at::<D, _>(settings, "settings")?),
            "raw" => Self::Raw,
            other => {
                let names = ProtocolKind::ALL
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>();
                return Err(tagged::unknown::<D>("protocol", other, &names));
            }
        })
    }

    /// The protocol kind.
    #[must_use]
    pub const fn kind(&self) -> ProtocolKind {
        match self {
            Self::Astm(_) => ProtocolKind::Astm,
            Self::Au(_) => ProtocolKind::Au,
            Self::Hl7(_) => ProtocolKind::Hl7,
            Self::Delimited(_) => ProtocolKind::Delimited,
            Self::Raw => ProtocolKind::Raw,
        }
    }

    /// The highest acknowledgement level this protocol can prove as a destination
    /// (`docs/spec/message-channels.md` §8), which is also the default `ack_required`.
    #[must_use]
    pub const fn highest_ack_level(&self) -> AckLevel {
        match self {
            Self::Delimited(_) | Self::Raw => AckLevel::HandedOff,
            Self::Hl7(s) if matches!(s.ack_mode, AckMode::None) => AckLevel::HandedOff,
            Self::Hl7(_) => AckLevel::Accepted,
            // Class A instruments send no acknowledgement.
            Self::Au(s) if matches!(s.framing.class, AuClass::A) => AckLevel::HandedOff,
            Self::Astm(_) | Self::Au(_) => AckLevel::Received,
        }
    }

    /// Why [`highest_ack_level`](Self::highest_ack_level) is the limit, for problems.
    #[must_use]
    pub fn ack_limit(&self) -> String {
        let highest = self.highest_ack_level();
        match self {
            Self::Au(s) if matches!(s.framing.class, AuClass::A) => {
                format!("au class A sends no acknowledgement, so at most {highest}")
            }
            Self::Hl7(s) if matches!(s.ack_mode, AckMode::None) => {
                format!("hl7 with ack_mode none can prove at most {highest}")
            }
            _ => format!("{} can prove at most {highest}", self.kind()),
        }
    }
}

/// Character set of text protocols.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Charset {
    /// 7-bit ASCII.
    Ascii,
    /// ISO-8859-1; never fails to decode.
    Latin1,
    /// UTF-8.
    #[default]
    Utf8,
}
