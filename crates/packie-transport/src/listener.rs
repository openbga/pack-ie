//! TCP listeners: bind, allowlist, one live peer at a time.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use packie_config::TcpListenerSettings;
use packie_core::{EndpointId, Timestamp};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use tracing::Instrument as _;

use crate::link::{
    Acceptor, Bind, BoxFuture, ByteStream, Incoming, Link, LinkError, Operation, PeerProbe,
    Rejections,
};
use crate::tcp::{finish, socket_for, tune};

pub(crate) struct TcpBind {
    pub(crate) settings: TcpListenerSettings,
    pub(crate) endpoint_id: EndpointId,
    pub(crate) label: String,
}

impl Bind for TcpBind {
    fn bind(&self) -> BoxFuture<'_, Result<Box<dyn Acceptor>, LinkError>> {
        Box::pin(async move {
            let settings = &self.settings;
            let addr = SocketAddr::new(settings.bind_address, settings.port);
            let listener = bind_listener(addr, settings.accept_backlog).map_err(|e| {
                let mut error = LinkError::io(Operation::Listen, &e);
                error.persistent = e.kind() != io::ErrorKind::AddrInUse;
                error.cause = format!("{} on {addr}", error.cause);
                error
            })?;
            let (tx, rx) = mpsc::channel(1);
            let shared = Arc::new(Shared {
                settings: settings.clone(),
                endpoint_id: self.endpoint_id,
                label: self.label.clone(),
                live: Arc::new(AtomicU32::new(0)),
                freed: Arc::new(Notify::new()),
                rejections: Mutex::new(Rejections::default()),
            });
            // Inherit the caller's span, so rejections carry the channel's fields.
            let task = tokio::spawn(
                accept_loop(listener, Arc::clone(&shared), tx).instrument(tracing::Span::current()),
            );
            Ok(Box::new(TcpAcceptor { rx, shared, task }) as Box<dyn Acceptor>)
        })
    }
}

/// A socket bound to `addr` as a listener binds it, not yet listening.
pub(crate) fn bind_socket(addr: SocketAddr) -> io::Result<TcpSocket> {
    let socket = socket_for(addr)?;
    #[cfg(not(windows))]
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    Ok(socket)
}

fn bind_listener(addr: SocketAddr, backlog: u32) -> io::Result<TcpListener> {
    bind_socket(addr)?.listen(backlog.max(1))
}

struct Shared {
    settings: TcpListenerSettings,
    endpoint_id: EndpointId,
    label: String,
    /// Links handed out (or waiting to be) and not yet dropped.
    live: Arc<AtomicU32>,
    /// Signalled when a link is dropped and its slot frees up.
    freed: Arc<Notify>,
    rejections: Mutex<Rejections>,
}

impl Shared {
    fn reject(&self, stream: TcpStream, addr: SocketAddr, code: &'static str) {
        Box::new(stream).reset();
        let reason = match code {
            "not_allowed" => "not in the allowlist",
            _ => "another peer is connected",
        };
        tracing::warn!(
            event = "listener.peer_rejected",
            endpoint_id = %self.endpoint_id,
            peer = %addr,
            error_code = code,
            "Listener {} refused {addr}: {reason}",
            self.label
        );
        let mut rejections = self
            .rejections
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        rejections.count += 1;
        rejections.last = Some((Timestamp::now(), addr.to_string(), code));
    }
}

/// How long a new peer waits for the previous session to release the slot. A sender that
/// closes and reconnects at once otherwise races its own old session's teardown.
const SLOT_GRACE: Duration = Duration::from_millis(250);

/// Whether the one-peer slot is free now or frees up within `grace`.
async fn slot_frees_within(shared: &Shared, grace: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        // Registered before the check, so a release in between is not missed.
        let freed = shared.freed.notified();
        if shared.live.load(Ordering::Acquire) == 0 {
            return true;
        }
        if tokio::time::timeout_at(deadline, freed).await.is_err() {
            return shared.live.load(Ordering::Acquire) == 0;
        }
    }
}

async fn accept_loop(
    listener: TcpListener,
    shared: Arc<Shared>,
    tx: mpsc::Sender<Result<Incoming, LinkError>>,
) {
    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                // Usually descriptor exhaustion; the socket stays bound, so try again.
                tracing::warn!(event = "listener.accept_failed", endpoint_id = %shared.endpoint_id, %error, "Accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        if !shared.settings.allows(addr.ip()) {
            shared.reject(stream, addr, "not_allowed");
            continue;
        }
        if !slot_frees_within(&shared, SLOT_GRACE).await {
            shared.reject(stream, addr, "session_active");
            continue;
        }
        let keep_alive = shared.settings.keep_alive();
        let incoming = match tune(&stream, keep_alive) {
            Ok(()) => {
                shared.live.fetch_add(1, Ordering::AcqRel);
                let guarded = Guarded {
                    inner: Some(finish(
                        stream,
                        keep_alive,
                        shared.settings.idle_timeout_seconds,
                    )),
                    live: Arc::clone(&shared.live),
                    freed: Arc::clone(&shared.freed),
                };
                Ok(Incoming {
                    link: Link {
                        stream: Box::new(guarded),
                        peer: Some(addr.to_string()),
                    },
                    ip: Some(addr.ip()),
                })
            }
            Err(e) => Err(LinkError::io(Operation::Accept, &e)),
        };
        // The slot holds one peer; a closed acceptor ends the loop.
        if tx.send(incoming).await.is_err() {
            return;
        }
    }
}

struct TcpAcceptor {
    rx: mpsc::Receiver<Result<Incoming, LinkError>>,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl Drop for TcpAcceptor {
    fn drop(&mut self) {
        // Closes the listening socket.
        self.task.abort();
    }
}

impl Acceptor for TcpAcceptor {
    fn accept(&mut self) -> BoxFuture<'_, Result<Incoming, LinkError>> {
        Box::pin(async move {
            match self.rx.recv().await {
                Some(incoming) => incoming,
                None => Err(LinkError::new(Operation::Accept, "listening socket closed")),
            }
        })
    }

    fn rejections(&self) -> Rejections {
        self.shared
            .rejections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// A link that frees its listener slot when dropped.
struct Guarded {
    inner: Option<Box<dyn ByteStream>>,
    live: Arc<AtomicU32>,
    freed: Arc<Notify>,
}

impl Guarded {
    fn inner(&mut self) -> io::Result<Pin<&mut Box<dyn ByteStream>>> {
        self.inner
            .as_mut()
            .map(Pin::new)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))
    }
}

impl Drop for Guarded {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
        self.freed.notify_one();
    }
}

impl AsyncRead for Guarded {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.inner() {
            Ok(inner) => inner.poll_read(cx, buf),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl AsyncWrite for Guarded {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.inner() {
            Ok(inner) => inner.poll_write(cx, data),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.inner() {
            Ok(inner) => inner.poll_flush(cx),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.inner() {
            Ok(inner) => inner.poll_shutdown(cx),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl ByteStream for Guarded {
    fn reset(mut self: Box<Self>) {
        if let Some(inner) = self.inner.take() {
            inner.reset();
        }
    }

    fn peer_probe(&self) -> Option<Box<dyn PeerProbe>> {
        self.inner.as_ref().and_then(|inner| inner.peer_probe())
    }
}
