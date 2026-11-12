//! Serial ports through a process-wide registry (`docs/spec/bridge-channels.md` §9).
//!
//! Each open port has one reader thread. It reads only while a channel holds a lease,
//! into a channel of a few chunks; when the holder stops reading, the thread blocks, so
//! the device buffer and hardware flow control hold the instrument back. Ports stay open
//! across leases (reconnect attempts) and are reopened when the settings change or the
//! device failed. Writes run on blocking threads, bounded by `write_timeout_ms`.
//!
//! This module is the registry and the dialer; `port` opens and reads devices and `stream`
//! is the link a lease hands out.

mod port;
mod stream;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc as std_mpsc};

use packie_config::SerialSettings;
use packie_core::ChannelId;
use serialport::SerialPort;
use tokio::sync::mpsc;

use crate::link::{BoxFuture, Dialer, Link, LinkError, Operation};
pub(crate) use port::probe_open;
use port::{OpenPort, ReaderCommand, open_with_retry};
use stream::{Lease, SerialStream};

/// Chunks waiting between the reader thread and the holder.
const READ_CHUNKS: usize = 4;

/// Owns every open serial port of the process. Cheap to clone; clones share ports.
#[derive(Debug, Clone, Default)]
pub(crate) struct PortRegistry {
    hosts: Arc<Mutex<HashMap<String, Host>>>,
}

/// Whether the engine is using a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortUse {
    /// Nobody in this process has it.
    Free,
    /// A channel instance holds it.
    Held(ChannelId),
    /// Open and kept between connection attempts of its channels.
    OpenIdle,
}

/// One port: its holder and open handles.
#[derive(Debug, Default)]
struct Host {
    holder: Option<ChannelId>,
    open: Option<OpenPort>,
}

/// The registry key: the link path for virtual ports, the port name otherwise.
fn key(settings: &SerialSettings) -> String {
    match &settings.virtual_link {
        Some(link) => format!("link:{}", link.display()),
        None => settings.port_name.to_lowercase(),
    }
}

impl PortRegistry {
    pub(crate) fn dialer(&self, settings: SerialSettings, holder: ChannelId) -> SerialDialer {
        SerialDialer {
            registry: self.clone(),
            settings,
            holder,
        }
    }

    /// Who in this process uses the port of `settings`.
    pub(crate) fn usage(&self, settings: &SerialSettings) -> PortUse {
        let hosts = self.lock();
        let Some(host) = hosts.get(&key(settings)) else {
            return PortUse::Free;
        };
        match (host.holder, &host.open) {
            (Some(holder), _) => PortUse::Held(holder),
            (None, Some(open)) if open.alive() => PortUse::OpenIdle,
            _ => PortUse::Free,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Host>> {
        self.hosts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Claims the port for `holder`. The returned lease releases the claim when dropped,
    /// so a caller that fails or is cancelled before it has a link never strands the
    /// port. The open port to use comes with it when the port is already open with these
    /// settings, or `None` when the caller must open it.
    fn claim(
        &self,
        settings: &SerialSettings,
        holder: ChannelId,
    ) -> Result<(Lease, Option<Shared>), LinkError> {
        let mut hosts = self.lock();
        let host = hosts.entry(key(settings)).or_default();
        if let Some(current) = host.holder {
            return Err(LinkError::new(
                Operation::Open,
                format!("endpoint in use by channel {current}"),
            ));
        }
        host.holder = Some(holder);
        let lease = Lease::new(self.clone(), key(settings));
        // Reopen after a device failure or a settings change.
        let reusable = host
            .open
            .as_ref()
            .is_some_and(|open| open.reusable_with(settings));
        if !reusable {
            host.open = None;
            return Ok((lease, None));
        }
        Ok((lease, host.open.as_ref().map(Shared::of)))
    }

    fn install(&self, settings: &SerialSettings, open: OpenPort) -> Shared {
        let shared = Shared::of(&open);
        self.lock().entry(key(settings)).or_default().open = Some(open);
        shared
    }

    fn release(&self, key: &str) {
        if let Some(host) = self.lock().get_mut(key) {
            host.holder = None;
        }
    }
}

/// What a lease needs from an open port.
struct Shared {
    writer: Arc<Mutex<Box<dyn SerialPort>>>,
    reader: std_mpsc::Sender<ReaderCommand>,
}

impl Shared {
    fn of(open: &OpenPort) -> Self {
        Self {
            writer: Arc::clone(&open.writer),
            reader: open.reader.clone(),
        }
    }
}

pub(crate) struct SerialDialer {
    registry: PortRegistry,
    settings: SerialSettings,
    holder: ChannelId,
}

impl SerialDialer {
    /// Claims the port and makes sure it is open. The claim is released when the lease is
    /// dropped: on failure, and when the caller cancels the open.
    async fn lease(&self) -> Result<(Lease, Shared), LinkError> {
        let (lease, open) = self.registry.claim(&self.settings, self.holder)?;
        if let Some(shared) = open {
            return Ok((lease, shared));
        }
        let open = open_with_retry(self.settings.clone()).await?;
        Ok((lease, self.registry.install(&self.settings, open)))
    }
}

impl Dialer for SerialDialer {
    fn dial(&self) -> BoxFuture<'_, Result<Link, LinkError>> {
        Box::pin(async move {
            let settings = &self.settings;
            let (lease, shared) = self.lease().await?;
            let (tx, rx) = mpsc::channel(READ_CHUNKS);
            if shared.reader.send(ReaderCommand::Attach(tx)).is_err() {
                return Err(LinkError::new(Operation::Open, "port reader stopped"));
            }
            let peer = settings
                .virtual_link
                .as_ref()
                .map_or_else(|| settings.port_name.clone(), |l| l.display().to_string());
            Ok(Link {
                stream: Box::new(SerialStream::new(rx, shared.writer, lease)),
                peer: Some(peer),
            })
        })
    }
}
