//! The audit log (`docs/spec/audit.md`, decision #14): an append-only, hash-chained record
//! of operator actions and content views, in its own `SQLite` files under
//! `<state>/audit/`, separate from the message store so it keeps working when the store
//! does not.
//!
//! - **Writer**: one connection behind a mutex. [`Audit::record`] is synchronous and
//!   cheap (WAL, `synchronous=NORMAL`: the commit is in the OS when it returns; a
//!   maintenance thread syncs it to disk within a second). Each event's `hash` is SHA-256
//!   over the previous hash and the event's canonical encoding (`docs/spec/audit.md` §4).
//! - **Append-only**: no code path updates or deletes an event, and triggers refuse it.
//!   Retention removes whole sealed yearly files ([`Audit::prune_expired`]) and records
//!   that it did, so verification explains the chain's new start.
//! - **Fail closed**: a failed write answers [`Error::Unavailable`] and marks the log
//!   unavailable ([`Audit::status`], the `audit_unavailable` alert) until a write succeeds;
//!   the engine records `requested` before an audited command runs and refuses the command
//!   when that fails.
//! - **Anchors**: an `audit_anchor` event, logged with its seq and hash as `audit.anchor`,
//!   is written daily while events arrive and when the last handle closes, so a rewrite of
//!   the whole log is detectable against the engine log.
//! - **Review**: [`Audit::query`] pages newest first, [`Audit::export`] chunks CSV or JSON
//!   Lines oldest first, [`Audit::verify`] (or [`verify_dir`] on a copy) walks the chain.

mod audit;
mod chain;
mod copy;
mod entry;
mod error;
mod files;
mod options;
mod query;
mod schema;
mod verify;
mod writer;

pub use audit::Audit;
pub use copy::{Copied, copy_dir};
pub use entry::{Entry, MAX_DETAILS, origin_of, this_process};
pub use error::{Error, Result};
pub use options::{Clock, DEFAULT_RETENTION_DAYS, Options, Status};
pub use packie_core::{Actor, ActorOrigin};
pub use verify::verify_dir;
