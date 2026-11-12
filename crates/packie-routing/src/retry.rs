//! Exponential backoff with jitter (`docs/spec/configuration.md` §6.3).

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use packie_config::RetryPolicy;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Delay calculator for one channel's retry policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Backoff {
    initial_ms: u64,
    max_ms: u64,
    multiplier: Decimal,
    jitter_percent: u64,
}

impl Backoff {
    /// The calculator for `policy`.
    pub(crate) fn new(policy: &RetryPolicy) -> Self {
        let initial_ms = u64::from(policy.initial_delay_ms.max(1));
        Self {
            initial_ms,
            max_ms: u64::from(policy.max_delay_ms).max(initial_ms),
            multiplier: policy.multiplier.max(Decimal::ONE),
            jitter_percent: u64::from(policy.jitter_percent.min(100)),
        }
    }

    /// The longest delay, used while an endpoint is in `error`.
    pub(crate) fn max_delay(&self) -> Duration {
        Duration::from_millis(self.max_ms)
    }

    /// Delay before retry `n` (1-based) without jitter:
    /// `min(max_delay_ms, initial_delay_ms × multiplier^(n−1))`.
    pub(crate) fn base_delay(&self, n: u32) -> Duration {
        // Exact decimal arithmetic: a multiplier such as 1.5 never goes through floats.
        let max = Decimal::from(self.max_ms);
        let mut delay = Decimal::from(self.initial_ms);
        for _ in 1..n {
            match delay.checked_mul(self.multiplier) {
                Some(next) if next < max => delay = next,
                _ => {
                    delay = max;
                    break;
                }
            }
        }
        Duration::from_millis(delay.trunc().to_u64().unwrap_or(self.max_ms))
    }

    /// Delay before retry `n` after a failure: the longest delay when retrying alone
    /// will not fix it (`persistent`), else [`Self::delay`].
    pub(crate) fn after_failure(&self, persistent: bool, n: u32) -> Duration {
        if persistent {
            self.max_delay()
        } else {
            self.delay(n)
        }
    }

    /// Delay before retry `n`, scaled by a random factor within ±`jitter_percent`.
    pub(crate) fn delay(&self, n: u32) -> Duration {
        self.jittered(self.base_delay(n), random_u64())
    }

    /// `base` scaled by the jitter that `random` selects (uniform over ±`jitter_percent`).
    fn jittered(&self, base: Duration, random: u64) -> Duration {
        if self.jitter_percent == 0 {
            return base;
        }
        let base_ms = u64::try_from(base.as_millis()).unwrap_or(u64::MAX);
        let span = self.jitter_percent * 2 + 1;
        // Percent in 100 - jitter ..= 100 + jitter.
        let percent = 100 - self.jitter_percent + random % span;
        Duration::from_millis(base_ms.saturating_mul(percent) / 100)
    }
}

/// A random number from the standard library's per-process hash keys; enough for jitter.
fn random_u64() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    RandomState::new().hash_one(COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Operator text for a delay: `200 ms`, `5s`, `1.5s`, `2 min`.
pub(crate) fn describe_delay(delay: Duration) -> String {
    let ms = delay.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 120_000 {
        let tenths = (ms % 1000) / 100;
        if tenths == 0 {
            format!("{}s", ms / 1000)
        } else {
            format!("{}.{tenths}s", ms / 1000)
        }
    } else {
        format!("{} min", ms / 60_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(initial: u32, max: u32, multiplier: Decimal, jitter: u8) -> RetryPolicy {
        RetryPolicy {
            initial_delay_ms: initial,
            max_delay_ms: max,
            multiplier,
            jitter_percent: jitter,
            ..RetryPolicy::default()
        }
    }

    #[test]
    fn doubles_up_to_the_cap() {
        let backoff = Backoff::new(&policy(200, 1000, Decimal::from(2), 0));
        let delays: Vec<u128> = (1..=6).map(|n| backoff.delay(n).as_millis()).collect();
        assert_eq!(delays, [200, 400, 800, 1000, 1000, 1000]);
        assert_eq!(backoff.max_delay(), Duration::from_millis(1000));
    }

    #[test]
    fn multiplier_one_is_constant_and_huge_n_saturates() {
        let constant = Backoff::new(&policy(500, 60_000, Decimal::from(1), 0));
        assert_eq!(constant.delay(50), Duration::from_millis(500));
        let fast = Backoff::new(&policy(1000, 60_000, Decimal::from(10), 0));
        assert_eq!(fast.delay(u32::MAX), Duration::from_millis(60_000));
    }

    #[test]
    fn fractional_multipliers_are_exact() {
        let backoff = Backoff::new(&policy(1000, 60_000, Decimal::new(15, 1), 0));
        let delays: Vec<u128> = (1..=4).map(|n| backoff.delay(n).as_millis()).collect();
        assert_eq!(delays, [1000, 1500, 2250, 3375]);
    }

    #[test]
    fn jitter_stays_within_bounds_and_covers_both_ends() {
        let backoff = Backoff::new(&policy(1000, 60_000, Decimal::from(2), 20));
        let base = Duration::from_millis(1000);
        assert_eq!(backoff.jittered(base, 0), Duration::from_millis(800));
        assert_eq!(backoff.jittered(base, 40), Duration::from_millis(1200));
        for _ in 0..1000 {
            let ms = backoff.delay(1).as_millis();
            assert!((800..=1200).contains(&ms), "{ms}");
        }
    }

    #[test]
    fn describes_delays_for_operators() {
        assert_eq!(describe_delay(Duration::from_millis(200)), "200 ms");
        assert_eq!(describe_delay(Duration::from_secs(5)), "5s");
        assert_eq!(describe_delay(Duration::from_millis(1500)), "1.5s");
        assert_eq!(describe_delay(Duration::from_secs(300)), "5 min");
    }
}
