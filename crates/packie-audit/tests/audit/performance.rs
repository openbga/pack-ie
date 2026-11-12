//! The cost of recording an event.

use std::time::{Duration, Instant};

use packie_audit::{Audit, Options};
use serde_json::json;

use crate::common::pause;

#[test]
#[ignore = "timing guard (acceptance M-08); run explicitly on an otherwise idle machine"]
fn recording_an_event_costs_under_a_millisecond_at_p99() {
    let dir = tempfile::tempdir().unwrap();
    let audit = Audit::open(dir.path(), Options::default()).unwrap();
    let entry = pause("lis").details(json!({ "config_version": 12, "note_length": 40 }));
    for _ in 0..200 {
        audit.record(&entry).unwrap();
    }
    let mut samples: Vec<Duration> = (0..5000)
        .map(|_| {
            let started = Instant::now();
            audit.record(&entry).unwrap();
            started.elapsed()
        })
        .collect();
    samples.sort();
    let p50 = samples[samples.len() / 2];
    let p99 = samples[samples.len() * 99 / 100];
    tracing::info!(?p50, ?p99, "audit record latency");
    assert!(p99 < Duration::from_millis(1), "p50 {p50:?}, p99 {p99:?}");
}
