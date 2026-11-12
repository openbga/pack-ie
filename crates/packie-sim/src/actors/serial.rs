//! Virtual serial pairs and the `serial_peer` actor (Unix pseudo-terminals).
//!
//! The engine opens a path in the run directory that is a symbolic link to the slave side
//! of a pseudo-terminal; the actor drives the master side. Unplugging closes both sides
//! and removes the link, so the engine's reads fail; plugging in again creates a new
//! pseudo-terminal and re-creates the link, which is what a reconnecting engine opens.

#[cfg(unix)]
pub use unix::{SerialPair, SerialPeer, serial_pair_supported};

#[cfg(not(unix))]
pub use fallback::{SerialPair, SerialPeer, serial_pair_supported};

#[cfg(unix)]
mod unix {
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serialport::{SerialPort, TTYPort};

    use crate::actors::conn::{Conn, ConnState, Shared, Writer, describe_io, lock};
    use crate::error::SimError;

    struct Ptys {
        /// The master end: keeps the pseudo-terminal alive while plugged in.
        master: TTYPort,
        /// The slave end the harness opened to create the pair; keeping it open stops
        /// the master from reporting end of file before the engine opens the port.
        _slave: TTYPort,
    }

    /// A pair named in `resources.serial_pairs`.
    #[derive(Debug)]
    pub struct SerialPair {
        link: PathBuf,
        ptys: Mutex<Option<Ptys>>,
        reader: Mutex<Option<std::thread::JoinHandle<()>>>,
    }

    impl std::fmt::Debug for Ptys {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Ptys").finish_non_exhaustive()
        }
    }

    /// Whether this host can provide a serial pair an engine can open: a pseudo-terminal
    /// pair must exist and the `serialport` crate, which the engine uses, must open its
    /// slave at a real baud rate. On macOS it cannot (setting the rate on a pseudo-terminal
    /// fails with `ENOTTY`), so scenarios that require `serial_pair` are skipped there.
    #[must_use]
    pub fn serial_pair_supported() -> bool {
        let Ok((_master, slave)) = open_pair() else {
            return false;
        };
        slave.name().is_some_and(|name| {
            serialport::new(name, 9600)
                // Only the open is tested; no read happens, so the value is irrelevant.
                .timeout(Duration::from_millis(50))
                .open()
                .is_ok()
        })
    }

    /// A pseudo-terminal pair whose descriptors are close-on-exec.
    ///
    /// `TTYPort::pair` opens the master without it; a clone is made with
    /// `F_DUPFD_CLOEXEC` and the originals are closed, all under [`crate::fdguard::creating`].
    fn open_pair() -> Result<(TTYPort, TTYPort), serialport::Error> {
        let _creating = crate::fdguard::creating();
        let (master, slave) = TTYPort::pair()?;
        Ok((master.try_clone_native()?, slave.try_clone_native()?))
    }

    impl SerialPair {
        /// Creates the pair and the link at `link`.
        pub fn create(link: PathBuf) -> Result<Self, SimError> {
            let pair = Self {
                link,
                ptys: Mutex::new(None),
                reader: Mutex::new(None),
            };
            pair.make_ptys()?;
            Ok(pair)
        }

        /// The path the engine opens.
        #[must_use]
        pub fn engine_path(&self) -> &Path {
            &self.link
        }

        fn make_ptys(&self) -> Result<(), SimError> {
            let (master, slave) = open_pair()
                .map_err(|e| SimError::other(format!("create a pseudo-terminal pair: {e}")))?;
            let slave_path = slave
                .name()
                .ok_or_else(|| SimError::other("the pseudo-terminal has no slave path"))?;
            let _ = std::fs::remove_file(&self.link);
            std::os::unix::fs::symlink(&slave_path, &self.link).map_err(|e| {
                SimError::io(format!("link {} to {slave_path}", self.link.display()), e)
            })?;
            *lock(&self.ptys) = Some(Ptys {
                master,
                _slave: slave,
            });
            Ok(())
        }

        /// Plugs the peer end in (`connect`): returns a connection over the master side.
        pub fn plug(&self) -> Result<Arc<Conn>, SimError> {
            if lock(&self.ptys).is_none() {
                self.make_ptys()?;
            }
            let (reader_port, writer_port) = {
                let ptys = lock(&self.ptys);
                let master = &ptys
                    .as_ref()
                    .ok_or_else(|| SimError::other("pair is not available"))?
                    .master;
                (
                    master
                        .try_clone_native()
                        .map_err(|e| SimError::other(format!("clone master: {e}")))?,
                    master
                        .try_clone_native()
                        .map_err(|e| SimError::other(format!("clone master: {e}")))?,
                )
            };
            let (conn, shared) = Conn::new(Writer::Serial(Arc::new(Mutex::new(writer_port))));
            let handle = std::thread::spawn(move || read_loop(reader_port, &shared));
            *lock(&self.reader) = Some(handle);
            Ok(conn)
        }

        /// Unplugs: stops the reader, closes both ends and removes the link.
        pub async fn unplug(&self) {
            let handle = lock(&self.reader).take();
            if let Some(handle) = handle {
                let _ = tokio::task::spawn_blocking(move || handle.join()).await;
            }
            drop(lock(&self.ptys).take());
            let _ = std::fs::remove_file(&self.link);
        }
    }

    fn read_loop(mut port: TTYPort, shared: &Arc<Shared>) {
        // A read returns after this long without data, so the loop notices a `stop` or
        // a pause request within one interval instead of blocking until bytes arrive.
        let _ = port.set_timeout(Duration::from_millis(50));
        let mut control = shared.control_receiver();
        let mut chunk = vec![0_u8; 4096];
        loop {
            let (reading, stop) = *control.borrow_and_update();
            if stop {
                return;
            }
            if !reading {
                // Paused: nothing is read (the peer's buffer fills, which is the point of
                // `pause_reading`); check for a resume this often.
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            match port.read(&mut chunk) {
                // A pseudo-terminal master can report end of file while no slave handle is
                // open (an engine reconnecting); wait instead of spinning.
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(n) => shared.push(&chunk[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                            | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => {
                    shared.close(ConnState::ClosedByPeer(describe_io(&e)));
                    return;
                }
            }
        }
    }

    /// The `serial_peer` actor: a pair plus the connection plugged into it.
    #[derive(Debug)]
    pub struct SerialPeer {
        pair: Arc<SerialPair>,
        current: Mutex<Option<Arc<Conn>>>,
    }

    impl SerialPeer {
        /// An actor driving `pair`.
        #[must_use]
        pub fn new(pair: Arc<SerialPair>) -> Self {
            Self {
                pair,
                current: Mutex::new(None),
            }
        }

        /// `connect`: plugs the peer end in.
        pub fn connect(&self) -> Result<(), SimError> {
            if lock(&self.current)
                .as_ref()
                .is_some_and(|c| c.state().is_open())
            {
                return Err(SimError::other("the peer end is already plugged in"));
            }
            *lock(&self.current) = Some(self.pair.plug()?);
            Ok(())
        }

        /// `close`: unplugs the peer end.
        pub async fn unplug(&self) {
            let conn = lock(&self.current).clone();
            if let Some(conn) = conn {
                conn.close(false).await;
            }
            self.pair.unplug().await;
        }

        /// The plugged-in connection.
        #[must_use]
        pub fn current(&self) -> Option<Arc<Conn>> {
            lock(&self.current).clone()
        }

        /// Removes the pair at the end of the run.
        pub async fn shutdown(&self) {
            self.unplug().await;
        }
    }
}

#[cfg(not(unix))]
mod fallback {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::actors::conn::Conn;
    use crate::error::SimError;

    /// Serial pairs need pseudo-terminals, which this platform lacks.
    #[derive(Debug)]
    pub struct SerialPair;

    /// Always `false` here.
    #[must_use]
    pub fn serial_pair_supported() -> bool {
        false
    }

    impl SerialPair {
        /// Unsupported.
        pub fn create(_link: PathBuf) -> Result<Self, SimError> {
            Err(SimError::other(
                "serial pairs are not supported on this platform",
            ))
        }

        /// Unsupported.
        #[must_use]
        pub fn engine_path(&self) -> &std::path::Path {
            std::path::Path::new("")
        }
    }

    /// Unsupported.
    #[derive(Debug)]
    pub struct SerialPeer;

    impl SerialPeer {
        /// Unsupported.
        #[must_use]
        pub fn new(_pair: Arc<SerialPair>) -> Self {
            Self
        }
        /// Unsupported.
        pub fn connect(&self) -> Result<(), SimError> {
            Err(SimError::other(
                "serial pairs are not supported on this platform",
            ))
        }
        /// Unsupported.
        pub async fn unplug(&self) {}
        /// Unsupported.
        #[must_use]
        pub fn current(&self) -> Option<Arc<Conn>> {
            None
        }
        /// Unsupported.
        pub async fn shutdown(&self) {}
    }
}
