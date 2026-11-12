//! Chaos: faults injected into the peers and the engine while the load runs, each with a
//! recovery budget.
//!
//! An episode injects one fault, removes it, and then waits for the system to recover:
//! the channel runs again and traffic reaches the receiving peer again within the budget.
//! Episodes of different channels overlap; the two that kill the engine run alone.
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`plan`] | When each kind of episode starts, from the seed. |
//! | `check` | The recovery check and its helpers. |
//! | `peer` | Episodes that act on peers: disconnects, stalls, a destination down, a slow file. |
//! | `engine` | Episodes that act on the engine: config applies, pause and resume, kill and restart, a blocked store. |

mod check;
mod cut;
mod engine;
mod peer;
pub(crate) mod plan;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::json;
use tokio::sync::RwLock;
use tokio::task::JoinSet;

use self::plan::{Kind, Planned};
use super::chan::ChannelRt;
use super::rig::{Owner, Rig};
use super::topology::Kind as ChannelKind;
use super::util::Rng;

/// A finished episode, as listed in the report.
#[derive(Debug, Clone)]
pub(crate) struct Episode {
    pub(crate) id: u32,
    pub(crate) kind: &'static str,
    pub(crate) target: String,
    pub(crate) detail: String,
    pub(crate) start_ms: u64,
    pub(crate) end_ms: u64,
    /// Time from the end of the fault to recovery, milliseconds.
    pub(crate) recovered_ms: Option<u64>,
    pub(crate) budget_ms: u64,
    /// Everything about the episode was as it should be.
    pub(crate) ok: bool,
    /// The system recovered within the budget (or the run ended first).
    pub(crate) recovered: bool,
    pub(crate) note: String,
}

/// Which channels episodes have worked on, and when: lets a check tell a change that an
/// episode caused on purpose from one it should not have seen.
#[derive(Debug, Default)]
pub(crate) struct Targets {
    spans: Vec<Span>,
    next: u64,
}

#[derive(Debug)]
struct Span {
    id: u64,
    channel: String,
    from_ms: u64,
    to_ms: Option<u64>,
}

/// How long a finished span is kept.
const SPAN_KEPT_MS: u64 = 600_000;

impl Targets {
    /// An episode starts working on `channel`; returns the span to end later.
    pub(crate) fn begin(&mut self, channel: &str, now_ms: u64) -> u64 {
        self.next += 1;
        self.spans.push(Span {
            id: self.next,
            channel: channel.to_owned(),
            from_ms: now_ms,
            to_ms: None,
        });
        self.next
    }

    /// The episode of `span` is over.
    pub(crate) fn end(&mut self, span: u64, now_ms: u64) {
        if let Some(s) = self.spans.iter_mut().find(|s| s.id == span) {
            s.to_ms = Some(now_ms);
        }
        self.spans
            .retain(|s| s.to_ms.is_none_or(|to| to + SPAN_KEPT_MS > now_ms));
    }

    /// The channels some episode worked on at any time between `from_ms` and `to_ms`.
    pub(crate) fn touched(&self, from_ms: u64, to_ms: u64) -> std::collections::HashSet<String> {
        self.spans
            .iter()
            .filter(|s| s.from_ms <= to_ms && s.to_ms.is_none_or(|end| end >= from_ms))
            .map(|s| s.channel.clone())
            .collect()
    }
}

/// Who an episode was about and when it ran.
struct Header {
    id: u32,
    kind: Kind,
    target: String,
    start_ms: u64,
    end_ms: u64,
}

/// What an episode reports back to the runner.
pub(crate) struct Outcome {
    pub(crate) detail: String,
    pub(crate) recovered: Option<Duration>,
    pub(crate) budget: Duration,
    pub(crate) note: String,
    /// The engine misbehaved in a way the episode itself detected (it was also recorded
    /// as an anomaly).
    pub(crate) flagged: bool,
    pub(crate) owner: Owner,
}

impl Outcome {
    pub(crate) fn new(detail: impl Into<String>, budget: Duration) -> Self {
        Self {
            detail: detail.into(),
            recovered: None,
            budget,
            note: String::new(),
            flagged: false,
            owner: Owner::Core,
        }
    }

    pub(crate) fn recovered(mut self, within: Option<Duration>) -> Self {
        self.recovered = within;
        self
    }

    pub(crate) fn note(mut self, note: impl Into<String>) -> Self {
        let note = note.into();
        if self.note.is_empty() {
            self.note = note;
        } else {
            self.note = format!("{}; {note}", self.note);
        }
        self
    }

    pub(crate) fn owner(mut self, owner: Owner) -> Self {
        self.owner = owner;
        self
    }
}

/// The runner of the plan.
pub(crate) struct Chaos {
    rig: Arc<Rig>,
    /// Episodes share it; the two that kill the engine hold it alone.
    gate: RwLock<()>,
    /// Channels an episode is working on.
    busy: Mutex<HashSet<String>>,
    /// Config applies one at a time.
    apply: tokio::sync::Mutex<()>,
    ids: AtomicU32,
}

/// Runs `plan` against the rig; returns when the last episode ended.
pub(crate) async fn run(rig: Arc<Rig>, plan: Vec<Planned>) {
    let chaos = Arc::new(Chaos {
        rig: Arc::clone(&rig),
        gate: RwLock::new(()),
        busy: Mutex::new(HashSet::new()),
        apply: tokio::sync::Mutex::new(()),
        ids: AtomicU32::new(0),
    });
    let mut running = JoinSet::new();
    for planned in plan {
        let wait = Duration::from_millis(planned.at_ms.saturating_sub(rig.now_ms()));
        if !rig.env.stop.sleep(wait).await {
            break;
        }
        let chaos = Arc::clone(&chaos);
        running.spawn(async move { chaos.execute(planned).await });
    }
    while running.join_next().await.is_some() {}
}

impl Chaos {
    /// Channels of the kinds an episode of `kind` can act on.
    fn candidates(&self, kind: Kind, rng: &mut Rng) -> Vec<Arc<ChannelRt>> {
        let all = self.rig.channels.iter();
        let pick: Vec<Arc<ChannelRt>> = match kind {
            Kind::SlowFolderWrite => all
                .filter(|c| c.plan.kind == ChannelKind::Folder)
                .cloned()
                .collect(),
            Kind::AstmBackToBack => all
                .filter(|c| c.plan.kind == ChannelKind::Astm)
                .cloned()
                .collect(),
            Kind::ConfigApply | Kind::PauseResume => all.cloned().collect(),
            Kind::DestinationDown => {
                let bridges = rng.chance(15);
                all.filter(|c| {
                    c.plan.kind.is_bridge() == bridges && c.plan.kind != ChannelKind::Folder
                })
                .cloned()
                .collect()
            }
            _ => all
                .filter(|c| c.plan.kind != ChannelKind::Folder)
                .cloned()
                .collect(),
        };
        let busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        pick.into_iter()
            .filter(|c| !busy.contains(&c.plan.id))
            .collect()
    }

    fn release(&self, channel: &str) {
        self.busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(channel);
    }

    async fn execute(self: Arc<Self>, planned: Planned) {
        let kind = planned.kind;
        let (_shared, _alone) = if kind.exclusive() {
            (None, Some(self.gate.write().await))
        } else {
            (Some(self.gate.read().await), None)
        };
        if self.rig.env.stop.is_set() {
            return;
        }
        let mut rng = Rng::new(planned.seed);
        let target = if kind.exclusive() {
            None
        } else {
            let list = self.candidates(kind, &mut rng);
            if list.is_empty() {
                self.rig.log_chaos(
                    "episode_skipped",
                    json!({ "kind": kind.name(), "why": "no channel is free" }),
                );
                return;
            }
            let ch = Arc::clone(&list[rng.index(list.len())]);
            self.busy
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(ch.plan.id.clone());
            Some(ch)
        };
        self.run_one(planned, target, rng).await;
    }

    async fn run_one(&self, planned: Planned, target: Option<Arc<ChannelRt>>, mut rng: Rng) {
        let rig = &self.rig;
        let id = self.ids.fetch_add(1, Ordering::SeqCst) + 1;
        let name = target
            .as_ref()
            .map_or("engine", |c| c.plan.id.as_str())
            .to_owned();
        let start_ms = rig.now_ms();
        rig.active_episodes.fetch_add(1, Ordering::SeqCst);
        rig.log_chaos(
            "episode_started",
            json!({ "id": id, "kind": planned.kind.name(), "target": name, "planned_ms": planned.at_ms }),
        );
        let span = target.as_ref().map(|c| {
            rig.targets
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .begin(&c.plan.id, start_ms)
        });
        let outcome = self.perform(planned.kind, target.as_ref(), &mut rng).await;
        if let Some(span) = span {
            rig.targets
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .end(span, rig.now_ms());
        }
        rig.active_episodes.fetch_sub(1, Ordering::SeqCst);
        let end_ms = rig.now_ms();
        rig.last_episode_end_ms.store(end_ms, Ordering::SeqCst);
        if let Some(ch) = &target {
            self.release(&ch.plan.id);
        }
        let header = Header {
            id,
            kind: planned.kind,
            target: name,
            start_ms,
            end_ms,
        };
        self.finish(header, outcome);
    }

    fn finish(&self, header: Header, o: Outcome) {
        let interrupted = o.recovered.is_none() && self.rig.env.stop.is_set();
        let recovered = o.recovered.is_some() || interrupted;
        let ok = recovered && !o.flagged;
        let Header {
            id,
            kind,
            target,
            start_ms,
            end_ms,
        } = header;
        let episode = Episode {
            id,
            kind: kind.name(),
            target,
            detail: o.detail.clone(),
            start_ms,
            end_ms,
            recovered_ms: o
                .recovered
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
            budget_ms: u64::try_from(o.budget.as_millis()).unwrap_or(u64::MAX),
            ok,
            recovered,
            note: if interrupted && o.note.is_empty() {
                "interrupted by the end of the run".to_owned()
            } else {
                o.note.clone()
            },
        };
        self.rig.log_chaos(
            "episode_finished",
            json!({ "id": id, "kind": episode.kind, "target": episode.target, "detail": episode.detail,
                    "recovered_ms": episode.recovered_ms, "budget_ms": episode.budget_ms,
                    "ok": ok, "note": episode.note }),
        );
        if o.recovered.is_none() && !self.rig.env.stop.is_set() {
            self.rig.fail(
                "episode_not_recovered",
                o.owner,
                format!(
                    "{} on {} ({}) did not recover within {} s{}",
                    episode.kind,
                    episode.target,
                    episode.detail,
                    episode.budget_ms / 1000,
                    if o.note.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", o.note)
                    }
                ),
            );
        }
        self.rig
            .episodes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(episode);
    }

    async fn perform(&self, kind: Kind, target: Option<&Arc<ChannelRt>>, rng: &mut Rng) -> Outcome {
        let rig = &self.rig;
        match (kind, target) {
            (Kind::SourceDisconnect, Some(ch)) => peer::source_disconnect(rig, ch, rng).await,
            (Kind::SinkDisconnect, Some(ch)) => peer::sink_disconnect(rig, ch, rng).await,
            (Kind::SinkStall, Some(ch)) => peer::sink_stall(rig, ch, rng).await,
            (Kind::DestinationDown, Some(ch)) => peer::destination_down(rig, ch, rng).await,
            (Kind::SlowFolderWrite, Some(ch)) => peer::slow_folder_write(rig, ch).await,
            (Kind::AstmBackToBack, Some(ch)) => peer::astm_back_to_back(rig, ch).await,
            (Kind::ConfigApply, Some(ch)) => {
                let _one_at_a_time = self.apply.lock().await;
                engine::config_apply(rig, ch, rng).await
            }
            (Kind::PauseResume, Some(ch)) => engine::pause_resume(rig, ch, rng).await,
            (Kind::CrashRestart, _) => engine::crash_restart(rig, rng).await,
            (Kind::StoreBlock, _) => engine::store_block(rig, rng).await,
            (_, None) => Outcome::new("no target", Duration::ZERO),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_overlapping_the_window_are_touched() {
        let mut t = Targets::default();
        let early = t.begin("hl7-01", 1_000);
        t.end(early, 2_000);
        let running = t.begin("hl7-07", 5_000);
        let later = t.begin("bridge-03", 9_000);
        t.end(later, 9_500);
        let touched = t.touched(4_000, 9_200);
        let mut names: Vec<_> = touched.iter().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["bridge-03", "hl7-07"]);
        t.end(running, 12_000);
        assert!(!t.touched(3_000, 3_500).contains("hl7-01"));
        assert!(t.touched(1_500, 1_800).contains("hl7-01"));
    }

    #[test]
    fn old_spans_are_forgotten() {
        let mut t = Targets::default();
        let old = t.begin("hl7-01", 0);
        t.end(old, 10);
        let now = t.begin("hl7-02", 1_000_000);
        t.end(now, 1_000_100);
        assert!(!t.touched(0, 50).contains("hl7-01"));
    }
}
