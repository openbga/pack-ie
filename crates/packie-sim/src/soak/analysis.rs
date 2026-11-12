//! Judging what the monitor recorded: resource growth, counter consistency and snapshot
//! timing. Pure functions over the samples, so they can be tested without an engine.

use super::samples::{ChanSample, Sample};
use super::util::median;

/// Allowed growth of the memory footprint between the start and the end of a stretch, percent.
pub(crate) const MEMORY_GROWTH_PERCENT: u64 = 20;
/// Allowed growth of open descriptors: this share of the start value, but at least
/// [`FD_GROWTH_FLOOR`].
pub(crate) const FD_GROWTH_PERCENT: u64 = 10;
/// Descriptors that connections coming and going may add.
pub(crate) const FD_GROWTH_FLOOR: u64 = 25;
/// A stretch needs this many samples after its warm-up to be judged.
pub(crate) const MIN_STRETCH: usize = 20;
/// Largest difference between the engine's counter and the peers' record, in messages.
pub(crate) const COUNTER_TOLERANCE: u64 = 10;
/// Bytes in flight between a bridge's counter and the peers.
pub(crate) const BYTE_SLACK: u64 = 256 * 1024;

/// How a value moved over a stretch of samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Growth {
    /// Median of the first tenth.
    pub(crate) early: u64,
    /// Median of the last tenth.
    pub(crate) late: u64,
    pub(crate) samples: usize,
}

impl Growth {
    /// Growth in percent of `early` (negative when it shrank), rounded down.
    pub(crate) fn percent(&self) -> i64 {
        if self.early == 0 {
            return 0;
        }
        let diff = i128::from(self.late) - i128::from(self.early);
        i64::try_from(diff * 100 / i128::from(self.early)).unwrap_or(0)
    }
}

/// Compares the first and the last tenth of `values`; `None` for fewer than
/// [`MIN_STRETCH`] values.
pub(crate) fn growth(values: &[u64]) -> Option<Growth> {
    if values.len() < MIN_STRETCH {
        return None;
    }
    let tenth = (values.len() / 10).max(3);
    Some(Growth {
        early: median(&values[..tenth]),
        late: median(&values[values.len() - tenth..]),
        samples: values.len(),
    })
}

/// One run of the engine process (between starts), after its warm-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stretch {
    pub(crate) incarnation: u32,
    pub(crate) from_s: u64,
    pub(crate) to_s: u64,
    pub(crate) memory_kib: Vec<u64>,
    pub(crate) fds: Vec<u64>,
}

/// The samples of each engine start, leaving out the first `warmup_s` seconds of the run
/// and of every start.
pub(crate) fn stretches(samples: &[Sample], warmup_s: u64, restart_warmup_s: u64) -> Vec<Stretch> {
    let mut out: Vec<Stretch> = Vec::new();
    let mut started: Option<(u32, u64)> = None;
    for s in samples.iter().filter(|s| s.up && s.memory_kib() > 0) {
        if started.is_none_or(|(inc, _)| inc != s.incarnation) {
            started = Some((s.incarnation, s.t_s));
        }
        let (_, since) = started.unwrap_or((0, 0));
        let warm = if s.incarnation <= 1 {
            warmup_s
        } else {
            since + restart_warmup_s
        };
        if s.t_s < warm {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.incarnation == s.incarnation => {
                last.to_s = s.t_s;
                last.memory_kib.push(s.memory_kib());
                last.fds.push(s.fds.total);
            }
            _ => out.push(Stretch {
                incarnation: s.incarnation,
                from_s: s.t_s,
                to_s: s.t_s,
                memory_kib: vec![s.memory_kib()],
                fds: vec![s.fds.total],
            }),
        }
    }
    out
}

/// A growth judgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Judged {
    pub(crate) what: String,
    pub(crate) growth: Option<Growth>,
    pub(crate) ok: bool,
}

/// Judges memory growth per stretch and across the whole run.
pub(crate) fn judge_memory(stretches: &[Stretch]) -> Vec<Judged> {
    judge(
        stretches,
        "footprint",
        |s| &s.memory_kib,
        |g| g.percent() <= i64::try_from(MEMORY_GROWTH_PERCENT).unwrap_or(20),
    )
}

/// Judges descriptor growth per stretch and across the whole run.
pub(crate) fn judge_fds(stretches: &[Stretch]) -> Vec<Judged> {
    judge(
        stretches,
        "fds",
        |s| &s.fds,
        |g| {
            let allowed = (g.early * FD_GROWTH_PERCENT / 100).max(FD_GROWTH_FLOOR);
            g.late <= g.early + allowed
        },
    )
}

fn judge(
    stretches: &[Stretch],
    name: &str,
    values: impl Fn(&Stretch) -> &Vec<u64>,
    within: impl Fn(&Growth) -> bool,
) -> Vec<Judged> {
    let mut out = Vec::new();
    for s in stretches {
        let g = growth(values(s));
        out.push(Judged {
            what: format!(
                "{name}, engine start {} ({}..{} s)",
                s.incarnation, s.from_s, s.to_s
            ),
            ok: g.as_ref().is_none_or(&within),
            growth: g,
        });
    }
    let all: Vec<u64> = stretches
        .iter()
        .flat_map(|s| values(s).iter().copied())
        .collect();
    let g = growth(&all);
    out.push(Judged {
        what: format!("{name}, whole run"),
        ok: g.as_ref().is_none_or(&within),
        growth: g,
    });
    out
}

/// Something the counters of a channel got wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CounterIssue {
    pub(crate) channel: String,
    pub(crate) from_s: u64,
    pub(crate) to_s: u64,
    pub(crate) what: String,
}

/// Running sums over one stretch of a channel in which the instance did not change.
#[derive(Default)]
struct Sums {
    from_s: u64,
    to_s: u64,
    received: u64,
    peer_acked: u64,
    acknowledged: u64,
    peer_received: u64,
    bytes_in: u64,
    peer_bytes_sent: u64,
    peer_bytes_received: u64,
}

fn same_instance(a: &ChanSample, b: &ChanSample) -> bool {
    a.incarnation == b.incarnation && a.instance == b.instance && !a.instance.is_empty()
}

fn backwards(a: &ChanSample, b: &ChanSample) -> Option<&'static str> {
    let pairs = [
        ("received", a.received, b.received),
        ("acknowledged", a.acknowledged, b.acknowledged),
        ("bytes_a_to_b", a.bytes_a_to_b, b.bytes_a_to_b),
        ("messages_a_to_b", a.messages_a_to_b, b.messages_a_to_b),
    ];
    pairs
        .iter()
        .find(|(_, x, y)| y < x)
        .map(|(name, _, _)| *name)
}

fn add(sums: &mut Sums, a: &ChanSample, b: &ChanSample) {
    sums.to_s = b.t_s;
    sums.received += b.received - a.received;
    sums.peer_acked += b.peer_acked - a.peer_acked;
    sums.acknowledged += b.acknowledged - a.acknowledged;
    sums.peer_received += b.peer_received - a.peer_received;
    sums.bytes_in += b.bytes_a_to_b - a.bytes_a_to_b;
    sums.peer_bytes_sent += b.peer_bytes_sent.saturating_sub(a.peer_bytes_sent);
    sums.peer_bytes_received += b.peer_bytes_received.saturating_sub(a.peer_bytes_received);
}

fn differ(a: u64, b: u64) -> u64 {
    a.abs_diff(b)
}

fn judge_sums(channel: &str, bridge: bool, sums: &Sums) -> Vec<CounterIssue> {
    let issue = |what: String| CounterIssue {
        channel: channel.to_owned(),
        from_s: sums.from_s,
        to_s: sums.to_s,
        what,
    };
    let mut out = Vec::new();
    if bridge {
        if sums.peer_bytes_received > sums.bytes_in + BYTE_SLACK {
            out.push(issue(format!(
                "the sink read {} bytes but the engine counted only {} bytes in",
                sums.peer_bytes_received, sums.bytes_in
            )));
        }
        if sums.bytes_in > sums.peer_bytes_sent + BYTE_SLACK {
            out.push(issue(format!(
                "the engine counted {} bytes in but the instrument wrote only {}",
                sums.bytes_in, sums.peer_bytes_sent
            )));
        }
        return out;
    }
    if differ(sums.received, sums.peer_acked) > COUNTER_TOLERANCE {
        out.push(issue(format!(
            "the engine counted {} messages received, the source peer got {} acknowledgements",
            sums.received, sums.peer_acked
        )));
    }
    if differ(sums.acknowledged, sums.peer_received) > COUNTER_TOLERANCE {
        out.push(issue(format!(
            "the engine counted {} deliveries acknowledged, the receiving peer got {} messages",
            sums.acknowledged, sums.peer_received
        )));
    }
    out
}

/// Compares the engine's counters with the peers' records, channel by channel, over every
/// stretch in which the channel instance did not change.
pub(crate) fn counter_issues(samples: &[ChanSample]) -> Vec<CounterIssue> {
    let mut channels: Vec<&str> = samples.iter().map(|s| s.channel.as_str()).collect();
    channels.sort_unstable();
    channels.dedup();
    let mut issues = Vec::new();
    for channel in channels {
        let rows: Vec<&ChanSample> = samples.iter().filter(|s| s.channel == channel).collect();
        issues.extend(channel_issues(channel, &rows));
    }
    issues
}

fn channel_issues(channel: &str, rows: &[&ChanSample]) -> Vec<CounterIssue> {
    let bridge = rows.first().is_some_and(|r| r.kind.contains("bridge"));
    let mut issues = Vec::new();
    let mut sums = Sums::default();
    let mut open = false;
    for pair in rows.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if !same_instance(a, b) {
            if open {
                issues.extend(judge_sums(channel, bridge, &sums));
            }
            open = false;
            continue;
        }
        if let Some(name) = backwards(a, b) {
            issues.push(CounterIssue {
                channel: channel.to_owned(),
                from_s: a.t_s,
                to_s: b.t_s,
                what: format!("the counter {name} went backwards within one channel instance"),
            });
            continue;
        }
        if !open {
            sums = Sums {
                from_s: a.t_s,
                ..Sums::default()
            };
            open = true;
        }
        add(&mut sums, a, b);
    }
    if open {
        issues.extend(judge_sums(channel, bridge, &sums));
    }
    issues
}

/// Snapshot publication timing over the whole run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SnapshotTiming {
    pub(crate) worst_age_p99_ms: u64,
    pub(crate) worst_gap_ms: u64,
    pub(crate) worst_rtt_ms: u64,
    pub(crate) windows: usize,
}

pub(crate) fn snapshot_timing(samples: &[Sample]) -> SnapshotTiming {
    let live = samples.iter().filter(|s| s.up && s.snapshots > 0);
    live.fold(SnapshotTiming::default(), |t, s| SnapshotTiming {
        worst_age_p99_ms: t.worst_age_p99_ms.max(s.age_p99_ms),
        worst_gap_ms: t.worst_gap_ms.max(s.gap_max_ms),
        worst_rtt_ms: t.worst_rtt_ms.max(s.rtt_ms),
        windows: t.windows + 1,
    })
}

#[cfg(test)]
mod tests {
    use super::super::probe::Fds;
    use super::*;

    fn sample(t_s: u64, incarnation: u32, rss: u64, fds: u64) -> Sample {
        Sample {
            t_s,
            incarnation,
            up: true,
            footprint_kib: rss,
            fds: Fds {
                total: fds,
                ..Fds::default()
            },
            ..Sample::default()
        }
    }

    fn series(incarnation: u32, from: u64, count: u64, rss: impl Fn(u64) -> u64) -> Vec<Sample> {
        (0..count)
            .map(|i| sample(from + i * 30, incarnation, rss(i), 100))
            .collect()
    }

    #[test]
    fn flat_memory_passes_and_steady_growth_fails() {
        let flat = stretches(&series(1, 0, 60, |_| 50_000), 0, 0);
        assert!(judge_memory(&flat).iter().all(|j| j.ok));
        let leaking = stretches(&series(1, 0, 60, |i| 50_000 + i * 400), 0, 0);
        let judged = judge_memory(&leaking);
        assert!(judged.iter().any(|j| !j.ok), "{judged:?}");
    }

    #[test]
    fn the_footprint_is_judged_and_the_resident_size_is_not() {
        // Resident size ratchets up with every burst; the footprint stays flat.
        let bursts: Vec<Sample> = (0..60)
            .map(|i| Sample {
                footprint_kib: 50_000,
                rss_kib: 50_000 + i * 2_000,
                ..sample(i * 30, 1, 0, 100)
            })
            .collect();
        assert!(judge_memory(&stretches(&bursts, 0, 0)).iter().all(|j| j.ok));
        // A footprint that grows is a leak whatever the resident size does.
        let leak: Vec<Sample> = (0..60)
            .map(|i| Sample {
                footprint_kib: 50_000 + i * 400,
                rss_kib: 90_000,
                ..sample(i * 30, 1, 0, 100)
            })
            .collect();
        assert!(judge_memory(&stretches(&leak, 0, 0)).iter().any(|j| !j.ok));
    }

    #[test]
    fn each_engine_start_is_judged_on_its_own() {
        let mut samples = series(1, 0, 40, |_| 50_000);
        samples.extend(series(2, 2_000, 40, |_| 51_000));
        let found = stretches(&samples, 0, 0);
        assert_eq!(found.len(), 2);
        assert!(
            judge_memory(&found).iter().all(|j| j.ok),
            "a restart is not growth"
        );
        let mut higher = series(1, 0, 40, |_| 50_000);
        higher.extend(series(2, 2_000, 40, |_| 80_000));
        let judged = judge_memory(&stretches(&higher, 0, 0));
        assert!(judged[..2].iter().all(|j| j.ok), "each start is flat");
        assert!(!judged[2].ok, "but the run as a whole grew by 60%");
    }

    #[test]
    fn warm_up_samples_are_left_out() {
        let samples = series(1, 0, 40, |i| if i < 10 { 20_000 } else { 50_000 });
        let found = stretches(&samples, 300, 0);
        assert!(found[0].memory_kib.iter().all(|r| *r == 50_000));
        assert!(judge_memory(&found).iter().all(|j| j.ok));
    }

    #[test]
    fn short_stretches_are_not_judged() {
        assert!(growth(&[1, 2, 3]).is_none());
    }

    #[test]
    fn descriptor_growth_has_a_floor() {
        let wobble = Stretch {
            incarnation: 1,
            from_s: 0,
            to_s: 600,
            memory_kib: vec![1; 30],
            fds: (0..30).map(|i| if i < 15 { 100 } else { 115 }).collect(),
        };
        assert!(judge_fds(&[wobble]).iter().all(|j| j.ok));
        let leak = Stretch {
            fds: (0..30).map(|i| 100 + i * 5).collect(),
            ..Stretch {
                incarnation: 1,
                from_s: 0,
                to_s: 600,
                memory_kib: vec![1; 30],
                fds: Vec::new(),
            }
        };
        assert!(judge_fds(&[leak]).iter().any(|j| !j.ok));
    }

    fn chan(t_s: u64, instance: &str, received: u64, acked: u64) -> ChanSample {
        ChanSample {
            t_s,
            incarnation: 1,
            channel: "hl7-01".to_owned(),
            kind: "hl7",
            instance: instance.to_owned(),
            received,
            peer_acked: acked,
            acknowledged: received,
            peer_received: acked,
            ..ChanSample::default()
        }
    }

    #[test]
    fn matching_counters_have_no_issue() {
        let rows: Vec<ChanSample> = (0..10)
            .map(|i| chan(i * 30, "a", i * 60, i * 60 + 1))
            .collect();
        assert!(counter_issues(&rows).is_empty());
    }

    #[test]
    fn a_counter_that_misses_messages_is_found() {
        let rows: Vec<ChanSample> = (0..10).map(|i| chan(i * 30, "a", i * 50, i * 60)).collect();
        let issues = counter_issues(&rows);
        assert!(!issues.is_empty());
        assert!(issues[0].what.contains("received"));
    }

    #[test]
    fn a_restarted_instance_starts_a_new_comparison() {
        let mut rows: Vec<ChanSample> = (0..5).map(|i| chan(i * 30, "a", i * 60, i * 60)).collect();
        rows.extend((5..10).map(|i| chan(i * 30, "b", (i - 5) * 60, i * 60)));
        assert!(counter_issues(&rows).is_empty());
    }

    #[test]
    fn a_counter_going_backwards_inside_an_instance_is_found() {
        let rows = vec![chan(0, "a", 100, 100), chan(30, "a", 90, 100)];
        let issues = counter_issues(&rows);
        assert!(issues[0].what.contains("backwards"));
    }

    #[test]
    fn bridge_bytes_must_fit_between_instrument_and_sink() {
        let row = |t, inb, sent, got| ChanSample {
            t_s: t,
            incarnation: 1,
            channel: "bridge-01".to_owned(),
            kind: "bridge",
            instance: "a".to_owned(),
            bytes_a_to_b: inb,
            peer_bytes_sent: sent,
            peer_bytes_received: got,
            ..ChanSample::default()
        };
        let fine = vec![row(0, 0, 0, 0), row(30, 300_000, 300_000, 299_000)];
        assert!(counter_issues(&fine).is_empty());
        let bad = vec![row(0, 0, 0, 0), row(30, 1_000_000, 300_000, 299_000)];
        assert_eq!(counter_issues(&bad).len(), 1);
    }

    #[test]
    fn snapshot_timing_ignores_windows_without_publications() {
        let mut a = sample(0, 1, 1, 1);
        a.snapshots = 30;
        a.age_p99_ms = 40;
        a.gap_max_ms = 1100;
        let b = sample(30, 1, 1, 1);
        let t = snapshot_timing(&[a, b]);
        assert_eq!(
            (t.worst_age_p99_ms, t.worst_gap_ms, t.windows),
            (40, 1100, 1)
        );
    }
}
