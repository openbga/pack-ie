use thiserror::Error;

use crate::ascii::{hex_pair, hex_value, is_printable};

/// Enquiry control byte.
pub const ENQ: u8 = 0x05;
/// Positive acknowledgement control byte.
pub const ACK: u8 = 0x06;
/// Negative acknowledgement control byte.
pub const NAK: u8 = 0x15;
/// End-of-transmission control byte.
pub const EOT: u8 = 0x04;
/// Start-of-text control byte.
pub const STX: u8 = 0x02;
/// End-of-text control byte for the last frame in a record.
pub const ETX: u8 = 0x03;
/// End-of-block control byte for an intermediate frame.
pub const ETB: u8 = 0x17;
/// Carriage return byte.
pub const CR: u8 = 0x0d;
/// Line feed byte.
pub const LF: u8 = 0x0a;

/// Frame text limit of the serial profile (E1381): the default everywhere.
pub(crate) const MAX_TEXT_BYTES: usize = 240;
/// Largest frame text any function accepts: ASTM over TCP lets vendors configure long
/// frames up to 64,000 characters.
pub const MAX_LONG_FRAME_TEXT: usize = 64_000;
/// STX, frame number, ETB/ETX, two checksum digits, CR, LF.
pub(crate) const FRAME_OVERHEAD_BYTES: usize = 7;

/// Whether a frame continues or ends the current record text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameEnd {
    /// More text follows in another frame.
    Intermediate,
    /// This is the final frame for the current record text.
    Final,
}

impl FrameEnd {
    fn byte(self) -> u8 {
        match self {
            Self::Intermediate => ETB,
            Self::Final => ETX,
        }
    }
}

/// A validated low-level frame with its exact received bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    number: u8,
    text: Vec<u8>,
    end: FrameEnd,
    raw: Vec<u8>,
}

impl Frame {
    /// Returns the numeric frame sequence in the range 0 through 7.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.number
    }

    /// Returns the validated application text exactly as received.
    #[must_use]
    pub fn text(&self) -> &[u8] {
        &self.text
    }

    /// Reports whether the frame is intermediate or final.
    #[must_use]
    pub const fn end(&self) -> FrameEnd {
        self.end
    }

    /// Returns the complete frame bytes, including control bytes and checksum.
    #[must_use]
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }
}

/// A framing or encoding failure that never includes received payload text.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum FrameError {
    /// The frame is too short to contain its required fields.
    #[error("ASTM frame is shorter than the required framing bytes")]
    TooShort,
    /// The leading start-of-text byte is absent.
    #[error("ASTM frame is missing STX")]
    MissingStart,
    /// The trailing carriage-return/line-feed pair is absent.
    #[error("ASTM frame is missing its CR/LF terminator")]
    MissingTerminator,
    /// Neither ETB nor ETX occurs in the framing position.
    #[error("ASTM frame is missing ETB or ETX")]
    MissingEndMarker,
    /// The frame sequence byte is not an ASCII digit from zero through seven.
    #[error("ASTM frame number is outside 0 through 7")]
    BadFrameNumber,
    /// The frame text exceeds the frame text limit in force (240 bytes unless configured).
    #[error("ASTM frame text exceeds the frame text limit")]
    Oversize,
    /// Text contains a byte outside printable ASCII and carriage return.
    #[error("ASTM frame text contains a disallowed byte at offset {offset}")]
    DisallowedTextByte {
        /// Zero-based byte position inside the frame text.
        offset: usize,
    },
    /// Checksum characters are not two uppercase hexadecimal digits.
    #[error("ASTM frame checksum is not two uppercase hexadecimal digits")]
    InvalidChecksumDigits,
    /// The transmitted checksum does not match the computed checksum.
    #[error("ASTM frame checksum does not match")]
    BadChecksum,
    /// A record passed to the encoder already contains a record terminator.
    #[error("ASTM record encoder input contains a carriage return")]
    EmbeddedRecordTerminator,
}

/// Computes the low byte of the sum from frame number through ETB/ETX.
#[must_use]
pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))
}

/// Encodes one validated frame using uppercase hexadecimal checksum digits.
///
/// The text may be up to [`MAX_LONG_FRAME_TEXT`] bytes; [`encode_records`] and
/// [`encode_records_within`] choose the size at which records are split.
pub fn encode_frame(number: u8, text: &[u8], end: FrameEnd) -> Result<Vec<u8>, FrameError> {
    validate_number(number)?;
    validate_text(text)?;

    let number_byte = b'0' + number;
    let end_byte = end.byte();
    let mut framed = Vec::with_capacity(text.len() + FRAME_OVERHEAD_BYTES);
    framed.extend_from_slice(&[STX, number_byte]);
    framed.extend_from_slice(text);
    framed.push(end_byte);
    let sum = checksum(&framed[1..]);
    framed.extend_from_slice(&hex_pair(sum));
    framed.extend_from_slice(&[CR, LF]);
    Ok(framed)
}

/// Encodes unterminated E1394 records, splitting records longer than 240 bytes
/// across frames.
///
/// # Errors
/// A record holds a CR or a byte E1394 does not allow in frame text.
pub fn encode_records(records: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, FrameError> {
    encode_records_within(records, MAX_TEXT_BYTES)
}

/// Encodes unterminated E1394 records with at most `max_text` bytes of text per frame
/// (clamped to 1 through [`MAX_LONG_FRAME_TEXT`]), splitting longer records across intermediate frames.
///
/// # Errors
/// A record holds a CR or a byte E1394 does not allow in frame text.
pub fn encode_records_within(
    records: &[Vec<u8>],
    max_text: usize,
) -> Result<Vec<Vec<u8>>, FrameError> {
    let size = clamp_text_limit(max_text);
    let mut frames = Vec::new();
    let mut number = 1_u8;

    for record in records {
        if record.contains(&CR) {
            return Err(FrameError::EmbeddedRecordTerminator);
        }
        validate_allowed_text(record)?;

        let mut terminated = record.clone();
        terminated.push(CR);
        let chunk_count = terminated.len().div_ceil(size);
        for (index, chunk) in terminated.chunks(size).enumerate() {
            let end = if index + 1 == chunk_count {
                FrameEnd::Final
            } else {
                FrameEnd::Intermediate
            };
            frames.push(encode_frame(number, chunk, end)?);
            number = (number + 1) % 8;
        }
    }
    Ok(frames)
}

/// Clamps a configured frame text limit into 1 through [`MAX_LONG_FRAME_TEXT`].
pub(crate) const fn clamp_text_limit(max_text: usize) -> usize {
    if max_text == 0 {
        1
    } else if max_text > MAX_LONG_FRAME_TEXT {
        MAX_LONG_FRAME_TEXT
    } else {
        max_text
    }
}

/// Parses and validates one complete low-level frame with the default 240-byte text limit.
pub fn parse_frame(raw: &[u8]) -> Result<Frame, FrameError> {
    parse_frame_within(raw, MAX_TEXT_BYTES)
}

/// Parses and validates one complete low-level frame whose text may hold up to
/// `max_text` bytes (clamped to 1 through [`MAX_LONG_FRAME_TEXT`]).
///
/// # Errors
/// The frame is malformed, its checksum is wrong or its text exceeds `max_text`.
pub fn parse_frame_within(raw: &[u8], max_text: usize) -> Result<Frame, FrameError> {
    if raw.len() < FRAME_OVERHEAD_BYTES {
        return Err(FrameError::TooShort);
    }
    if raw.first() != Some(&STX) {
        return Err(FrameError::MissingStart);
    }
    if !raw.ends_with(&[CR, LF]) {
        return Err(FrameError::MissingTerminator);
    }

    let number = parse_number(raw[1])?;
    let marker_index = raw.len() - 5;
    let end = match raw[marker_index] {
        ETB => FrameEnd::Intermediate,
        ETX => FrameEnd::Final,
        _ => return Err(FrameError::MissingEndMarker),
    };
    let text = &raw[2..marker_index];
    if text.len() > clamp_text_limit(max_text) {
        return Err(FrameError::Oversize);
    }
    validate_allowed_text(text)?;
    let transmitted = parse_hex_pair(&raw[marker_index + 1..marker_index + 3])?;
    let expected = checksum(&raw[1..=marker_index]);
    if transmitted != expected {
        return Err(FrameError::BadChecksum);
    }

    Ok(Frame {
        number,
        text: text.to_vec(),
        end,
        raw: raw.to_vec(),
    })
}

fn validate_number(number: u8) -> Result<(), FrameError> {
    if number <= 7 {
        Ok(())
    } else {
        Err(FrameError::BadFrameNumber)
    }
}

fn parse_number(byte: u8) -> Result<u8, FrameError> {
    let number = byte.checked_sub(b'0').ok_or(FrameError::BadFrameNumber)?;
    validate_number(number)?;
    Ok(number)
}

fn validate_text(text: &[u8]) -> Result<(), FrameError> {
    if text.len() > MAX_LONG_FRAME_TEXT {
        return Err(FrameError::Oversize);
    }
    validate_allowed_text(text)
}

fn validate_allowed_text(text: &[u8]) -> Result<(), FrameError> {
    if let Some(offset) = text
        .iter()
        .position(|byte| *byte != CR && !is_printable(*byte))
    {
        return Err(FrameError::DisallowedTextByte { offset });
    }
    Ok(())
}

fn parse_hex_pair(bytes: &[u8]) -> Result<u8, FrameError> {
    let [high, low] = *bytes else {
        return Err(FrameError::InvalidChecksumDigits);
    };
    if high.is_ascii_lowercase() || low.is_ascii_lowercase() {
        return Err(FrameError::InvalidChecksumDigits);
    }
    match (hex_value(high), hex_value(low)) {
        (Some(high), Some(low)) => Ok((high << 4) | low),
        _ => Err(FrameError::InvalidChecksumDigits),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn frame_round_trip_uses_uppercase_checksum() {
        let raw = encode_frame(1, b"H|\\^&|||SYNTH-ANALYZER^1.0\r", FrameEnd::Final)
            .expect("synthetic frame encodes");
        let parsed = parse_frame(&raw).expect("encoded frame parses");
        assert_eq!(parsed.number(), 1);
        assert_eq!(parsed.end(), FrameEnd::Final);
        assert_eq!(parsed.text(), b"H|\\^&|||SYNTH-ANALYZER^1.0\r");
        assert!(
            raw[raw.len() - 4..raw.len() - 2]
                .iter()
                .all(|byte| !byte.is_ascii_lowercase())
        );
    }

    #[test]
    fn checksum_covers_number_text_and_end_marker() {
        let raw = encode_frame(7, b"R|1|^^^SYN-TEST|1.230|mg/L\r", FrameEnd::Final)
            .expect("synthetic frame encodes");
        let marker = raw.len() - 5;
        let expected = checksum(&raw[1..=marker]);
        assert_eq!(parse_hex_pair(&raw[marker + 1..marker + 3]), Ok(expected));
    }

    #[test]
    fn frame_text_over_the_default_is_rejected_by_default_parsing_only() {
        let text = vec![b'A'; 241];
        let raw = encode_frame(1, &text, FrameEnd::Final).expect("long frame encodes");
        assert_eq!(parse_frame(&raw), Err(FrameError::Oversize));
        assert_eq!(parse_frame_within(&raw, 240), Err(FrameError::Oversize));
        assert_eq!(parse_frame_within(&raw, 241).expect("fits").text(), text);
    }

    #[test]
    fn default_text_limit_is_exactly_240() {
        let raw = encode_frame(1, &[b'A'; 240], FrameEnd::Final).expect("frame encodes");
        assert!(parse_frame(&raw).is_ok());
    }

    #[test]
    fn frame_text_is_bounded_by_the_long_frame_maximum() {
        let at_max = vec![b'A'; MAX_LONG_FRAME_TEXT];
        let raw = encode_frame(1, &at_max, FrameEnd::Final).expect("maximum frame encodes");
        assert_eq!(raw.len(), MAX_LONG_FRAME_TEXT + FRAME_OVERHEAD_BYTES);
        assert!(parse_frame_within(&raw, MAX_LONG_FRAME_TEXT).is_ok());
        // A configured limit above the maximum is clamped to it.
        assert!(parse_frame_within(&raw, usize::MAX).is_ok());

        let too_long = vec![b'A'; MAX_LONG_FRAME_TEXT + 1];
        assert_eq!(
            encode_frame(1, &too_long, FrameEnd::Final),
            Err(FrameError::Oversize)
        );
    }

    #[test]
    fn a_zero_text_limit_is_clamped_to_one() {
        let one = encode_frame(1, b"A", FrameEnd::Final).expect("frame encodes");
        let two = encode_frame(1, b"AB", FrameEnd::Final).expect("frame encodes");
        assert!(parse_frame_within(&one, 0).is_ok());
        assert_eq!(parse_frame_within(&two, 0), Err(FrameError::Oversize));
    }

    #[test]
    fn records_split_at_the_configured_size_and_reassemble() {
        let record = vec![b'R'; 1000];
        let records = [record.clone()];

        let one_frame = encode_records_within(&records, MAX_LONG_FRAME_TEXT).expect("encodes");
        assert_eq!(one_frame.len(), 1);
        let parsed = parse_frame_within(&one_frame[0], MAX_LONG_FRAME_TEXT).expect("parses");
        assert_eq!(parsed.end(), FrameEnd::Final);
        assert_eq!(&parsed.text()[..1000], &record[..]);

        let split = encode_records_within(&records, 300).expect("encodes");
        assert_eq!(split.len(), 4);
        let mut text = Vec::new();
        for (index, raw) in split.iter().enumerate() {
            let frame = parse_frame_within(raw, 300).expect("parses");
            let last = index + 1 == split.len();
            assert_eq!(
                frame.end(),
                if last {
                    FrameEnd::Final
                } else {
                    FrameEnd::Intermediate
                }
            );
            text.extend_from_slice(frame.text());
        }
        assert_eq!(text, [record.as_slice(), &[CR]].concat());
    }

    #[test]
    fn encode_records_defaults_stay_at_240() {
        let records = [vec![b'R'; 500]];
        let frames = encode_records(&records).expect("encodes");
        assert_eq!(frames.len(), 3);
        assert!(frames.iter().all(|raw| parse_frame(raw).is_ok()));
    }

    proptest! {
        #[test]
        fn parsing_arbitrary_bytes_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
            let _ = parse_frame(&bytes);
        }

        #[test]
        fn encoded_checksum_always_parses(
            number in 0_u8..8,
            text in prop::collection::vec(0x20_u8..0x7f, 0..241),
            final_frame in any::<bool>(),
        ) {
            let end = if final_frame { FrameEnd::Final } else { FrameEnd::Intermediate };
            let raw = encode_frame(number, &text, end).expect("generated frame encodes");
            let parsed = parse_frame(&raw).expect("generated frame parses");
            prop_assert_eq!(parsed.text(), text);
        }

        #[test]
        fn long_frames_round_trip_within_their_limit(
            text in prop::collection::vec(0x20_u8..0x7f, 241..2000),
            slack in 0_usize..50,
        ) {
            let raw = encode_frame(1, &text, FrameEnd::Final).expect("generated frame encodes");
            prop_assert_eq!(parse_frame(&raw), Err(FrameError::Oversize));
            let parsed = parse_frame_within(&raw, text.len() + slack).expect("fits the limit");
            prop_assert_eq!(parsed.text(), text.as_slice());
        }

        #[test]
        fn parsing_arbitrary_bytes_within_any_limit_never_panics(
            bytes in prop::collection::vec(any::<u8>(), 0..512),
            limit in any::<usize>(),
        ) {
            let _ = parse_frame_within(&bytes, limit);
        }
    }
}
