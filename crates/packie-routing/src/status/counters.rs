//! Atomic counters of one channel instance, read by snapshots without locking.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

use packie_core::Timestamp;

/// Counters of a message channel instance (`diagnostics.md` §4.3), since it started.
#[derive(Debug, Default)]
pub(crate) struct MessageCounters {
    pub(crate) received: AtomicU64,
    pub(crate) duplicates: AtomicU64,
    pub(crate) invalid: AtomicU64,
    pub(crate) filtered: AtomicU64,
    pub(crate) aborted: AtomicU64,
    pub(crate) bytes_in: AtomicU64,
    /// A multi-frame transfer is in progress.
    pub(crate) receiving: AtomicBool,
    /// A peer is attached to the source (a listener source keeps reading `listening`).
    pub(crate) peer_attached: AtomicBool,
    /// Unix milliseconds of the last committed message; 0 when none.
    pub(crate) last_received_ms: AtomicI64,
    /// The last commit failed; cleared by the next successful one.
    pub(crate) store_unavailable: AtomicBool,
}

impl MessageCounters {
    /// A message was committed.
    pub(crate) fn committed(&self, bytes: usize) {
        self.received.fetch_add(1, Ordering::Relaxed);
        self.bytes_in
            .fetch_add(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
        self.last_received_ms
            .store(Timestamp::now().unix_millis(), Ordering::Relaxed);
    }

    /// Adds one to `counter`.
    pub(crate) fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// Tap observations of one instance.
#[derive(Debug, Default)]
pub(crate) struct TapCounters {
    pub(crate) messages: [AtomicU64; 2],
    pub(crate) framing_errors: AtomicU64,
    pub(crate) dropped: AtomicU64,
}
