use crate::{AuConfig, AuError, Redacted};

/// Standalone positive acknowledgement byte.
pub(crate) const ACK: u8 = 0x06;
/// Standalone negative acknowledgement byte.
pub(crate) const NAK: u8 = 0x15;
/// End of transmission block, the terminator of intermediate blocks.
const ETB: u8 = 0x17;

/// Application text is printable ASCII, 0x20-0x7E.
fn is_text_byte(byte: &u8) -> bool {
    (0x20..=0x7e).contains(byte)
}

/// Application text must be printable ASCII and fit the wire limit with the frame's controls.
/// An ETB terminator is as long as the sole ETX it replaces, so the overhead is exact.
pub(crate) fn check_text(config: &AuConfig, text: &[u8]) -> Result<(), AuError> {
    if text.len() + config.overhead() > usize::from(config.framing.max_wire_len) {
        return Err(AuError::FrameTooLong);
    }
    if !text.iter().all(is_text_byte) {
        return Err(AuError::Character);
    }
    Ok(())
}

/// XOR block check character over `bytes`.
fn bcc(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |sum, byte| sum ^ byte)
}

/// Text-ending versus intermediate-block terminator (manual p. 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameEnd {
    /// The configured end code.
    Text,
    /// ETB, for intermediate blocks when `use_etb` is set.
    Block,
}

/// Exact wire and application text; debug formatting always redacts both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Every received byte, delimiters and BCC included.
    pub wire: Redacted<Vec<u8>>,
    /// The application text between start and end codes.
    pub text: Redacted<Vec<u8>>,
    /// Which terminator ended the frame.
    pub end: FrameEnd,
}

/// Transport event only; decoding a frame says nothing about its meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireEvent {
    /// A complete, checksum-verified frame.
    Frame(Frame),
    /// A standalone ACK byte.
    Ack,
    /// A standalone NAK byte.
    Nak,
}

/// Incremental decoder retaining at most the configured wire limit.
#[derive(Debug)]
pub struct StreamDecoder {
    config: AuConfig,
    pending: Redacted<Vec<u8>>,
}

/// Where a complete frame's terminator sits in the pending wire bytes.
struct Terminator {
    offset: usize,
    end: FrameEnd,
    len: usize,
}

impl StreamDecoder {
    /// Validates the configuration and creates an idle decoder.
    pub fn new(config: AuConfig) -> Result<Self, AuError> {
        config.validate()?;
        Ok(Self {
            config,
            pending: Redacted::new(Vec::new()),
        })
    }
    /// Feeds arbitrary chunk boundaries, returning independent events.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<WireEvent, AuError>> {
        let mut events = Vec::new();
        for byte in bytes {
            match self.feed(*byte) {
                Ok(Some(event)) => events.push(Ok(event)),
                Ok(None) => {}
                Err(error) => {
                    // Preserve a potential new start after malformed partial framing.
                    self.resync(*byte);
                    events.push(Err(error));
                }
            }
        }
        events
    }
    /// Reports incomplete wire data at end of stream; clears it for a fresh connection.
    pub fn finish(&mut self) -> Result<(), AuError> {
        if self.is_pending() {
            self.reset();
            Err(AuError::Framing)
        } else {
            Ok(())
        }
    }
    /// True while an incomplete frame awaits more bytes.
    pub fn is_pending(&self) -> bool {
        self.buffered_len() > 0
    }
    /// Discards any incomplete wire data.
    pub fn reset(&mut self) {
        self.pending.expose_mut().clear();
    }
    /// Amount of retained wire data, excluding the returned event queue.
    pub fn buffered_len(&self) -> usize {
        self.pending.expose().len()
    }

    /// Starts a new frame at `byte` when it can begin one; the buffer is empty after an error.
    fn resync(&mut self, byte: u8) {
        if byte == self.config.framing.start[0] {
            self.pending.expose_mut().push(byte);
        }
    }
    /// Consumes one byte; any error leaves the buffer empty.
    fn feed(&mut self, byte: u8) -> Result<Option<WireEvent>, AuError> {
        if !self.is_pending() {
            match byte {
                ACK => return Ok(Some(WireEvent::Ack)),
                NAK => return Ok(Some(WireEvent::Nak)),
                _ if byte != self.config.framing.start[0] => return Err(AuError::Framing),
                _ => {}
            }
        }
        self.pending.expose_mut().push(byte);
        match self.terminator() {
            Ok(None) => Ok(None),
            Ok(Some(terminator)) => {
                let wire = std::mem::take(self.pending.expose_mut());
                self.complete(wire, &terminator).map(Some)
            }
            Err(error) => {
                self.reset();
                Err(error)
            }
        }
    }

    /// Checks the pending bytes and locates a terminator that completes the frame.
    fn terminator(&self) -> Result<Option<Terminator>, AuError> {
        let pending = self.pending.expose();
        let framing = &self.config.framing;
        if pending.len() > usize::from(framing.max_wire_len) {
            return Err(AuError::FrameTooLong);
        }
        let start = framing.start.len();
        if pending.len() <= start {
            return if framing.start.starts_with(pending) {
                Ok(None)
            } else {
                Err(AuError::Framing)
            };
        }
        let Some(offset) = pending[start..].iter().position(|b| !is_text_byte(b)) else {
            return Ok(None);
        };
        let offset = start + offset;
        let rest = &pending[offset..];
        let (end, code) = if self.config.etb_enabled() && rest[0] == ETB {
            (FrameEnd::Block, &[ETB][..])
        } else {
            (FrameEnd::Text, framing.end.as_slice())
        };
        let compare_len = rest.len().min(code.len());
        if rest[..compare_len] != code[..compare_len] {
            return Err(AuError::Framing);
        }
        let terminator = Terminator {
            offset,
            end,
            len: code.len(),
        };
        let complete = pending.len() == offset + code.len() + usize::from(framing.bcc);
        Ok(complete.then_some(terminator))
    }

    fn complete(&self, wire: Vec<u8>, terminator: &Terminator) -> Result<WireEvent, AuError> {
        let start = self.config.framing.start.len();
        let covered_end = terminator.offset + terminator.len;
        if self.config.framing.bcc && wire[covered_end] != bcc(&wire[start..covered_end]) {
            return Err(AuError::Checksum);
        }
        let text = Redacted::new(wire[start..terminator.offset].to_vec());
        Ok(WireEvent::Frame(Frame {
            wire: Redacted::new(wire),
            text,
            end: terminator.end,
        }))
    }
}

/// Encodes printable ASCII text and XOR BCC covering text plus terminator (p. 5).
pub fn encode_frame(config: &AuConfig, text: &[u8], end: FrameEnd) -> Result<Vec<u8>, AuError> {
    config.validate()?;
    check_text(config, text)?;
    let terminator = match end {
        FrameEnd::Block if config.etb_enabled() => &[ETB],
        FrameEnd::Text | FrameEnd::Block => config.framing.end.as_slice(),
    };
    let mut wire = config.framing.start.clone();
    wire.extend_from_slice(text);
    wire.extend_from_slice(terminator);
    if config.framing.bcc {
        wire.push(bcc(&wire[config.framing.start.len()..]));
    }
    Ok(wire)
}
