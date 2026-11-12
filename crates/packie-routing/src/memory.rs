//! An in-memory network for tests and simulations: listeners and connections by address
//! text, backed by `tokio::io::duplex` pairs.
//!
//! Endpoints map to addresses as `host:port` (TCP client), `bind_address:port` (TCP
//! listener) and `serial:<port_name>` (serial). A test plays the peer with
//! [`MemoryNetwork::connect`] and [`MemoryNetwork::listen`]. Engine listeners enforce
//! their allowlist like the real ones: a refused peer is reset before any byte is read.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use packie_config::{EndpointConfig, TcpListenerSettings, Transport};
use packie_core::{ChannelId, Timestamp};
use packie_transport::{
    Acceptor, Bind, BoxFuture, ByteStream, Dialer, Incoming, Link, LinkError, Opener, Operation,
    Rejections, Transports,
};
use tokio::io::DuplexStream;
use tokio::sync::mpsc;

/// Bytes each in-memory pipe buffers per direction before the writer waits.
const PIPE_BYTES: usize = 64 * 1024;

/// Listeners by address. Cheap to clone; clones share state.
#[derive(Debug, Clone, Default)]
pub struct MemoryNetwork {
    listeners: Arc<Mutex<HashMap<String, Registration>>>,
    generation: Arc<AtomicU64>,
}

#[derive(Debug)]
struct Registration {
    generation: u64,
    backlog: mpsc::Sender<(DuplexStream, IpAddr)>,
}

/// One accepted in-memory connection.
#[derive(Debug)]
pub struct Accepted {
    /// The stream.
    pub stream: DuplexStream,
    /// The connecting side's address.
    pub peer: IpAddr,
}

/// A bound in-memory listener; unbinds when dropped.
#[derive(Debug)]
pub struct MemoryListener {
    network: MemoryNetwork,
    address: String,
    generation: u64,
    backlog: mpsc::Receiver<(DuplexStream, IpAddr)>,
}

impl MemoryNetwork {
    /// An empty network.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `address`, accepting up to `backlog` pending connections.
    ///
    /// # Errors
    /// `AddrInUse` when something already listens there.
    pub fn listen(&self, address: &str, backlog: usize) -> io::Result<MemoryListener> {
        let mut listeners = self.lock();
        if listeners
            .get(address)
            .is_some_and(|r| !r.backlog.is_closed())
        {
            return Err(io::Error::from(io::ErrorKind::AddrInUse));
        }
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(backlog.max(1));
        listeners.insert(
            address.to_owned(),
            Registration {
                generation,
                backlog: tx,
            },
        );
        Ok(MemoryListener {
            network: self.clone(),
            address: address.to_owned(),
            generation,
            backlog: rx,
        })
    }

    /// Connects to `address` from 127.0.0.1.
    ///
    /// # Errors
    /// `ConnectionRefused` when nothing listens there or its backlog is full.
    pub fn connect(&self, address: &str) -> io::Result<DuplexStream> {
        self.connect_from(address, IpAddr::V4(Ipv4Addr::LOCALHOST))
    }

    /// Connects to `address` from `peer`.
    ///
    /// # Errors
    /// `ConnectionRefused` when nothing listens there or its backlog is full.
    pub fn connect_from(&self, address: &str, peer: IpAddr) -> io::Result<DuplexStream> {
        let listeners = self.lock();
        let refused = || io::Error::from(io::ErrorKind::ConnectionRefused);
        let registration = listeners.get(address).ok_or_else(refused)?;
        let (ours, theirs) = tokio::io::duplex(PIPE_BYTES);
        registration
            .backlog
            .try_send((theirs, peer))
            .map_err(|_| refused())?;
        Ok(ours)
    }

    /// Whether something listens on `address`.
    #[must_use]
    pub fn is_listening(&self, address: &str) -> bool {
        self.lock()
            .get(address)
            .is_some_and(|r| !r.backlog.is_closed())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Registration>> {
        self.listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn address_of(endpoint: &EndpointConfig) -> Result<String, LinkError> {
        match &endpoint.transport {
            Transport::TcpClient(s) => Ok(format!("{}:{}", s.host, s.port)),
            Transport::TcpListener(s) => Ok(format!("{}:{}", s.bind_address, s.port)),
            Transport::Serial(s) => Ok(format!("serial:{}", s.port_name)),
            Transport::Folder(_) | Transport::Sftp(_) => Err(LinkError::persistent(
                Operation::Open,
                "not a stream endpoint",
            )),
        }
    }
}

impl MemoryListener {
    /// The next connection, or `None` when the network is gone.
    pub async fn accept(&mut self) -> Option<Accepted> {
        self.backlog
            .recv()
            .await
            .map(|(stream, peer)| Accepted { stream, peer })
    }
}

impl Drop for MemoryListener {
    fn drop(&mut self) {
        let mut listeners = self.network.lock();
        if listeners
            .get(&self.address)
            .is_some_and(|r| r.generation == self.generation)
        {
            listeners.remove(&self.address);
        }
    }
}

impl Transports for MemoryNetwork {
    fn opener(&self, endpoint: &EndpointConfig, _holder: ChannelId) -> Result<Opener, LinkError> {
        let address = Self::address_of(endpoint)?;
        let target = MemoryTarget {
            network: self.clone(),
            address,
            serial: matches!(endpoint.transport, Transport::Serial(_)),
        };
        Ok(match &endpoint.transport {
            Transport::TcpListener(settings) => Opener::Listen(Box::new(MemoryBind {
                target,
                settings: settings.clone(),
            })),
            _ => Opener::Dial(Box::new(target)),
        })
    }
}

struct MemoryTarget {
    network: MemoryNetwork,
    address: String,
    serial: bool,
}

impl Dialer for MemoryTarget {
    fn dial(&self) -> BoxFuture<'_, Result<Link, LinkError>> {
        Box::pin(async move {
            let operation = if self.serial {
                Operation::Open
            } else {
                Operation::Connect
            };
            let stream = self
                .network
                .connect(&self.address)
                .map_err(|e| LinkError::io(operation, &e))?;
            Ok(Link {
                stream: Box::new(stream),
                peer: Some(self.address.clone()),
            })
        })
    }
}

struct MemoryBind {
    target: MemoryTarget,
    settings: TcpListenerSettings,
}

impl Bind for MemoryBind {
    fn bind(&self) -> BoxFuture<'_, Result<Box<dyn Acceptor>, LinkError>> {
        Box::pin(async move {
            let address = &self.target.address;
            let listener = self.target.network.listen(address, 16).map_err(|e| {
                let mut error = LinkError::io(Operation::Listen, &e);
                error.cause = format!("{} on {address}", error.cause);
                error
            })?;
            Ok(Box::new(MemoryAcceptor {
                listener,
                settings: self.settings.clone(),
                rejections: Rejections::default(),
            }) as Box<dyn Acceptor>)
        })
    }
}

/// An engine-side listener: the allowlist applies.
struct MemoryAcceptor {
    listener: MemoryListener,
    settings: TcpListenerSettings,
    rejections: Rejections,
}

impl MemoryAcceptor {
    async fn next_allowed(&mut self) -> Result<Incoming, LinkError> {
        loop {
            let Some(accepted) = self.listener.accept().await else {
                return Err(LinkError::new(Operation::Accept, "listener closed"));
            };
            if let Some(incoming) = self.admit(accepted) {
                return Ok(incoming);
            }
        }
    }

    /// The connection when its peer is allowed; otherwise resets it and counts it.
    fn admit(&mut self, accepted: Accepted) -> Option<Incoming> {
        let peer = accepted.peer.to_string();
        if self.settings.allows(accepted.peer) {
            return Some(Incoming {
                link: Link {
                    stream: Box::new(accepted.stream),
                    peer: Some(peer),
                },
                ip: Some(accepted.peer),
            });
        }
        Box::new(accepted.stream).reset();
        self.rejections.count = self.rejections.count.saturating_add(1);
        self.rejections.last = Some((Timestamp::now(), peer, "not_allowed"));
        None
    }
}

impl Acceptor for MemoryAcceptor {
    fn accept(&mut self) -> BoxFuture<'_, Result<Incoming, LinkError>> {
        Box::pin(self.next_allowed())
    }

    fn rejections(&self) -> Rejections {
        self.rejections.clone()
    }
}
