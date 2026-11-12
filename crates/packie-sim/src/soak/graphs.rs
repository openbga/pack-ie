//! ASCII graphs of the sampled series, with the chaos episodes marked underneath.

use super::chaos::Episode;
use super::util::median;

/// Ramp from low to high.
const RAMP: &[u8] = b" .:-=+*#%@";

/// Reduces `values` to `width` buckets, each the largest value of its share of the series.
pub(crate) fn buckets(values: &[u64], width: usize) -> Vec<u64> {
    if values.is_empty() || width == 0 {
        return Vec::new();
    }
    let width = width.min(values.len());
    (0..width)
        .map(|i| {
            let from = i * values.len() / width;
            let to = ((i + 1) * values.len() / width).max(from + 1);
            values[from..to.min(values.len())]
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
        })
        .collect()
}

/// One character per bucket: the ramp scaled between the series' minimum and maximum.
pub(crate) fn sparkline(values: &[u64], width: usize) -> String {
    let b = buckets(values, width);
    let (Some(&low), Some(&high)) = (b.iter().min(), b.iter().max()) else {
        return String::new();
    };
    let span = (high - low).max(1);
    b.iter()
        .map(|v| {
            let level = (v - low) * (RAMP.len() as u64 - 1) / span;
            char::from(RAMP[usize::try_from(level).unwrap_or(0)])
        })
        .collect()
}

/// A row that marks where episodes of the engine kinds fall: `K` kill, `S` store block,
/// `D` destination down, `P` pause, `A` config apply.
pub(crate) fn markers(episodes: &[Episode], total_s: u64, width: usize) -> String {
    let mut row = vec![' '; width];
    if total_s == 0 || width == 0 {
        return String::new();
    }
    let mark = |kind: &str| match kind {
        "crash_restart" => Some(('K', 5)),
        "store_block" => Some(('S', 4)),
        "destination_down" => Some(('D', 3)),
        "pause_resume" => Some(('P', 2)),
        "config_apply" => Some(('A', 1)),
        _ => None,
    };
    let mut strength = vec![0_u8; width];
    for e in episodes {
        let Some((c, s)) = mark(e.kind) else { continue };
        let from = bucket_of(e.start_ms / 1000, total_s, width);
        let to = bucket_of(e.end_ms / 1000, total_s, width).max(from);
        for slot in from..=to {
            if strength[slot] < s {
                strength[slot] = s;
                row[slot] = c;
            }
        }
    }
    row.into_iter().collect()
}

fn bucket_of(second: u64, total_s: u64, width: usize) -> usize {
    let at = second.min(total_s.saturating_sub(1)) * width as u64 / total_s.max(1);
    usize::try_from(at).unwrap_or(0).min(width - 1)
}

/// A labelled graph: statistics, the sparkline and the marker row.
pub(crate) fn graph(
    label: &str,
    unit: &str,
    values: &[u64],
    episodes: &[Episode],
    total_s: u64,
) -> String {
    const WIDTH: usize = 72;
    if values.is_empty() {
        return format!("{label}: no samples\n");
    }
    let (min, max) = (
        values.iter().copied().min().unwrap_or(0),
        values.iter().copied().max().unwrap_or(0),
    );
    format!(
        "{label} ({unit}): min {min}, median {}, max {max}, first {}, last {}\n  |{}|\n  |{}|\n",
        median(values),
        values.first().copied().unwrap_or(0),
        values.last().copied().unwrap_or(0),
        sparkline(values, WIDTH),
        markers(episodes, total_s, values.len().min(WIDTH)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(kind: &'static str, start_s: u64, end_s: u64) -> Episode {
        Episode {
            id: 1,
            kind,
            target: String::new(),
            detail: String::new(),
            start_ms: start_s * 1000,
            end_ms: end_s * 1000,
            recovered_ms: Some(1),
            budget_ms: 1,
            ok: true,
            recovered: true,
            note: String::new(),
        }
    }

    #[test]
    fn a_rising_series_rises_on_the_ramp() {
        let line = sparkline(&(0..100).collect::<Vec<u64>>(), 10);
        assert_eq!(line.len(), 10);
        assert_eq!(line.chars().next(), Some(' '));
        assert_eq!(line.chars().last(), Some('@'));
    }

    #[test]
    fn a_flat_series_is_flat() {
        let line = sparkline(&[5; 40], 8);
        assert!(line.chars().all(|c| c == ' '));
    }

    #[test]
    fn short_series_are_not_stretched() {
        assert_eq!(sparkline(&[1, 2, 3], 72).len(), 3);
        assert!(sparkline(&[], 72).is_empty());
    }

    #[test]
    fn markers_fall_under_the_time_of_the_episode() {
        let row = markers(
            &[
                episode("crash_restart", 50, 55),
                episode("config_apply", 10, 11),
            ],
            100,
            10,
        );
        assert_eq!(row.chars().nth(5), Some('K'));
        assert_eq!(row.chars().nth(1), Some('A'));
        assert_eq!(row.trim().len(), 5);
    }

    #[test]
    fn a_graph_states_its_extremes() {
        let text = graph("RSS", "MiB", &[10, 20, 30], &[], 90);
        assert!(
            text.contains("min 10, median 20, max 30, first 10, last 30"),
            "{text}"
        );
    }
}
