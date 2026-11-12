//! `idle_timeout_seconds`: a link silent in both directions that long fails its next read.

use std::future::Future as _;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

use crate::link::{ByteStream, PeerProbe};

/// Wraps a stream; any successful read or write restarts the idle clock.
pub(crate) struct IdleStream {
    inner: Box<dyn ByteStream>,
    timeout: Duration,
    deadline: Pin<Box<Sleep>>,
}

impl IdleStream {
    pub(crate) fn wrap(inner: Box<dyn ByteStream>, timeout: Duration) -> Box<dyn ByteStream> {
        Box::new(Self {
            inner,
            timeout,
            deadline: Box::pin(tokio::time::sleep(timeout)),
        })
    }

    fn touch(&mut self) {
        let next = Instant::now() + self.timeout;
        self.deadline.as_mut().reset(next);
    }
}

impl AsyncRead for IdleStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if buf.filled().len() > before {
                    self.touch();
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => match self.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("idle for {}s", self.timeout.as_secs()),
                ))),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

impl AsyncWrite for IdleStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, data);
        if let Poll::Ready(Ok(n)) = result
            && n > 0
        {
            self.touch();
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl ByteStream for IdleStream {
    fn reset(self: Box<Self>) {
        self.inner.reset();
    }

    fn peer_probe(&self) -> Option<Box<dyn PeerProbe>> {
        self.inner.peer_probe()
    }
}
