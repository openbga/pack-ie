//! Payload forms of `send`, `expect` and file steps (scenarios.md §6): exactly one of
//! `text`, `hex`, `base64` or `generate`.

use serde::Deserialize;

/// Largest payload a single step may produce, so a typo cannot exhaust memory.
pub const MAX_PAYLOAD_BYTES: u64 = 1 << 30;

/// The four ways a step can state bytes. Embedded (flattened by hand) in steps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadSpec {
    /// UTF-8 text.
    #[serde(default)]
    pub text: Option<String>,
    /// Hex digits; spaces and dashes ignored.
    #[serde(default)]
    pub hex: Option<String>,
    /// Standard base64.
    #[serde(default)]
    pub base64: Option<String>,
    /// A generated pattern.
    #[serde(default)]
    pub generate: Option<Generate>,
}

/// `{"pattern": "counter32", "bytes": N, "seed": S}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generate {
    /// Pattern name; only `counter32` exists.
    pub pattern: Pattern,
    /// Number of bytes.
    pub bytes: u64,
    /// First counter value (default 0).
    #[serde(default)]
    pub seed: u32,
}

/// Generator patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Consecutive 32-bit little-endian counters.
    Counter32,
}

/// A payload problem, phrased for the file author.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PayloadError(pub String);

impl PayloadSpec {
    /// Builds a spec from the four optional fields of a step.
    #[must_use]
    pub fn from_parts(
        text: Option<String>,
        hex: Option<String>,
        base64: Option<String>,
        generate: Option<Generate>,
    ) -> Self {
        Self {
            text,
            hex,
            base64,
            generate,
        }
    }

    /// Checks that exactly one form is present and well-formed, without generating
    /// pattern bytes.
    pub fn check(&self) -> Result<(), PayloadError> {
        match &self.generate {
            Some(g) if self.text.is_none() && self.hex.is_none() && self.base64.is_none() => {
                if g.bytes > MAX_PAYLOAD_BYTES {
                    Err(PayloadError(format!(
                        "generate.bytes {} exceeds the limit of {MAX_PAYLOAD_BYTES}",
                        g.bytes
                    )))
                } else {
                    Ok(())
                }
            }
            _ => self.bytes().map(|_| ()),
        }
    }

    /// Checks that exactly one form is present and decodes it.
    pub fn bytes(&self) -> Result<Vec<u8>, PayloadError> {
        let given = usize::from(self.text.is_some())
            + usize::from(self.hex.is_some())
            + usize::from(self.base64.is_some())
            + usize::from(self.generate.is_some());
        if given != 1 {
            return Err(PayloadError(format!(
                "a payload needs exactly one of text, hex, base64, generate (found {given})"
            )));
        }
        if let Some(text) = &self.text {
            return Ok(text.as_bytes().to_vec());
        }
        if let Some(hex) = &self.hex {
            return decode_hex(hex);
        }
        if let Some(b64) = &self.base64 {
            return decode_base64(b64);
        }
        match &self.generate {
            Some(generate) => generate.bytes(),
            None => Err(PayloadError("empty payload".into())),
        }
    }
}

impl Generate {
    /// The pattern's bytes.
    pub fn bytes(&self) -> Result<Vec<u8>, PayloadError> {
        if self.bytes > MAX_PAYLOAD_BYTES {
            return Err(PayloadError(format!(
                "generate.bytes {} exceeds the limit of {MAX_PAYLOAD_BYTES}",
                self.bytes
            )));
        }
        let Pattern::Counter32 = self.pattern;
        let len = usize::try_from(self.bytes)
            .map_err(|_| PayloadError("generate.bytes does not fit memory".into()))?;
        let mut out = Vec::with_capacity(len);
        let mut counter = self.seed;
        while out.len() < len {
            let word = counter.to_le_bytes();
            let take = (len - out.len()).min(4);
            out.extend_from_slice(&word[..take]);
            counter = counter.wrapping_add(1);
        }
        Ok(out)
    }
}

/// Decodes hex digits, ignoring spaces and dashes.
pub fn decode_hex(text: &str) -> Result<Vec<u8>, PayloadError> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|b| !matches!(b, b' ' | b'-' | b'\t' | b'\r' | b'\n'))
        .collect();
    if !digits.len().is_multiple_of(2) {
        return Err(PayloadError("hex has an odd number of digits".into()));
    }
    digits
        .chunks(2)
        .map(|pair| {
            let hi = hex_value(pair[0])?;
            let lo = hex_value(pair[1])?;
            Ok(hi << 4 | lo)
        })
        .collect()
}

fn hex_value(digit: u8) -> Result<u8, PayloadError> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        other => Err(PayloadError(format!(
            "invalid hex digit {:?}",
            char::from(other)
        ))),
    }
}

/// Encodes bytes as lowercase hex without separators.
#[must_use]
pub fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes bytes as standard padded base64.
#[must_use]
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard base64 (padding optional, whitespace ignored).
pub fn decode_base64(text: &str) -> Result<Vec<u8>, PayloadError> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0_u32, 0_u32);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b' ' | b'\t' | b'\r' | b'\n' => continue,
            other => {
                return Err(PayloadError(format!(
                    "invalid base64 character {:?}",
                    char::from(other)
                )));
            }
        };
        acc = acc << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

/// Renders bytes for a failure report: printable text with escapes plus length.
#[must_use]
pub fn preview(bytes: &[u8], limit: usize) -> String {
    let shown = &bytes[..bytes.len().min(limit)];
    let mut s = String::new();
    for &b in shown {
        match b {
            b'\r' => s.push_str("\\r"),
            b'\n' => s.push_str("\\n"),
            b'\t' => s.push_str("\\t"),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(char::from(b)),
            _ => s.push_str(&format!("\\x{b:02x}")),
        }
    }
    if bytes.len() > limit {
        s.push_str(&format!("… ({} bytes total)", bytes.len()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter32_is_little_endian_words_from_the_seed() {
        let g = Generate {
            pattern: Pattern::Counter32,
            bytes: 10,
            seed: 0x0102,
        };
        assert_eq!(
            g.bytes().unwrap(),
            vec![0x02, 0x01, 0, 0, 0x03, 0x01, 0, 0, 0x04, 0x01]
        );
    }

    #[test]
    fn hex_ignores_separators_and_rejects_bad_digits() {
        assert_eq!(decode_hex("0b 4d-53").unwrap(), vec![0x0b, 0x4d, 0x53]);
        assert!(decode_hex("0g").is_err());
        assert!(decode_hex("abc").is_err());
    }

    #[test]
    fn base64_round_trips_all_lengths() {
        for len in 0..9_u8 {
            let data: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(decode_base64(&encode_base64(&data)).unwrap(), data);
        }
        assert_eq!(encode_base64(b"Man"), "TWFu");
        assert_eq!(decode_base64("TWE=").unwrap(), b"Ma");
    }

    #[test]
    fn exactly_one_form_is_required() {
        let none = PayloadSpec::default();
        assert!(none.bytes().is_err());
        let two = PayloadSpec {
            text: Some("a".into()),
            hex: Some("00".into()),
            ..PayloadSpec::default()
        };
        assert!(two.bytes().is_err());
    }
}
