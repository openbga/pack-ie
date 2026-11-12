//! The link a serial lease hands out.

use std::future::Future as _;
use std::io::{self, Write as _};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use serialport::SerialPort;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use super::PortRegistry;
use crate::link::ByteStream;

/// A claim on a port, taken before the port is opened. Frees the port for the next holder
/// when dropped: with the link, or with an open that failed or was cancelled.
pub(super) struct Lease {
    registry: PortRegistry,
    key: String,
}

impl Lease {
    pub(super) fn new(registry: PortRegistry, key: String) -> Self {
        Self { registry, key }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.registry.release(&self.key);
    }
}

pub(super) struct SerialStream {
    rx: mpsc::Receiver<io::Result<Vec<u8>>>,
    pending: Vec<u8>,
    offset: usize,
    writer: Arc<Mutex<Box<dyn SerialPort>>>,
    write: Option<tokio::task::JoinHandle<io::Result<usize>>>,
    /// Released when the stream is dropped.
    _lease: Lease,
}

impl SerialStream {
    pub(super) fn new(
        rx: mpsc::Receiver<io::Result<Vec<u8>>>,
        writer: Arc<Mutex<Box<dyn SerialPort>>>,
        lease: Lease,
    ) -> Self {
        Self {
            rx,
            pending: Vec::new(),
            offset: 0,
            writer,
            write: None,
            _lease: lease,
        }
    }
}

impl AsyncRead for SerialStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.offset >= self.pending.len() {
            match self.rx.poll_recv(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(e)),
                Poll::Ready(Some(Ok(chunk))) => {
                    self.pending = chunk;
                    self.offset = 0;
                }
            }
        }
        let n = buf.remaining().min(self.pending.len() - self.offset);
        let start = self.offset;
        buf.put_slice(&self.pending[start..start + n]);
        self.offset += n;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for SerialStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.write.is_none() {
            let writer = Arc::clone(&self.writer);
            let data = data.to_vec();
            self.write = Some(tokio::task::spawn_blocking(move || {
                let mut port = writer.lock().unwrap_or_else(PoisonError::into_inner);
                port.write(&data)
            }));
        }
        let Some(write) = self.write.as_mut() else {
            return Poll::Ready(Err(io::Error::other("write state lost")));
        };
        let result = match Pin::new(write).poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(_)) => Err(io::Error::other("write task stopped")),
        };
        self.write = None;
        Poll::Ready(result.map_err(|e| match e.kind() {
            io::ErrorKind::TimedOut => io::Error::new(io::ErrorKind::TimedOut, "write timed out"),
            _ => e,
        }))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Writes are handed to the driver synchronously.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl ByteStream for SerialStream {}
