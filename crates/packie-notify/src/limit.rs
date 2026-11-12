//! At most one notification per alert in a time window.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::{Duration, Instant};

use packie_core::{ChannelId, ConnectorId, EndpointId};
use packie_diagnostics::{Alert, AlertKind};

/// An alert's identity for rate limiting: its code and scope. Severity and state are not
/// part of it, so a raise and the clear that follows count against the same window.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AlertKey(
    AlertKind,
    Option<ChannelId>,
    Option<EndpointId>,
    Option<ConnectorId>,
);

impl AlertKey {
    pub(crate) fn of(alert: &Alert) -> Self {
        Self(
            alert.kind,
            alert.channel_id,
            alert.endpoint_id,
            alert.destination_id,
        )
    }
}

/// When each alert last produced a message. The caller passes the clock reading, so tests
/// do not wait.
#[derive(Debug)]
pub(crate) struct RateLimiter {
    window: Duration,
    sent: HashMap<AlertKey, Instant>,
}

impl RateLimiter {
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            window,
            sent: HashMap::new(),
        }
    }

    /// Whether `key` may send at `now`; when it may, the window starts at `now`.
    pub(crate) fn allow(&mut self, key: AlertKey, now: Instant) -> bool {
        let window = self.window;
        self.sent
            .retain(|_, at| now.saturating_duration_since(*at) < window);
        match self.sent.entry(key) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(now);
                true
            }
        }
    }
}
