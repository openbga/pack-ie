//! Small shared pieces of the soak runner: the stop signal, the seeded random source and
//! time helpers.

use std::time::Duration;

use tokio::sync::watch;

/// The run-wide stop signal. Cloning shares it.
#[derive(Debug, Clone)]
pub(crate) struct Stop {
    tx: std::sync::Arc<watch::Sender<bool>>,
}

impl Stop {
    pub(crate) fn new() -> Self {
        Self {
            tx: std::sync::Arc::new(watch::channel(false).0),
        }
    }

    /// Asks every task to finish.
    pub(crate) fn set(&self) {
        self.tx.send_replace(true);
    }

    /// Whether the run is ending.
    pub(crate) fn is_set(&self) -> bool {
        *self.tx.borrow()
    }

    /// Sleeps `duration`; `false` when the stop signal ended the sleep early.
    pub(crate) async fn sleep(&self, duration: Duration) -> bool {
        let mut rx = self.tx.subscribe();
        tokio::select! {
            () = tokio::time::sleep(duration) => true,
            _ = rx.wait_for(|stopped| *stopped) => false,
        }
    }
}

/// SplitMix64: a small, fast generator whose sequence is fixed by the seed, so a chaos plan
/// can be replayed from the seed in the report.
#[derive(Debug, Clone)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// A generator for one task, derived from the run seed and a label.
    pub(crate) fn derive(seed: u64, label: &str) -> Self {
        let mut hash = seed ^ 0xcbf2_9ce4_8422_2325;
        for byte in label.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        Self(hash)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (0 when `n` is 0).
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }

    /// A value in `low..=high`.
    pub(crate) fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high.saturating_sub(low) + 1)
    }

    /// `true` with probability `percent` out of 100.
    pub(crate) fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// An index of a non-empty slice of length `len`.
    pub(crate) fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).unwrap_or(0)
    }
}

/// `1h02m03s`, `4m05s` or `6s`.
pub(crate) fn hms(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Milliseconds since the Unix epoch.
pub(crate) fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// An RFC 3339 time to the second for a Unix time in milliseconds.
pub(crate) fn utc(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(packie_core::Timestamp::from_unix_millis)
        .map_or_else(
            || "?".to_owned(),
            |t| {
                let text = t.to_string();
                format!("{}Z", text.get(..19).unwrap_or(&text))
            },
        )
}

/// The Unix time in milliseconds of an RFC 3339 `text`.
pub(crate) fn parse_utc_ms(text: &str) -> Option<u64> {
    let millis = packie_core::Timestamp::parse(text).ok()?.unix_millis();
    u64::try_from(millis).ok()
}

/// Nearest-rank percentile of `sorted` (ascending); 0 when empty.
pub(crate) fn percentile(sorted: &[u64], p: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * p).div_ceil(100).saturating_sub(1);
    sorted.get(rank.min(sorted.len() - 1)).copied().unwrap_or(0)
}

/// The median of `values` (0 when empty).
pub(crate) fn median(values: &[u64]) -> u64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    percentile(&sorted, 50)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_is_fixed_by_the_seed() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        assert_eq!(
            (0..5).map(|_| a.next_u64()).collect::<Vec<_>>(),
            (0..5).map(|_| b.next_u64()).collect::<Vec<_>>()
        );
        assert_ne!(Rng::new(7).next_u64(), Rng::new(8).next_u64());
    }

    #[test]
    fn derived_generators_differ_by_label() {
        assert_ne!(
            Rng::derive(1, "a").next_u64(),
            Rng::derive(1, "b").next_u64()
        );
    }

    #[test]
    fn between_stays_inside_its_bounds() {
        let mut rng = Rng::new(3);
        assert!((0..1000).all(|_| (5..=9).contains(&rng.between(5, 9))));
    }

    #[test]
    fn percentiles_and_medians() {
        assert_eq!(percentile(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 99), 10);
        assert_eq!(percentile(&[], 50), 0);
        assert_eq!(median(&[9, 1, 5]), 5);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(hms(3723), "1h02m03s");
        assert_eq!(hms(245), "4m05s");
        assert_eq!(hms(6), "6s");
    }

    #[tokio::test]
    async fn stop_ends_a_sleep() {
        let stop = Stop::new();
        let other = stop.clone();
        let task = tokio::spawn(async move { other.sleep(Duration::from_secs(30)).await });
        stop.set();
        assert!(!task.await.unwrap_or(true));
        assert!(stop.is_set());
    }
}
