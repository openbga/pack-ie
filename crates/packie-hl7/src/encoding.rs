use crate::Error;

/// Wire delimiters from MSH-1 and MSH-2, in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingCharacters {
    field: u8,
    component: u8,
    repetition: u8,
    escape: u8,
    subcomponent: u8,
}

impl Default for EncodingCharacters {
    fn default() -> Self {
        Self {
            field: b'|',
            component: b'^',
            repetition: b'~',
            escape: b'\\',
            subcomponent: b'&',
        }
    }
}

impl EncodingCharacters {
    /// All five delimiters must be distinct ASCII punctuation characters.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidEncodingCharacters`] when any delimiter is not ASCII
    /// punctuation or two delimiters are equal.
    pub fn new(
        field: u8,
        component: u8,
        repetition: u8,
        escape: u8,
        subcomponent: u8,
    ) -> Result<Self, Error> {
        let bytes = [field, component, repetition, escape, subcomponent];
        if bytes.iter().any(|byte| !byte.is_ascii_punctuation())
            || bytes
                .iter()
                .enumerate()
                .any(|(i, byte)| bytes[..i].contains(byte))
        {
            return Err(Error::InvalidEncodingCharacters);
        }
        Ok(Self {
            field,
            component,
            repetition,
            escape,
            subcomponent,
        })
    }
    /// Field delimiter (MSH-1).
    #[must_use]
    pub fn field(self) -> u8 {
        self.field
    }
    /// Component delimiter.
    #[must_use]
    pub fn component(self) -> u8 {
        self.component
    }
    /// Repetition delimiter.
    #[must_use]
    pub fn repetition(self) -> u8 {
        self.repetition
    }
    /// Escape delimiter.
    #[must_use]
    pub fn escape(self) -> u8 {
        self.escape
    }
    /// Subcomponent delimiter.
    #[must_use]
    pub fn subcomponent(self) -> u8 {
        self.subcomponent
    }
    /// The MSH-2 bytes: component, repetition, escape, subcomponent.
    #[must_use]
    pub fn msh2(self) -> [u8; 4] {
        [
            self.component,
            self.repetition,
            self.escape,
            self.subcomponent,
        ]
    }
}

impl EncodingCharacters {
    /// Each delimiter with the escape code (`F`, `S`, `T`, `R`, `E`) that stands for it.
    fn escape_codes(self) -> [(u8, u8); 5] {
        [
            (self.field, b'F'),
            (self.component, b'S'),
            (self.subcomponent, b'T'),
            (self.repetition, b'R'),
            (self.escape, b'E'),
        ]
    }
}

/// Escape a literal UTF-8 value, including controls (as hexadecimal escapes).
/// Unknown escape-looking text supplied here is literal and its escape delimiter
/// is escaped too; parsed unknown escapes retain their original wire form.
#[must_use]
pub fn escape(value: &str, encoding: EncodingCharacters) -> Vec<u8> {
    let codes = encoding.escape_codes();
    let mut out = Vec::with_capacity(value.len());
    for byte in value.bytes() {
        if let Some((_, code)) = codes.iter().find(|(delimiter, _)| *delimiter == byte) {
            out.extend_from_slice(&[encoding.escape, *code, encoding.escape]);
        } else if byte.is_ascii_control() {
            let hex = b"0123456789ABCDEF";
            out.extend_from_slice(&[
                encoding.escape,
                b'X',
                hex[usize::from(byte >> 4)],
                hex[usize::from(byte & 15)],
                encoding.escape,
            ]);
        } else {
            out.push(byte);
        }
    }
    out
}

/// Decode standard delimiter and `Xhh..` escapes to bytes. Unknown sequences
/// (including `Z..`) remain verbatim; unterminated and invalid hex escapes fail.
/// The byte result avoids silently corrupting non-UTF-8 partner encodings.
///
/// # Errors
///
/// [`Error::TruncatedEscape`] when an escape is not closed, and
/// [`Error::InvalidHexEscape`] for a hexadecimal escape that is empty, has an odd
/// number of digits or contains a non-hexadecimal digit.
pub fn decode(value: &[u8], encoding: EncodingCharacters) -> Result<Vec<u8>, Error> {
    let codes = encoding.escape_codes();
    let mut out = Vec::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.iter().position(|byte| *byte == encoding.escape) {
        out.extend_from_slice(&rest[..start]);
        let tail = &rest[start + 1..];
        let end = tail
            .iter()
            .position(|byte| *byte == encoding.escape)
            .ok_or(Error::TruncatedEscape)?;
        let code = &tail[..end];
        if let [code] = code
            && let Some((delimiter, _)) = codes.iter().find(|(_, known)| known == code)
        {
            out.push(*delimiter);
        } else if let [b'X', hex @ ..] = code {
            decode_hex(hex, &mut out)?;
        } else {
            out.extend_from_slice(&rest[start..start + end + 2]);
        }
        rest = &tail[end + 1..];
    }
    out.extend_from_slice(rest);
    Ok(out)
}

fn decode_hex(hex: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return Err(Error::InvalidHexEscape);
    }
    for pair in hex.chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or(Error::InvalidHexEscape)?;
        let low = hex_digit(pair[1]).ok_or(Error::InvalidHexEscape)?;
        out.push(high << 4 | low);
    }
    Ok(())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn split(value: &[u8], delimiter: u8, escape: u8) -> Vec<&[u8]> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (index, byte) in value.iter().enumerate() {
        if *byte == escape {
            escaped = !escaped;
        } else if *byte == delimiter && !escaped {
            pieces.push(&value[start..index]);
            start = index + 1;
        }
    }
    pieces.push(&value[start..]);
    pieces
}
