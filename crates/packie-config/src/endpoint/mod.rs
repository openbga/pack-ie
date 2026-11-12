//! Endpoints (`docs/spec/configuration.md` §5): one side of a connection, its transport and
//! settings. Written `{endpoint_id, label, enabled, kind, settings, tags}`.

mod cidr;
mod files;
mod serial;
mod tcp;

use std::collections::BTreeMap;

use packie_core::{EndpointId, EndpointKind};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

use crate::defaults::yes;
use crate::tagged;

pub(crate) use cidr::CIDR_ERROR;
pub use cidr::{Cidr, CidrError};
pub use files::{FolderMode, FolderSettings, OnConsumed, SftpAuth, SftpSettings, WriteMode};
pub use serial::{FlowControl, Parity, SerialSettings, StopBits};
pub use tcp::{KeepAlive, TcpClientSettings, TcpListenerSettings};

/// A configured endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct EndpointConfig {
    /// Stable operator-chosen id.
    #[serde(rename = "endpoint_id")]
    pub id: EndpointId,
    /// Operator-facing name, 1–100 characters.
    pub label: String,
    /// Disabled endpoints are kept in the document but never opened.
    pub enabled: bool,
    /// Free operator metadata, at most 20 entries.
    pub tags: BTreeMap<String, String>,
    /// `kind` and `settings`.
    #[serde(flatten)]
    pub transport: Transport,
}

/// The fields next to `kind` and `settings`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointCommon {
    endpoint_id: EndpointId,
    label: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

impl<'de> Deserialize<'de> for EndpointConfig {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        let (kind, settings) = tagged::split_kind::<D>(&mut map, "kind", "settings")?;
        let common: EndpointCommon = tagged::variant::<D, _>(map)?;
        let transport = match kind.as_str() {
            "serial" => Transport::Serial(tagged::at::<D, _>(settings, "settings")?),
            "tcp_client" => Transport::TcpClient(tagged::at::<D, _>(settings, "settings")?),
            "tcp_listener" => Transport::TcpListener(tagged::at::<D, _>(settings, "settings")?),
            "folder" => Transport::Folder(tagged::at::<D, _>(settings, "settings")?),
            "sftp" => Transport::Sftp(tagged::at::<D, _>(settings, "settings")?),
            other => {
                let kinds = EndpointKind::ALL
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>();
                return Err(tagged::unknown::<D>("kind", other, &kinds));
            }
        };
        Ok(Self {
            id: common.endpoint_id,
            label: common.label,
            enabled: common.enabled,
            tags: common.tags,
            transport,
        })
    }
}

impl EndpointConfig {
    /// The transport kind.
    #[must_use]
    pub fn kind(&self) -> EndpointKind {
        self.transport.kind()
    }

    /// A non-secret one-line description of the settings, for snapshots
    /// (`COM3 9600 8N1`, `0.0.0.0:3001 (2 allowed peers)`).
    #[must_use]
    pub fn settings_summary(&self) -> String {
        match &self.transport {
            Transport::Serial(s) => {
                let parity = match s.parity {
                    Parity::None => 'N',
                    Parity::Even => 'E',
                    Parity::Odd => 'O',
                };
                let stop = match s.stop_bits {
                    StopBits::One => 1,
                    StopBits::Two => 2,
                };
                let flow = match s.flow_control {
                    FlowControl::None => "",
                    FlowControl::RtsCts => " RTS/CTS",
                    FlowControl::XonXoff => " XON/XOFF",
                };
                let port = s.virtual_link.as_ref().map_or_else(
                    || s.port_name.clone(),
                    |link| format!("virtual {}", link.display()),
                );
                format!("{port} {} {}{parity}{stop}{flow}", s.baud_rate, s.data_bits)
            }
            Transport::TcpClient(s) => format!("{}:{}", s.host, s.port),
            Transport::TcpListener(s) => {
                let peers = match (s.allowed_peer_cidrs.len(), s.allow_any_peer) {
                    (0, true) => "any peer".to_owned(),
                    (0, false) => "local peers".to_owned(),
                    (n, _) => format!("{n} allowed peer{}", if n == 1 { "" } else { "s" }),
                };
                format!("{}:{} ({peers})", s.bind_address, s.port)
            }
            Transport::Folder(s) => format!("{} ({})", s.path.display(), s.file_pattern),
            Transport::Sftp(s) => {
                format!("{}@{}:{}{}", s.username, s.host, s.port, s.remote_directory)
            }
        }
    }
}

/// Transport settings, written as `kind` and `settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", content = "settings", rename_all = "snake_case")]
pub enum Transport {
    /// A serial port.
    Serial(SerialSettings),
    /// An outbound TCP connection.
    TcpClient(TcpClientSettings),
    /// A TCP listener.
    TcpListener(TcpListenerSettings),
    /// A local or shared folder.
    Folder(FolderSettings),
    /// A directory on an SFTP server.
    Sftp(SftpSettings),
}

impl Transport {
    /// The transport kind.
    #[must_use]
    pub const fn kind(&self) -> EndpointKind {
        match self {
            Self::Serial(_) => EndpointKind::Serial,
            Self::TcpClient(_) => EndpointKind::TcpClient,
            Self::TcpListener(_) => EndpointKind::TcpListener,
            Self::Folder(_) => EndpointKind::Folder,
            Self::Sftp(_) => EndpointKind::Sftp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_use_kind_and_settings() {
        let json = r#"{"endpoint_id":"hema-serial","label":"Analyzer","kind":"serial",
            "settings":{"port_name":"COM3","baud_rate":9600}}"#;
        let endpoint: EndpointConfig = serde_json::from_str(json).unwrap();
        let Transport::Serial(serial) = &endpoint.transport else {
            panic!("expected serial");
        };
        assert_eq!(
            (serial.baud_rate, serial.data_bits, endpoint.enabled),
            (9600, 8, true)
        );
        let back = serde_json::to_value(&endpoint).unwrap();
        assert_eq!(back["kind"], "serial");
        assert_eq!(back["settings"]["port_name"], "COM3");
        assert_eq!(back["endpoint_id"], "hema-serial");
    }
}
