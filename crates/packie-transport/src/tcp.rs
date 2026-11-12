//! TCP client and the socket helpers the listener shares.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use packie_config::{KeepAlive, TcpClientSettings};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpSocket, TcpStream};

use crate::idle::IdleStream;
use crate::link::{
    BoxFuture, ByteStream, Dialer, Link, LinkError, Operation, PROBE_AFTER, PeerProbe,
};

/// Applies keepalive (with probe tuning where the OS supports it) and no-delay.
pub(crate) fn tune(stream: &TcpStream, keep_alive: KeepAlive) -> io::Result<()> {
    // Latency matters more than packet count for instrument traffic.
    stream.set_nodelay(true)?;
    let socket = socket2::SockRef::from(stream);
    if let Err(error) = apply_keep_alive(&socket, keep_alive) {
        if !keep_alive.enabled {
            return Err(error);
        }
        // Older Windows lacks probe tuning: fall back to the OS timing.
        tracing::warn!(event = "endpoint.keepalive_untuned", %error, "TCP keepalive tuning not supported; using OS defaults");
        socket.set_keepalive(true)?;
    }
    Ok(())
}

fn apply_keep_alive(socket: &socket2::Socket, keep_alive: KeepAlive) -> io::Result<()> {
    if !keep_alive.enabled {
        return socket.set_keepalive(false);
    }
    let params = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(u64::from(keep_alive.time_seconds)))
        .with_interval(interval(keep_alive));
    #[cfg(not(any(target_os = "openbsd", target_os = "redox", target_os = "solaris")))]
    let params = params.with_retries(keep_alive.retry_count);
    socket.set_tcp_keepalive(&params)
}

fn interval(keep_alive: KeepAlive) -> Duration {
    Duration::from_secs(u64::from(keep_alive.interval_seconds.max(1)))
}

/// Wraps a tuned stream with its endpoint's keep-alive and the idle timeout, when one is
/// set.
pub(crate) fn finish(
    stream: TcpStream,
    keep_alive: KeepAlive,
    idle_timeout_seconds: u32,
) -> Box<dyn ByteStream> {
    let stream: Box<dyn ByteStream> = Box::new(Tcp { stream, keep_alive });
    if idle_timeout_seconds == 0 {
        stream
    } else {
        IdleStream::wrap(stream, Duration::from_secs(u64::from(idle_timeout_seconds)))
    }
}

/// A tuned TCP connection and the keep-alive its endpoint configured.
struct Tcp {
    stream: TcpStream,
    keep_alive: KeepAlive,
}

impl AsyncRead for Tcp {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for Tcp {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, data)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl ByteStream for Tcp {
    fn reset(self: Box<Self>) {
        Box::new(self.stream).reset();
    }

    fn peer_probe(&self) -> Option<Box<dyn PeerProbe>> {
        let socket = socket2::SockRef::from(&self.stream).try_clone().ok()?;
        Some(Box::new(TcpProbe {
            socket,
            keep_alive: self.keep_alive,
        }))
    }
}

/// A second handle to a TCP socket that shortens its keep-alive idle time while a message
/// is half received, and applies the endpoint's keep-alive again after.
struct TcpProbe {
    socket: socket2::Socket,
    keep_alive: KeepAlive,
}

impl PeerProbe for TcpProbe {
    fn waiting(&self, waiting: bool) {
        // Failure leaves the endpoint's keep-alive and the protocol timer in charge.
        let _ = if waiting {
            let params = socket2::TcpKeepalive::new()
                .with_time(PROBE_AFTER)
                .with_interval(interval(self.keep_alive));
            self.socket.set_tcp_keepalive(&params)
        } else {
            apply_keep_alive(&self.socket, self.keep_alive)
        };
    }
}

pub(crate) struct TcpDialer(pub(crate) TcpClientSettings);

impl Dialer for TcpDialer {
    fn dial(&self) -> BoxFuture<'_, Result<Link, LinkError>> {
        Box::pin(async move {
            let settings = &self.0;
            let timeout = Duration::from_millis(u64::from(settings.connect_timeout_ms.max(1)));
            let attempt = connect_any(&settings.host, settings.port);
            let (stream, addr) = tokio::time::timeout(timeout, attempt)
                .await
                .map_err(|_| LinkError::new(Operation::Connect, "connection timed out"))??;
            let keep_alive = settings.keep_alive();
            tune(&stream, keep_alive).map_err(|e| LinkError::io(Operation::Connect, &e))?;
            Ok(Link {
                stream: finish(stream, keep_alive, settings.idle_timeout_seconds),
                peer: Some(addr.to_string()),
            })
        })
    }
}

/// Tries every resolved address in order; the last failure is reported.
async fn connect_any(host: &str, port: u16) -> Result<(TcpStream, SocketAddr), LinkError> {
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| LinkError::io(Operation::Connect, &e))?;
    let mut last = LinkError::new(Operation::Connect, "host name did not resolve");
    for addr in addrs {
        match connect_one(addr).await {
            Ok(stream) => return Ok((stream, addr)),
            Err(error) => last = LinkError::io(Operation::Connect, &error),
        }
    }
    Err(last)
}

pub(crate) async fn connect_one(addr: SocketAddr) -> io::Result<TcpStream> {
    socket_for(addr)?.connect(addr).await
}

/// A new socket of the address family of `addr`.
pub(crate) fn socket_for(addr: SocketAddr) -> io::Result<TcpSocket> {
    if addr.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    async fn tcp(keep_alive: KeepAlive) -> (Tcp, TcpStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (peer, accepted) = tokio::join!(TcpStream::connect(addr), listener.accept());
        let stream = accepted.unwrap().0;
        tune(&stream, keep_alive).unwrap();
        (Tcp { stream, keep_alive }, peer.unwrap())
    }

    fn settings(enabled: bool) -> KeepAlive {
        KeepAlive {
            enabled,
            time_seconds: 45,
            interval_seconds: 7,
            retry_count: 3,
        }
    }

    /// Waits once with the probe on `tcp` and returns the socket's keep-alive after:
    /// whether it is on and its idle time and interval.
    fn after_waiting(tcp: &Tcp) -> (bool, Duration, Duration) {
        let socket = socket2::SockRef::from(&tcp.stream);
        let probe = tcp.peer_probe().unwrap();
        probe.waiting(true);
        assert!(socket.keepalive().unwrap());
        assert_eq!(socket.tcp_keepalive_time().unwrap(), PROBE_AFTER);
        probe.waiting(false);
        (
            socket.keepalive().unwrap(),
            socket.tcp_keepalive_time().unwrap(),
            socket.tcp_keepalive_interval().unwrap(),
        )
    }

    /// Keep-alive is on with `PROBE_AFTER` while the caller waits, then as configured.
    #[tokio::test]
    async fn the_endpoints_keep_alive_returns_when_the_caller_stops_waiting() {
        let (on, _peer) = tcp(settings(true)).await;
        assert_eq!(
            after_waiting(&on),
            (true, Duration::from_secs(45), Duration::from_secs(7))
        );
        let (off, _peer) = tcp(settings(false)).await;
        assert!(!after_waiting(&off).0, "the endpoint had keep-alive off");
    }
}
