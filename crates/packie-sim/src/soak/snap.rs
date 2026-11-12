//! The engine's snapshots as the soak reads them: a subscription that measures how late
//! each publication arrives, the latest snapshot for the chaos checks, and accessors for the
//! numbers the monitor records.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use packie_ipc::{Event, IpcClient, Topic};
use serde_json::Value;

use super::util::unix_ms;

/// How old the latest snapshot may be to still be used.
const FRESH: Duration = Duration::from_millis(2500);

/// Publication timing since the last [`SnapshotWatch::take_window`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Window {
    /// Publications received.
    pub(crate) count: u64,
    /// The longest time between two publications, milliseconds.
    pub(crate) max_gap_ms: u64,
    /// Ages at receipt (receipt time minus capture time), milliseconds.
    pub(crate) ages_ms: Vec<u64>,
}

#[derive(Debug)]
struct Latest {
    at: Instant,
    value: Arc<Value>,
}

/// The snapshot subscription of the current engine.
#[derive(Debug, Default)]
pub(crate) struct SnapshotWatch {
    latest: Mutex<Option<Latest>>,
    window: Mutex<Window>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SnapshotWatch {
    /// The latest published snapshot when it is recent.
    pub(crate) fn fresh(&self) -> Option<Arc<Value>> {
        lock(&self.latest)
            .as_ref()
            .filter(|l| l.at.elapsed() < FRESH)
            .map(|l| Arc::clone(&l.value))
    }

    /// Takes the timing statistics gathered since the last call.
    pub(crate) fn take_window(&self) -> Window {
        std::mem::take(&mut *lock(&self.window))
    }

    fn publish(&self, value: Value, captured_ms: Option<u64>, gap: Option<Duration>) {
        *lock(&self.latest) = Some(Latest {
            at: Instant::now(),
            value: Arc::new(value),
        });
        let mut window = lock(&self.window);
        window.count += 1;
        if let Some(gap) = gap {
            let gap = u64::try_from(gap.as_millis()).unwrap_or(u64::MAX);
            window.max_gap_ms = window.max_gap_ms.max(gap);
        }
        if let Some(captured) = captured_ms {
            window.ages_ms.push(unix_ms().saturating_sub(captured));
        }
    }

    /// Forgets the latest snapshot (the engine ended).
    pub(crate) fn clear(&self) {
        *lock(&self.latest) = None;
    }

    /// Follows the engine at `endpoint` until `stop` ends; reconnects after every engine
    /// start. Gaps while the engine is down are not counted as publication gaps.
    pub(crate) async fn follow(self: Arc<Self>, endpoint: String, stop: super::util::Stop) {
        while !stop.is_set() {
            let Ok(client) = IpcClient::connect(&endpoint).await else {
                self.clear();
                stop.sleep(Duration::from_millis(500)).await;
                continue;
            };
            let Ok(subscription) = client.subscribe(Topic::Snapshots).await else {
                stop.sleep(Duration::from_millis(500)).await;
                continue;
            };
            self.consume(subscription, &stop).await;
            self.clear();
        }
    }

    /// Reads publications until the subscription ends or the run does.
    async fn consume(&self, mut subscription: packie_ipc::Subscription, stop: &super::util::Stop) {
        let mut previous: Option<Instant> = None;
        loop {
            let next = tokio::select! {
                event = subscription.next() => event,
                _ = stop.sleep(Duration::from_secs(3600)) => return,
            };
            let Some(event) = next else { return };
            self.take(event, &mut previous);
        }
    }

    fn take(&self, event: Event, previous: &mut Option<Instant>) {
        let Event::Snapshot { snapshot } = event else {
            return;
        };
        let captured = u64::try_from(snapshot.node.captured_at.unix_millis()).ok();
        let now = Instant::now();
        let gap = previous.map(|p| now - p);
        *previous = Some(now);
        if let Ok(value) = serde_json::to_value(&*snapshot) {
            self.publish(value, captured, gap);
        }
    }
}

/// The element of `list` whose `key` equals `id`.
fn find<'a>(list: &'a Value, key: &str, id: &str) -> Option<&'a Value> {
    list.as_array()?
        .iter()
        .find(|item| item.get(key).and_then(Value::as_str) == Some(id))
}

/// A channel of a snapshot.
pub(crate) fn channel<'a>(snapshot: &'a Value, id: &str) -> Option<&'a Value> {
    find(&snapshot["channels"], "channel_id", id)
}

pub(crate) fn text<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    dig(value, path).as_str().unwrap_or("")
}

pub(crate) fn number(value: &Value, path: &[&str]) -> u64 {
    dig(value, path).as_u64().unwrap_or(0)
}

fn dig<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter()
        .fold(value, |v, key| v.get(*key).unwrap_or(&Value::Null))
}

/// The state of a channel (`running`, `stopped`, ...).
pub(crate) fn channel_state(channel: &Value) -> &str {
    text(channel, &["state"])
}

/// The first destination of a message channel.
pub(crate) fn destination(channel: &Value) -> &Value {
    dig(channel, &["message", "destinations"])
        .get(0)
        .unwrap_or(&Value::Null)
}

/// Deliveries waiting or in flight at the first destination.
pub(crate) fn queued(channel: &Value) -> u64 {
    let d = destination(channel);
    number(d, &["queue", "queued"]) + number(d, &["queue", "sending"])
}

/// Deliveries that need attention, over every destination of a message channel.
pub(crate) fn unresolved(channel: &Value) -> u64 {
    ["rejected", "timed_out", "outcome_unknown", "failed"]
        .iter()
        .map(|k| number(channel, &["message", "unresolved", k]))
        .sum()
}

/// The ids of the channels of a snapshot with their state.
pub(crate) fn channel_states(snapshot: &Value) -> Vec<(String, String)> {
    snapshot["channels"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|c| {
                    (
                        text(c, &["channel_id"]).to_owned(),
                        channel_state(c).to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sample() -> Value {
        json!({
            "channels": [{
                "channel_id": "hl7-01", "state": "running",
                "message": {
                    "unresolved": { "rejected": 0, "timed_out": 1, "outcome_unknown": 2, "failed": 0 },
                    "destinations": [{ "queue": { "queued": 3, "sending": 1 } }]
                }
            }]
        })
    }

    #[test]
    fn accessors_read_the_snapshot_shape() {
        let s = sample();
        let c = channel(&s, "hl7-01").cloned().unwrap_or(Value::Null);
        assert_eq!(channel_state(&c), "running");
        assert_eq!(queued(&c), 4);
        assert_eq!(unresolved(&c), 3);
        assert!(channel(&s, "nope").is_none());
        assert_eq!(
            channel_states(&s),
            vec![("hl7-01".into(), "running".into())]
        );
    }

    #[test]
    fn the_window_is_taken_once() {
        let watch = SnapshotWatch::default();
        watch.publish(json!({}), Some(unix_ms().saturating_sub(40)), None);
        watch.publish(json!({}), None, Some(Duration::from_millis(1200)));
        let w = watch.take_window();
        assert_eq!((w.count, w.max_gap_ms, w.ages_ms.len()), (2, 1200, 1));
        assert_eq!(watch.take_window(), Window::default());
        assert!(watch.fresh().is_some());
        watch.clear();
        assert!(watch.fresh().is_none());
    }
}
