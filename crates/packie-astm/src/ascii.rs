//! Byte classes and hexadecimal digits shared by framing and record escaping.

/// Reports whether the byte is printable seven-bit ASCII (space through tilde).
pub(crate) const fn is_printable(byte: u8) -> bool {
    matches!(byte, 0x20..=0x7e)
}

/// Returns the value of one hexadecimal digit of either case.
pub(crate) const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Returns the two uppercase hexadecimal digits of a byte.
pub(crate) const fn hex_pair(byte: u8) -> [u8; 2] {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    [DIGITS[(byte >> 4) as usize], DIGITS[(byte & 0x0f) as usize]]
}
