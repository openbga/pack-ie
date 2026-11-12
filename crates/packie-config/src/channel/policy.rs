//! Retry, delivery and retention policies of channels (`docs/spec/configuration.md` §6.3
//! and §6.4).

use packie_core::AckLevel;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::defaults::{ms_1000, secs_30, three};

/// Exponential backoff with jitter (§6.3). Delay before retry n:
/// `min(max_delay_ms, initial_delay_ms × multiplier^(n−1))`, scaled by ±`jitter_percent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    /// > 0. Default 1000.
    #[serde(default = "ms_1000")]
    pub initial_delay_ms: u32,
    /// ≥ `initial_delay_ms`. Default 60000.
    #[serde(default = "ms_60000")]
    pub max_delay_ms: u32,
    /// ≥ 1.0, kept exact (written as a JSON number). Default 2.0.
    #[serde(default = "two", with = "rust_decimal::serde::float")]
    #[ts(type = "number")]
    pub multiplier: Decimal,
    /// 0–100; 0 makes timing deterministic. Default 20.
    #[serde(default = "twenty")]
    pub jitter_percent: u8,
    /// > 0. Bridge channels only. Default 3.
    #[serde(default = "three")]
    pub max_consecutive_failures_before_alert: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial_delay_ms: ms_1000(),
            max_delay_ms: ms_60000(),
            multiplier: two(),
            jitter_percent: twenty(),
            max_consecutive_failures_before_alert: three(),
        }
    }
}

/// Delivery policy (§6.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DeliveryPolicy {
    /// Attempts per attempt cycle, 1–100. Default 5.
    #[serde(default = "five")]
    pub max_attempts: u32,
    /// Backoff between attempts (`max_consecutive_failures_before_alert` is ignored).
    #[serde(default)]
    pub retry: RetryPolicy,
    /// Time to write the whole message, 1–3600. Default 30.
    #[serde(default = "secs_30")]
    pub send_timeout_seconds: u32,
    /// Time after the last byte for the required acknowledgement, 1–3600. Default 30.
    #[serde(default = "secs_30")]
    pub ack_timeout_seconds: u32,
    /// `handed_off`, `received` or `accepted`; default the protocol's highest level.
    #[serde(default)]
    pub ack_required: Option<AckLevel>,
    /// Retry attempts that sent everything but got no acknowledgement. Default `false`.
    #[serde(default)]
    pub retry_unacknowledged: bool,
}

impl Default for DeliveryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: five(),
            retry: RetryPolicy::default(),
            send_timeout_seconds: secs_30(),
            ack_timeout_seconds: secs_30(),
            ack_required: None,
            retry_unacknowledged: false,
        }
    }
}

/// Destination overrides of the channel's [`DeliveryPolicy`]; absent fields inherit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DeliveryOverrides {
    /// See [`DeliveryPolicy::max_attempts`].
    #[serde(default)]
    pub max_attempts: Option<u32>,
    /// See [`DeliveryPolicy::retry`].
    #[serde(default)]
    pub retry: Option<RetryPolicy>,
    /// See [`DeliveryPolicy::send_timeout_seconds`].
    #[serde(default)]
    pub send_timeout_seconds: Option<u32>,
    /// See [`DeliveryPolicy::ack_timeout_seconds`].
    #[serde(default)]
    pub ack_timeout_seconds: Option<u32>,
    /// See [`DeliveryPolicy::ack_required`].
    #[serde(default)]
    pub ack_required: Option<AckLevel>,
    /// See [`DeliveryPolicy::retry_unacknowledged`].
    #[serde(default)]
    pub retry_unacknowledged: Option<bool>,
}

/// The delivery policy a destination actually uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveDelivery {
    /// Attempts per cycle.
    pub max_attempts: u32,
    /// Backoff.
    pub retry: RetryPolicy,
    /// Write budget.
    pub send_timeout_seconds: u32,
    /// Acknowledgement budget.
    pub ack_timeout_seconds: u32,
    /// Level at which a delivery is `acknowledged`.
    pub ack_required: AckLevel,
    /// Retry unacknowledged sends.
    pub retry_unacknowledged: bool,
}

/// How long a message channel keeps stored messages. Messages with unresolved deliveries
/// are never purged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    /// 1–36500 days; `null` means no age limit. Default 90.
    #[serde(default = "days_90")]
    pub max_age_days: Option<u32>,
    /// 1000–1000000000; `null` (default) means no count limit.
    #[serde(default)]
    pub max_messages: Option<u64>,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            max_age_days: days_90(),
            max_messages: None,
        }
    }
}

/// How long a bridge tap keeps the messages it recorded (state `observed`); the same
/// shape as [`Retention`], with its own defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TapRetention {
    /// 1–36500 days; `null` means no age limit. Default 30.
    #[serde(default = "days_30")]
    pub max_age_days: Option<u32>,
    /// 1000–1000000000; `null` (default) means no count limit.
    #[serde(default)]
    pub max_messages: Option<u64>,
}

impl Default for TapRetention {
    fn default() -> Self {
        Self {
            max_age_days: days_30(),
            max_messages: None,
        }
    }
}

const fn five() -> u32 {
    5
}
const fn twenty() -> u8 {
    20
}
const fn two() -> Decimal {
    Decimal::TWO
}
const fn ms_60000() -> u32 {
    60_000
}
#[allow(clippy::unnecessary_wraps)]
const fn days_90() -> Option<u32> {
    Some(90)
}
#[allow(clippy::unnecessary_wraps)]
const fn days_30() -> Option<u32> {
    Some(30)
}
