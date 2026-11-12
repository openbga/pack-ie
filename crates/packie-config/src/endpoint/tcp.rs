//! TCP client and listener settings (`docs/spec/configuration.md` §5.2 and §5.3).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::cidr::Cidr;
use crate::defaults::{ms_5000, secs_10, secs_30, three, yes};

/// TCP keepalive of one socket, from the `keep_alive*` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAlive {
    /// Whether keepalive is on.
    pub enabled: bool,
    /// Idle seconds before the first probe.
    pub time_seconds: u32,
    /// Seconds between unanswered probes.
    pub interval_seconds: u32,
    /// Unanswered probes before the connection is declared dead.
    pub retry_count: u32,
}

/// Outbound TCP connection settings (§5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TcpClientSettings {
    /// Host name, IPv4 or IPv6 literal.
    pub host: String,
    /// 1–65535.
    pub port: u16,
    /// 1–120000. Default 5000.
    #[serde(default = "ms_5000")]
    pub connect_timeout_ms: u32,
    /// 0 disables; otherwise 1–604800. Default 0.
    #[serde(default)]
    pub idle_timeout_seconds: u32,
    /// Default `true`.
    #[serde(default = "yes")]
    pub keep_alive: bool,
    /// 1–7200. Default 30.
    #[serde(default = "secs_30")]
    pub keep_alive_time_sec: u32,
    /// 1–3600. Default 10.
    #[serde(default = "secs_10")]
    pub keep_alive_interval_sec: u32,
    /// 1–100. Default 3.
    #[serde(default = "three")]
    pub keep_alive_retry_count: u32,
}

impl TcpClientSettings {
    /// The keepalive settings.
    #[must_use]
    pub const fn keep_alive(&self) -> KeepAlive {
        KeepAlive {
            enabled: self.keep_alive,
            time_seconds: self.keep_alive_time_sec,
            interval_seconds: self.keep_alive_interval_sec,
            retry_count: self.keep_alive_retry_count,
        }
    }
}

/// TCP listener settings (§5.3). One peer session at a time. A listener bound to a
/// non-loopback address must list `allowed_peer_cidrs` or set `allow_any_peer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TcpListenerSettings {
    /// IPv4 or IPv6 literal; no default, so a wildcard is never bound by accident.
    #[ts(as = "String")]
    pub bind_address: IpAddr,
    /// 1–65535.
    pub port: u16,
    /// 1–1024. Default 16.
    #[serde(default = "sixteen")]
    pub accept_backlog: u32,
    /// Accepted peers (`10.0.0.0/8`; a bare address is a single host). Empty: any peer.
    #[serde(default)]
    pub allowed_peer_cidrs: Vec<Cidr>,
    /// Required `true` for a non-loopback bind with an empty allowlist. Default `false`.
    #[serde(default)]
    pub allow_any_peer: bool,
    /// As for `tcp_client`; the listener keeps listening. Default 0.
    #[serde(default)]
    pub idle_timeout_seconds: u32,
    /// On the accepted socket. Default `true`.
    #[serde(default = "yes")]
    pub keep_alive: bool,
    /// Default 30.
    #[serde(default = "secs_30")]
    pub keep_alive_time_sec: u32,
    /// Default 10.
    #[serde(default = "secs_10")]
    pub keep_alive_interval_sec: u32,
    /// Default 3.
    #[serde(default = "three")]
    pub keep_alive_retry_count: u32,
}

impl TcpListenerSettings {
    /// Whether a peer at `addr` may connect.
    #[must_use]
    pub fn allows(&self, addr: IpAddr) -> bool {
        if self.allowed_peer_cidrs.is_empty() {
            // Validation requires `allow_any_peer` unless the bind is loopback-only.
            return self.allow_any_peer || self.bind_address.is_loopback();
        }
        self.allowed_peer_cidrs
            .iter()
            .any(|cidr| cidr.contains(addr))
    }

    /// The keepalive settings of accepted sockets.
    #[must_use]
    pub const fn keep_alive(&self) -> KeepAlive {
        KeepAlive {
            enabled: self.keep_alive,
            time_seconds: self.keep_alive_time_sec,
            interval_seconds: self.keep_alive_interval_sec,
            retry_count: self.keep_alive_retry_count,
        }
    }
}

const fn sixteen() -> u32 {
    16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_allowlist_defaults_to_local_peers() {
        let mut listener: TcpListenerSettings =
            serde_json::from_str(r#"{"bind_address":"0.0.0.0","port":3001}"#).unwrap();
        assert!(!listener.allows("10.0.0.5".parse().unwrap()));
        listener.allow_any_peer = true;
        assert!(listener.allows("10.0.0.5".parse().unwrap()));
        listener.allowed_peer_cidrs = vec!["10.0.0.0/24".parse().unwrap()];
        assert!(listener.allows("10.0.0.5".parse().unwrap()));
        assert!(!listener.allows("10.0.1.5".parse().unwrap()));
    }
}
