//! The seam between channels and transports.

use std::fmt;
use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::time::Duration;

use packie_config::EndpointConfig;
use packie_core::{ChannelId, Timestamp};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream};
use tokio::net::TcpStream;

use crate::files::{FileSink, FileSource};
use crate::probe::ProbeReport;

/// A boxed future, so the transport traits can be trait objects. The same type as
/// `packie_engine::BoxFuture`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A connected byte stream.
pub trait ByteStream: AsyncRead + AsyncWrite + Send + Unpin + 'static {
    /// Closes the stream at once, resetting it where the transport can, for a peer that
    /// is refused.
    fn reset(self: Box<Self>) {}

    /// A handle that makes the transport probe the peer while the caller waits for the
    /// rest of a message; `None` where the transport has no such probe.
    fn peer_probe(&self) -> Option<Box<dyn PeerProbe>> {
        None
    }
}

/// Probes a connection's peer while a message is half received (see
/// [`ByteStream::peer_probe`]).
///
/// A TCP peer that resets its connection right behind bytes the receiver has not yet
/// acknowledged can have its reset ignored by the receiving system (macOS does this on a
/// connection that has carried a few dozen exchanges; the reset no longer matches the last
/// acknowledgement sent). The connection then stays established with nothing to read, and
/// only a protocol timer ends it. A keep-alive probe draws a fresh reset from the peer's
/// system, which is taken, so the pending read fails at once.
pub trait PeerProbe: Send + Sync {
    /// `true` while the caller waits for the rest of a message: the peer is probed after
    /// [`PROBE_AFTER`] without bytes from it. `false` returns the connection to the
    /// endpoint's own keep-alive settings.
    fn waiting(&self, waiting: bool);
}

/// Silence after which a connection waiting for the rest of a message probes its peer.
pub const PROBE_AFTER: Duration = Duration::from_secs(1);

impl ByteStream for DuplexStream {}

impl ByteStream for TcpStream {
    fn reset(self: Box<Self>) {
        // A zero linger turns the close into a reset; failure only means a normal close.
        let _ = socket2::SockRef::from(&*self).set_linger(Some(Duration::ZERO));
    }
}

/// A connection ready for I/O.
pub struct Link {
    /// The bytes.
    pub stream: Box<dyn ByteStream>,
    /// Peer address or port name for snapshots (never secrets).
    pub peer: Option<String>,
}

impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Link")
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

/// A connection accepted by a listener and allowed by its allowlist.
#[derive(Debug)]
pub struct Incoming {
    /// The connection.
    pub link: Link,
    /// The peer's IP address.
    pub ip: Option<IpAddr>,
}

/// The operation that failed, as named in error summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    /// Opening an outbound connection.
    Connect,
    /// Binding a listening socket.
    Listen,
    /// Accepting a peer (also a pairing timeout on a listener side).
    Accept,
    /// Opening a serial port or a file location.
    Open,
    /// Reading.
    Read,
    /// Writing.
    Write,
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Connect => "connect",
            Self::Listen => "listen",
            Self::Accept => "accept",
            Self::Open => "open",
            Self::Read => "read",
            Self::Write => "write",
        })
    }
}

/// A transport failure. `cause` is operator text without payload bytes or secrets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{operation} failed: {cause}")]
pub struct LinkError {
    /// What failed.
    pub operation: Operation,
    /// Short cause, such as `connection refused`.
    pub cause: String,
    /// Retrying alone will not fix it (permission denied, bad address, host key mismatch):
    /// the endpoint reports `error` and retries continue at the longest delay.
    pub persistent: bool,
}

impl LinkError {
    /// A failure that a retry may fix.
    pub fn new(operation: Operation, cause: impl Into<String>) -> Self {
        Self {
            operation,
            cause: cause.into(),
            persistent: false,
        }
    }

    /// A failure that needs a fix before a retry can succeed.
    pub fn persistent(operation: Operation, cause: impl Into<String>) -> Self {
        Self {
            persistent: true,
            ..Self::new(operation, cause)
        }
    }

    /// The endpoint is of a kind that has no files.
    pub(crate) fn not_a_file_endpoint() -> Self {
        Self::persistent(Operation::Open, "not a file endpoint")
    }

    /// From an I/O error, with persistence judged from its kind.
    #[must_use]
    pub fn io(operation: Operation, error: &io::Error) -> Self {
        let persistent = matches!(
            error.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::AddrNotAvailable
        );
        Self {
            operation,
            cause: describe_io(error),
            persistent,
        }
    }
}

/// Operator text for an I/O error.
#[must_use]
pub fn describe_io(error: &io::Error) -> String {
    use io::ErrorKind as K;
    match error.kind() {
        K::ConnectionRefused => "connection refused".into(),
        K::ConnectionReset => "connection reset by peer".into(),
        K::ConnectionAborted => "connection aborted".into(),
        K::TimedOut => "connection timed out".into(),
        K::BrokenPipe => "connection closed by peer".into(),
        K::UnexpectedEof => "peer closed the connection".into(),
        K::AddrInUse => "address in use".into(),
        K::AddrNotAvailable => "address not available on this host".into(),
        K::PermissionDenied => "permission denied".into(),
        K::NotFound => "not found".into(),
        K::HostUnreachable => "host unreachable".into(),
        K::NetworkUnreachable => "network unreachable".into(),
        _ => error.to_string(),
    }
}

/// Opens outbound connections (TCP client, serial port).
pub trait Dialer: Send + Sync {
    /// One connection attempt.
    ///
    /// # Errors
    ///
    /// The attempt failed; the error says why and whether retrying alone can fix it.
    fn dial(&self) -> BoxFuture<'_, Result<Link, LinkError>>;
}

/// Binds a listening socket.
pub trait Bind: Send + Sync {
    /// Binds; the result accepts until dropped.
    ///
    /// # Errors
    ///
    /// The address cannot be bound (in use, not available, no permission).
    fn bind(&self) -> BoxFuture<'_, Result<Box<dyn Acceptor>, LinkError>>;
}

/// Peers a listener refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rejections {
    /// Peers refused since the listener was bound.
    pub count: u64,
    /// The latest refusal: when, which peer, and why (`not_allowed`, `session_active`).
    pub last: Option<(Timestamp, String, &'static str)>,
}

/// A bound listener.
pub trait Acceptor: Send {
    /// The next allowed connection. Peers outside the allowlist, and peers arriving while
    /// a link from this listener is open, are reset before this returns.
    /// Cancel-safe.
    ///
    /// # Errors
    ///
    /// The accepted socket could not be set up, or the listening socket closed.
    fn accept(&mut self) -> BoxFuture<'_, Result<Incoming, LinkError>>;

    /// Peers refused so far.
    fn rejections(&self) -> Rejections {
        Rejections::default()
    }
}

/// How a channel brings up one endpoint.
pub enum Opener {
    /// The engine dials (TCP client) or opens (serial).
    Dial(Box<dyn Dialer>),
    /// The engine listens and accepts one peer.
    Listen(Box<dyn Bind>),
}

impl fmt::Debug for Opener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dial(_) => "Opener::Dial",
            Self::Listen(_) => "Opener::Listen",
        })
    }
}

/// Provides openers and file access for configured endpoints.
pub trait Transports: Send + Sync + 'static {
    /// The opener for a stream endpoint, used by the channel instance `holder` (which
    /// leases exclusive resources such as serial ports), or a persistent failure when this
    /// build cannot serve its kind.
    ///
    /// # Errors
    ///
    /// The endpoint is not a stream endpoint, or this build cannot serve its kind.
    fn opener(&self, endpoint: &EndpointConfig, holder: ChannelId) -> Result<Opener, LinkError>;

    /// A source of complete files from a folder or SFTP endpoint.
    ///
    /// # Errors
    ///
    /// The endpoint is not a file endpoint.
    fn file_source(&self, endpoint: &EndpointConfig) -> Result<Box<dyn FileSource>, LinkError> {
        let _ = endpoint;
        Err(LinkError::not_a_file_endpoint())
    }

    /// A writer of files to a folder or SFTP endpoint.
    ///
    /// # Errors
    ///
    /// The endpoint is not a file endpoint.
    fn file_sink(&self, endpoint: &EndpointConfig) -> Result<Box<dyn FileSink>, LinkError> {
        let _ = endpoint;
        Err(LinkError::not_a_file_endpoint())
    }

    /// Probes `endpoint` read-only within `timeout` (see [`crate::probe()`]); `None` when
    /// these transports cannot probe.
    fn probe_endpoint<'a>(
        &'a self,
        endpoint: &'a EndpointConfig,
        timeout: Duration,
    ) -> BoxFuture<'a, Option<ProbeReport>> {
        let _ = (endpoint, timeout);
        Box::pin(async { None })
    }
}
