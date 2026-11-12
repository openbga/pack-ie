//! What the store holds at the end of a run, and whether retention kept it in bounds.
//!
//! Message channels have `retention.max_messages`; bridge channels have no retention
//! setting, yet a tap that records messages stores them. The census shows how many messages
//! each channel holds, by what the engine's own query reports, next to what retention
//! removed according to the engine's log.

use packie_core::ChannelId;
use packie_ipc::messages::{MessageFilter, MessageQuery};
use packie_ipc::{Request, Response};
use serde_json::{Value, json};

use super::logscan::LogScan;
use super::rig::{Owner, Rig};

/// Messages a purge leaves over its limit: it removes 1000 at a time.
const PURGE_BATCH: u64 = 1000;

/// One channel's messages in the store.
#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelCensus {
    pub(crate) channel: String,
    pub(crate) bridge: bool,
    /// Messages stored for the channel, in every state.
    pub(crate) stored: u64,
    /// Of those, the ones a tap recorded.
    pub(crate) observed: u64,
    /// Messages retention removed during the run.
    pub(crate) purged: u64,
}

/// The census.
#[derive(Debug, Clone, Default)]
pub(crate) struct Census {
    pub(crate) channels: Vec<ChannelCensus>,
    /// Messages in the whole store by state.
    pub(crate) by_state: Vec<(String, u64)>,
}

/// The states a message can have.
const STATES: [&str; 8] = [
    "receiving",
    "received",
    "routed",
    "filtered",
    "duplicate",
    "invalid",
    "aborted",
    "observed",
];

async fn total(rig: &Rig, filter: MessageFilter) -> u64 {
    let query = MessageQuery {
        filter,
        limit: 1,
        ..MessageQuery::default()
    };
    match rig.call(Request::GetMessages(Box::new(query))).await {
        Ok(Response::Messages(list)) => serde_json::to_value(&list)
            .ok()
            .and_then(|v| v["total"].as_u64())
            .unwrap_or(0),
        _ => 0,
    }
}

fn state_filter(state: &str) -> Option<MessageFilter> {
    serde_json::from_value(json!({ "states": [state] })).ok()
}

fn channel_filter(channel: &str, observed_only: bool) -> Option<MessageFilter> {
    let id = channel.parse::<ChannelId>().ok()?;
    let mut value: Value = json!({ "states": [] });
    if observed_only {
        value["states"] = json!(["observed"]);
    }
    let mut filter: MessageFilter = serde_json::from_value(value).ok()?;
    filter.channels = vec![id];
    Some(filter)
}

/// Counts the store's messages by state and by channel.
pub(crate) async fn take(rig: &Rig, scan: &LogScan) -> Census {
    let mut census = Census::default();
    for state in STATES {
        if let Some(filter) = state_filter(state) {
            census
                .by_state
                .push((state.to_owned(), total(rig, filter).await));
        }
    }
    for ch in &rig.channels {
        let id = ch.plan.id.clone();
        let stored = match channel_filter(&id, false) {
            Some(f) => total(rig, f).await,
            None => 0,
        };
        let observed = match channel_filter(&id, true) {
            Some(f) => total(rig, f).await,
            None => 0,
        };
        census.channels.push(ChannelCensus {
            bridge: ch.plan.kind.is_bridge(),
            purged: scan.purged.get(&id).copied().unwrap_or(0),
            channel: id,
            stored,
            observed,
        });
    }
    census
}

/// Notes what retention did not keep in bounds. These are findings about the engine, not
/// failures of the run.
pub(crate) fn judge(rig: &Rig, census: &Census, limit: u64) {
    for c in &census.channels {
        if c.bridge && c.observed > limit && c.purged == 0 {
            rig.warn(
                "retention_gap",
                Owner::RoutingStore,
                format!(
                    "{}: the tap recorded {} messages and none was ever purged. A bridge channel has no retention setting, so its recorded messages stay forever and the store grows without bound ({} messages in the store for this one channel).",
                    c.channel, c.observed, c.stored
                ),
            );
        }
        if !c.bridge && c.stored > limit + PURGE_BATCH + rig.unresolved_allowance() {
            rig.warn(
                "retention_exceeded",
                Owner::RoutingStore,
                format!(
                    "{}: {} messages are stored although retention keeps {limit} (purged so far: {})",
                    c.channel, c.stored, c.purged
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_parse() {
        assert!(state_filter("observed").is_some());
        assert!(channel_filter("bridge-01", true).is_some_and(|f| f.states.len() == 1));
        assert!(channel_filter("bridge-01", false).is_some_and(|f| f.states.is_empty()));
    }
}
