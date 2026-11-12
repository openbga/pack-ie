//! Typed UUIDv7 identifiers.
//!
//! The marker type `K` names the entity, so a [`MessageId`] can never be passed where a
//! [`DeliveryId`] is expected. Another crate that needs an identifier declares a marker
//! and an alias:
//!
//! ```
//! use packie_core::Id;
//!
//! pub enum Attempt {}
//! pub type AttemptId = Id<Attempt>;
//!
//! let id = AttemptId::generate();
//! assert_eq!(id.to_string().parse::<AttemptId>(), Ok(id));
//! ```

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::error::ParseError;

// Marker types for the identifiers defined here; the aliases below are their public names.
pub(crate) mod kinds {
    /// Marker for [`ChannelId`](crate::ChannelId).
    pub enum Channel {}
    /// Marker for [`EndpointId`](crate::EndpointId).
    pub enum Endpoint {}
    /// Marker for [`ConnectorId`](crate::ConnectorId).
    pub enum Connector {}
    /// Marker for [`MessageId`](crate::MessageId).
    pub enum Message {}
    /// Marker for [`DeliveryId`](crate::DeliveryId).
    pub enum Delivery {}
    /// Marker for [`AlertId`](crate::AlertId).
    pub enum Alert {}
}

/// A bridge or message channel (`channel_id`, operator-chosen).
pub type ChannelId = crate::Name<kinds::Channel>;
/// A configured endpoint (`endpoint_id`, operator-chosen).
pub type EndpointId = crate::Name<kinds::Endpoint>;
/// A destination of a message channel (`destination_id`, operator-chosen, unique within
/// its channel).
pub type ConnectorId = crate::Name<kinds::Connector>;
/// One stored message.
pub type MessageId = Id<kinds::Message>;
/// One message to one destination.
pub type DeliveryId = Id<kinds::Delivery>;
/// One raised alert.
pub type AlertId = Id<kinds::Alert>;

/// A UUIDv7 identifier of the entity named by the marker `K`.
pub struct Id<K> {
    uuid: Uuid,
    kind: PhantomData<fn() -> K>,
}

impl<K> Id<K> {
    /// A new time-ordered identifier from the system clock and OS randomness.
    #[must_use]
    pub fn generate() -> Self {
        Self::from_uuid(Uuid::now_v7())
    }

    const fn from_uuid(uuid: Uuid) -> Self {
        Self {
            uuid,
            kind: PhantomData,
        }
    }

    /// Accepts only the canonical lowercase hyphenated UUIDv7 form, so stored and
    /// compared text always matches.
    fn parse(text: &str) -> Result<Self, ParseError> {
        let uuid = Uuid::try_parse(text).map_err(|_| ParseError::Id)?;
        if uuid.get_version_num() != 7 || uuid.hyphenated().to_string() != text {
            return Err(ParseError::Id);
        }
        Ok(Self::from_uuid(uuid))
    }
}

impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Id<K> {}

impl<K> PartialEq for Id<K> {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl<K> Eq for Id<K> {}

impl<K> Hash for Id<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

impl<K> PartialOrd for Id<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for Id<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.uuid.cmp(&other.uuid)
    }
}

impl<K> fmt::Debug for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = std::any::type_name::<K>()
            .rsplit("::")
            .next()
            .unwrap_or("?");
        write!(f, "{kind}Id({})", self.uuid)
    }
}

impl<K> fmt::Display for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.uuid.hyphenated().fmt(f)
    }
}

impl<K> FromStr for Id<K> {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl<K> Serialize for Id<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de, K> Deserialize<'de> for Id<K> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl<K: 'static> ts_rs::TS for Id<K> {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }

    fn inline(cfg: &ts_rs::Config) -> String {
        Self::name(cfg)
    }
}

#[cfg(feature = "rusqlite")]
mod sql {
    use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

    use super::Id;

    impl<K> ToSql for Id<K> {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            Ok(ToSqlOutput::from(self.to_string()))
        }
    }

    impl<K> FromSql for Id<K> {
        fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
            Id::parse(value.as_str()?).map_err(FromSqlError::other)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let id = MessageId::generate();
        let text = id.to_string();
        assert_eq!(text.len(), 36);
        assert_eq!(text, text.to_lowercase());
        assert_eq!(text.parse::<MessageId>().unwrap(), id);
    }

    #[test]
    fn generated_ids_are_time_ordered() {
        let first = MessageId::generate();
        let second = MessageId::generate();
        assert!(first < second);
    }

    #[test]
    fn parse_rejects_non_v7_and_non_canonical_text() {
        assert_eq!(
            "4f1d2a3b-5c6d-4e7f-8a9b-0c1d2e3f4a5b".parse::<MessageId>(),
            Err(ParseError::Id)
        );
        assert_eq!(
            "00000000-0000-0000-0000-000000000000".parse::<MessageId>(),
            Err(ParseError::Id)
        );
        let id = MessageId::generate().to_string();
        for text in [
            id.to_uppercase(),
            format!("{{{id}}}"),
            id.replace('-', ""),
            String::new(),
        ] {
            assert_eq!(text.parse::<MessageId>(), Err(ParseError::Id), "{text}");
        }
    }

    #[test]
    fn serde_uses_the_string_form() {
        let id = DeliveryId::generate();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<DeliveryId>(&json).unwrap(), id);
        assert!(serde_json::from_str::<DeliveryId>("42").is_err());
    }

    #[test]
    fn debug_names_the_kind() {
        let id = AlertId::generate();
        assert_eq!(format!("{id:?}"), format!("AlertId({id})"));
    }

    #[test]
    fn typescript_type_is_string() {
        use ts_rs::TS;
        assert_eq!(MessageId::inline(&ts_rs::Config::default()), "string");
    }
}
