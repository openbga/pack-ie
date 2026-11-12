//! Operator-chosen identifiers of configured things: channels, endpoints, destinations.
//!
//! A [`Name<K>`] is a string matching `^[a-z0-9][a-z0-9._-]{0,63}$`, written as is in
//! JSON, TypeScript (`string`), logs and SQLite. Names are interned: each distinct text is
//! stored once for the life of the process, which keeps names `Copy` and cheap to compare.
//! The set of distinct names is small (the ids of configured things), so the table stays
//! small.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ParseError;

/// Longest name, in bytes.
pub const MAX_NAME: usize = 64;

/// An operator-chosen identifier of the thing named by the marker `K`.
pub struct Name<K> {
    text: &'static str,
    kind: PhantomData<fn() -> K>,
}

fn intern(text: &str) -> &'static str {
    static TABLE: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut table = TABLE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(existing) = table.get(text) {
        return existing;
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    table.insert(leaked);
    leaked
}

/// Whether `text` is a valid name.
#[must_use]
pub fn is_valid_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && text.len() <= MAX_NAME
        && bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

impl<K> Name<K> {
    /// The name `text`, if it matches the pattern.
    pub fn new(text: &str) -> Result<Self, ParseError> {
        if !is_valid_name(text) {
            return Err(ParseError::Name);
        }
        Ok(Self {
            text: intern(text),
            kind: PhantomData,
        })
    }

    /// A fresh unique name (`gen-` and 32 hex digits), for tests and generated documents.
    #[must_use]
    pub fn generate() -> Self {
        let text = format!("gen-{}", uuid::Uuid::now_v7().simple());
        Self {
            text: intern(&text),
            kind: PhantomData,
        }
    }

    /// The text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.text
    }
}

impl<K> Clone for Name<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Name<K> {}

impl<K> PartialEq for Name<K> {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl<K> Eq for Name<K> {}

impl<K> Hash for Name<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl<K> PartialOrd for Name<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for Name<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.text.cmp(other.text)
    }
}

impl<K> fmt::Debug for Name<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = std::any::type_name::<K>()
            .rsplit("::")
            .next()
            .unwrap_or("?");
        write!(f, "{kind}Id({})", self.text)
    }
}

impl<K> fmt::Display for Name<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text)
    }
}

impl<K> FromStr for Name<K> {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl<K> AsRef<str> for Name<K> {
    fn as_ref(&self) -> &str {
        self.text
    }
}

impl<K> Serialize for Name<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.text)
    }
}

impl<'de, K> Deserialize<'de> for Name<K> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

impl<K: 'static> ts_rs::TS for Name<K> {
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

    use super::Name;

    impl<K> ToSql for Name<K> {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            Ok(ToSqlOutput::from(self.text))
        }
    }

    impl<K> FromSql for Name<K> {
        fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
            Name::new(value.as_str()?).map_err(FromSqlError::other)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ChannelId, EndpointId};

    use super::*;

    #[test]
    fn valid_names_round_trip_and_intern() {
        let a: ChannelId = "hema-bridge.01_a".parse().unwrap();
        let b = ChannelId::new("hema-bridge.01_a").unwrap();
        assert_eq!(a, b);
        assert!(std::ptr::eq(a.as_str(), b.as_str()));
        assert_eq!(a.to_string(), "hema-bridge.01_a");
        assert_eq!(serde_json::to_string(&a).unwrap(), "\"hema-bridge.01_a\"");
        assert_eq!(format!("{a:?}"), "ChannelId(hema-bridge.01_a)");
    }

    #[test]
    fn invalid_names_are_rejected() {
        for bad in [
            "",
            "Upper",
            "-lead",
            ".lead",
            "has space",
            "slash/x",
            &"x".repeat(65),
        ] {
            assert_eq!(EndpointId::new(bad), Err(ParseError::Name), "{bad}");
        }
        assert!(EndpointId::new(&"x".repeat(64)).is_ok());
        assert!(serde_json::from_str::<EndpointId>("\"Bad\"").is_err());
    }

    #[test]
    fn generated_names_are_valid_and_distinct() {
        let (a, b) = (EndpointId::generate(), EndpointId::generate());
        assert_ne!(a, b);
        assert!(is_valid_name(a.as_str()));
    }
}
