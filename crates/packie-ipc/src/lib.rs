//! The local management interface of a Positive ACK engine (`docs/decisions.md` #8).
//!
//! - **Transport**: a Unix domain socket (`<state>/packie.sock`, mode `0600`, or `0660` for
//!   the `ipc.group`) or a Windows named pipe (`\\.\pipe\packie-<node>`, remote clients
//!   rejected, local users admitted by its DACL; served through `interprocess`, which
//!   takes the security descriptor without `unsafe`). No network port, no authentication
//!   beyond the OS.
//! - **Frames** ([`frame`]): a `u32` big-endian length, then JSON; at most 16 MiB.
//! - **Protocol** ([`protocol`]): [`ClientFrame`] `{id, request}` → [`ServerFrame`]
//!   `response`/`error` with the same id; subscriptions ([`Topic`]: snapshots, logs,
//!   alerts, traffic) push `event` frames tagged with their subscription id.
//! - **Messages** ([`messages`]): the Messages grid query, message detail with deliveries
//!   and attempts, bodies (the only place message content crosses IPC), resend and close.
//!   Failed operator preconditions are `rejected` errors with a stable
//!   [`RejectionCode`](messages::RejectionCode).
//! - **Audit and backup** ([`audit`], [`backup`]): the audit review query, export chunks
//!   and chain verification; taking a backup while the engine runs and its status.
//! - **Server**: [`IpcServer::bind`] then [`IpcServer::serve`] with a [`Handler`]; the
//!   server owns framing and subscription bookkeeping.
//! - **Client**: [`IpcClient`] for the CLI and console: concurrent requests and
//!   [`Subscription`] streams. Dropping a `Subscription` stops delivery locally; send
//!   [`Request::Unsubscribe`] to stop it at the engine.
//!
//! Every DTO derives `ts_rs::TS`; `cargo test -p packie-ipc` regenerates
//! `console/src/ipc/generated/` (with `u64` as `number`).

pub mod audit;
pub mod backup;
pub mod checks;
mod client;
pub mod frame;
pub mod messages;
pub mod protocol;
pub mod reports;
mod server;
mod transport;

pub use client::{ClientError, IpcClient, Subscription};
pub use protocol::{
    Capability, ChannelCommand, ClientExportKind, ClientFrame, ErrorCode, Event, IpcError,
    PROTOCOL_VERSION, Request, Response, ServerFrame, TestHook, Topic,
};
pub use server::{ClientInfo, Handler, IpcServer};
pub use transport::TransportError;

/// The account this process runs as, read from its process token as the pipe reads a
/// client's: `(DOMAIN\user name, SID text)`. Commands that work without an engine
/// attribute their audit events with it; the environment (`USERNAME`) is never used.
///
/// # Errors
///
/// Operator text when Windows cannot say.
#[cfg(windows)]
pub fn this_process_account() -> Result<(String, String), String> {
    transport::windows_account()
}
