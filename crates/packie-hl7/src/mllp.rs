//! Minimal Lower Layer Protocol framing, independent of HL7 message parsing.
//!
//! Frames are `VT payload FS CR`. Payload bytes are preserved exactly; `VT` and
//! `FS` are reserved for framing. A nested `VT` abandons the current frame and
//! starts another. An invalid trailer abandons its frame. Oversized frames are
//! discarded through their trailer or the next `VT`, whichever arrives first.
//!
//! Some senders end a frame with `FS` alone. [`Decoder::with_optional_end_cr`] accepts
//! that: the frame is delivered at `FS`, and a `CR` right after it is swallowed.

use std::collections::TryReserveError;

use thiserror::Error;

/// Start-of-block byte (`VT`).
pub const START_BLOCK: u8 = 0x0b;
/// End-of-block byte (`FS`).
pub const END_BLOCK: u8 = 0x1c;
/// Final byte of the frame trailer (`CR`).
pub const CARRIAGE_RETURN: u8 = 0x0d;

/// The decoder's current position in the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Searching for a start byte; other bytes are reported as garbage.
    SeekingStart,
    /// Collecting the payload after a start byte.
    InFrame,
    /// An end byte was received; the next byte must be a carriage return.
    AwaitingCarriageReturn,
    /// Discarding a frame already reported as oversized or unbufferable.
    Discarding,
    /// Looking for a carriage return to finish discarding a rejected frame.
    DiscardingAwaitingCarriageReturn,
    /// Optional-trailer decoders only: a frame ended at its end byte; a following
    /// carriage return belongs to it.
    AfterEndBlock,
}

/// Why a frame was abandoned before its trailer completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MalformedReason {
    /// A new start byte arrived before the previous frame completed.
    UnexpectedStartBlock,
    /// An end byte was followed by something other than a carriage return.
    MissingCarriageReturn,
}

/// An observation from the stream. Errors carry counts, never payload bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A complete frame, with its framing bytes removed.
    Message(Vec<u8>),
    /// Bytes outside a frame were discarded. Counts are grouped per feed call.
    Garbage {
        /// Number of discarded bytes.
        bytes: usize,
    },
    /// An incomplete frame was abandoned; a new start byte can recover it.
    Malformed {
        /// Why the frame was abandoned.
        reason: MalformedReason,
        /// Payload bytes buffered, and now dropped, when the frame was abandoned.
        buffered_bytes: usize,
    },
    /// A payload exceeded the configured limit; emitted once per rejected frame.
    Oversize {
        /// The configured payload limit.
        limit: usize,
    },
    /// Buffer allocation failed; the current frame is discarded.
    AllocationFailed,
    /// End of input arrived while a frame (possibly already rejected) was open.
    Truncated {
        /// The state in which the input ended.
        state: State,
        /// Payload bytes buffered, and now dropped, at the end of input.
        buffered_bytes: usize,
    },
}

/// Incremental decoder with at most `max_payload_len` retained payload bytes.
///
/// Input is visited in place, including oversized chunks. Events are delivered
/// immediately to a callback, so the decoder never queues completed messages.
/// Callers that retain events must impose their own queue bound. A zero limit
/// accepts empty frames only. Construction does not allocate a payload buffer.
pub struct Decoder {
    state: State,
    payload: Vec<u8>,
    max_payload_len: usize,
    end_cr_optional: bool,
}

impl Decoder {
    /// Construct a decoder with an explicit payload limit, excluding framing.
    #[must_use]
    pub const fn new(max_payload_len: usize) -> Self {
        Self {
            state: State::SeekingStart,
            payload: Vec::new(),
            max_payload_len,
            end_cr_optional: false,
        }
    }

    /// The same decoder accepting a frame that ends at `FS` without the trailing `CR`.
    /// A `CR` that does follow is swallowed; any other byte after `FS` is outside a frame.
    #[must_use]
    pub const fn with_optional_end_cr(mut self) -> Self {
        self.end_cr_optional = true;
        self
    }

    /// Current framing state, including whether a rejected frame is open.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Number of retained payload bytes; never greater than the configured limit.
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.payload.len()
    }

    /// Configured payload limit, excluding framing bytes.
    #[must_use]
    pub const fn max_payload_len(&self) -> usize {
        self.max_payload_len
    }

    /// Consume any chunk, delivering messages and framing diagnostics in order.
    ///
    /// Garbage is reported before a following frame's events and at the end of
    /// this call. Its grouping may therefore change with input chunk boundaries.
    pub fn feed(&mut self, input: &[u8], mut emit: impl FnMut(Event)) {
        let mut garbage = 0;
        for &byte in input {
            if byte == START_BLOCK {
                emit_garbage(&mut garbage, &mut emit);
                self.start_frame(&mut emit);
            } else if self.receive_byte(byte, &mut emit) {
                // This count cannot exceed the length of the input slice.
                garbage += 1;
            }
        }
        emit_garbage(&mut garbage, &mut emit);
    }

    /// Finish a stream, report an unfinished frame, and reset for a new stream.
    ///
    /// This also reports truncation of an oversized frame whose end was never
    /// observed. Repeated calls without new input return `None`.
    pub fn finish(&mut self) -> Option<Event> {
        let open = !matches!(self.state, State::SeekingStart | State::AfterEndBlock);
        let event = open.then_some(Event::Truncated {
            state: self.state,
            buffered_bytes: self.payload.len(),
        });
        self.payload.clear();
        self.state = State::SeekingStart;
        event
    }

    fn start_frame(&mut self, emit: &mut impl FnMut(Event)) {
        if matches!(self.state, State::InFrame | State::AwaitingCarriageReturn) {
            emit(Event::Malformed {
                reason: MalformedReason::UnexpectedStartBlock,
                buffered_bytes: self.payload.len(),
            });
        }
        self.payload.clear();
        self.state = State::InFrame;
    }

    // Return whether this non-start byte is garbage outside a frame.
    fn receive_byte(&mut self, byte: u8, emit: &mut impl FnMut(Event)) -> bool {
        match self.state {
            State::SeekingStart => return true,
            State::AfterEndBlock => {
                self.state = State::SeekingStart;
                return byte != CARRIAGE_RETURN;
            }
            State::InFrame => self.receive_payload_byte(byte, emit),
            State::AwaitingCarriageReturn => return self.receive_trailer_byte(byte, emit),
            State::Discarding => {
                if byte == END_BLOCK {
                    self.state = self.state_after_rejected_end();
                }
            }
            State::DiscardingAwaitingCarriageReturn => {
                self.state = match byte {
                    CARRIAGE_RETURN => State::SeekingStart,
                    END_BLOCK => State::DiscardingAwaitingCarriageReturn,
                    _ => State::Discarding,
                };
            }
        }
        false
    }

    /// Where a rejected frame goes at its end byte.
    const fn state_after_rejected_end(&self) -> State {
        if self.end_cr_optional {
            State::AfterEndBlock
        } else {
            State::DiscardingAwaitingCarriageReturn
        }
    }

    fn receive_payload_byte(&mut self, byte: u8, emit: &mut impl FnMut(Event)) {
        if byte == END_BLOCK {
            if self.end_cr_optional {
                self.state = State::AfterEndBlock;
                emit(Event::Message(std::mem::take(&mut self.payload)));
            } else {
                self.state = State::AwaitingCarriageReturn;
            }
            return;
        }
        let rejection = if self.payload.len() == self.max_payload_len {
            Some(Event::Oversize {
                limit: self.max_payload_len,
            })
        } else if self.reserve_byte().is_err() {
            Some(Event::AllocationFailed)
        } else {
            None
        };
        if let Some(event) = rejection {
            self.payload.clear();
            self.state = State::Discarding;
            emit(event);
        } else {
            self.payload.push(byte);
        }
    }

    fn receive_trailer_byte(&mut self, byte: u8, emit: &mut impl FnMut(Event)) -> bool {
        self.state = State::SeekingStart;
        if byte == CARRIAGE_RETURN {
            emit(Event::Message(std::mem::take(&mut self.payload)));
            false
        } else {
            emit(Event::Malformed {
                reason: MalformedReason::MissingCarriageReturn,
                buffered_bytes: self.payload.len(),
            });
            self.payload.clear();
            true
        }
    }

    fn reserve_byte(&mut self) -> Result<(), TryReserveError> {
        if self.payload.len() == self.payload.capacity() {
            // Cap growth at the configured limit rather than Vec's uncapped
            // geometric growth. Never reserve based on the input chunk's size.
            let target = self
                .payload
                .capacity()
                .saturating_mul(2)
                .max(64)
                .min(self.max_payload_len);
            self.payload
                .try_reserve_exact(target - self.payload.len())?;
        }
        Ok(())
    }
}

fn emit_garbage(garbage: &mut usize, emit: &mut impl FnMut(Event)) {
    if *garbage != 0 {
        emit(Event::Garbage { bytes: *garbage });
        *garbage = 0;
    }
}

/// A payload cannot be represented as one bounded MLLP frame.
#[derive(Debug, Error)]
pub enum EncodeError {
    /// The payload is larger than the configured maximum.
    #[error("MLLP payload has {length} bytes, exceeding limit {limit}")]
    Oversize {
        /// Payload length in bytes.
        length: usize,
        /// The configured payload limit.
        limit: usize,
    },
    /// Framing bytes cannot occur inside an MLLP payload.
    #[error("reserved MLLP byte {byte:#04x} at payload offset {offset}")]
    ReservedByte {
        /// The reserved byte found.
        byte: u8,
        /// Its offset in the payload.
        offset: usize,
    },
    /// The framed size cannot be represented on this platform.
    #[error("MLLP frame size overflow")]
    SizeOverflow,
    /// Space for the bounded frame could not be allocated.
    #[error("could not allocate MLLP frame")]
    AllocationFailed(#[source] TryReserveError),
}

/// Encode a payload as `VT payload FS CR`, enforcing the same bound as a decoder.
///
/// Empty payloads are permitted; HL7 message validation is a separate layer.
/// Reserved `VT` and `FS` payload bytes are rejected rather than producing an
/// ambiguous stream. Carriage returns inside the payload are preserved.
///
/// # Errors
///
/// [`EncodeError`] when the payload exceeds `max_payload_len`, contains `VT` or
/// `FS`, or the frame cannot be allocated.
pub fn encode(payload: &[u8], max_payload_len: usize) -> Result<Vec<u8>, EncodeError> {
    if payload.len() > max_payload_len {
        return Err(EncodeError::Oversize {
            length: payload.len(),
            limit: max_payload_len,
        });
    }
    if let Some((offset, &byte)) = payload
        .iter()
        .enumerate()
        .find(|(_, byte)| matches!(**byte, START_BLOCK | END_BLOCK))
    {
        return Err(EncodeError::ReservedByte { byte, offset });
    }
    let capacity = payload
        .len()
        .checked_add(3)
        .ok_or(EncodeError::SizeOverflow)?;
    let mut frame = Vec::new();
    frame
        .try_reserve_exact(capacity)
        .map_err(EncodeError::AllocationFailed)?;
    frame.push(START_BLOCK);
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&[END_BLOCK, CARRIAGE_RETURN]);
    Ok(frame)
}
