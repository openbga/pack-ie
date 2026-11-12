//! Opening serial devices (physical or pseudo-terminal) and the thread that reads them.

use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use packie_config::{FlowControl, Parity, SerialSettings, StopBits};
use serialport::SerialPort;
use tokio::sync::mpsc;

use crate::link::{LinkError, Operation};

/// Size of one read.
const CHUNK: usize = 4096;
/// Total time to retry a briefly locked port.
const OPEN_RETRY: Duration = Duration::from_secs(2);
const OPEN_RETRY_STEP: Duration = Duration::from_millis(200);

/// An open device with its reader thread.
pub(super) struct OpenPort {
    settings: SerialSettings,
    pub(super) writer: Arc<Mutex<Box<dyn SerialPort>>>,
    pub(super) reader: std_mpsc::Sender<ReaderCommand>,
    thread: JoinHandle<()>,
    _link: Option<VirtualLink>,
}

impl OpenPort {
    /// The reader thread still runs, so the device still works.
    pub(super) fn alive(&self) -> bool {
        !self.thread.is_finished()
    }

    /// Open with these settings and its device still working.
    pub(super) fn reusable_with(&self, settings: &SerialSettings) -> bool {
        self.settings == *settings && !self.thread.is_finished()
    }
}

impl std::fmt::Debug for OpenPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenPort")
            .field("port", &self.settings.port_name)
            .finish_non_exhaustive()
    }
}

impl Drop for OpenPort {
    fn drop(&mut self) {
        let _ = self.reader.send(ReaderCommand::Close);
    }
}

pub(super) enum ReaderCommand {
    Attach(mpsc::Sender<io::Result<Vec<u8>>>),
    Close,
}

/// Why a port would not open.
enum OpenError {
    /// Locked by another program or still releasing; worth a short retry.
    Busy(String),
    /// Will not open until something changes (`persistent`), or may on a later attempt.
    Failed { cause: String, persistent: bool },
}

impl OpenError {
    fn retryable(cause: impl Into<String>) -> Self {
        Self::Failed {
            cause: cause.into(),
            persistent: false,
        }
    }

    fn persistent(cause: impl Into<String>) -> Self {
        Self::Failed {
            cause: cause.into(),
            persistent: true,
        }
    }

    fn into_link(self, settings: &SerialSettings) -> LinkError {
        let (cause, persistent) = match self {
            Self::Busy(cause) => (cause, false),
            Self::Failed { cause, persistent } => (cause, persistent),
        };
        LinkError {
            operation: Operation::Open,
            cause: format!("{}: {cause}", settings.port_name),
            persistent,
        }
    }
}

/// Opens the port on a blocking thread, retrying a briefly locked one for [`OPEN_RETRY`].
pub(super) async fn open_with_retry(settings: SerialSettings) -> Result<OpenPort, LinkError> {
    let mut waited = Duration::ZERO;
    loop {
        let attempt = settings.clone();
        let result = tokio::task::spawn_blocking(move || open_port(&attempt))
            .await
            .map_err(|_| LinkError::new(Operation::Open, "open task stopped"))?;
        match result {
            Err(OpenError::Busy(_)) if waited < OPEN_RETRY => {
                tokio::time::sleep(OPEN_RETRY_STEP).await;
                waited += OPEN_RETRY_STEP;
            }
            Err(error) => return Err(error.into_link(&settings)),
            Ok(open) => return Ok(open),
        }
    }
}

/// Opens a physical port with `settings` and closes it again without reading or writing,
/// for a probe. One attempt: a port held by another program is reported at once.
pub(crate) fn probe_open(settings: &SerialSettings) -> Result<(), LinkError> {
    let timeout = Duration::from_millis(u64::from(settings.read_timeout_ms.max(1)));
    open_physical(settings, timeout)
        .map(drop)
        .map_err(|e| e.into_link(settings))
}

fn classify(error: &serialport::Error) -> OpenError {
    use serialport::ErrorKind as K;
    match error.kind() {
        K::NoDevice => OpenError::retryable("port not found"),
        K::InvalidInput => {
            OpenError::persistent(format!("settings not supported: {}", error.description))
        }
        // Windows reports a port held by another program as access denied.
        K::Io(io::ErrorKind::PermissionDenied) if cfg!(windows) => {
            OpenError::Busy("in use by another program".into())
        }
        K::Io(io::ErrorKind::PermissionDenied) => {
            OpenError::persistent("permission denied (is the service account in the serial group?)")
        }
        K::Io(io::ErrorKind::ResourceBusy) => OpenError::Busy("in use by another program".into()),
        _ => OpenError::retryable(error.description.clone()),
    }
}

fn open_port(settings: &SerialSettings) -> Result<OpenPort, OpenError> {
    let read_timeout = Duration::from_millis(u64::from(settings.read_timeout_ms.max(1)));
    let write_timeout = Duration::from_millis(u64::from(settings.write_timeout_ms.max(1)));
    let (port, link) = match &settings.virtual_link {
        Some(path) => open_virtual(path)?,
        None => (open_physical(settings, read_timeout)?, None),
    };
    let mut reader = port.try_clone().map_err(|e| classify(&e))?;
    reader.set_timeout(read_timeout).map_err(|e| classify(&e))?;
    let mut writer = port;
    writer
        .set_timeout(write_timeout)
        .map_err(|e| classify(&e))?;
    let (commands, rx) = std_mpsc::channel();
    let thread = std::thread::Builder::new()
        .name(format!("serial {}", settings.port_name))
        .spawn(move || read_loop(reader, &rx))
        .map_err(|e| OpenError::retryable(format!("cannot start reader: {e}")))?;
    Ok(OpenPort {
        settings: settings.clone(),
        writer: Arc::new(Mutex::new(writer)),
        reader: commands,
        thread,
        _link: link,
    })
}

fn open_physical(
    settings: &SerialSettings,
    timeout: Duration,
) -> Result<Box<dyn SerialPort>, OpenError> {
    let data_bits = match settings.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    };
    serialport::new(&settings.port_name, settings.baud_rate)
        .data_bits(data_bits)
        .parity(match settings.parity {
            Parity::None => serialport::Parity::None,
            Parity::Even => serialport::Parity::Even,
            Parity::Odd => serialport::Parity::Odd,
        })
        .stop_bits(match settings.stop_bits {
            StopBits::One => serialport::StopBits::One,
            StopBits::Two => serialport::StopBits::Two,
        })
        .flow_control(match settings.flow_control {
            FlowControl::None => serialport::FlowControl::None,
            FlowControl::RtsCts => serialport::FlowControl::Hardware,
            FlowControl::XonXoff => serialport::FlowControl::Software,
        })
        .timeout(timeout)
        .open()
        .map_err(|e| classify(&e))
}

/// Serializes every change to virtual links: an open abandoned by a cancelled dial goes on
/// in its blocking thread, so it can create or remove its link at the same path while a
/// newer open replaces it (bridge-channels §9).
static LINKS: Mutex<()> = Mutex::new(());

fn links() -> std::sync::MutexGuard<'static, ()> {
    LINKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A pseudo-terminal pair with the peer end exposed at a path; removed when dropped, unless
/// a newer link has replaced it there.
struct VirtualLink {
    path: PathBuf,
    /// Where the link at `path` points while it is ours.
    target: PathBuf,
    /// Kept open so the engine end does not see a hang-up when the peer program closes.
    _peer: Box<dyn SerialPort>,
}

impl Drop for VirtualLink {
    fn drop(&mut self) {
        // An abandoned open can finish after a newer one created its link at this path.
        let _links = links();
        if std::fs::read_link(&self.path).is_ok_and(|current| current == self.target) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn open_virtual(path: &Path) -> Result<(Box<dyn SerialPort>, Option<VirtualLink>), OpenError> {
    let (engine_end, peer_end) = serialport::TTYPort::pair()
        .map_err(|e| OpenError::persistent(format!("cannot create pseudo-terminal: {e}")))?;
    let peer_name = peer_end
        .name()
        .ok_or_else(|| OpenError::persistent("pseudo-terminal has no name"))?;
    // Replace a link left by an earlier run or an abandoned open; refuse to replace
    // anything else.
    let _links = links();
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if !meta.file_type().is_symlink() {
            return Err(OpenError::persistent(format!(
                "{} exists and is not a link",
                path.display()
            )));
        }
        std::fs::remove_file(path)
            .map_err(|e| OpenError::persistent(format!("{}: {e}", path.display())))?;
    }
    std::os::unix::fs::symlink(&peer_name, path).map_err(|e| {
        OpenError::persistent(format!("cannot create link {}: {e}", path.display()))
    })?;
    Ok((
        Box::new(engine_end),
        Some(VirtualLink {
            path: path.to_owned(),
            target: PathBuf::from(peer_name),
            _peer: Box::new(peer_end),
        }),
    ))
}

#[cfg(not(unix))]
fn open_virtual(_path: &Path) -> Result<(Box<dyn SerialPort>, Option<VirtualLink>), OpenError> {
    Err(OpenError::persistent("virtual serial links need Unix"))
}

/// Reads while a holder is attached; a chunk the holder no longer takes is kept for the
/// next one.
fn read_loop(mut port: Box<dyn SerialPort>, commands: &std_mpsc::Receiver<ReaderCommand>) {
    let mut holder: Option<mpsc::Sender<io::Result<Vec<u8>>>> = None;
    let mut kept: Option<Vec<u8>> = None;
    let mut buf = vec![0_u8; CHUNK];
    loop {
        // Without a holder, wait for one instead of reading.
        let command = if holder.is_none() {
            commands.recv().ok()
        } else {
            commands.try_recv().ok()
        };
        match command {
            Some(ReaderCommand::Attach(tx)) => holder = Some(tx),
            Some(ReaderCommand::Close) => return,
            None if holder.is_none() => return,
            None => {}
        }
        let Some(tx) = &holder else { continue };
        let chunk = match kept.take() {
            Some(chunk) => Ok(chunk),
            None => match port.read(&mut buf) {
                Ok(0) => continue,
                Ok(n) => Ok(buf[..n].to_vec()),
                Err(e) if e.kind() == io::ErrorKind::TimedOut => continue,
                Err(e) => Err(e),
            },
        };
        let failed = chunk.is_err();
        if let Err(mpsc::error::SendError(unsent)) = tx.blocking_send(chunk) {
            // The lease ended; keep the bytes for the next holder.
            kept = unsent.ok();
            holder = None;
        }
        if failed {
            // The device failed (for example unplugged); the next lease reopens it.
            return;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_virtual_link_is_not_removed_by_the_older_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ttyPACKIE0");
        let (_older_port, older) = open_virtual(&path).ok().unwrap();
        let (_newer_port, newer) = open_virtual(&path).ok().unwrap();
        let newer_target = std::fs::read_link(&path).unwrap();

        drop(older);
        assert_eq!(std::fs::read_link(&path).unwrap(), newer_target);

        drop(newer);
        assert!(std::fs::symlink_metadata(&path).is_err());
    }
}
