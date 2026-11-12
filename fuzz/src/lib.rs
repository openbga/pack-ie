//! Shared helpers for the fuzz targets.
//!
//! The driver targets decode their input with [`Reader`], a plain front-to-back byte reader,
//! rather than a structure-aware decoder, so a seed is readable as bytes (an opcode, its
//! operands, the next opcode) and a mutation changes one step instead of reshuffling the rest.

/// Reads operands from the front of the fuzz input. Every method returns `None` once the
/// input is exhausted; targets stop decoding there.
pub struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn u8(&mut self) -> Option<u8> {
        let (&byte, rest) = self.data.split_first()?;
        self.data = rest;
        Some(byte)
    }

    pub fn bool(&mut self) -> Option<bool> {
        self.u8().map(|byte| byte & 1 != 0)
    }

    /// A value in `low..=high`, reduced from one byte (so `high - low` must be below 256).
    pub fn range(&mut self, low: u8, high: u8) -> Option<u8> {
        let span = u16::from(high - low) + 1;
        self.u8().map(|byte| low + (u16::from(byte) % span) as u8)
    }

    /// Up to `len` bytes, fewer when the input is shorter.
    pub fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.data.is_empty() {
            return None;
        }
        let (taken, rest) = self.data.split_at(len.min(self.data.len()));
        self.data = rest;
        Some(taken)
    }

    pub fn choose<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        self.u8().map(|byte| items[usize::from(byte) % items.len()])
    }
}

/// Maps arbitrary bytes onto the printable-ASCII-plus-CR alphabet the ASTM record layer
/// accepts, so a short random buffer becomes a plausible record text instead of being
/// rejected at the first byte. Zero becomes the record terminator.
#[must_use]
pub fn astm_text(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|byte| match byte {
            0 => b'\r',
            other => 0x20 + other % 95,
        })
        .collect()
}

/// One step of an ASTM driver session, decoded from the fuzz input.
#[derive(Debug)]
pub enum AstmStep {
    /// Arbitrary bytes from the peer.
    Bytes(Vec<u8>),
    /// A single control byte (ENQ, ACK, NAK or EOT).
    Control(u8),
    /// A well-formed frame (valid checksum) around plausible record text.
    Frame(Vec<u8>),
    /// A whole transfer: ENQ, the frames of some records, EOT.
    Transfer(Vec<Vec<u8>>),
    /// Confirm the nth frame the driver asked to persist (any, in any order, repeatedly).
    Persisted(usize),
    /// Fire the nth timer the driver started (stale ones included).
    Timer(usize),
    /// Confirm every outstanding persistence request, oldest first.
    Drain,
    /// Queue records for sending (link driver only).
    Submit(Vec<Vec<u8>>),
}

fn records(input: &mut Reader<'_>) -> Option<Vec<Vec<u8>>> {
    let count = input.range(0, 4)?;
    let mut records = Vec::new();
    for _ in 0..count {
        let len = usize::from(input.u8()?) + usize::from(input.u8()?) % 2 * 256;
        let text = astm_text(input.take(len)?);
        records.push(text.into_iter().filter(|byte| *byte != b'\r').collect());
    }
    Some(records)
}

fn step(input: &mut Reader<'_>) -> Option<Option<AstmStep>> {
    use packie_astm::{ACK, ENQ, EOT, FrameEnd, NAK, encode_frame};

    Some(match input.range(0, 9)? {
        0 | 1 => {
            let len = usize::from(input.range(0, 64)?);
            Some(AstmStep::Bytes(input.take(len)?.to_vec()))
        }
        2 => Some(AstmStep::Control(
            input.choose(&[ENQ, ACK, NAK, EOT, ENQ, ACK])?,
        )),
        3 | 4 => {
            let number = input.range(0, 7)?;
            let len = usize::from(input.u8()?) % 251;
            let text = astm_text(input.take(len)?);
            let end = if input.bool()? {
                FrameEnd::Final
            } else {
                FrameEnd::Intermediate
            };
            encode_frame(number, &text, end).ok().map(AstmStep::Frame)
        }
        5 => Some(AstmStep::Transfer(records(input)?)),
        6 => Some(AstmStep::Persisted(input.u8()?.into())),
        7 => Some(AstmStep::Timer(input.u8()?.into())),
        8 => Some(AstmStep::Drain),
        _ => Some(AstmStep::Submit(records(input)?)),
    })
}

/// Decodes the whole input into steps; decoding stops quietly when the input runs out.
#[must_use]
pub fn astm_steps(data: &[u8]) -> Vec<AstmStep> {
    let mut input = Reader::new(data);
    let mut steps = Vec::new();
    while steps.len() < 256 {
        match step(&mut input) {
            Some(Some(step)) => steps.push(step),
            Some(None) => {}
            None => break,
        }
    }
    steps
}
