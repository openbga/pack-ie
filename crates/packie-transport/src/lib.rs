//! Endpoint transports for Positive ACK (`docs/spec/bridge-channels.md` §9–§10,
//! `docs/spec/configuration.md` §5).
//!
//! Channels own retry, pairing and sessions; transports only open, accept, read and write,
//! with no retry loops of their own (except the short serial open retry of §9).
//!
//! - [`Transports::opener`] gives a channel an [`Opener`] for a stream endpoint: a
//!   [`Dialer`] (TCP client, serial port) or a [`Bind`] (TCP listener). Each attempt dials
//!   again, or accepts on the kept [`Acceptor`]. A [`Link`] is one connected
//!   [`ByteStream`] with its peer description.
//! - TCP listeners enforce the allowlist ([`packie_config::TcpListenerSettings::allows`]) and
//!   one peer at a time themselves: refused peers are reset before any byte is read, logged
//!   as `listener.peer_rejected` and counted in [`Acceptor::rejections`]. Keepalive and
//!   `idle_timeout_seconds` apply to every TCP link.
//! - Serial ports go through a process-wide registry: one holder at a time, leases keyed by
//!   port name and settings, reads paced by the holder.
//! - [`Transports::file_source`] and [`Transports::file_sink`] serve folder and SFTP
//!   endpoints: complete files as units (read only up to the source's size limit),
//!   consumed per `on_consumed`, written per `write_mode` and [`packie_config::OnExists`]
//!   through exclusively created temporary files, never through symbolic links.
//! - [`probe`] (and [`SystemTransports::probe`], which also knows the serial ports this engine
//!   holds) checks an endpoint definition without changing anything: it reports each step
//!   with its outcome and duration, sends and writes nothing, and returns counts, never file
//!   names. [`list_serial_ports`] lists what the OS reports.
//! - Every failure is a [`LinkError`]: the [`Operation`], an operator-facing cause (never
//!   payload or secrets) and whether retrying alone can fix it.
//! - Secrets are resolved at connect time through a [`SecretResolver`].
//!
//! [`SystemTransports`] is the real implementation.

#![warn(missing_docs)]

mod files;
mod folder;
mod idle;
mod link;
mod listener;
mod probe;
mod serial;
mod sftp;
mod system;
mod tcp;

pub use files::{FileSink, FileSource, FileUnit};
pub use link::{
    Acceptor, Bind, BoxFuture, ByteStream, Dialer, Incoming, Link, LinkError, Opener, Operation,
    PROBE_AFTER, PeerProbe, Rejections, Transports, describe_io,
};
pub use packie_config::{MapSecrets, SecretResolver};
pub use probe::{ProbeReport, ProbeStep, SerialPortInfo, SerialPortKind, list_serial_ports, probe};
pub use system::SystemTransports;
