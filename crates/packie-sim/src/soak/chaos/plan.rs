//! The chaos plan: which episodes happen when, drawn from the run's seed.
//!
//! The plan is made before the run starts and printed in the report, so a run can be
//! repeated from its seed. Each kind of episode has a mean interval per chaos level; every
//! kind that is enabled at the level happens at least once in a run that is long enough.
//!
//! | Level | Meaning |
//! | --- | --- |
//! | 0 | No chaos: a baseline of the steady load. |
//! | 1 | Rare, short episodes. |
//! | 2 | A fault every minute or two, a destination down for minutes, a config apply every few minutes. |
//! | 3 | Aggressive: roughly three times as often, longer stalls, a kill every 30 minutes. |

use std::time::Duration;

use crate::soak::util::Rng;

/// A kind of chaos episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    /// The instrument drops its connection in the middle of a message.
    SourceDisconnect,
    /// The receiving side drops its connection instead of acknowledging.
    SinkDisconnect,
    /// The receiving side stops reading.
    SinkStall,
    /// The destination does not listen for minutes.
    DestinationDown,
    /// One channel changes through a config apply.
    ConfigApply,
    /// A channel is paused and resumed.
    PauseResume,
    /// A file is written slowly into the folder channel.
    SlowFolderWrite,
    /// An ASTM transfer ends with EOT and the next ENQ in one write.
    AstmBackToBack,
    /// The engine is killed with SIGKILL and started again.
    CrashRestart,
    /// The engine is killed, the store is blocked by a file, the engine is started and the
    /// store is unblocked later.
    StoreBlock,
}

impl Kind {
    pub(crate) const ALL: [Self; 10] = [
        Self::SourceDisconnect,
        Self::SinkDisconnect,
        Self::SinkStall,
        Self::DestinationDown,
        Self::ConfigApply,
        Self::PauseResume,
        Self::SlowFolderWrite,
        Self::AstmBackToBack,
        Self::CrashRestart,
        Self::StoreBlock,
    ];

    /// The kind with this name.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::SourceDisconnect => "source_disconnect",
            Self::SinkDisconnect => "sink_disconnect",
            Self::SinkStall => "sink_stall",
            Self::DestinationDown => "destination_down",
            Self::ConfigApply => "config_apply",
            Self::PauseResume => "pause_resume",
            Self::SlowFolderWrite => "slow_folder_write",
            Self::AstmBackToBack => "astm_back_to_back",
            Self::CrashRestart => "crash_restart",
            Self::StoreBlock => "store_block",
        }
    }

    /// Episodes that need the engine to themselves.
    pub(crate) fn exclusive(self) -> bool {
        matches!(self, Self::CrashRestart | Self::StoreBlock)
    }

    /// Mean seconds between episodes at `level`; `None` when the kind is off.
    fn mean_secs(self, level: u8, restart_every: u64) -> Option<u64> {
        let by_level = |l1: u64, l2: u64, l3: u64| match level {
            1 => Some(l1),
            2 => Some(l2),
            3 => Some(l3),
            _ => None,
        };
        match self {
            Self::SourceDisconnect => by_level(400, 150, 60),
            Self::SinkDisconnect => by_level(600, 240, 90),
            Self::SinkStall => by_level(900, 360, 150),
            Self::DestinationDown => by_level(2400, 1200, 600),
            Self::ConfigApply => by_level(600, 240, 90),
            Self::PauseResume => by_level(1200, 480, 180),
            Self::SlowFolderWrite => by_level(1200, 600, 240),
            Self::AstmBackToBack => by_level(3600, 1800, 900),
            Self::CrashRestart => by_level(restart_every, restart_every, restart_every / 2),
            Self::StoreBlock => by_level(14_400, 7_200, 3_600),
        }
    }
}

/// One planned episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Planned {
    /// Run time at which it starts, milliseconds.
    pub(crate) at_ms: u64,
    pub(crate) kind: Kind,
    /// Seed of the choices the episode makes (target, length).
    pub(crate) seed: u64,
}

/// What the plan is made for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlanInput<'a> {
    /// When not empty, only these kinds of episode happen.
    pub(crate) only: &'a [Kind],
    pub(crate) level: u8,
    pub(crate) seed: u64,
    pub(crate) duration: Duration,
    /// No chaos before this run time.
    pub(crate) warmup: Duration,
    /// Period of the engine kill.
    pub(crate) restart_every: Duration,
}

/// The run time after which no episode starts: the system settles before the end.
pub(crate) fn tail(duration: Duration) -> Duration {
    (duration / 10).clamp(Duration::from_secs(180), Duration::from_secs(900))
}

/// Where in the usable span the first occurrence of an exclusive kind falls, percent.
fn exclusive_window(kind: Kind) -> (u64, u64) {
    match kind {
        Kind::CrashRestart => (20, 45),
        _ => (55, 70),
    }
}

/// Episodes in the order they start.
pub(crate) fn schedule(input: &PlanInput<'_>) -> Vec<Planned> {
    let mut plan = Vec::new();
    let start = input.warmup.as_secs();
    let usable = input
        .duration
        .saturating_sub(tail(input.duration))
        .as_secs();
    let span = usable.saturating_sub(start);
    if span < 30 {
        return plan;
    }
    for kind in Kind::ALL {
        if !input.only.is_empty() && !input.only.contains(&kind) {
            continue;
        }
        let Some(mean) = kind.mean_secs(input.level, input.restart_every.as_secs()) else {
            continue;
        };
        let mut rng = Rng::derive(input.seed, kind.name());
        // Exclusive episodes need minutes to finish: they must start well before the end.
        let last = if kind.exclusive() {
            span.saturating_sub((span * 15 / 100).min(1200))
        } else {
            span
        };
        let mut at = first_at(kind, mean.max(1), span, &mut rng);
        while at <= last {
            plan.push(Planned {
                at_ms: (start + at) * 1000,
                kind,
                seed: rng.next_u64(),
            });
            at += jitter(mean, kind, &mut rng);
        }
    }
    plan.sort_by_key(|p| (p.at_ms, p.kind));
    plan
}

/// When the first episode of `kind` starts, seconds after the warm-up.
fn first_at(kind: Kind, mean: u64, span: u64, rng: &mut Rng) -> u64 {
    if kind.exclusive() && mean >= span * 7 / 10 {
        let (low, high) = exclusive_window(kind);
        return rng.between(span * low / 100, span * high / 100);
    }
    let cap = (span * 7 / 10).max(10);
    rng.between(span / 10, mean.min(cap).max(span / 10))
}

/// Time to the next episode: the kill is periodic, the others are spread around the mean.
fn jitter(mean: u64, kind: Kind, rng: &mut Rng) -> u64 {
    if kind == Kind::CrashRestart {
        return mean;
    }
    rng.between(mean / 2, mean * 3 / 2).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(level: u8, minutes: u64) -> PlanInput<'static> {
        PlanInput {
            only: &[],
            level,
            seed: 42,
            duration: Duration::from_secs(minutes * 60),
            warmup: Duration::from_secs(minutes.min(66) * 9),
            restart_every: Duration::from_secs(3600),
        }
    }

    #[test]
    fn the_plan_is_fixed_by_the_seed() {
        assert_eq!(schedule(&input(2, 60)), schedule(&input(2, 60)));
        let other = PlanInput {
            seed: 43,
            ..input(2, 60)
        };
        assert_ne!(schedule(&input(2, 60)), schedule(&other));
    }

    #[test]
    fn only_the_chosen_kinds_are_planned() {
        let only = [Kind::SinkStall, Kind::PauseResume];
        let plan = schedule(&PlanInput {
            only: &only,
            ..input(3, 60)
        });
        assert!(!plan.is_empty());
        assert!(plan.iter().all(|p| only.contains(&p.kind)));
        assert_eq!(Kind::from_name("sink_stall"), Some(Kind::SinkStall));
        assert_eq!(Kind::from_name("nope"), None);
    }

    #[test]
    fn level_zero_has_no_chaos() {
        assert!(schedule(&input(0, 60)).is_empty());
    }

    #[test]
    fn every_kind_happens_in_an_hour_at_level_two() {
        let plan = schedule(&input(2, 60));
        for kind in Kind::ALL {
            assert!(
                plan.iter().any(|p| p.kind == kind),
                "{} is missing",
                kind.name()
            );
        }
    }

    #[test]
    fn the_plan_stays_between_warm_up_and_tail() {
        let i = input(3, 60);
        let plan = schedule(&i);
        let end = (i.duration - tail(i.duration)).as_millis() as u64;
        assert!(
            plan.iter()
                .all(|p| p.at_ms >= i.warmup.as_millis() as u64 && p.at_ms <= end)
        );
        assert!(plan.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
    }

    #[test]
    fn the_kill_comes_once_an_hour_and_the_store_block_after_it_in_short_runs() {
        let plan = schedule(&input(2, 60));
        let kills: Vec<_> = plan
            .iter()
            .filter(|p| p.kind == Kind::CrashRestart)
            .collect();
        let blocks: Vec<_> = plan.iter().filter(|p| p.kind == Kind::StoreBlock).collect();
        assert_eq!(kills.len(), 1);
        assert_eq!(blocks.len(), 1);
        assert!(kills[0].at_ms < blocks[0].at_ms);
    }

    #[test]
    fn long_runs_kill_every_hour() {
        let i = PlanInput {
            duration: Duration::from_secs(8 * 3600),
            warmup: Duration::from_secs(600),
            ..input(2, 480)
        };
        let kills = schedule(&i)
            .iter()
            .filter(|p| p.kind == Kind::CrashRestart)
            .count();
        assert!((7..=8).contains(&kills), "{kills}");
    }

    #[test]
    fn a_run_too_short_for_chaos_has_no_plan() {
        let i = PlanInput {
            duration: Duration::from_secs(120),
            warmup: Duration::from_secs(60),
            ..input(2, 2)
        };
        assert!(schedule(&i).is_empty());
    }
}
