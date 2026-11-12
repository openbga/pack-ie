//! `tcp_server` and `tcp_client` actors.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::net::{TcpListener, TcpSocket, TcpStream, lookup_host};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::conn::{Conn, ConnState, Writer, describe_io, lock};
use crate::error::SimError;

/// Builds a [`Conn`] around an established stream and starts its reader.
fn connection_from(stream: TcpStream) -> Arc<Conn> {
    let (read, write) = stream.into_split();
    let (conn, shared) = Conn::new(Writer::Tcp(write));
    Conn::spawn_tcp_reader(shared, read);
    conn
}

/// Resets a connection nobody claimed.
fn reset(stream: &TcpStream) {
    let _ = stream.set_zero_linger();
}

#[derive(Debug, Default)]
struct ServerState {
    accept_task: Option<JoinHandle<()>>,
    unclaimed: VecDeque<(TcpStream, SocketAddr)>,
}

/// A scripted server: accepts into an unclaimed queue; `accept` claims the oldest.
#[derive(Debug)]
pub struct TcpServer {
    bind: IpAddr,
    port: u16,
    backlog: u32,
    recv_buffer: Option<u32>,
    /// A socket bound to the port but not yet listening, handed over by the port
    /// reservation; the first `listen` uses it.
    reserved: Mutex<Option<TcpSocket>>,
    state: Arc<Mutex<ServerState>>,
    arrived: Arc<Notify>,
    current: Mutex<Option<Arc<Conn>>>,
    claimed: Mutex<Vec<Arc<Conn>>>,
}

impl TcpServer {
    /// A server for `bind:port`; nothing is bound yet.
    pub fn new(
        bind: &str,
        port: u16,
        backlog: u32,
        recv_buffer: Option<u32>,
    ) -> Result<Self, SimError> {
        let bind: IpAddr = bind.parse().map_err(|_| {
            SimError::other(format!("tcp_server bind `{bind}` is not an IP address"))
        })?;
        Ok(Self {
            bind,
            port,
            backlog,
            recv_buffer,
            reserved: Mutex::new(None),
            state: Arc::new(Mutex::new(ServerState::default())),
            arrived: Arc::new(Notify::new()),
            current: Mutex::new(None),
            claimed: Mutex::new(Vec::new()),
        })
    }

    /// Hands over a socket already bound to this server's address and port (see
    /// [`crate::ports::PortLease::take_socket`]); the next `listen` listens on it.
    pub fn adopt(&self, socket: TcpSocket) {
        *lock(&self.reserved) = Some(socket);
    }

    /// The listening port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The bind address.
    #[must_use]
    pub fn host(&self) -> String {
        self.bind.to_string()
    }

    /// Binds the port and starts accepting (`listen` step, or scenario start).
    pub fn listen(&self) -> Result<(), SimError> {
        if lock(&self.state).accept_task.is_some() {
            return Err(SimError::other("the server is already listening"));
        }
        let adopted = lock(&self.reserved).take();
        let socket = match adopted {
            Some(socket) => socket,
            None => bound_socket(self.bind, self.port, self.recv_buffer)?,
        };
        // Adopted port reservations must honor the actor's receive buffer too.
        if let Some(size) = self.recv_buffer {
            socket
                .set_recv_buffer_size(size)
                .map_err(|e| SimError::io("set receive buffer", e))?;
        }
        let listener: TcpListener = socket
            .listen(self.backlog)
            .map_err(|e| SimError::io("listen", e))?;
        let state = Arc::clone(&self.state);
        let arrived = Arc::clone(&self.arrived);
        let task = tokio::spawn(async move {
            while let Ok((stream, peer)) = listener.accept().await {
                lock(&state).unclaimed.push_back((stream, peer));
                arrived.notify_waiters();
            }
        });
        lock(&self.state).accept_task = Some(task);
        Ok(())
    }

    /// Closes the listening socket and resets unclaimed connections. The port is free again
    /// when this returns.
    pub async fn stop_listening(&self) -> Result<(), SimError> {
        let (task, queued) = {
            let mut state = lock(&self.state);
            (
                state.accept_task.take(),
                std::mem::take(&mut state.unclaimed),
            )
        };
        let Some(task) = task else {
            return Err(SimError::other("the server is not listening"));
        };
        task.abort();
        let _ = task.await;
        for (stream, _) in queued {
            reset(&stream);
        }
        Ok(())
    }

    /// Whether a connection is waiting to be claimed.
    #[must_use]
    pub fn unclaimed(&self) -> usize {
        lock(&self.state).unclaimed.len()
    }

    /// Claims the oldest unclaimed connection, waiting up to `timeout`.
    pub async fn accept(
        &self,
        timeout: Duration,
    ) -> Result<Option<(Arc<Conn>, SocketAddr)>, SimError> {
        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.arrived.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let next = lock(&self.state).unclaimed.pop_front();
            if let Some((stream, peer)) = next {
                let conn = connection_from(stream);
                *lock(&self.current) = Some(Arc::clone(&conn));
                lock(&self.claimed).push(Arc::clone(&conn));
                return Ok(Some((conn, peer)));
            }
            tokio::select! {
                () = &mut notified => {}
                () = tokio::time::sleep_until(deadline) => return Ok(None),
            }
        }
    }

    /// The current connection: the last one claimed by `accept`.
    #[must_use]
    pub fn current(&self) -> Option<Arc<Conn>> {
        lock(&self.current).clone()
    }

    /// Closes everything: connections, queue, listener.
    pub async fn shutdown(&self) {
        let _ = self.stop_listening().await;
        let claimed = std::mem::take(&mut *lock(&self.claimed));
        for conn in claimed {
            conn.close(true).await;
        }
    }
}

/// A scripted client.
#[derive(Debug)]
pub struct TcpClient {
    bind_source: Option<IpAddr>,
    recv_buffer: Option<u32>,
    current: Mutex<Option<Arc<Conn>>>,
    all: Mutex<Vec<Arc<Conn>>>,
}

/// The result of a `connect` step.
#[derive(Debug)]
pub enum ConnectOutcome {
    /// Established.
    Connected,
    /// Refused by the target.
    Refused,
}

impl TcpClient {
    /// A client; nothing is connected yet.
    pub fn new(bind_source: Option<&str>, recv_buffer: Option<u32>) -> Result<Self, SimError> {
        let bind_source = bind_source
            .map(|s| {
                s.parse::<IpAddr>()
                    .map_err(|_| SimError::other(format!("bind_source `{s}` is not an IP address")))
            })
            .transpose()?;
        Ok(Self {
            bind_source,
            recv_buffer,
            current: Mutex::new(None),
            all: Mutex::new(Vec::new()),
        })
    }

    /// Connects to `host:port`; the new connection becomes current.
    pub async fn connect(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<ConnectOutcome, SimError> {
        let target = lookup_host((host, port))
            .await
            .map_err(|e| SimError::io(format!("resolve {host}"), e))?
            .next()
            .ok_or_else(|| SimError::other(format!("{host} resolves to no address")))?;
        let socket = {
            let _creating = crate::fdguard::creating();
            if target.is_ipv4() {
                TcpSocket::new_v4()
            } else {
                TcpSocket::new_v6()
            }
        }
        .map_err(|e| SimError::io("create socket", e))?;
        if let Some(size) = self.recv_buffer {
            socket
                .set_recv_buffer_size(size)
                .map_err(|e| SimError::io("set receive buffer", e))?;
        }
        if let Some(source) = self.bind_source {
            socket
                .bind(SocketAddr::new(source, 0))
                .map_err(|e| SimError::io(format!("bind source {source}"), e))?;
        }
        match tokio::time::timeout(timeout, socket.connect(target)).await {
            Err(_) => Err(SimError::other(format!(
                "connect to {target} timed out after {} ms",
                timeout.as_millis()
            ))),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                Ok(ConnectOutcome::Refused)
            }
            // The target accepted the connection and reset it before `connect` looked at
            // the result: the handshake completed, so the connection existed and was closed
            // by the peer. A peer that rejects a second session on purpose does exactly this,
            // and whether the reset beats `connect` is a race the scenario must not depend on.
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                let (conn, shared) = Conn::new(Writer::Closed);
                shared.close(ConnState::ClosedByPeer(describe_io(&e)));
                Ok(self.register(conn))
            }
            Ok(Err(e)) => Err(SimError::io(format!("connect to {target}"), e)),
            Ok(Ok(stream)) => Ok(self.register(connection_from(stream))),
        }
    }

    /// Makes `conn` the current connection.
    fn register(&self, conn: Arc<Conn>) -> ConnectOutcome {
        *lock(&self.current) = Some(Arc::clone(&conn));
        lock(&self.all).push(conn);
        ConnectOutcome::Connected
    }

    /// The current connection.
    #[must_use]
    pub fn current(&self) -> Option<Arc<Conn>> {
        lock(&self.current).clone()
    }

    /// Closes every connection.
    pub async fn shutdown(&self) {
        let all = std::mem::take(&mut *lock(&self.all));
        for conn in all {
            conn.close(true).await;
        }
    }
}

/// A socket bound to `bind:port` with `SO_REUSEADDR`, not yet listening.
pub(crate) fn bound_socket(
    bind: IpAddr,
    port: u16,
    recv_buffer: Option<u32>,
) -> Result<TcpSocket, SimError> {
    let _creating = crate::fdguard::creating();
    let socket = match bind {
        IpAddr::V4(_) => TcpSocket::new_v4(),
        IpAddr::V6(_) => TcpSocket::new_v6(),
    }
    .map_err(|e| SimError::io("create listening socket", e))?;
    socket
        .set_reuseaddr(true)
        .map_err(|e| SimError::io("set SO_REUSEADDR", e))?;
    if let Some(size) = recv_buffer {
        socket
            .set_recv_buffer_size(size)
            .map_err(|e| SimError::io("set receive buffer", e))?;
    }
    socket
        .bind(SocketAddr::new(bind, port))
        .map_err(|e| SimError::io(format!("bind {bind}:{port}"), e))?;
    Ok(socket)
}
