//! The recovery check: a channel has recovered when it runs again and traffic reaches
//! the receiving peer again.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::soak::chan::ChannelRt;
use crate::soak::rig::Rig;
use crate::soak::snap;

/// How often the check looks.
const POLL: Duration = Duration::from_millis(500);

/// Where a channel's traffic stood when the fault ended.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Baseline {
    delivered: u64,
    accepted: u64,
}

impl Baseline {
    /// What had reached the receiving peer at the time.
    pub(crate) fn delivered(self) -> u64 {
        self.delivered
    }

    pub(crate) fn of(ch: &ChannelRt) -> Self {
        Self {
            delivered: ch.delivered(),
            accepted: ch.accepted(),
        }
    }
}

/// What a channel must show to count as recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Need {
    /// The channel runs and traffic flows.
    Flowing,
    /// As above, and no more than a couple of deliveries wait in its queue.
    Drained,
}

fn recovered(view: Option<&Value>, ch: &ChannelRt, base: Baseline, need: Need) -> bool {
    let Some(view) = view else { return false };
    if snap::channel_state(view) != "running" {
        return false;
    }
    if ch.delivered() <= base.delivered || ch.accepted() <= base.accepted {
        return false;
    }
    need == Need::Flowing || ch.plan.kind.is_bridge() || snap::queued(view) <= 2
}

/// Waits until every channel of `channels` has recovered; returns how long that took, or
/// `None` when `budget` passed or the run ended first.
pub(crate) async fn wait_recovered(
    rig: &Rig,
    channels: &[(Arc<ChannelRt>, Baseline)],
    budget: Duration,
    need: Need,
) -> Option<Duration> {
    let started = tokio::time::Instant::now();
    loop {
        let snapshot = rig.snapshot().await;
        let all = snapshot.as_ref().is_some_and(|s| {
            channels
                .iter()
                .all(|(ch, base)| recovered(snap::channel(s, &ch.plan.id), ch, *base, need))
        });
        if all {
            return Some(started.elapsed());
        }
        if started.elapsed() >= budget || !rig.env.stop.sleep(POLL).await {
            return None;
        }
    }
}

/// Waits until `condition` holds; `false` when `within` passed or the run ended first.
pub(crate) async fn wait_for(rig: &Rig, within: Duration, condition: impl Fn() -> bool) -> bool {
    let started = tokio::time::Instant::now();
    while !condition() {
        if started.elapsed() >= within || !rig.env.stop.sleep(Duration::from_millis(100)).await {
            return false;
        }
    }
    true
}

/// The current view of `channel` from a fresh snapshot.
pub(crate) async fn view_of(rig: &Rig, channel: &str) -> Option<Value> {
    let snapshot = rig.snapshot().await?;
    snap::channel(&snapshot, channel).cloned()
}

/// The channels of `rig` with their baselines taken now.
pub(crate) fn all_with_baselines(rig: &Rig) -> Vec<(Arc<ChannelRt>, Baseline)> {
    rig.channels
        .iter()
        .map(|c| (Arc::clone(c), Baseline::of(c)))
        .collect()
}
