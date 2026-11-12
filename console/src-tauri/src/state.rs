//! What the window is told about the engine connection, and how it is told.

use std::time::Duration;

use serde::Serialize;
use packie_core::{Actor, Timestamp};
use packie_ipc::{Capability, Event};

/// Phase of the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    /// Connected and greeted; subscriptions are flowing.
    Connected,
    /// No connection; a retry is scheduled (or no location is configured).
    Disconnected,
    /// An attempt to connect is in flight.
    Reconnecting,
}

/// The engine, as it introduced itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct EngineInfo {
    pub(crate) engine_version: String,
    pub(crate) protocol_version: u32,
    pub(crate) node_id: String,
    pub(crate) node_name: String,
    /// Optional engine features (for example `test_hooks`).
    pub(crate) capabilities: Vec<Capability>,
    /// The OS account the engine attributes this connection's actions to.
    pub(crate) actor: Option<Box<Actor>>,
}

/// Payload of `packie://connection` and the result of `ipc_connection`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ConnectionState {
    pub(crate) state: Phase,
    pub(crate) endpoint: Option<String>,
    pub(crate) error: Option<String>,
    /// Consecutive failed attempts since the last connected state; 0 when connected.
    pub(crate) attempt: u32,
    pub(crate) engine: Option<EngineInfo>,
    /// When this state began (RFC 3339).
    pub(crate) since: Timestamp,
}

impl ConnectionState {
    /// The state before the first attempt.
    pub(crate) fn initial() -> Self {
        Self {
            state: Phase::Disconnected,
            endpoint: None,
            error: None,
            attempt: 0,
            engine: None,
            since: Timestamp::now(),
        }
    }
}

/// Payload of `packie://event`.
#[derive(Debug, Serialize)]
pub(crate) struct EventPayload<'a> {
    /// The console-side subscription id, stable across reconnects.
    pub(crate) subscription: u64,
    pub(crate) event: &'a Event,
}

/// Where connection changes and subscription events go. The shell emits Tauri events; tests
/// collect them.
pub(crate) trait Emitter: Send + Sync + 'static {
    fn connection(&self, state: &ConnectionState);
    fn event(&self, payload: &EventPayload<'_>);
}

/// Delays between connection attempts: `base`, doubling, capped at `cap`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Backoff {
    pub(crate) base: Duration,
    pub(crate) cap: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(500),
            cap: Duration::from_secs(5),
        }
    }
}

impl Backoff {
    /// The wait before the retry that follows `failures` consecutive failed attempts
    /// (0 after a lost connection), moved by `jitter_percent` (-20 to 20).
    pub(crate) fn delay(&self, failures: u32, jitter_percent: i64) -> Duration {
        let doubled = self
            .base
            .saturating_mul(2_u32.saturating_pow(failures.saturating_sub(1)));
        let nominal = doubled.min(self.cap);
        let percent = u32::try_from(100 + jitter_percent.clamp(-20, 20)).unwrap_or(100);
        nominal.saturating_mul(percent) / 100
    }
}

/// Jitter in -20..=20 percent from the clock's sub-second nanoseconds.
pub(crate) fn jitter_percent() -> i64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    i64::from(nanos % 41) - 20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_the_cap_and_applies_jitter() {
        let backoff = Backoff::default();
        let ms = |failures, jitter| backoff.delay(failures, jitter).as_millis();
        assert_eq!(ms(0, 0), 500);
        assert_eq!(ms(1, 0), 500);
        assert_eq!(ms(2, 0), 1000);
        assert_eq!(ms(3, 0), 2000);
        assert_eq!(ms(4, 0), 4000);
        assert_eq!(ms(5, 0), 5000);
        assert_eq!(ms(500, 0), 5000);
        assert_eq!(ms(1, -20), 400);
        assert_eq!(ms(1, 20), 600);
        assert_eq!(ms(1, 99), 600);
        assert!((-20..=20).contains(&jitter_percent()));
    }

    #[test]
    fn state_serializes_as_the_window_expects() {
        let mut state = ConnectionState::initial();
        state.engine = Some(EngineInfo {
            engine_version: "1".into(),
            protocol_version: 1,
            node_id: "n".into(),
            node_name: "N".into(),
            capabilities: Vec::new(),
            actor: None,
        });
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "disconnected");
        assert_eq!(json["attempt"], 0);
        assert!(json["endpoint"].is_null());
        assert_eq!(json["engine"]["node_name"], "N");
        assert!(json["engine"]["actor"].is_null());
        assert!(json["since"].as_str().unwrap().ends_with('Z'));
    }
}
