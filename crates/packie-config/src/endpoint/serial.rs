//! Serial port settings (`docs/spec/configuration.md` §5.1).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::defaults::ms_1000;

/// Serial port settings (§5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SerialSettings {
    /// `COM3`, `/dev/ttyUSB0`; `"virtual"` with `virtual_link`.
    pub port_name: String,
    /// Required, > 0.
    pub baud_rate: u32,
    /// 5 to 8; default 8.
    #[serde(default = "eight")]
    pub data_bits: u8,
    /// Default `none`.
    #[serde(default)]
    pub parity: Parity,
    /// Default `one`.
    #[serde(default)]
    pub stop_bits: StopBits,
    /// Default `none`.
    #[serde(default)]
    pub flow_control: FlowControl,
    /// Poll window of one read, 1–60000; expiry never means disconnect. Default 1000.
    #[serde(default = "ms_1000")]
    pub read_timeout_ms: u32,
    /// A write that exceeds it fails the attempt, 1–60000. Default 1000.
    #[serde(default = "ms_1000")]
    pub write_timeout_ms: u32,
    /// Unix only: create a pseudo-terminal pair, serve this endpoint on one end and expose
    /// the other end at this path (a symlink). `port_name` must then be `"virtual"`.
    #[serde(default)]
    #[ts(as = "Option<String>")]
    pub virtual_link: Option<PathBuf>,
}

impl SerialSettings {
    /// The `port_name` that marks a virtual (pseudo-terminal) endpoint.
    pub const VIRTUAL: &'static str = "virtual";
}

/// Serial parity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Parity {
    /// No parity bit.
    #[default]
    None,
    /// Even parity.
    Even,
    /// Odd parity.
    Odd,
}

/// Serial stop bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StopBits {
    /// One stop bit.
    #[default]
    One,
    /// Two stop bits.
    Two,
}

/// Serial flow control.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FlowControl {
    /// No flow control.
    #[default]
    None,
    /// Hardware (RTS/CTS).
    RtsCts,
    /// Software (XON/XOFF).
    XonXoff,
}

const fn eight() -> u8 {
    8
}
