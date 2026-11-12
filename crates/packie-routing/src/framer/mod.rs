//! Protocol framers for the bridge tap.
//!
//! A tap attaches one framer to each direction of a bridge channel. The framer receives a
//! copy of the bytes already forwarded, counts complete messages and framing errors, and
//! hands the observations to the tap. It never sees the live stream, so it cannot alter,
//! delay or block it, and it never logs or reports payload bytes in an error.
//!
//! Every framer is a small state machine fed arbitrary chunks: a stream split at any byte
//! boundary yields the same observations as the unsplit stream. The framing itself comes
//! from the pure protocol crates ([`packie_hl7::mllp`], [`packie_astm`], [`packie_au`]); the
//! framers add only the grouping and de-duplication rules a passive observer needs.

use packie_astm::{ACK, ENQ, EOT, ETX, Frame, FrameEnd, LF, NAK, STX, parse_frame};
use packie_au::{
    AuConfig, AuError, FormatConfig, FramingConfig, StreamDecoder, TransmissionClass, WireEvent,
};
use packie_core::ProtocolKind;
use packie_hl7::mllp::{Decoder, Event, MalformedReason};

/// What a framer observed in a copy of one direction's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Observation {
    /// One complete protocol message, with its bytes (framing removed as described per protocol).
    Message(Vec<u8>),
    /// Garbage, a malformed or over-long frame: one framing error. Never carries payload bytes.
    FramingError,
}

/// A protocol framer over one direction of a bridge.
pub(crate) trait Framer: Send {
    /// Feeds the next forwarded bytes, appending observations in stream order.
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<Observation>);
}

/// Default limit for one observed message (`tap.max_message_bytes`, 1 MiB).
#[cfg(test)]
pub(crate) const DEFAULT_MAX_MESSAGE_BYTES: usize = 1_048_576;

/// The framer for `kind`, or `None` for kinds without framing (`raw`, `delimited`).
///
/// `max_message_bytes` bounds the bytes retained for one message. For HL7 it limits the
/// payload between the MLLP delimiters, for ASTM the concatenated record text of one
/// message, for AU the concatenated text of one message's blocks.
pub(crate) fn framer_for(kind: ProtocolKind, max_message_bytes: usize) -> Option<Box<dyn Framer>> {
    match kind {
        ProtocolKind::Hl7 => Some(Box::new(MllpFramer::new(max_message_bytes))),
        ProtocolKind::Astm => Some(Box::new(AstmFramer::new(max_message_bytes))),
        ProtocolKind::Au => {
            AuFramer::new(max_message_bytes).map(|framer| Box::new(framer) as Box<dyn Framer>)
        }
        ProtocolKind::Delimited | ProtocolKind::Raw => None,
    }
}

// ---------------------------------------------------------------------------------------
// HL7 over MLLP
// ---------------------------------------------------------------------------------------

/// MLLP framer: `VT payload FS CR`. A message is the payload without the framing bytes.
struct MllpFramer {
    decoder: Decoder,
    /// Whether the previous event was garbage, so a run split across chunks counts once.
    in_garbage: bool,
}

impl MllpFramer {
    fn new(max_message_bytes: usize) -> Self {
        Self {
            decoder: Decoder::new(max_message_bytes),
            in_garbage: false,
        }
    }
}

impl Framer for MllpFramer {
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<Observation>) {
        let in_garbage = &mut self.in_garbage;
        self.decoder.feed(bytes, |event| match event {
            Event::Message(payload) => {
                *in_garbage = false;
                out.push(Observation::Message(payload));
            }
            Event::Garbage { .. } => {
                if !*in_garbage {
                    out.push(Observation::FramingError);
                }
                *in_garbage = true;
            }
            Event::Malformed { reason, .. } => {
                out.push(Observation::FramingError);
                // The byte that broke the trailer is reported as garbage right after this
                // event; it belongs to the same fault.
                *in_garbage = reason == MalformedReason::MissingCarriageReturn;
            }
            Event::Oversize { .. } | Event::AllocationFailed => {
                *in_garbage = false;
                out.push(Observation::FramingError);
            }
            Event::Truncated { .. } => {}
        });
    }
}

// ---------------------------------------------------------------------------------------
// ASTM E1381 / E1394
// ---------------------------------------------------------------------------------------

/// Longest raw frame the framer buffers: a frame is at most 247 bytes (240 text bytes plus
/// seven bytes of framing), so anything longer without a line feed cannot be a frame.
const MAX_RAW_FRAME_BYTES: usize = 512;

/// Field delimiter assumed until a header record announces the real one.
const DEFAULT_FIELD_DELIMITER: u8 = b'|';

/// Carriage return, the E1394 record terminator inside frame text.
const RECORD_END: u8 = packie_astm::CR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AstmState {
    /// Between frames: control bytes are recognised, anything else is stray.
    Idle,
    /// Collecting a frame from its `STX` through the line feed.
    InFrame,
    /// Dropping the rest of an over-long frame until `ENQ`, `EOT` or `STX`.
    Discarding,
}

/// ASTM framer. One message is a run of records from the header to the terminator record
/// (`L` and the field delimiter) finished by a final (`ETX`) frame; the observed bytes are the
/// concatenated frame texts, i.e. the records with their carriage returns.
///
/// A valid frame carrying the same number as the last accepted one is a retransmission and is
/// ignored. `ENQ` and `EOT` end the current transfer and drop any partial message. A message
/// beyond the byte limit counts one framing error and is skipped until its terminator record,
/// `ENQ` or `EOT`.
struct AstmFramer {
    max_message_bytes: usize,
    state: AstmState,
    /// The frame being collected, `STX` first.
    frame: Vec<u8>,
    /// Whether the current run of stray bytes has already been counted.
    stray: bool,
    last_number: Option<u8>,
    /// Records completed so far in the current message.
    message: Vec<u8>,
    /// Text of the current record, collected across `ETB` frames.
    record: Vec<u8>,
    delimiter: u8,
    /// The current message exceeded the limit and is being skipped.
    overflowed: bool,
}

impl AstmFramer {
    fn new(max_message_bytes: usize) -> Self {
        Self {
            max_message_bytes,
            state: AstmState::Idle,
            frame: Vec::new(),
            stray: false,
            last_number: None,
            message: Vec::new(),
            record: Vec::new(),
            delimiter: DEFAULT_FIELD_DELIMITER,
            overflowed: false,
        }
    }

    fn byte(&mut self, byte: u8, out: &mut Vec<Observation>) {
        match self.state {
            AstmState::Idle => self.idle_byte(byte, out),
            AstmState::InFrame => self.frame_byte(byte, out),
            AstmState::Discarding => {
                if matches!(byte, STX | ENQ | EOT) {
                    self.state = AstmState::Idle;
                    self.idle_byte(byte, out);
                }
            }
        }
    }

    fn idle_byte(&mut self, byte: u8, out: &mut Vec<Observation>) {
        match byte {
            STX => {
                self.stray = false;
                self.frame.clear();
                self.frame.push(STX);
                self.state = AstmState::InFrame;
            }
            ENQ | EOT => {
                self.stray = false;
                self.reset_transfer();
            }
            ACK | NAK => self.stray = false,
            _ => {
                if !self.stray {
                    self.stray = true;
                    out.push(Observation::FramingError);
                }
            }
        }
    }

    fn frame_byte(&mut self, byte: u8, out: &mut Vec<Observation>) {
        if matches!(byte, STX | ENQ | EOT) {
            // The frame was abandoned before its line feed.
            out.push(Observation::FramingError);
            self.frame.clear();
            self.state = AstmState::Idle;
            self.idle_byte(byte, out);
            return;
        }
        self.frame.push(byte);
        if byte == LF {
            let raw = std::mem::take(&mut self.frame);
            self.state = AstmState::Idle;
            match parse_frame(&raw) {
                Ok(frame) => self.accept(&frame, out),
                Err(_) => out.push(Observation::FramingError),
            }
        } else if self.frame.len() > MAX_RAW_FRAME_BYTES {
            self.frame.clear();
            self.state = AstmState::Discarding;
            out.push(Observation::FramingError);
        }
    }

    fn reset_transfer(&mut self) {
        self.message.clear();
        self.record.clear();
        self.last_number = None;
        self.delimiter = DEFAULT_FIELD_DELIMITER;
        self.overflowed = false;
    }

    fn accept(&mut self, frame: &Frame, out: &mut Vec<Observation>) {
        let number = frame.number();
        if self.last_number == Some(number) {
            return;
        }
        self.last_number = Some(number);
        if self.overflowed {
            // Skipping an over-long message; its terminator record ends the skip.
            if frame.end() == FrameEnd::Final && self.is_terminator(frame.text()) {
                self.overflowed = false;
            }
            return;
        }
        let total = self
            .message
            .len()
            .saturating_add(self.record.len())
            .saturating_add(frame.text().len());
        if total > self.max_message_bytes {
            self.overflowed = true;
            self.message.clear();
            self.record.clear();
            out.push(Observation::FramingError);
            return;
        }
        self.record.extend_from_slice(frame.text());
        if frame.end() == FrameEnd::Final {
            self.finish_record(out);
        }
    }

    /// Adds the completed record text to the message and emits it after its terminator.
    fn finish_record(&mut self, out: &mut Vec<Observation>) {
        let record = std::mem::take(&mut self.record);
        let mut last = false;
        for piece in record.split(|&byte| byte == RECORD_END) {
            if piece.first() == Some(&b'H') {
                self.delimiter = piece.get(1).copied().unwrap_or(DEFAULT_FIELD_DELIMITER);
            }
            last |= self.is_terminator(piece);
        }
        self.message.extend_from_slice(&record);
        if last {
            out.push(Observation::Message(std::mem::take(&mut self.message)));
        }
    }

    /// Whether `text` starts with the terminator record: `L` and the field delimiter.
    fn is_terminator(&self, text: &[u8]) -> bool {
        text.first() == Some(&b'L')
            && matches!(text.get(1), Some(&byte) if byte == self.delimiter || byte == RECORD_END)
    }
}

impl Framer for AstmFramer {
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<Observation>) {
        for &byte in bytes {
            self.byte(byte, out);
        }
    }
}

// ---------------------------------------------------------------------------------------
// AU
// ---------------------------------------------------------------------------------------

/// AU framer built on [`StreamDecoder`]. A frame ending with the text terminator is one
/// message (the text between the delimiters); blocks ended with `ETB` are joined to the
/// frame that completes them. Standalone `ACK` and `NAK` bytes are not messages.
///
/// Limitation: the AU wire format is an instrument setting (delimiters, block check,
/// maximum frame length), and a bridge channel carries no AU profile. The framer therefore
/// assumes the common settings (see [`au_framing_config`]: `STX` text `ETX` with a block
/// check character, `ETB` blocks, 1024 byte frames). An instrument configured differently
/// produces framing errors rather than messages. Message contents are never interpreted.
struct AuFramer {
    decoder: StreamDecoder,
    max_message_bytes: usize,
    /// Text of the `ETB` blocks received so far for the current message.
    blocks: Vec<u8>,
    /// Whether the current run of framing faults has already been counted.
    in_fault: bool,
}

/// The framing settings assumed for every AU tap.
fn au_framing_config() -> AuConfig {
    AuConfig {
        framing: FramingConfig {
            start: vec![STX],
            end: vec![ETX],
            bcc: true,
            use_etb: true,
            max_wire_len: 1024,
            class: TransmissionClass::A,
        },
        // The format only matters when parsing message contents, which a tap never does.
        format: FormatConfig {
            unit_no: true,
            rack_digits: 4,
            sample_id_digits: 8,
            sex: false,
            age: false,
            month: false,
            patient_info_digits: [0; 6],
            result_digits: 6,
            test_code_digits: 2,
        },
    }
}

impl AuFramer {
    fn new(max_message_bytes: usize) -> Option<Self> {
        let decoder = StreamDecoder::new(au_framing_config()).ok()?;
        Some(Self {
            decoder,
            max_message_bytes,
            blocks: Vec::new(),
            in_fault: false,
        })
    }

    fn fault(&mut self, out: &mut Vec<Observation>) {
        self.blocks.clear();
        self.in_fault = true;
        out.push(Observation::FramingError);
    }

    fn frame(&mut self, frame: &packie_au::Frame, out: &mut Vec<Observation>) {
        self.in_fault = false;
        let text = frame.text.expose();
        if self.blocks.len().saturating_add(text.len()) > self.max_message_bytes {
            self.fault(out);
            return;
        }
        self.blocks.extend_from_slice(text);
        if frame.end == packie_au::FrameEnd::Text {
            out.push(Observation::Message(std::mem::take(&mut self.blocks)));
        }
    }
}

impl Framer for AuFramer {
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<Observation>) {
        for event in self.decoder.push(bytes) {
            match event {
                Ok(WireEvent::Frame(frame)) => self.frame(&frame, out),
                Ok(WireEvent::Ack | WireEvent::Nak) => {}
                Err(error) => self.error(error, out),
            }
        }
    }
}

impl AuFramer {
    /// Every stray byte is a separate decoder error; count the run once. Faults after an
    /// over-long or corrupt frame are part of that frame's fault.
    fn error(&mut self, error: AuError, out: &mut Vec<Observation>) {
        if error != AuError::Framing || !self.in_fault {
            self.fault(out);
        }
    }
}

#[cfg(test)]
mod tests;
