//! UTC instants.
//!
//! [`Timestamp`] is written in one canonical form: RFC 3339 in UTC with exactly nine
//! fractional digits, for example `2026-11-06T14:03:07.123456789Z`. The fixed width makes
//! text order equal time order, so SQLite TEXT columns sort correctly. Parsing accepts any
//! RFC 3339 instant with an offset (`2026-11-06T12:00:00Z`, `...+01:00`) and converts it to
//! UTC, so hand-written configuration stays readable.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ParseError;

/// An instant in UTC with nanosecond precision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(jiff::Timestamp);

impl Timestamp {
    /// The current instant from the system clock.
    #[must_use]
    pub fn now() -> Self {
        Self(jiff::Timestamp::now())
    }

    /// Parses an RFC 3339 instant with an offset (see the module documentation).
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        text.parse::<jiff::Timestamp>()
            .map(Self)
            .map_err(|_| ParseError::Timestamp)
    }

    /// The instant `millis` milliseconds after the Unix epoch, if representable.
    #[must_use]
    pub fn from_unix_millis(millis: i64) -> Option<Self> {
        jiff::Timestamp::from_millisecond(millis).ok().map(Self)
    }

    /// Milliseconds since the Unix epoch, rounded towards zero.
    #[must_use]
    pub fn unix_millis(self) -> i64 {
        self.0.as_millisecond()
    }

    /// `self + duration`, or `None` beyond the supported range.
    #[must_use]
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.0.checked_add(duration).ok().map(Self)
    }

    /// `self - duration`, or `None` beyond the supported range.
    #[must_use]
    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        self.0.checked_sub(duration).ok().map(Self)
    }

    /// The time elapsed since `earlier`, or zero if `earlier` is later.
    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::try_from(self.0.duration_since(earlier.0)).unwrap_or(Duration::ZERO)
    }

    /// The underlying `jiff` instant, for calendar arithmetic.
    #[must_use]
    pub const fn as_jiff(self) -> jiff::Timestamp {
        self.0
    }
}

impl From<jiff::Timestamp> for Timestamp {
    fn from(value: jiff::Timestamp) -> Self {
        Self(value)
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({self})")
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.9}", self.0)
    }
}

impl FromStr for Timestamp {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl ts_rs::TS for Timestamp {
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

    use super::Timestamp;

    impl ToSql for Timestamp {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            Ok(ToSqlOutput::from(self.to_string()))
        }
    }

    impl FromSql for Timestamp {
        fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
            Timestamp::parse(value.as_str()?).map_err(FromSqlError::other)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn canonical_form_has_nine_fraction_digits() {
        let value = Timestamp::from_unix_millis(1_762_430_587_120).unwrap();
        assert_eq!(value.to_string(), "2025-11-06T12:03:07.120000000Z");
        assert_eq!(Timestamp::parse(&value.to_string()), Ok(value));
    }

    #[test]
    fn parse_accepts_offsets_and_normalizes_to_utc() {
        let utc = Timestamp::parse("2026-11-06T12:00:00Z").unwrap();
        let offset = Timestamp::parse("2026-11-06T13:00:00+01:00").unwrap();
        assert_eq!(utc, offset);
        assert_eq!(offset.to_string(), "2026-11-06T12:00:00.000000000Z");
    }

    #[test]
    fn parse_rejects_text_without_an_offset() {
        for text in ["2026-11-06T12:00:00", "2026-11-06", "", "yesterday"] {
            assert_eq!(Timestamp::parse(text), Err(ParseError::Timestamp), "{text}");
        }
    }

    #[test]
    fn duration_since_saturates_at_zero() {
        let earlier = Timestamp::parse("2026-11-06T12:00:00Z").unwrap();
        let later = earlier.checked_add(Duration::from_millis(1500)).unwrap();
        assert_eq!(
            later.saturating_duration_since(earlier),
            Duration::from_millis(1500)
        );
        assert_eq!(earlier.saturating_duration_since(later), Duration::ZERO);
    }

    #[test]
    fn serde_uses_the_canonical_string() {
        let value = Timestamp::parse("2026-11-06T12:00:00.5Z").unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"2026-11-06T12:00:00.500000000Z\"");
        assert_eq!(serde_json::from_str::<Timestamp>(&json).unwrap(), value);
    }

    proptest! {
        #[test]
        fn text_order_is_time_order(a in 0_i64..253_402_300_799_000, b in 0_i64..253_402_300_799_000) {
            let (ta, tb) = (Timestamp::from_unix_millis(a).unwrap(), Timestamp::from_unix_millis(b).unwrap());
            prop_assert_eq!(ta.cmp(&tb), ta.to_string().cmp(&tb.to_string()));
        }
    }
}
