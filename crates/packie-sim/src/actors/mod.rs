//! Scripted peers: `tcp_server`, `tcp_client`, `serial_peer` and `folder`.
//!
//! Every connection actor exposes a [`Conn`] with a read buffer, a state and
//! a reading flag; folder actors read and write files. Steps are executed against these
//! by the runner (`runner.rs`).

pub mod conn;
pub mod folder;
pub mod serial;
pub mod tcp;

use std::sync::Arc;

use conn::Conn;

/// A running actor.
#[derive(Debug)]
pub enum RtActor {
    /// `tcp_server`.
    Server(tcp::TcpServer),
    /// `tcp_client`.
    Client(tcp::TcpClient),
    /// `serial_peer`.
    Serial(serial::SerialPeer),
    /// `folder`.
    Folder(folder::FolderActor),
}

impl RtActor {
    /// The actor's current connection, when it has one.
    #[must_use]
    pub fn conn(&self) -> Option<Arc<Conn>> {
        match self {
            Self::Server(s) => s.current(),
            Self::Client(c) => c.current(),
            Self::Serial(s) => s.current(),
            Self::Folder(_) => None,
        }
    }

    /// Closes everything the actor holds.
    pub async fn shutdown(&self) {
        match self {
            Self::Server(s) => s.shutdown().await,
            Self::Client(c) => c.shutdown().await,
            Self::Serial(s) => s.shutdown().await,
            Self::Folder(_) => {}
        }
    }
}
