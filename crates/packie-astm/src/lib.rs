//! Pure ASTM E1381 / CLSI LIS01-A2 framing, ASTM E1394 / LIS02-A2 records, and
//! session state machines for receiving and sending messages over a byte link.
//!
//! # What it covers
//!
//! - **Frames** ([`encode_frame`], [`encode_records`], [`encode_records_within`],
//!   [`parse_frame`], [`parse_frame_within`]): STX, frame number 1 to 7 then 0, text up
//!   to 240 bytes by default or up to [`MAX_LONG_FRAME_TEXT`] (64,000) bytes when
//!   configured, ETB for intermediate and ETX for final frames, uppercase hexadecimal
//!   checksum, CR LF. Seven-bit printable ASCII plus the record-separating carriage
//!   return.
//! - **Records** ([`parse_message_records`], [`parse_record`], [`assemble_message`]):
//!   H, P, O, R, C, Q, M and L records with the header's delimiters, repeat and
//!   component structure, and escape decoding (`F`, `S`, `R`, `E` and even-length hex
//!   escapes). Each [`Record`] keeps its exact raw text ([`Record::raw`],
//!   [`Record::raw_bytes`]) next to the decoded fields. Typed read-only views exist
//!   for H, P, O, R, C and Q ([`Record::as_result`], [`Record::as_query`], ...). Values
//!   stay text: result quantities, units, comparators and timestamps are never
//!   converted. [`assemble_message`] validates a result or query message (H, patients
//!   with orders with results, or queries, then L) and checks sequence numbers.
//! - **Building** ([`MessageBuilder`]): order downloads (H, P, O, L) and queries
//!   (H, Q, L) as record byte strings, with every value escaped and sequence numbers
//!   assigned.
//! - **Receiving** ([`ReceiveDriver`]): ENQ, ACK/NAK, retransmission detection,
//!   six-attempt NAK policy, 30 s timeout, size bounds, a configurable frame text limit
//!   ([`ReceiveDriver::with_max_frame_text`]), and an ACK that is released only after
//!   the caller confirms the frame is durably stored.
//! - **Sending and bidirectional lines** ([`LinkDriver`]): establishment with ENQ,
//!   reply timeouts, NAK busy retry, contention with a configurable [`LineRole`],
//!   frame transmission with frame-number sequencing, at most six retransmissions per
//!   frame, EOT termination, receiver interrupt (EOT in place of ACK), and inbound
//!   transfers on the same line.
//!
//! # What it does not cover
//!
//! Sockets, serial ports, clocks, threads, persistence, semantic acceptance of
//! results, order lookup, and vendor dialects. Other character sets are not
//! supported. The crate has no dependency on other engine crates.
//!
//! # How a session loop uses the drivers
//!
//! A driver is a state machine. The loop feeds it [`Event`]s and carries out the
//! returned [`Action`]s in order:
//!
//! | Event | When |
//! | --- | --- |
//! | [`Event::Bytes`] | Any bytes read from the transport; chunk boundaries do not matter. |
//! | [`Event::Persisted`] | The frame of an earlier [`Action::PersistFrame`] is durably stored. |
//! | [`Event::TimerExpired`] | The timer of an earlier [`Action::StartTimer`] fired. |
//!
//! | Action | What the loop does |
//! | --- | --- |
//! | [`Action::Write`] | Write the bytes to the transport (ENQ, ACK, NAK, EOT or a frame). |
//! | [`Action::PersistFrame`] | Store the frame durably, then feed [`Event::Persisted`]. The ACK is written only after that. |
//! | [`Action::MessageReceived`] | A complete inbound message, as unterminated records. |
//! | [`Action::ReceiveAborted`] | The inbound transfer failed; nothing partial is exposed. |
//! | [`Action::MessageSent`] | The outbound message was acknowledged and EOT written. |
//! | [`Action::SendAborted`] | The outbound message was abandoned; retry it from the start. |
//! | [`Action::StartTimer`] | Arm the single timer slot, replacing any running timer. |
//! | [`Action::StopTimer`] | Cancel the timer slot. |
//!
//! Drivers never read a clock. Expiries from replaced timers are ignored, so a late
//! [`Event::TimerExpired`] is harmless. [`LinkDriver::submit`] starts one outbound
//! message and returns actions; submit the next message after `MessageSent` or
//! `SendAborted`. Whether a failed delivery is retried, and how often, is the
//! caller's policy. After [`SendAbortReason::ReceiverInterrupted`] the receiver asked
//! to transmit: wait before retrying.
//!
//! ```
//! use packie_astm::{ACK, Action, ENQ, Event, LinkDriver, LinkOptions, MessageBuilder, QueryFields};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let records = MessageBuilder::new()
//!     .sender("PACKIE", "1.0")
//!     .query(QueryFields::for_specimen("SYN-0001"))
//!     .build()?;
//! let mut link = LinkDriver::new(LinkOptions::default());
//!
//! // submit: ENQ goes out and a reply timer starts.
//! let actions = link.submit(&records)?;
//! assert_eq!(actions[0], Action::Write(vec![ENQ]));
//!
//! // The peer answers ACK: the first frame is written.
//! let actions = link.handle(Event::Bytes(vec![ACK]))?;
//! assert!(matches!(&actions[0], Action::Write(frame) if frame[0] == packie_astm::STX));
//! # Ok(())
//! # }
//! ```
//!
//! A receive-only connector uses [`ReceiveDriver`] with the same events and actions.
//! The wire encoding is lossless: [`Action::MessageReceived`] records are the exact
//! bytes sent, and [`Action::PersistFrame`] carries the exact frame bytes.
//!
//! # Timing and limits
//!
//! Defaults follow LIS01-A2 and are set in [`LinkOptions`]: 15 s reply timeout, 10 s
//! wait after a NAK to ENQ, 1 s (instrument) or 20 s (computer) contention wait,
//! three ENQ attempts per message ([`LinkOptions::max_enq_attempts`], each followed by
//! a [`LinkOptions::busy_retry_delay`] wait after a NAK or silence), six
//! retransmissions per frame. The receiver waits 30 s between frames
//! ([`RECEIVE_TIMEOUT`]) and bounds each transfer by [`DEFAULT_MAX_FRAMES_PER_TRANSFER`]
//! frames, the message text limit and that limit plus 50 KiB of exact frame bytes
//! ([`DEFAULT_MAX_WIRE_BYTES_PER_TRANSFER`] at the default).
//!
//! Frame text is limited to 240 bytes unless [`LinkOptions::max_frame_text`] (or
//! [`ReceiveDriver::with_max_frame_text`], [`parse_frame_within`],
//! [`encode_records_within`]) selects a larger size, up to [`MAX_LONG_FRAME_TEXT`], for
//! ASTM over TCP peers that document long frames. One setting governs both
//! directions of a [`LinkDriver`]: records are split over ETB frames at that size,
//! and a received frame with more text than that is NAKed like any malformed frame
//! (six NAKs end the transfer). The peer must use the same size; a sender with a larger
//! limit than its receiver is refused. Buffered input is bounded by the message limit,
//! but always allows one maximum-size frame (text plus seven framing bytes), and the
//! transfer's frame-byte bound likewise never drops below one such frame.
//! [`encode_frame`] itself accepts any text up to [`MAX_LONG_FRAME_TEXT`]; the split
//! size is chosen by the caller of [`encode_records_within`].
//!
//! # Robustness
//!
//! No input makes any function in this crate panic. Frame, record and message
//! errors are typed and never include payload text.

#![warn(clippy::wildcard_enum_match_arm)]

mod accessors;
mod ascii;
mod builder;
mod contract;
mod escape;
mod frame;
mod link;
mod message;
mod receive;
mod record;
mod record_model;
mod send;
#[cfg(test)]
mod testing;
mod timer;

pub use accessors::{
    CommentRecord, HeaderRecord, OrderRecord, PatientRecord, QueryRecord, ResultRecord,
};
pub use builder::{BuildError, MessageBuilder, OrderFields, PatientFields, QueryFields};
pub use contract::{
    Action, DEFAULT_MAX_FRAMES_PER_TRANSFER, DEFAULT_MAX_MESSAGE_BYTES,
    DEFAULT_MAX_WIRE_BYTES_PER_TRANSFER, DriverError, Event, FrameId, LineRole, LinkOptions,
    RECEIVE_TIMEOUT, ReceiveAbortReason, SendAbortReason, TimerId,
};
pub use frame::{
    ACK, CR, ENQ, EOT, ETB, ETX, Frame, FrameEnd, FrameError, LF, MAX_LONG_FRAME_TEXT, NAK, STX,
    checksum, encode_frame, encode_records, encode_records_within, parse_frame, parse_frame_within,
};
pub use link::LinkDriver;
pub use message::{Message, MessageError, OrderNode, PatientNode, ResultNode, assemble_message};
pub use receive::ReceiveDriver;
pub use record::{parse_message_records, parse_record};
pub use record_model::{Component, Delimiters, Field, Record, RecordError, RecordType, Repeat};
