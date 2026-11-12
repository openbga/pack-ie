//! A connection as scenarios see it (scenarios.md §5): a read buffer filled by a
//! background reader, a state, a reading flag, and a writer.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{Notify, watch};
use tokio::time::Instant;

use crate::payload::preview;

/// Locks a std mutex, ignoring poisoning (the data is plain buffers).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// State of a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    /// Usable.
    Open,
    /// The peer ended it (end of stream, reset, or the serial line failed).
    ClosedByPeer(String),
    /// The actor closed it.
    ClosedByActor,
}

impl ConnState {
    /// Whether the connection is still open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }
}

impl std::fmt::Display for ConnState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::ClosedByPeer(cause) => write!(f, "closed_by_peer ({cause})"),
            Self::ClosedByActor => f.write_str("closed_by_actor"),
        }
    }
}

#[derive(Debug)]
struct Buffer {
    data: VecDeque<u8>,
    state: ConnState,
}

/// Shared between the connection handle and its background reader.
#[derive(Debug)]
pub(crate) struct Shared {
    buffer: Mutex<Buffer>,
    notify: Notify,
    /// `(reading, stop)`.
    control: watch::Sender<(bool, bool)>,
}

impl Shared {
    fn new() -> Self {
        Self {
            buffer: Mutex::new(Buffer {
                data: VecDeque::new(),
                state: ConnState::Open,
            }),
            notify: Notify::new(),
            control: watch::channel((true, false)).0,
        }
    }

    /// Appends received bytes.
    pub(crate) fn push(&self, bytes: &[u8]) {
        lock(&self.buffer).data.extend(bytes);
        self.notify.notify_waiters();
    }

    /// A receiver of `(reading, stop)` for reader threads.
    #[cfg(unix)]
    pub(crate) fn control_receiver(&self) -> watch::Receiver<(bool, bool)> {
        self.control.subscribe()
    }

    /// Records that the connection ended, unless it already ended.
    pub(crate) fn close(&self, state: ConnState) {
        {
            let mut buffer = lock(&self.buffer);
            if buffer.state.is_open() {
                buffer.state = state;
            }
        }
        self.notify.notify_waiters();
    }
}

/// The writing half of a connection.
#[derive(Debug)]
pub(crate) enum Writer {
    /// A TCP socket.
    Tcp(OwnedWriteHalf),
    /// A serial line (blocking writes on a thread).
    #[cfg(unix)]
    Serial(Arc<Mutex<serialport::TTYPort>>),
    /// A connection already closed.
    Closed,
}

/// What a wait ended with.
#[derive(Debug)]
pub enum Waited<T> {
    /// The predicate held.
    Ready(T),
    /// The deadline passed.
    TimedOut,
}

/// A connection: handle for steps.
#[derive(Debug)]
pub struct Conn {
    shared: Arc<Shared>,
    writer: tokio::sync::Mutex<Writer>,
    closed_flag: AtomicBool,
}

impl Conn {
    pub(crate) fn new(writer: Writer) -> (Arc<Self>, Arc<Shared>) {
        let shared = Arc::new(Shared::new());
        let conn = Arc::new(Self {
            shared: Arc::clone(&shared),
            writer: tokio::sync::Mutex::new(writer),
            closed_flag: AtomicBool::new(false),
        });
        (conn, shared)
    }

    /// Starts the background reader of a TCP connection.
    pub(crate) fn spawn_tcp_reader(shared: Arc<Shared>, read: OwnedReadHalf) {
        tokio::spawn(tcp_read_loop(shared, read));
    }

    /// The connection's state.
    #[must_use]
    pub fn state(&self) -> ConnState {
        lock(&self.shared.buffer).state.clone()
    }

    /// Bytes waiting in the read buffer.
    #[must_use]
    pub fn buffered(&self) -> usize {
        lock(&self.shared.buffer).data.len()
    }

    /// A copy of the first `limit` buffered bytes.
    #[must_use]
    pub fn peek(&self, limit: usize) -> Vec<u8> {
        lock(&self.shared.buffer)
            .data
            .iter()
            .take(limit)
            .copied()
            .collect()
    }

    /// Removes and returns up to `n` bytes from the front.
    #[must_use]
    pub fn take(&self, n: usize) -> Vec<u8> {
        let mut buffer = lock(&self.shared.buffer);
        let n = n.min(buffer.data.len());
        buffer.data.drain(..n).collect()
    }

    /// Stops or restarts draining the socket (`pause_reading`, `resume_reading`).
    pub fn set_reading(&self, reading: bool) {
        self.shared.control.send_modify(|c| c.0 = reading);
    }

    /// Whether the reader drains the socket.
    #[must_use]
    pub fn is_reading(&self) -> bool {
        self.shared.control.borrow().0
    }

    /// Waits until `ready` returns `Some`, evaluated on the buffer after every change,
    /// or until `deadline`.
    pub async fn wait_until<T>(
        &self,
        deadline: Instant,
        mut ready: impl FnMut(&[u8], &ConnState) -> Option<T>,
    ) -> Waited<T> {
        loop {
            let notified = self.shared.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(value) = self.evaluate(&mut ready) {
                return Waited::Ready(value);
            }
            tokio::select! {
                () = &mut notified => {}
                () = tokio::time::sleep_until(deadline) => return Waited::TimedOut,
            }
        }
    }

    /// Runs `ready` once over the whole buffer.
    fn evaluate<T>(&self, ready: &mut impl FnMut(&[u8], &ConnState) -> Option<T>) -> Option<T> {
        let mut guard = lock(&self.shared.buffer);
        let Buffer { data, state } = &mut *guard;
        ready(data.make_contiguous(), state)
    }

    /// `expect`: the next bytes are exactly `expected`. Consumes them on success. A wrong
    /// byte fails at once without waiting for the rest.
    pub async fn expect_bytes(&self, expected: &[u8], timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        enum Outcome {
            Match,
            Mismatch(usize),
            Closed,
        }
        let mut checked = 0;
        let outcome = self
            .wait_until(deadline, |data, state| {
                let common = data.len().min(expected.len());
                if let Some(at) = (checked..common).find(|&i| data[i] != expected[i]) {
                    return Some(Outcome::Mismatch(at));
                }
                checked = common;
                if data.len() >= expected.len() {
                    return Some(Outcome::Match);
                }
                (!state.is_open()).then_some(Outcome::Closed)
            })
            .await;
        let observed_all = |this: &Self| this.peek(expected.len().max(64));
        match outcome {
            Waited::Ready(Outcome::Match) => {
                let _ = self.take(expected.len());
                Ok(())
            }
            Waited::Ready(Outcome::Mismatch(at)) => Err(mismatch_report(
                expected,
                &observed_all(self),
                Some(at),
                self.buffered(),
            )),
            Waited::Ready(Outcome::Closed) => Err(format!(
                "connection {} before enough bytes arrived: {}",
                self.state(),
                mismatch_report(expected, &observed_all(self), None, self.buffered())
            )),
            Waited::TimedOut => Err(format!(
                "timed out after {} ms: {}",
                timeout.as_millis(),
                mismatch_report(expected, &observed_all(self), None, self.buffered())
            )),
        }
    }

    /// Writes all bytes, optionally in chunks with a delay between them.
    pub async fn write(
        &self,
        bytes: &[u8],
        chunk_size: Option<usize>,
        chunk_delay: Duration,
    ) -> io::Result<()> {
        if !self.state().is_open() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                format!("connection is {}", self.state()),
            ));
        }
        let chunk = chunk_size.unwrap_or(usize::MAX).max(1);
        let mut pieces = bytes.chunks(chunk).peekable();
        while let Some(piece) = pieces.next() {
            self.write_piece(piece).await?;
            if pieces.peek().is_some() && !chunk_delay.is_zero() {
                tokio::time::sleep(chunk_delay).await;
            }
        }
        Ok(())
    }

    async fn write_piece(&self, piece: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.lock().await;
        match &mut *writer {
            Writer::Tcp(w) => w.write_all(piece).await,
            #[cfg(unix)]
            Writer::Serial(port) => {
                let port = Arc::clone(port);
                let data = piece.to_vec();
                tokio::task::spawn_blocking(move || {
                    use std::io::Write;
                    let mut port = lock(&port);
                    port.write_all(&data)?;
                    port.flush()
                })
                .await
                .map_err(|e| io::Error::other(e.to_string()))?
            }
            Writer::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "connection closed",
            )),
        }
    }

    /// Closes the connection: graceful (FIN) or with a reset (`abort`). Idempotent.
    pub async fn close(&self, abort: bool) {
        if self.closed_flag.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.control.send_modify(|c| c.1 = true);
        let mut writer = self.writer.lock().await;
        match std::mem::replace(&mut *writer, Writer::Closed) {
            Writer::Tcp(mut w) => {
                if abort {
                    let _ = w.as_ref().set_zero_linger();
                } else {
                    let _ = w.shutdown().await;
                }
            }
            #[cfg(unix)]
            Writer::Serial(_) | Writer::Closed => {}
            #[cfg(not(unix))]
            Writer::Closed => {}
        }
        drop(writer);
        self.shared.close(ConnState::ClosedByActor);
    }
}

/// Renders a TCP or serial I/O error as a short cause.
pub(crate) fn describe_io(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::ConnectionReset => "connection reset".to_owned(),
        io::ErrorKind::BrokenPipe => "broken pipe".to_owned(),
        _ => error.to_string(),
    }
}

/// Expected versus observed bytes, with the first difference for long payloads.
pub(crate) fn mismatch_report(
    expected: &[u8],
    observed: &[u8],
    first_difference: Option<usize>,
    buffered_total: usize,
) -> String {
    let mut s = format!("expected {} bytes", expected.len());
    if let Some(at) = first_difference {
        s.push_str(&format!(", first difference at byte {at}"));
        let from = at.saturating_sub(8);
        let to_exp = (at + 16).min(expected.len());
        let to_obs = (at + 16).min(observed.len());
        s.push_str(&format!(
            "\n    expected [{from}..{to_exp}]: {}\n    observed [{from}..{to_obs}]: {}",
            preview(&expected[from.min(expected.len())..to_exp], 64),
            preview(&observed[from.min(observed.len())..to_obs], 64),
        ));
    } else {
        s.push_str(&format!(
            "\n    expected: {}\n    observed so far ({buffered_total} bytes buffered): {}",
            preview(expected, 96),
            preview(observed, 96),
        ));
    }
    s
}

/// Drains a TCP socket into the connection's buffer until it ends, fails or is stopped.
/// While the reading flag is off nothing is read, so the kernel's buffers fill and the
/// sender is held back.
async fn tcp_read_loop(shared: Arc<Shared>, mut read: OwnedReadHalf) {
    use tokio::io::AsyncReadExt;
    let mut control = shared.control.subscribe();
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        let (reading, stop) = *control.borrow_and_update();
        if stop || (!reading && control.changed().await.is_err()) {
            return;
        }
        if !reading {
            continue;
        }
        let ended = tokio::select! {
            result = read.read(&mut chunk) => match result {
                Ok(0) => Some(ConnState::ClosedByPeer("end of stream".into())),
                Ok(n) => {
                    shared.push(&chunk[..n]);
                    None
                }
                Err(e) => Some(ConnState::ClosedByPeer(describe_io(&e))),
            },
            _ = control.changed() => None,
        };
        if let Some(state) = ended {
            shared.close(state);
            return;
        }
    }
}
