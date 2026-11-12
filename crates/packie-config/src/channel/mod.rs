//! Channels (`docs/spec/configuration.md` §6): bridge (live bytes between two endpoints)
//! and message (store-and-forward), tagged by `kind`.

mod bridge;
mod policy;

use std::collections::BTreeMap;

use packie_core::{ChannelId, ChannelKind, ConnectorId, EndpointId};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

use crate::defaults::yes;
use crate::protocol::ProtocolSettings;
use crate::tagged::{self, tagged_deserialize};

pub use bridge::{
    BridgeChannel, BufferPolicy, ConnectStrategy, Connection, OverflowAction, Tap, TapFramer,
};
pub use policy::{
    DeliveryOverrides, DeliveryPolicy, EffectiveDelivery, Retention, RetryPolicy, TapRetention,
};

/// A configured channel, tagged by `kind`.
// A document holds a few dozen channels; boxing the larger variant buys nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChannelConfig {
    /// Bytes pass unchanged between two endpoints.
    Bridge(BridgeChannel),
    /// Messages are stored, filtered, transformed and delivered.
    Message(MessageChannel),
}

tagged_deserialize!(ChannelConfig, "kind", {
    "bridge" => Bridge(BridgeChannel),
    "message" => Message(MessageChannel),
});

impl ChannelConfig {
    /// Stable operator-chosen id.
    #[must_use]
    pub const fn id(&self) -> ChannelId {
        match self {
            Self::Bridge(c) => c.id,
            Self::Message(c) => c.id,
        }
    }

    /// Operator-facing name.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Bridge(c) => &c.label,
            Self::Message(c) => &c.label,
        }
    }

    /// Whether the channel should run.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        match self {
            Self::Bridge(c) => c.enabled,
            Self::Message(c) => c.enabled,
        }
    }

    /// The channel kind.
    #[must_use]
    pub const fn kind(&self) -> ChannelKind {
        match self {
            Self::Bridge(_) => ChannelKind::Bridge,
            Self::Message(_) => ChannelKind::Message,
        }
    }

    /// Free operator metadata.
    #[must_use]
    pub const fn tags(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Bridge(c) => &c.tags,
            Self::Message(c) => &c.tags,
        }
    }

    /// Every endpoint the channel uses, in document order, without repeats.
    #[must_use]
    pub fn endpoint_ids(&self) -> Vec<EndpointId> {
        let mut ids = match self {
            Self::Bridge(c) => vec![c.endpoint_a, c.endpoint_b],
            Self::Message(c) => std::iter::once(c.source.endpoint)
                .chain(c.destinations.iter().map(|d| d.endpoint))
                .collect(),
        };
        let mut seen = std::collections::HashSet::new();
        ids.retain(|id| seen.insert(*id));
        ids
    }
}

/// A message channel (§6.4): one source, filters, one or more destinations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MessageChannel {
    /// Stable operator-chosen id.
    #[serde(rename = "channel_id")]
    pub id: ChannelId,
    /// Operator-facing name, 1–100 characters.
    pub label: String,
    /// Default `true`.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Free operator metadata.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// Where messages come from.
    pub source: SourceConnector,
    /// Ordered filter expressions (§8.1); a message must satisfy all. Interpreted by
    /// `packie-transform`.
    #[serde(default)]
    #[ts(type = "Array<unknown>")]
    pub filters: Vec<serde_json::Value>,
    /// 1–20 destinations; `destination_id` unique within the channel.
    pub destinations: Vec<Destination>,
    /// Reconnecting and reopening the channel's endpoints.
    #[serde(default)]
    pub retry: RetryPolicy,
    /// Default delivery policy of the destinations.
    #[serde(default)]
    pub delivery: DeliveryPolicy,
    /// How long stored messages are kept.
    #[serde(default)]
    pub retention: Retention,
}

/// The source connector: an endpoint bound to a protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct SourceConnector {
    /// The endpoint.
    pub endpoint: EndpointId,
    /// `protocol` and `settings`.
    #[serde(flatten)]
    pub protocol: ProtocolSettings,
    /// Default `true`.
    pub duplicate_detection: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceCommon {
    endpoint: EndpointId,
    #[serde(default = "yes")]
    duplicate_detection: bool,
}

impl<'de> Deserialize<'de> for SourceConnector {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        let (protocol, settings) = tagged::split_kind::<D>(&mut map, "protocol", "settings")?;
        let common: SourceCommon = tagged::variant::<D, _>(map)?;
        Ok(Self {
            endpoint: common.endpoint,
            protocol: ProtocolSettings::from_parts::<D>(&protocol, settings)?,
            duplicate_detection: common.duplicate_detection,
        })
    }
}

/// A destination connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Destination {
    /// Operator-chosen id, unique within the channel.
    #[serde(rename = "destination_id")]
    pub id: ConnectorId,
    /// Operator-facing name, 1–100 characters.
    pub label: String,
    /// A disabled destination gets no new deliveries; queued ones wait.
    pub enabled: bool,
    /// The endpoint.
    pub endpoint: EndpointId,
    /// `protocol` and `settings`.
    #[serde(flatten)]
    pub protocol: ProtocolSettings,
    /// Destination filters; all must pass for a delivery to be created.
    #[ts(type = "Array<unknown>")]
    pub filters: Vec<serde_json::Value>,
    /// Mapping (§8.2); `null` passes the source message through and needs the source's
    /// protocol.
    #[ts(type = "unknown")]
    pub mapping: Option<serde_json::Value>,
    /// Field-by-field overrides of the channel's delivery policy.
    pub delivery: DeliveryOverrides,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DestinationCommon {
    destination_id: ConnectorId,
    label: String,
    #[serde(default = "yes")]
    enabled: bool,
    endpoint: EndpointId,
    #[serde(default)]
    filters: Vec<serde_json::Value>,
    #[serde(default)]
    mapping: Option<serde_json::Value>,
    #[serde(default)]
    delivery: DeliveryOverrides,
}

impl<'de> Deserialize<'de> for Destination {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        let (protocol, settings) = tagged::split_kind::<D>(&mut map, "protocol", "settings")?;
        let common: DestinationCommon = tagged::variant::<D, _>(map)?;
        Ok(Self {
            id: common.destination_id,
            label: common.label,
            enabled: common.enabled,
            endpoint: common.endpoint,
            protocol: ProtocolSettings::from_parts::<D>(&protocol, settings)?,
            filters: common.filters,
            mapping: common.mapping,
            delivery: common.delivery,
        })
    }
}

impl Destination {
    /// The channel's delivery policy with this destination's overrides applied, and
    /// `ack_required` resolved to the protocol's highest level when unset.
    #[must_use]
    pub fn effective_delivery(&self, channel: &DeliveryPolicy) -> EffectiveDelivery {
        let o = &self.delivery;
        EffectiveDelivery {
            max_attempts: o.max_attempts.unwrap_or(channel.max_attempts),
            retry: o.retry.clone().unwrap_or_else(|| channel.retry.clone()),
            send_timeout_seconds: o
                .send_timeout_seconds
                .unwrap_or(channel.send_timeout_seconds),
            ack_timeout_seconds: o.ack_timeout_seconds.unwrap_or(channel.ack_timeout_seconds),
            ack_required: o
                .ack_required
                .or(channel.ack_required)
                .unwrap_or_else(|| self.protocol.highest_ack_level()),
            retry_unacknowledged: o
                .retry_unacknowledged
                .unwrap_or(channel.retry_unacknowledged),
        }
    }
}

#[cfg(test)]
mod tests {
    use packie_core::AckLevel;
    use rust_decimal::Decimal;

    use super::*;

    #[test]
    fn bridge_defaults_fill_policies() {
        let json = serde_json::json!({
            "kind": "bridge", "channel_id": "hema-bridge", "label": "Analyzer to LIS",
            "endpoint_a": "hema-serial", "endpoint_b": "lis-listener",
            "retry": {"multiplier": 1.5},
        });
        let channel: ChannelConfig = serde_json::from_value(json).unwrap();
        let ChannelConfig::Bridge(bridge) = &channel else {
            panic!("expected bridge");
        };
        assert!(bridge.enabled);
        assert_eq!(bridge.connection, Connection::default());
        assert_eq!(bridge.buffer.max_bytes, 10 * 1024 * 1024);
        assert_eq!(bridge.retry.multiplier, Decimal::new(15, 1));
        assert_eq!(bridge.tap.framer, TapFramer::None);
        let back = serde_json::to_value(&channel).unwrap();
        assert_eq!(back["retry"]["multiplier"], 1.5);
        assert_eq!(back["channel_id"], "hema-bridge");
        assert_eq!(channel.endpoint_ids().len(), 2);
    }

    #[test]
    fn destinations_inherit_the_channel_delivery_policy() {
        let json = serde_json::json!({
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-hl7",
            "protocol": "hl7_mllp", "settings": {"ack_mode": "enhanced"},
            "delivery": {"max_attempts": 9},
        });
        let destination: Destination = serde_json::from_value(json).unwrap();
        let effective = destination.effective_delivery(&DeliveryPolicy::default());
        assert_eq!(effective.max_attempts, 9);
        assert_eq!(effective.ack_timeout_seconds, 30);
        assert_eq!(effective.ack_required, AckLevel::Accepted);
        let back = serde_json::to_value(&destination).unwrap();
        assert_eq!(back["protocol"], "hl7_mllp");
        assert_eq!(back["settings"]["ack_mode"], "enhanced");
    }
}
