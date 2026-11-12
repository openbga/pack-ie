//! Pure Olympus AU400/AU640 online-protocol codec, result assembler and receive driver.
//!
//! The protocol is proprietary fixed-width text, not ASTM. This crate follows the *AU400 &
//! AU640 Online Specifications*, Olympus Optical Co., Ltd., combined issue 1999-02-03; page
//! numbers (pp.) cite the printed pages of that document. The tests use original synthetic
//! fixtures, not text copied from the manual.
//!
//! # What it covers
//!
//! * **Codec**: [`StreamDecoder`] (incremental, bounded framing with optional XOR BCC and
//!   standalone ACK/NAK events), [`encode_frame`], [`parse_message`] into the typed [`Message`]
//!   model, and requisition encoding ([`encode_tri`], [`encode_stop`]). [`AuConfig`] mirrors the
//!   instrument's online parameter sheet and has no defaults.
//! * **Assembler**: [`ResultAssembler`] joins blocks `0`-`9` and the final `E` into one
//!   [`SampleResults`], checking the repeated headers, the block sequence and retransmissions.
//! * **Driver**: [`ReceiveDriver`] composes the above into a transport-driven receive session:
//!   bytes in, [`Action`]s out (see [`driver`] module docs for the ordering guarantees).
//!
//! # What it does not cover
//!
//! I/O, persistence, scheduling and clocks (the driver asks the caller to write bytes, store
//! frames and arm one timer), clinical interpretation (units, flag meanings, all-9 or LIH
//! values keep their exact bytes), laboratory test mapping, test-order logic (only an explicit
//! caller-provided requisition is encoded; nothing is inferred from queries), and restart or
//! deduplication across connections. An identical final `E` frame cannot be told apart from a
//! legitimate repeated sample without session context, so every completed occurrence is
//! reported; reconciliation is the caller's job.
//!
//! Every raw field, test code and wire frame is wrapped in [`Redacted`], so `Debug` output of
//! decoded structures never contains message content. Errors contain no received content.
//! Fixed-width values keep spaces and leading zeroes; there are no numeric conversions.
//!
//! `FormatConfig::test_code_digits` selects 2-digit or 3-digit test codes. A result item is the
//! code, the raw value (`result_digits`) and two flag bytes; a requisition test is the code, plus
//! one method byte for reruns. A code must be all digits and not all zeros, and the width applies
//! to every test code in both directions; data written for the other width fails the length or
//! field checks.
//!
//! # Evidence
//!
//! | Implemented behavior | Manual evidence |
//! | --- | --- |
//! | Configurable start/end codes, optional XOR BCC, class A/B | pp. 4-5 |
//! | Wire limits 256/512/1024; block IDs 0-9 and E, conditional ETB | pp. 6-7, 36 |
//! | RB, R-space, RH, RE query messages | p. 8 |
//! | S-space and SH explicit TRI; SE stops the session; no Unit No in S messages | pp. 9-10, 24 |
//! | D-space, DH, d-space, dH patient result messages | pp. 10-12 |
//! | DR, DA, DQ kept as distinct nonpatient classifications | pp. 12-13 |
//! | Rack width 0, 4 or 5 with a two-byte cup only when rack is present; sample ID 1-20; six demographic fields of width 0-20; value width 6 or 9; two flag bytes | pp. 14, 36 |
//! | Test codes of 2 or 3 digits (`test_code_digits`), in result items and requisition test lists | online parameter sheet setting |
//! | Unknown/all-9/LIH values and every flag retain exact bytes | pp. 14-17, 31-32 |
//! | Class B response no earlier than 500 ms after the frame; T1/T7 inter-block timers | pp. 20-22, 26 |
//!
//! # Unsupported or uncertain profiles
//!
//! | Profile | Behavior and reason |
//! | --- | --- |
//! | Delimiters containing ACK (06) or NAK (15) | Rejected to keep idle acknowledgement events unambiguous. Other one/two-byte control sequences, including matching start/end prefixes, are supported. |
//! | Extended text bytes | Rejected. p. 4 says A1-DF but p. 34 gives A1-DC; only printable ASCII 20-7E is accepted. |
//! | Sample ID width 0 | Rejected. p. 14 specifies 1-20 while p. 36 allows 0; sample identity is unavailable at 0. |
//! | Nonpatient result with rack width 0 | Rejected. pp. 12-13 describe dummy rack fields when rack is absent without unambiguously specifying their width relative to p. 14's omitted-rack format. |
//! | More than ten intermediate blocks | Rejected. p. 7 defines 0-9 and E with no wrap rule. |
//! | Calibration first/second-data annotation | Not interpreted. The field association is unclear on p. 12; exact values and flags remain available. |
//!
//! A final `E` is valid without any preceding numeric block. Numeric blocks carry no total, so
//! `E` cannot reveal a lost final intermediate block; class B acknowledgements and transfer
//! qualification remain necessary. Requisition encoding needs every fixed-width header and
//! demographic field explicitly and at least one test; whole test items are split across
//! frames, each repeating the header, and the last frame uses `E`. Empty requisitions are
//! refused because the manual does not describe an empty per-sample reply.
//!
//! # How a session loop uses the driver
//!
//! The loop owns the transport, a store and one timer slot. It feeds every read, storage
//! confirmation and timer expiry to [`ReceiveDriver::handle`] and carries out each action.
//!
//! ```
//! use packie_au::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = AuConfig {
//!     framing: FramingConfig {
//!         start: vec![2],
//!         end: vec![3],
//!         bcc: true,
//!         use_etb: true,
//!         max_wire_len: 256,
//!         class: TransmissionClass::B,
//!     },
//!     format: FormatConfig {
//!         unit_no: true,
//!         rack_digits: 4,
//!         sample_id_digits: 8,
//!         sex: false,
//!         age: false,
//!         month: false,
//!         patient_info_digits: [0; 6],
//!         result_digits: 6,
//!         test_code_digits: 2,
//!     },
//! };
//! let mut driver = ReceiveDriver::new(config.clone(), ReceiveOptions::default())?;
//!
//! // Bytes read from the transport: a "results start" session message.
//! let wire = encode_frame(&config, b"DB01", FrameEnd::Text)?;
//! let actions = driver.handle(ReceiveEvent::Bytes(wire))?;
//! let [Action::PersistFrame { frame_id, .. }, Action::StartTimer { timer_id, duration }] =
//!     actions.as_slice()
//! else {
//!     panic!("expected a store request and a turnaround timer");
//! };
//! assert_eq!(*duration, ACK_TURNAROUND);
//!
//! // The ACK is released only after the frame is stored and the turnaround has elapsed.
//! assert!(driver.handle(ReceiveEvent::Persisted(*frame_id))?.is_empty());
//! let actions = driver.handle(ReceiveEvent::TimerExpired(*timer_id))?;
//! assert!(matches!(actions[0], Action::Send(ControlResponse::Ack)));
//! assert!(matches!(actions[1], Action::Control { .. }));
//! # Ok(())
//! # }
//! ```
#![warn(clippy::wildcard_enum_match_arm)]
#![warn(missing_docs)]

mod assemble;
mod config;
pub mod driver;
mod encode;
mod error;
mod fields;
mod frame;
mod model;
mod parse;
mod redacted;

pub use assemble::{AssemblyOutcome, ResultAssembler};
pub use config::{AuConfig, FormatConfig, FramingConfig, TransmissionClass};
pub use driver::{
    ACK_TURNAROUND, AbortReason, Action, ControlResponse, DriverError, FrameId, MAX_QUEUED_EVENTS,
    ReceiveDriver, ReceiveEvent, ReceiveOptions, TimerId,
};
pub use encode::{Tri, TriTest, encode_stop, encode_tri};
pub use error::AuError;
pub use frame::{Frame, FrameEnd, StreamDecoder, WireEvent, encode_frame};
pub use model::{
    BlockId, Demographics, Message, Query, ResultBlock, ResultKind, ResultTest, SampleHeader,
    SampleResults, SessionKind, TriBlock,
};
pub use parse::parse_message;
pub use redacted::Redacted;
