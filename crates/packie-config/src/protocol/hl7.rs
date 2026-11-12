//! HL7 v2 settings (`docs/spec/configuration.md` §7.3).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Charset;
use crate::defaults::{mib_1, ms_30000, yes};

/// HL7 acknowledgement mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AckMode {
    /// Original mode: AA/AE/AR application acknowledgements.
    #[default]
    Original,
    /// Enhanced mode: accept acknowledgements (CA/CE/CR), application ones per MSH-16.
    Enhanced,
    /// Destination only: no acknowledgement expected; the level reached is `handed_off`.
    None,
}

/// What a source answers an order message with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReplyType {
    /// A general `ACK` for every message.
    #[default]
    Ack,
    /// The order response of the message type: `ORL^O34` to `OML^O33` and so on;
    /// every other message still gets an `ACK`.
    OrderResponse,
}

/// HL7 processing id (MSH-11).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum ProcessingId {
    /// Production.
    #[default]
    P,
    /// Debugging.
    D,
    /// Training.
    T,
}

/// HL7 v2 settings (§7.3). Framing is MLLP; acknowledgement timeouts of destinations
/// belong to the delivery policy (`ack_timeout_seconds`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Hl7Settings {
    /// Source: `original` or `enhanced`; destination also `none`. Default `original`.
    #[serde(default)]
    pub ack_mode: AckMode,
    /// Source in `enhanced` mode: send the application acknowledgement when asked.
    /// Default `true`.
    #[serde(default = "yes")]
    pub send_application_ack: bool,
    /// MSH-12 of generated messages, `2.x` or `2.x.y`. Default `2.5.1`.
    #[serde(default = "hl7_version")]
    pub version: String,
    /// MSH-3 of generated messages and ACKs; ≤ 180 characters.
    #[serde(default)]
    pub sending_application: String,
    /// MSH-4; ≤ 180 characters.
    #[serde(default)]
    pub sending_facility: String,
    /// MSH-5; ≤ 180 characters.
    #[serde(default)]
    pub receiving_application: String,
    /// MSH-6; ≤ 180 characters.
    #[serde(default)]
    pub receiving_facility: String,
    /// Default `P`.
    #[serde(default)]
    pub processing_id: ProcessingId,
    /// Written to MSH-18 and used to decode. Default `utf8`.
    #[serde(default)]
    pub charset: Charset,
    /// 1024–67108864. Default 1 MiB.
    #[serde(default = "mib_1")]
    pub max_message_bytes: u64,
    /// `false`: a frame also ends at the end block (0x1C) without the carriage return after
    /// it. Default `true`.
    #[serde(default = "yes")]
    pub require_end_cr: bool,
    /// Destination: MSH-9.1 values accepted as the acknowledgement of a sent message; 1–8
    /// values of three capitals or digits. Default `["ACK"]`.
    #[serde(default = "ack_types")]
    pub ack_message_types: Vec<String>,
    /// Source: the reply to an order message. Default `ack`.
    #[serde(default)]
    pub reply_type: ReplyType,
    /// A started frame that gets no further bytes for this long (counted while the engine
    /// waits for them) is discarded; 1000–300000. Default 30000.
    #[serde(default = "ms_30000")]
    pub frame_timeout_ms: u32,
}

fn ack_types() -> Vec<String> {
    vec!["ACK".to_owned()]
}

fn hl7_version() -> String {
    "2.5.1".to_owned()
}
