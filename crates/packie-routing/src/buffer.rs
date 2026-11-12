//! The bounded per-direction buffer of a bridge (`docs/spec/bridge-channels.md` §6).
//!
//! One reader pump pushes segments and one writer pump pops them. Segments and bytes are
//! bounded together; a push into a full buffer blocks (backpressure reaches the
//! transport because the reader stops reading). When a push stays blocked longer than
//! the block timeout an overflow episode starts (reported once); the push keeps waiting
//! without a timeout. The episode ends when the buffer has drained to half of both
//! limits. Nothing is ever dropped while the session lives.

use std::collections::VecDeque;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use packie_config::BufferPolicy;
use tokio::sync::Notify;

/// Limits of one direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlowLimits {
    /// `buffer.max_buffered_segments`.
    pub(crate) max_segments: u32,
    /// `buffer.max_bytes`.
    pub(crate) max_bytes: u64,
    /// `buffer.block_timeout_ms`.
    pub(crate) block_timeout: Duration,
}

impl FlowLimits {
    /// The limits of `policy`.
    pub(crate) fn new(policy: &BufferPolicy) -> Self {
        Self {
            max_segments: policy.max_buffered_segments.max(1),
            max_bytes: policy.max_bytes.max(1),
            block_timeout: Duration::from_millis(u64::from(policy.block_timeout_ms.max(1))),
        }
    }
}

/// Current occupancy of one direction, read without locking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FlowGauge {
    pub(crate) segments: u32,
    pub(crate) bytes: u64,
    pub(crate) overflowing: bool,
    pub(crate) overflow_count: u64,
}

/// Nothing to pop yet.
struct Empty;

/// The buffer was closed; the segment was not accepted.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Closed;

/// One direction's bounded segment queue.
#[derive(Debug)]
pub(crate) struct FlowBuffer {
    limits: FlowLimits,
    inner: Mutex<Inner>,
    space: Notify,
    data: Notify,
    segments: AtomicU32,
    bytes: AtomicU64,
    overflowing: AtomicBool,
    overflow_count: AtomicU64,
}

#[derive(Debug, Default)]
struct Inner {
    queue: VecDeque<Vec<u8>>,
    bytes: u64,
    closed: bool,
}

impl Inner {
    fn admits(&self, limits: FlowLimits, len: u64) -> bool {
        // An oversized segment is admitted only into an empty buffer, so it cannot
        // deadlock the channel.
        self.queue.is_empty()
            || (self.queue.len() < limits.max_segments as usize
                && self.bytes.saturating_add(len) <= limits.max_bytes)
    }
}

impl FlowBuffer {
    /// An empty, open buffer.
    pub(crate) fn new(limits: FlowLimits) -> Self {
        Self {
            limits,
            inner: Mutex::new(Inner::default()),
            space: Notify::new(),
            data: Notify::new(),
            segments: AtomicU32::new(0),
            bytes: AtomicU64::new(0),
            overflowing: AtomicBool::new(false),
            overflow_count: AtomicU64::new(0),
        }
    }

    /// The limits.
    pub(crate) const fn limits(&self) -> FlowLimits {
        self.limits
    }

    /// Current occupancy.
    pub(crate) fn gauge(&self) -> FlowGauge {
        FlowGauge {
            segments: self.segments.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            overflowing: self.overflowing.load(Ordering::Relaxed),
            overflow_count: self.overflow_count.load(Ordering::Relaxed),
        }
    }

    /// Whether an overflow episode is in progress.
    pub(crate) fn is_overflowing(&self) -> bool {
        self.overflowing.load(Ordering::Relaxed)
    }

    /// Appends `segment`, waiting for room. `on_overflow` runs once if this push starts
    /// an overflow episode (blocked longer than the block timeout). Fails only when the
    /// buffer is closed. Not cancel-safe for the segment: dropping the future drops it.
    pub(crate) async fn push(
        &self,
        segment: Vec<u8>,
        on_overflow: impl FnOnce(),
    ) -> Result<(), Closed> {
        let mut segment = Some(segment);
        let mut on_overflow = Some(on_overflow);
        let deadline = tokio::time::Instant::now() + self.limits.block_timeout;
        loop {
            let mut room = pin!(self.space.notified());
            room.as_mut().enable();
            if self.try_push(&mut segment)? {
                return Ok(());
            }
            if on_overflow.is_none() {
                room.await;
            } else if tokio::time::timeout_at(deadline, room).await.is_err() {
                // From here on the push waits without a timeout.
                self.start_episode(on_overflow.take());
            }
        }
    }

    /// Appends the segment if it fits now.
    fn try_push(&self, segment: &mut Option<Vec<u8>>) -> Result<bool, Closed> {
        let len = segment.as_ref().map_or(0, |s| s.len() as u64);
        let mut inner = self.lock();
        if inner.closed {
            return Err(Closed);
        }
        if !inner.admits(self.limits, len) {
            return Ok(false);
        }
        if let Some(segment) = segment.take() {
            inner.queue.push_back(segment);
        }
        inner.bytes = inner.bytes.saturating_add(len);
        self.publish(&inner);
        drop(inner);
        self.data.notify_one();
        Ok(true)
    }

    /// Starts an overflow episode unless one is already in progress.
    fn start_episode(&self, report: Option<impl FnOnce()>) {
        if self.overflowing.swap(true, Ordering::Relaxed) {
            return;
        }
        self.overflow_count.fetch_add(1, Ordering::Relaxed);
        if let Some(report) = report {
            report();
        }
    }

    /// Removes the oldest segment, waiting for one. Returns `None` once the buffer is
    /// closed and empty. The flag is `true` when this pop ended an overflow episode.
    /// Cancel-safe.
    pub(crate) async fn pop(&self) -> Option<(Vec<u8>, bool)> {
        loop {
            let mut data = pin!(self.data.notified());
            data.as_mut().enable();
            match self.try_pop() {
                Ok(popped) => return popped,
                Err(Empty) => data.await,
            }
        }
    }

    /// The oldest segment, `Ok(None)` when closed and empty, `Err` when empty and open.
    fn try_pop(&self) -> Result<Option<(Vec<u8>, bool)>, Empty> {
        let mut inner = self.lock();
        let Some(segment) = inner.queue.pop_front() else {
            return if inner.closed { Ok(None) } else { Err(Empty) };
        };
        inner.bytes = inner.bytes.saturating_sub(segment.len() as u64);
        self.publish(&inner);
        let drained = inner.queue.len() * 2 <= self.limits.max_segments as usize
            && inner.bytes.saturating_mul(2) <= self.limits.max_bytes;
        drop(inner);
        let cleared = drained && self.overflowing.swap(false, Ordering::Relaxed);
        self.space.notify_one();
        Ok(Some((segment, cleared)))
    }

    /// Accepts no further segments; buffered ones can still be popped.
    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.data.notify_one();
        self.space.notify_one();
    }

    /// Discards everything and reopens the buffer for a new session. Returns `true` when
    /// an overflow episode was in progress (it ends with the session).
    pub(crate) fn reset(&self) -> bool {
        let mut inner = self.lock();
        *inner = Inner::default();
        self.publish(&inner);
        drop(inner);
        self.space.notify_one();
        self.overflowing.swap(false, Ordering::Relaxed)
    }

    fn publish(&self, inner: &Inner) {
        self.segments.store(
            u32::try_from(inner.queue.len()).unwrap_or(u32::MAX),
            Ordering::Relaxed,
        );
        self.bytes.store(inner.bytes, Ordering::Relaxed);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn buffer(max_segments: u32, max_bytes: u64, block_ms: u64) -> Arc<FlowBuffer> {
        Arc::new(FlowBuffer::new(FlowLimits {
            max_segments,
            max_bytes,
            block_timeout: Duration::from_millis(block_ms),
        }))
    }

    async fn produce(flow: Arc<FlowBuffer>, episodes: Arc<AtomicUsize>) {
        for i in 0..32_u8 {
            let episodes = Arc::clone(&episodes);
            let report = move || {
                episodes.fetch_add(1, Ordering::Relaxed);
            };
            flow.push(vec![i, i], report).await.unwrap();
        }
        flow.close();
    }

    async fn push_one(flow: Arc<FlowBuffer>, byte: u8) -> Result<(), Closed> {
        flow.push(vec![byte], || {}).await
    }

    #[tokio::test]
    async fn keeps_order_and_counts_occupancy() {
        let flow = buffer(10, 100, 50);
        flow.push(b"one".to_vec(), || {}).await.unwrap();
        flow.push(b"two".to_vec(), || {}).await.unwrap();
        assert_eq!(flow.gauge().segments, 2);
        assert_eq!(flow.gauge().bytes, 6);
        assert_eq!(flow.pop().await.unwrap().0, b"one");
        assert_eq!(flow.pop().await.unwrap().0, b"two");
        assert_eq!(flow.gauge(), FlowGauge::default());
    }

    #[tokio::test]
    async fn oversized_segment_enters_only_an_empty_buffer() {
        let flow = buffer(10, 4, 10_000);
        flow.push(vec![0; 64], || {}).await.unwrap();
        let blocked = tokio::time::timeout(Duration::from_millis(50), flow.push(vec![1], || {}));
        assert!(blocked.await.is_err(), "a full buffer blocks");
        assert_eq!(flow.pop().await.unwrap().0.len(), 64);
        flow.push(vec![1], || {}).await.unwrap();
    }

    #[tokio::test]
    async fn segment_limit_applies_with_the_byte_limit() {
        let flow = buffer(2, 1000, 10_000);
        flow.push(vec![1], || {}).await.unwrap();
        flow.push(vec![2], || {}).await.unwrap();
        let third = tokio::time::timeout(Duration::from_millis(50), flow.push(vec![3], || {}));
        assert!(third.await.is_err());
    }

    #[tokio::test]
    async fn block_then_degrade_reports_one_episode_and_loses_nothing() {
        let flow = buffer(4, 8, 30);
        let episodes = Arc::new(AtomicUsize::new(0));
        let producer = tokio::spawn(produce(Arc::clone(&flow), Arc::clone(&episodes)));
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(
            episodes.load(Ordering::Relaxed),
            1,
            "one episode while stalled"
        );
        let gauge = flow.gauge();
        assert!(gauge.overflowing);
        assert_eq!(
            (gauge.segments, gauge.bytes, gauge.overflow_count),
            (4, 8, 1)
        );

        let mut received = Vec::new();
        let mut cleared = 0;
        while let Some((segment, ended)) = flow.pop().await {
            received.push(segment[0]);
            cleared += usize::from(ended);
        }
        producer.await.unwrap();
        assert_eq!(
            received,
            (0..32).collect::<Vec<u8>>(),
            "order kept, nothing lost"
        );
        assert_eq!(cleared, 1, "the episode ends once, at half of both limits");
        assert!(!flow.gauge().overflowing);
    }

    #[tokio::test]
    async fn close_wakes_both_sides() {
        let flow = buffer(1, 1, 10_000);
        flow.push(vec![1], || {}).await.unwrap();
        let pusher = tokio::spawn(push_one(Arc::clone(&flow), 2));
        tokio::time::sleep(Duration::from_millis(20)).await;
        flow.close();
        assert_eq!(pusher.await.unwrap(), Err(Closed));
        assert_eq!(flow.pop().await.unwrap().0, vec![1], "buffered data drains");
        assert!(flow.pop().await.is_none());
        assert!(!flow.reset());
        flow.push(vec![3], || {}).await.unwrap();
    }
}
