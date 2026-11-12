//! Pure HL7 v2 message parsing, editing, acknowledgement policy and MLLP framing.
//!
//! [`Message::parse`] accepts bytes, retains [`Message::original_bytes`], and exposes
//! a generic [`Segment`] → [`Field`] → [`Repetition`] → [`Component`] →
//! [`Subcomponent`] tree. Unchanged messages encode byte-identically, including LF
//! or CRLF input. Edited messages use CR segment terminators. Empty and trailing
//! fields, repetitions, components and subcomponents are retained.
//!
//! Delimiters come from MSH-1/MSH-2 (four distinct encoding characters; defaults
//! `|^~\\&`). Standard delimiter and hexadecimal escapes are decoded on access;
//! unknown/local escapes remain verbatim. Wire bytes need not be UTF-8: decoded
//! string getters return `None` when the addressed value is not UTF-8. Character
//! set negotiation/transcoding, formatting escapes and profile validation belong
//! to the caller. Literal setters escape delimiters and control bytes.
//!
//! Paths use one-based field/component/subcomponent numbers and segment occurrence
//! numbers in parentheses; repetition indices in brackets are zero-based:
//! `PID-3`, `PID-3[1].4`, `OBX(2)-5.1`, `MSH-9.2`, `PID-3[0].4.1`.
//! Omitting a repetition while selecting a component selects repetition zero.
//! Selecting an aggregate returns its decoded text with structural delimiters.
//! Setters replace the addressed value with a literal and create intermediate
//! structure. MSH-1/MSH-2 are immutable; choose delimiters with [`Message::new`].
//! Each path index is limited to 65535 to bound accidental sparse expansion.
//!
//! [`ack`] separates commit and application outcomes. The caller supplies the new
//! ACK id/timestamp and must commit storage before reporting commit success. It
//! owns correlation, delivery state, retries and timing. [`mllp`] is a bounded
//! streaming byte framer, independent of message parsing; it performs no I/O.
//!
//! This crate covers generic HL7 v2 encoding (tested with synthetic v2.3, v2.4,
//! and v2.5.1 messages), not a full typed segment library, HL7 batch/continuation
//! protocol, transport/session implementation, or conformance to any message
//! profile. A partner's contract and acceptance tests decide interoperability.
//!
//! ```
//! use packie_hl7::Message;
//! let mut message = Message::parse(
//!     b"MSH|^~\\&|DEVICE|LAB|APP|LAB|202601011200||ORU^R01|m1|T|2.5.1\rPID|||sample\r"
//! )?;
//! assert_eq!(message.get("MSH-9.2"), Some("R01"));
//! message.set("PID-3[1].4", "Lab & West")?;
//! assert_eq!(message.get("PID-3[1].4"), Some("Lab & West"));
//! # Ok::<(), packie_hl7::Error>(())
//! ```

pub mod ack;
mod encoding;
pub mod mllp;
mod model;
mod path;

pub use encoding::{EncodingCharacters, decode, escape};
pub use model::{Component, Field, Message, Repetition, Segment, Subcomponent};
pub use path::Path;

/// A structural encoding error. Message data is deliberately excluded from errors.
///
/// Each module keeps its own typed error: [`ack::AckError`] wraps this enum for
/// acknowledgement policy outcomes, and [`mllp::EncodeError`] describes framing
/// limits, which have nothing to do with message structure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The message does not start with `MSH`.
    #[error("message must start with MSH")]
    MissingMsh,
    /// MSH-1/MSH-2 do not hold valid, distinct delimiters.
    #[error("MSH-2 must contain four distinct printable ASCII punctuation characters")]
    InvalidEncodingCharacters,
    /// MSH-9, 10, 11 or 12 is missing or empty; carries the field number.
    #[error("MSH is missing required field {0}")]
    MissingHeaderField(usize),
    /// A segment header is malformed; carries the one-based segment number.
    #[error("invalid segment header at segment {0}")]
    InvalidSegment(usize),
    /// `MSH` appears after the first segment; carries the one-based segment number.
    #[error("unexpected MSH at segment {0}")]
    UnexpectedMsh(usize),
    /// A raw control byte other than CR or LF; carries its byte offset.
    #[error("unescaped control byte at offset {0}")]
    ControlCharacter(usize),
    /// An escape sequence is not closed.
    #[error("unterminated escape sequence")]
    TruncatedEscape,
    /// A hexadecimal escape is empty, odd-length or not hexadecimal.
    #[error("hexadecimal escape must contain complete hexadecimal byte pairs")]
    InvalidHexEscape,
    /// The path text is not a valid HL7 path.
    #[error("invalid HL7 path")]
    InvalidPath,
    /// A path index exceeds 65535.
    #[error("path index exceeds the supported limit of 65535")]
    PathIndexTooLarge,
    /// The path addresses MSH-1 or MSH-2.
    #[error("MSH-1 and MSH-2 cannot be edited")]
    ImmutableEncodingCharacters,
}
