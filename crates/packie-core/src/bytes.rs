//! Byte counts for operators.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A number of bytes. Serializes as a plain integer; `Display` uses binary units
/// (`512 B`, `1.5 KiB`, `10.0 MiB`) with one decimal, computed without floats.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS,
)]
#[serde(transparent)]
#[ts(as = "u32")]
pub struct ByteSize(pub u64);

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [(&str, u64); 4] = [
            ("TiB", 1 << 40),
            ("GiB", 1 << 30),
            ("MiB", 1 << 20),
            ("KiB", 1 << 10),
        ];
        for (unit, size) in UNITS {
            if self.0 >= size {
                // Tenths, rounded half up, in integer arithmetic.
                let tenths = (u128::from(self.0) * 10 + u128::from(size) / 2) / u128::from(size);
                return write!(f, "{}.{} {unit}", tenths / 10, tenths % 10);
            }
        }
        write!(f, "{} B", self.0)
    }
}

/// Bytes as text with control characters made visible (`<STX>`, `<CR>`, ...). A `<CR>` or
/// `<LF>` ends a display line, so HL7 segments and ASTM records show one per line. Bytes that
/// are not valid UTF-8 are shown as `<xHH>`.
#[must_use]
pub fn visible_text(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        chunk
            .valid()
            .chars()
            .for_each(|c| push_visible(&mut out, c));
        for b in chunk.invalid() {
            out.push_str(&format!("<x{b:02X}>"));
        }
    }
    out
}

fn push_visible(out: &mut String, c: char) {
    const NAMES: [&str; 32] = [
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR",
        "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB",
        "ESC", "FS", "GS", "RS", "US",
    ];
    match u8::try_from(u32::from(c)) {
        Ok(b) if b < 32 => {
            out.push('<');
            out.push_str(NAMES[usize::from(b)]);
            out.push('>');
            if b == b'\r' || b == b'\n' {
                out.push('\n');
            }
        }
        Ok(0x7f) => out.push_str("<DEL>"),
        _ => out.push(c),
    }
}

/// A hex dump: offset, 16 bytes in hex, and the printable ASCII, one line per 16 bytes.
#[must_use]
pub fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (line, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    char::from(b)
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!(
            "{:08x}  {:<47}  {ascii}\n",
            line * 16,
            hex.join(" ")
        ));
    }
    out
}

/// The bytes of a [`hex_dump`], read back from its hex column; `None` when a line is not
/// one the dump produces (bad offset, bad hex digits, a column of the wrong width).
#[must_use]
pub fn hex_dump_bytes(dump: &str) -> Option<Vec<u8>> {
    /// Offset, two spaces, then up to 16 pairs joined by single spaces.
    const HEX_START: usize = 10;
    const HEX_WIDTH: usize = 47;
    let mut out = Vec::new();
    for line in dump.lines() {
        let offset = usize::from_str_radix(line.get(..8)?, 16).ok()?;
        if offset != out.len() || line.get(8..HEX_START)? != "  " {
            return None;
        }
        let rest = line.get(HEX_START..)?;
        let column = rest.get(..HEX_WIDTH.min(rest.len()))?;
        for pair in column.split_whitespace() {
            if pair.len() != 2 {
                return None;
            }
            out.push(u8::from_str_radix(pair, 16).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uses_binary_units_with_one_decimal() {
        assert_eq!(ByteSize(0).to_string(), "0 B");
        assert_eq!(ByteSize(1023).to_string(), "1023 B");
        assert_eq!(ByteSize(1024).to_string(), "1.0 KiB");
        assert_eq!(ByteSize(1536).to_string(), "1.5 KiB");
        assert_eq!(ByteSize(10 << 20).to_string(), "10.0 MiB");
        assert_eq!(ByteSize(u64::MAX).to_string(), "16777216.0 TiB");
    }

    #[test]
    fn hex_dump_bytes_inverts_hex_dump() {
        for length in [0, 1, 15, 16, 17, 32, 33] {
            let bytes: Vec<u8> = (0..length)
                .map(|i| u8::try_from(i * 7 % 256).unwrap_or(0))
                .collect();
            assert_eq!(hex_dump_bytes(&hex_dump(&bytes)), Some(bytes), "{length}");
        }
        assert_eq!(hex_dump_bytes(""), Some(Vec::new()));
        // Text that looks like hex in the ASCII column is not read as bytes.
        assert_eq!(
            hex_dump_bytes(&hex_dump(b"41 42 43")),
            Some(b"41 42 43".to_vec())
        );
        assert_eq!(hex_dump_bytes("not a dump"), None);
        assert_eq!(hex_dump_bytes("00000000  zz  ."), None);
        assert_eq!(hex_dump_bytes("00000010  41  A"), None);
    }

    proptest::proptest! {
        #[test]
        fn hex_dump_bytes_round_trips_arbitrary_bytes(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..200)) {
            proptest::prop_assert_eq!(hex_dump_bytes(&hex_dump(&bytes)), Some(bytes));
        }
    }

    #[test]
    fn visible_text_names_control_characters() {
        assert_eq!(
            visible_text(b"\x02MSH|^~\\&\rPID\x03"),
            "<STX>MSH|^~\\&<CR>\nPID<ETX>"
        );
        assert_eq!(visible_text(&[b'a', 0xff, b'b']), "a<xFF>b");
        assert_eq!(visible_text("é".as_bytes()), "é");
    }

    #[test]
    fn hex_dump_has_offsets_and_ascii() {
        let dump = hex_dump(b"\x02ABC");
        assert_eq!(dump, format!("00000000  {:<47}  .ABC\n", "02 41 42 43"));
        assert_eq!(hex_dump(&[0; 17]).lines().count(), 2);
    }

    #[test]
    fn serializes_as_an_integer() {
        assert_eq!(serde_json::to_string(&ByteSize(2048)).unwrap(), "2048");
        assert_eq!(serde_json::from_str::<ByteSize>("5").unwrap(), ByteSize(5));
    }
}
