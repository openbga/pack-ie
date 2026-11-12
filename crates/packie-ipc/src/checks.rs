//! Read-only checks an operator runs while setting up: the serial ports of the computer, a
//! probe of an endpoint definition, and a test message to the alert notification targets
//! (`docs/spec/diagnostics.md` §6.1, §10). Probes never send data to a peer or device.

use packie_core::{ChannelId, EndpointId, EndpointKind};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Longest probe an operator may ask for.
pub const MAX_PROBE_SECONDS: u32 = 60;
/// The probe limit when the request does not set one.
pub const DEFAULT_PROBE_SECONDS: u32 = 10;

/// What kind of bus a serial port sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SerialPortKind {
    /// A USB serial adapter or device.
    Usb,
    /// A PCI or built-in port.
    Pci,
    /// A Bluetooth serial port.
    Bluetooth,
    /// The operating system did not say.
    Unknown,
}

/// A serial port the operating system reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SerialPort {
    /// `COM3`, `/dev/ttyUSB0`.
    pub name: String,
    /// The bus.
    pub kind: SerialPortKind,
    /// USB vendor id.
    pub vid: Option<u16>,
    /// USB product id.
    pub pid: Option<u16>,
    /// USB serial number of the adapter.
    pub serial_number: Option<String>,
    /// USB manufacturer text.
    pub manufacturer: Option<String>,
    /// USB product text.
    pub product: Option<String>,
    /// The configured endpoints that name this port.
    pub used_by: Vec<PortUse>,
}

/// A configured endpoint that names a serial port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PortUse {
    /// The endpoint.
    pub endpoint_id: EndpointId,
    /// Its label.
    pub endpoint_label: String,
    /// The channels that use the endpoint.
    pub channels: Vec<ChannelId>,
    /// The channel holding the port open right now, if any.
    pub held_by: Option<ChannelId>,
}

/// An endpoint definition to probe, written like an endpoint of the runtime document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct EndpointTest {
    /// The transport kind.
    pub kind: EndpointKind,
    /// The settings of that kind (`docs/spec/configuration.md` §5); secrets by reference.
    #[ts(type = "Record<string, unknown>")]
    pub settings: serde_json::Value,
    /// The overall limit, 1 to [`MAX_PROBE_SECONDS`]; [`DEFAULT_PROBE_SECONDS`] when absent.
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
}

/// The result of a read-only probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProbeReport {
    /// Every step that ran succeeded.
    pub ok: bool,
    /// The steps that ran, in order; a step after a failed one is omitted.
    pub steps: Vec<ProbeStep>,
    /// One sentence for the operator.
    pub summary: String,
}

/// One step of a probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProbeStep {
    /// `resolve`, `connect`, `open`, `bind`, `read directory`, `write test`,
    /// `authenticate`, `list`, ...
    pub name: String,
    /// The step succeeded.
    pub ok: bool,
    /// What was found or why it failed: counts and causes, never file names or secrets.
    pub detail: String,
    /// How long the step took.
    #[ts(type = "number")]
    pub millis: u64,
}

/// The outcome of a test message to one notification target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NotificationResult {
    /// `email` or `webhook`.
    pub target: String,
    /// The target accepted the message.
    pub ok: bool,
    /// `sent`, or why not.
    pub detail: String,
}
