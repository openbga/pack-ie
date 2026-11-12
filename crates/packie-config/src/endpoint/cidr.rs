//! IP address blocks of listener allowlists.

use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

/// An IPv4 or IPv6 address block, written `addr/prefix` or as a bare address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

/// The text of [`CidrError`], used to classify parse failures as `invalid_cidr`.
pub(crate) const CIDR_ERROR: &str = "not an IP address or CIDR block";

/// Text that is not an address or CIDR block.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not an IP address or CIDR block (expected 10.0.0.0/8 or 192.168.1.20)")]
pub struct CidrError;

impl Cidr {
    /// Whether `addr` is inside this block. IPv4-mapped IPv6 peers match IPv4 blocks.
    #[must_use]
    pub fn contains(&self, addr: IpAddr) -> bool {
        let addr = match addr {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(addr, IpAddr::V4),
            IpAddr::V4(_) => addr,
        };
        match (self.addr, addr) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                prefix_match(&net.octets(), &ip.octets(), self.prefix)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                prefix_match(&net.octets(), &ip.octets(), self.prefix)
            }
            _ => false,
        }
    }
}

fn prefix_match(net: &[u8], ip: &[u8], prefix: u8) -> bool {
    let full = usize::from(prefix / 8);
    let rest = prefix % 8;
    if net[..full] != ip[..full] {
        return false;
    }
    if rest == 0 {
        return true;
    }
    let mask = 0xff_u8 << (8 - rest);
    (net[full] & mask) == (ip[full] & mask)
}

impl FromStr for Cidr {
    type Err = CidrError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (addr, prefix) = match text.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (text, None),
        };
        let addr: IpAddr = addr.parse().map_err(|_| CidrError)?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or(CidrError)?,
            None => max,
        };
        Ok(Self { addr, prefix })
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let max = if self.addr.is_ipv4() { 32 } else { 128 };
        if self.prefix == max {
            write!(f, "{}", self.addr)
        } else {
            write!(f, "{}/{}", self.addr, self.prefix)
        }
    }
}

impl Serialize for Cidr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Cidr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl TS for Cidr {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }

    fn inline(cfg: &ts_rs::Config) -> String {
        Self::name(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cidr(text: &str) -> Cidr {
        text.parse().unwrap()
    }

    #[test]
    fn cidr_matches_prefixes() {
        let net = cidr("10.1.0.0/16");
        assert!(net.contains("10.1.200.3".parse().unwrap()));
        assert!(!net.contains("10.2.0.1".parse().unwrap()));
        assert!(cidr("192.168.1.20").contains("192.168.1.20".parse().unwrap()));
        assert!(!cidr("192.168.1.20").contains("192.168.1.21".parse().unwrap()));
        assert!(cidr("10.0.0.0/9").contains("10.127.0.1".parse().unwrap()));
        assert!(!cidr("10.0.0.0/9").contains("10.128.0.1".parse().unwrap()));
        assert!(cidr("0.0.0.0/0").contains("8.8.8.8".parse().unwrap()));
        assert!(cidr("fd00::/8").contains("fd12::1".parse().unwrap()));
        assert!(cidr("10.0.0.0/8").contains("::ffff:10.0.0.1".parse().unwrap()));
        assert!(!cidr("10.0.0.0/8").contains("fd00::1".parse().unwrap()));
    }

    #[test]
    fn cidr_rejects_bad_text_and_round_trips() {
        for text in [
            "",
            "10.0.0.0/33",
            "10.0.0/8",
            "host",
            "::/129",
            "10.0.0.0/x",
        ] {
            assert_eq!(text.parse::<Cidr>(), Err(CidrError), "{text}");
        }
        for text in ["10.0.0.0/8", "192.168.1.20", "fd00::/8"] {
            assert_eq!(cidr(text).to_string(), text);
        }
    }
}
