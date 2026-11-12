//! Counters that channels and endpoints update on their hot paths.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use packie_core::Timestamp;

use crate::snapshot::{ErrorSummary, TrafficSnapshot};

/// Traffic counters, last activity and last error of one endpoint or channel.
///
/// Updates are relaxed atomic adds (and one clock read), so they are cheap enough for
/// every segment. Share it with `Arc<Counters>`. "In" and "out" are relative to the
/// engine: received from and sent to the endpoint. A bridge channel counts A→B as in
/// and B→A as out.
#[derive(Debug, Default)]
pub struct Counters {
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    messages_in: AtomicU64,
    messages_out: AtomicU64,
    errors: AtomicU64,
    /// Unix milliseconds of the last activity; 0 when none yet.
    last_activity_ms: AtomicI64,
    last_error: Mutex<Option<ErrorSummary>>,
}

impl Counters {
    /// New counters at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `bytes` were received.
    pub fn bytes_in(&self, bytes: usize) {
        self.bytes_in.fetch_add(as_u64(bytes), Ordering::Relaxed);
        self.touch();
    }

    /// `bytes` were sent.
    pub fn bytes_out(&self, bytes: usize) {
        self.bytes_out.fetch_add(as_u64(bytes), Ordering::Relaxed);
        self.touch();
    }

    /// One message was received.
    pub fn message_in(&self) {
        self.messages_in.fetch_add(1, Ordering::Relaxed);
        self.touch();
    }

    /// One message was sent.
    pub fn message_out(&self) {
        self.messages_out.fetch_add(1, Ordering::Relaxed);
        self.touch();
    }

    /// A failure happened; `summary` becomes the last error.
    pub fn error(&self, summary: ErrorSummary) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        *self.last_error_slot() = Some(summary);
    }

    /// Clears the last error (for example after a successful reconnect). The error count
    /// is kept.
    pub fn clear_error(&self) {
        *self.last_error_slot() = None;
    }

    /// Current values.
    #[must_use]
    pub fn traffic(&self) -> TrafficSnapshot {
        let last = self.last_activity_ms.load(Ordering::Relaxed);
        TrafficSnapshot {
            bytes_in: self.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.bytes_out.load(Ordering::Relaxed),
            messages_in: self.messages_in.load(Ordering::Relaxed),
            messages_out: self.messages_out.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            last_activity: (last != 0)
                .then(|| Timestamp::from_unix_millis(last))
                .flatten(),
        }
    }

    /// The last error, if not cleared.
    #[must_use]
    pub fn last_error(&self) -> Option<ErrorSummary> {
        self.last_error_slot().clone()
    }

    fn last_error_slot(&self) -> MutexGuard<'_, Option<ErrorSummary>> {
        self.last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn touch(&self) {
        self.last_activity_ms
            .store(Timestamp::now().unix_millis(), Ordering::Relaxed);
    }
}

fn as_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_remembers_the_last_error() {
        let counters = Counters::new();
        assert_eq!(counters.traffic().last_activity, None);
        counters.bytes_in(10);
        counters.bytes_in(5);
        counters.bytes_out(3);
        counters.message_in();
        counters.error(ErrorSummary::new(
            "connect failed: refused",
            Some("Retrying in 2s"),
        ));
        let traffic = counters.traffic();
        assert_eq!((traffic.bytes_in, traffic.bytes_out), (15, 3));
        assert_eq!((traffic.messages_in, traffic.errors), (1, 1));
        assert!(traffic.last_activity.is_some());
        assert_eq!(
            counters.last_error().unwrap().summary,
            "connect failed: refused"
        );
        counters.clear_error();
        assert!(counters.last_error().is_none());
        assert_eq!(counters.traffic().errors, 1);
    }
}
