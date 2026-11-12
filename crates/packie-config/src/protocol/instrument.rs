//! ASTM and AU settings (`docs/spec/configuration.md` §7.1 and §7.2).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Charset;
use crate::defaults::{mib_1, ms_30000, three};

/// The side PACK-IE plays in LIS01-A2 link contention.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AstmRole {
    /// Yields to a simultaneous instrument ENQ and sends after receiving.
    #[default]
    Computer,
    /// Has priority when both sides send ENQ.
    Instrument,
}

/// ASTM settings (§7.1). `max_frame_retries` is the codec's
/// `max_frame_retransmissions`, `ack_timeout_ms` its `reply_timeout`, and
/// `contention_wait_ms` its contention wait for the role in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AstmSettings {
    /// Default `latin1`.
    #[serde(default = "latin1")]
    pub charset: Charset,
    /// Default `computer`.
    #[serde(default)]
    pub role: AstmRole,
    /// Sender waits this long for ACK/NAK; 1000–120000. Default 15000.
    #[serde(default = "ms_15000")]
    pub ack_timeout_ms: u32,
    /// Receiver waits this long for the next frame; 1000–300000. Default 30000.
    #[serde(default = "ms_30000")]
    pub receive_timeout_ms: u32,
    /// Retransmissions of one frame before the transfer aborts; 0–20. Default 6.
    #[serde(default = "six")]
    pub max_frame_retries: u32,
    /// Text characters per frame, sent and accepted; 64–64000. Default 240 (E1381). More
    /// than 240 is a long frame, which instruments document for ASTM over TCP.
    #[serde(default = "chars_240")]
    pub max_frame_chars: u32,
    /// Wait before sending ENQ again after losing contention; 1000–60000. Default 20000.
    #[serde(default = "ms_20000")]
    pub contention_wait_ms: u32,
    /// Larger receives abort (`message_too_large`); 1024–67108864. Default 1 MiB.
    #[serde(default = "mib_1")]
    pub max_message_bytes: u64,
    /// ENQ transmissions per sent message before the send is abandoned; 1–10. Default 3.
    #[serde(default = "three")]
    pub enq_attempts: u32,
    /// Wait after a NAK or silence before the next ENQ; 1000–60000. Default 10000.
    #[serde(default = "ms_10000")]
    pub busy_retry_ms: u32,
    /// Source: where the specimen id sits in the first order record, as `O.<field>` or
    /// `O.<field>.<component>` with field 3 to 99 (field 1 is the record type) and component
    /// 1 to 99. It is the message's `sample_id` (searchable, part of the duplicate identity).
    /// Default `O.3`.
    #[serde(default = "sample_o3")]
    pub sample_id_path: String,
}

impl AstmSettings {
    /// The field and optional component of [`sample_id_path`](Self::sample_id_path), or
    /// `None` when the path is not of the form `O.<field>[.<component>]` within the limits.
    #[must_use]
    pub fn sample_id_position(&self) -> Option<(usize, Option<usize>)> {
        let mut parts = self.sample_id_path.strip_prefix("O.")?.split('.');
        let number = |text: &str, range: std::ops::RangeInclusive<usize>| -> Option<usize> {
            if text.starts_with('0') || !text.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            text.parse().ok().filter(|n| range.contains(n))
        };
        let field = number(parts.next()?, 3..=99)?;
        let component = match parts.next() {
            None => None,
            Some(text) => Some(number(text, 1..=99)?),
        };
        parts.next().is_none().then_some((field, component))
    }
}

/// AU instrument profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AuProfile {
    /// AU400.
    Au400,
    /// AU640.
    Au640,
}

/// AU transmission class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AuClass {
    /// No ACK/NAK on receive.
    A,
    /// ACK after the durable commit.
    B,
}

/// AU framing, from the instrument's online parameter sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AuFraming {
    /// 1–2 control bytes (1–31), never ACK (6) or NAK (21).
    pub start: Vec<u8>,
    /// 1–2 control bytes (1–31), never ACK (6) or NAK (21).
    pub end: Vec<u8>,
    /// One XOR check byte per frame.
    pub bcc: bool,
    /// ETB for intermediate blocks; only when `end` is `[3]`.
    pub use_etb: bool,
    /// 256, 512 or 1024 wire bytes per frame.
    pub max_wire_len: u16,
    /// `a` or `b`.
    pub class: AuClass,
}

/// AU fixed-width layout, from the instrument's online parameter sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AuFormat {
    /// Two-digit unit number in incoming messages.
    pub unit_no: bool,
    /// 0, 4 or 5.
    pub rack_digits: u8,
    /// 1–20.
    pub sample_id_digits: u8,
    /// Patient header has a sex byte.
    pub sex: bool,
    /// Patient header has three age bytes.
    pub age: bool,
    /// Patient header has two month bytes.
    pub month: bool,
    /// Widths of the six patient information fields, each 0–20.
    pub patient_info_digits: [u8; 6],
    /// 6 or 9.
    pub result_digits: u8,
    /// Digits of a test code in result items and requests: 2 or 3. Default 2.
    #[serde(default = "two")]
    pub test_code_digits: u8,
}

/// AU settings (§7.2). The laboratory supplies the profile, framing and format; only the
/// block timer has a default (9900 ms, the codec's).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AuSettings {
    /// `au400` or `au640`.
    pub profile: AuProfile,
    /// Frame delimiters, check byte and class.
    pub framing: AuFraming,
    /// Message layout.
    pub format: AuFormat,
    /// Time allowed for one frame from its first byte; 100–9900 in steps of 100.
    pub frame_timeout_ms: u32,
    /// Wait for the next block of an unfinished sample; 100–9900 in steps of 100.
    /// Default 9900.
    #[serde(default = "ms_9900")]
    pub block_timeout_ms: u32,
}

const fn two() -> u8 {
    2
}
fn sample_o3() -> String {
    "O.3".to_owned()
}
const fn six() -> u32 {
    6
}
const fn chars_240() -> u32 {
    240
}
const fn ms_9900() -> u32 {
    9900
}
const fn ms_10000() -> u32 {
    10_000
}
const fn ms_15000() -> u32 {
    15_000
}
const fn ms_20000() -> u32 {
    20_000
}
const fn latin1() -> Charset {
    Charset::Latin1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn astm_defaults_follow_the_spec() {
        let astm: AstmSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(astm.role, AstmRole::Computer);
        assert_eq!(astm.charset, Charset::Latin1);
        assert_eq!(
            (
                astm.max_frame_retries,
                astm.max_frame_chars,
                astm.contention_wait_ms,
                astm.enq_attempts,
                astm.busy_retry_ms
            ),
            (6, 240, 20_000, 3, 10_000)
        );
        assert_eq!(astm.sample_id_position(), Some((3, None)));
    }

    #[test]
    fn the_sample_id_path_names_a_field_and_an_optional_component_of_the_order_record() {
        let position = |path: &str| {
            serde_json::from_value::<AstmSettings>(serde_json::json!({ "sample_id_path": path }))
                .unwrap()
                .sample_id_position()
        };
        assert_eq!(position("O.3"), Some((3, None)));
        assert_eq!(position("O.4.3"), Some((4, Some(3))));
        assert_eq!(position("O.99.99"), Some((99, Some(99))));
        for bad in [
            "", "O", "O.", "O.2", "O.100", "O.3.", "O.3.0", "O.3.100", "O.3.1.1", "P.3", "R.3",
            "o.3", "O.03", "O.3.01", "O. 3", "O.+3", "O.3a", "$.3", "OBR.3",
        ] {
            assert_eq!(position(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn au_requires_the_instrument_profile() {
        let err = serde_json::from_str::<AuSettings>("{}").unwrap_err();
        assert!(err.to_string().contains("missing field `profile`"), "{err}");
    }
}
