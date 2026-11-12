//! Shared contracts for Positive ACK: identifiers, time, state enums, byte sizes.
//!
//! Everything here crosses crate boundaries and the IPC boundary, so it is small and
//! stable:
//!
//! - Configured things have operator-chosen [`Name`]s: [`ChannelId`], [`EndpointId`],
//!   [`ConnectorId`] (destinations), matching `^[a-z0-9][a-z0-9._-]{0,63}$`.
//! - Things the engine creates have [`Id`]s: [`MessageId`], [`DeliveryId`],
//!   [`AlertId`], each a UUIDv7 in lowercase hyphenated form.
//!   Both are strings in JSON, TypeScript and SQLite.
//! - [`Timestamp`] is a UTC instant written in one canonical RFC 3339 form with nine
//!   fractional digits, so text order is time order.
//! - The state enums ([`ChannelState`], [`EndpointState`], [`SessionState`],
//!   [`DeliveryState`], ...) are the vocabulary of `docs/architecture.md` §2. They
//!   serialize as `snake_case` strings and [`as_str`](ChannelState::as_str) gives the same
//!   text for structured log fields.
//! - [`ByteSize`] formats byte counts for operators; [`visible_text`] and [`hex_dump`]
//!   render message bytes for the console and CLI (message bodies are shown only there).
//!
//! # Error conventions
//!
//! Every crate defines its own error enum with `thiserror`; nothing panics outside tests.
//! Error text is operator-facing and never contains message bodies or secrets. Parsing
//! of the values in this crate fails with [`ParseError`], which never echoes its input.
//!
//! - [`Actor`] is who performed an action: the OS account behind a management connection
//!   (read from the connection, never from the client), or the engine itself.
//!
//! The `rusqlite` feature adds `ToSql`/`FromSql` for [`Id`] and [`Timestamp`].

mod actor;
mod bytes;
mod error;
mod id;
mod kinds;
mod name;
mod time;

pub use actor::{Actor, ActorOrigin};
pub use bytes::{ByteSize, hex_dump, hex_dump_bytes, visible_text};
pub use error::ParseError;
pub use id::{AlertId, ChannelId, ConnectorId, DeliveryId, EndpointId, Id, MessageId};
pub use kinds::{
    AckLevel, AttemptResult, BridgeSide, ChannelCommand, ChannelKind, ChannelState, DeliveryState,
    DeliverySummary, Direction, EndpointKind, EndpointState, MessageDirection, MessageOrigin,
    MessageState, ProtocolKind, SessionState, Severity, StopReason,
};
pub use name::{MAX_NAME, Name, is_valid_name};
pub use time::Timestamp;
