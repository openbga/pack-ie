//! Opening options and the availability snapshot.

use std::sync::Arc;
use std::time::Duration;

use packie_core::Timestamp;

/// Default retention: six years (HIPAA documentation retention, §164.316(b)(2)).
pub const DEFAULT_RETENTION_DAYS: u32 = 2190;

/// The source of event times.
pub type Clock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// Opening options.
#[derive(Clone)]
pub struct Options {
    /// Sealed files whose newest event is older than this many days are removed.
    pub retention_days: u32,
    /// How long a write waits for a lock before the log counts as unavailable.
    pub busy_timeout: Duration,
    /// Interval between anchors while events arrive.
    pub anchor_interval: Duration,
    /// Run the maintenance thread (checkpoint every second; anchors, retention and
    /// reopening every minute). Tests call [`Audit::run_maintenance`](crate::Audit::run_maintenance) themselves.
    pub background: bool,
    /// Event times; the system clock by default.
    pub clock: Clock,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            retention_days: DEFAULT_RETENTION_DAYS,
            busy_timeout: Duration::from_secs(2),
            anchor_interval: Duration::from_hours(24),
            background: true,
            clock: Arc::new(Timestamp::now),
        }
    }
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("retention_days", &self.retention_days)
            .field("busy_timeout", &self.busy_timeout)
            .field("anchor_interval", &self.anchor_interval)
            .field("background", &self.background)
            .finish_non_exhaustive()
    }
}

/// Whether the log can be written, for snapshots and the `audit_unavailable` alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The last write (or open) succeeded.
    pub available: bool,
    /// Why the last write failed.
    pub last_error: Option<String>,
    /// The newest event's seq (0 before the first).
    pub head_seq: u64,
    /// The newest event's hash.
    pub head_hash: String,
}
