use crate::ascii::{hex_pair, hex_value, is_printable};
use crate::record_model::{Delimiters, RecordError};

/// Escape codes for the four delimiters, as `(code, delimiter byte)`.
const fn named_escapes(delimiters: Delimiters) -> [(u8, u8); 4] {
    [
        (b'F', delimiters.field()),
        (b'S', delimiters.component()),
        (b'R', delimiters.repeat()),
        (b'E', delimiters.escape()),
    ]
}

pub(crate) fn decode_escapes(raw: &[u8], delimiters: Delimiters) -> Result<String, RecordError> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        if raw[index] != delimiters.escape() {
            decoded.push(raw[index]);
            index += 1;
            continue;
        }
        let tail = &raw[index + 1..];
        let Some(end) = tail.iter().position(|byte| *byte == delimiters.escape()) else {
            return Err(RecordError::MalformedEscape);
        };
        decode_escape_code(&tail[..end], delimiters, &mut decoded)?;
        index += end + 2;
    }
    String::from_utf8(decoded).map_err(|_| RecordError::InvalidHexText)
}

fn decode_escape_code(
    code: &[u8],
    delimiters: Delimiters,
    output: &mut Vec<u8>,
) -> Result<(), RecordError> {
    match code {
        [name] => {
            let (_, delimiter) = named_escapes(delimiters)
                .into_iter()
                .find(|(candidate, _)| candidate == name)
                .ok_or(RecordError::MalformedEscape)?;
            output.push(delimiter);
        }
        [b'X', hex @ ..] if !hex.is_empty() && hex.len() % 2 == 0 => {
            for pair in hex.chunks_exact(2) {
                output.push(decode_hex_pair(pair)?);
            }
        }
        _ => return Err(RecordError::MalformedEscape),
    }
    Ok(())
}

fn decode_hex_pair(pair: &[u8]) -> Result<u8, RecordError> {
    match (hex_value(pair[0]), hex_value(pair[1])) {
        (Some(high), Some(low)) => Ok((high << 4) | low),
        _ => Err(RecordError::MalformedEscape),
    }
}

/// Encodes text for use inside a field: delimiter bytes become `F`, `S`, `R`
/// and `E` escapes, printable ASCII stays as is, and every other byte
/// (including carriage return and non-ASCII UTF-8) becomes a hex escape; a run
/// of such bytes shares one `X` escape.
pub(crate) fn encode_escapes(text: &str, delimiters: Delimiters) -> String {
    let escape = char::from(delimiters.escape());
    let named = named_escapes(delimiters);
    let mut encoded = String::with_capacity(text.len());
    let mut in_hex_run = false;
    for byte in text.bytes() {
        let code = named
            .iter()
            .find_map(|(code, delimiter)| (*delimiter == byte).then_some(*code));
        if code.is_none() && !is_printable(byte) {
            if !in_hex_run {
                encoded.push(escape);
                encoded.push('X');
                in_hex_run = true;
            }
            encoded.extend(hex_pair(byte).map(char::from));
            continue;
        }
        if in_hex_run {
            encoded.push(escape);
            in_hex_run = false;
        }
        match code {
            Some(code) => {
                encoded.push(escape);
                encoded.push(char::from(code));
                encoded.push(escape);
            }
            None => encoded.push(char::from(byte)),
        }
    }
    if in_hex_run {
        encoded.push(escape);
    }
    encoded
}
