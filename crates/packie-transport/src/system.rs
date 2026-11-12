//! The real transports.

use std::sync::Arc;
use std::time::Duration;

use packie_config::{EndpointConfig, SecretResolver, Transport};
use packie_core::ChannelId;

use crate::files::{FileSink, FileSource};
use crate::link::{BoxFuture, LinkError, Opener, Operation, Transports};
use crate::listener::TcpBind;
use crate::probe::{PortUse, ProbeReport};
use crate::serial::PortRegistry;
use crate::tcp::TcpDialer;
use crate::{folder, sftp};

/// TCP, serial, folder and SFTP endpoints of this host.
#[derive(Clone)]
pub struct SystemTransports {
    ports: PortRegistry,
    secrets: Arc<dyn SecretResolver>,
}

impl std::fmt::Debug for SystemTransports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemTransports").finish_non_exhaustive()
    }
}

impl SystemTransports {
    /// Transports resolving secrets through `secrets`.
    #[must_use]
    pub fn new(secrets: Arc<dyn SecretResolver>) -> Self {
        Self {
            ports: PortRegistry::default(),
            secrets,
        }
    }
}

impl SystemTransports {
    /// Probes `endpoint` read-only (see [`probe`](crate::probe())), spending at most `timeout`.
    /// Unlike the free function it knows which serial ports this engine uses, and reports
    /// such a port as in use without opening it.
    pub async fn probe(&self, endpoint: &EndpointConfig, timeout: Duration) -> ProbeReport {
        let usage = match &endpoint.transport {
            Transport::Serial(settings) => self.ports.usage(settings),
            _ => PortUse::Free,
        };
        crate::probe::run(endpoint, self.secrets.as_ref(), timeout, usage).await
    }

    /// The channel instance that holds the serial port of `endpoint` right now, if any.
    /// `None` for other endpoint kinds and for a port that is free or only kept open between
    /// attempts.
    #[must_use]
    pub fn serial_holder(&self, endpoint: &EndpointConfig) -> Option<ChannelId> {
        match &endpoint.transport {
            Transport::Serial(settings) => match self.ports.usage(settings) {
                PortUse::Held(holder) => Some(holder),
                PortUse::Free | PortUse::OpenIdle => None,
            },
            _ => None,
        }
    }
}

impl Transports for SystemTransports {
    fn probe_endpoint<'a>(
        &'a self,
        endpoint: &'a EndpointConfig,
        timeout: Duration,
    ) -> BoxFuture<'a, Option<ProbeReport>> {
        Box::pin(async move { Some(self.probe(endpoint, timeout).await) })
    }

    fn opener(&self, endpoint: &EndpointConfig, holder: ChannelId) -> Result<Opener, LinkError> {
        match &endpoint.transport {
            Transport::TcpClient(settings) => {
                Ok(Opener::Dial(Box::new(TcpDialer(settings.clone()))))
            }
            Transport::TcpListener(settings) => Ok(Opener::Listen(Box::new(TcpBind {
                settings: settings.clone(),
                endpoint_id: endpoint.id,
                label: endpoint.label.clone(),
            }))),
            Transport::Serial(settings) => Ok(Opener::Dial(Box::new(
                self.ports.dialer(settings.clone(), holder),
            ))),
            Transport::Folder(_) | Transport::Sftp(_) => Err(LinkError::persistent(
                Operation::Open,
                "not a stream endpoint",
            )),
        }
    }

    fn file_source(&self, endpoint: &EndpointConfig) -> Result<Box<dyn FileSource>, LinkError> {
        match &endpoint.transport {
            Transport::Folder(settings) => Ok(folder::source(settings.clone(), endpoint.id)),
            Transport::Sftp(settings) => Ok(sftp::source(
                settings.clone(),
                Arc::clone(&self.secrets),
                endpoint.id,
            )),
            _ => Err(LinkError::not_a_file_endpoint()),
        }
    }

    fn file_sink(&self, endpoint: &EndpointConfig) -> Result<Box<dyn FileSink>, LinkError> {
        match &endpoint.transport {
            Transport::Folder(settings) => Ok(folder::sink(settings.clone())),
            Transport::Sftp(settings) => Ok(sftp::sink(
                settings.clone(),
                Arc::clone(&self.secrets),
                endpoint.id,
            )),
            _ => Err(LinkError::not_a_file_endpoint()),
        }
    }
}
