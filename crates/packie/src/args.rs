//! Argument parsers shared by the commands, so a flag reads the same everywhere.

use std::time::Duration;

use packie_core::{DeliveryState, MessageDirection, MessageState, Timestamp};
use packie_ipc::audit::{AuditAction, AuditObjectType, AuditOrigin, AuditOutcome};

/// Defines a clap value parser for an enum with `ALL`, `as_str` and `FromStr`, whose error
/// lists the accepted values.
macro_rules! wire_parser {
    ($(#[$meta:meta])* $name:ident, $ty:ty) => {
        $(#[$meta])*
        pub(crate) fn $name(text: &str) -> Result<$ty, String> {
            text.parse().map_err(|_| {
                let names: Vec<&str> = <$ty>::ALL.iter().map(|v| v.as_str()).collect();
                format!("`{text}` is not one of: {}", names.join(", "))
            })
        }
    };
}

wire_parser!(
    /// A message state such as `routed` or `invalid`.
    message_state,
    MessageState
);
wire_parser!(
    /// A delivery state such as `rejected` or `timed_out`.
    delivery_state,
    DeliveryState
);
wire_parser!(
    /// A message direction: `inbound`, `a_to_b` or `b_to_a`.
    direction,
    MessageDirection
);

wire_parser!(
    /// An audit action such as `config_apply` or `message_body_view`.
    audit_action,
    AuditAction
);
wire_parser!(
    /// An audit origin: `console`, `cli`, `engine` or `other`.
    audit_origin,
    AuditOrigin
);
wire_parser!(
    /// An audit outcome: `requested`, `succeeded`, `rejected` or `failed`.
    audit_outcome,
    AuditOutcome
);
wire_parser!(
    /// An audit object type such as `channel` or `config_version`.
    audit_object_type,
    AuditObjectType
);

/// An instant: RFC 3339 (`2026-11-06T08:00:00Z`) or an age before now (`90s`, `15m`,
/// `2h`, `7d`).
pub(crate) fn time(text: &str) -> Result<Timestamp, String> {
    time_at(text, Timestamp::now())
}

/// [`time`] with `now` given.
pub(crate) fn time_at(text: &str, now: Timestamp) -> Result<Timestamp, String> {
    if let Some(age) = age(text) {
        return now
            .checked_sub(age)
            .ok_or_else(|| format!("`{text}` is too long ago"));
    }
    Timestamp::parse(text).map_err(|_| {
        format!(
            "`{text}` is not a time: use RFC 3339 (2026-11-06T08:00:00Z) or an age such as 15m, 2h or 7d"
        )
    })
}

/// `45s`, `15m`, `2h` or `7d` as a duration.
fn age(text: &str) -> Option<Duration> {
    let unit = text.chars().last()?;
    let seconds_per_unit: u64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return None,
    };
    let digits = &text[..text.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let amount: u64 = digits.parse().ok()?;
    Some(Duration::from_secs(amount.checked_mul(seconds_per_unit)?))
}

/// A log level: `trace`, `debug`, `info`, `warn` or `error`.
pub(crate) fn log_level(text: &str) -> Result<packie_diagnostics::LogLevel, String> {
    serde_json::from_value(serde_json::Value::String(text.to_lowercase()))
        .map_err(|_| format!("`{text}` is not one of: trace, debug, info, warn, error"))
}

#[cfg(test)]
mod tests {
    use packie_core::{MessageState, Timestamp};

    use super::{log_level, message_state, time_at};

    #[test]
    fn times_are_rfc_3339_or_ages() {
        let now = Timestamp::parse("2026-11-06T12:00:00Z").unwrap();
        let at = |text: &str| time_at(text, now).map(|t| t.to_string());
        assert_eq!(at("15m").unwrap(), "2026-11-06T11:45:00.000000000Z");
        assert_eq!(at("2h").unwrap(), "2026-11-06T10:00:00.000000000Z");
        assert_eq!(at("1d").unwrap(), "2026-11-05T12:00:00.000000000Z");
        assert_eq!(
            at("2026-11-06T08:00:00Z").unwrap(),
            "2026-11-06T08:00:00.000000000Z"
        );
        for bad in ["", "h", "-5m", "5x", "yesterday", "1.5h"] {
            assert!(at(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn enum_parsers_list_the_accepted_values() {
        assert_eq!(message_state("invalid"), Ok(MessageState::Invalid));
        let error = message_state("bogus").unwrap_err();
        assert!(
            error.contains("`bogus` is not one of: receiving, received, routed"),
            "{error}"
        );
    }

    #[test]
    fn log_levels_ignore_case() {
        assert!(log_level("WARN").is_ok());
        assert!(log_level("loud").is_err());
    }
}
