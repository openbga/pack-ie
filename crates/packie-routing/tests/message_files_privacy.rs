//! File names may carry identifiers (a destination `file_name` with `{sample_id}`, a
//! partner's source file name), so they never reach log events, snapshots or delivery error
//! summaries (`docs/spec/diagnostics.md` §8). One test per binary: it installs the global
//! log subscriber at debug level and greps everything the engine emitted.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use packie_core::DeliveryState;
use packie_engine::ChannelRegistry;
use serde_json::{Value, json};

/// The partner's source file name and the sample ids used in destination file names.
const SOURCE_SENTINEL: &str = "SENTINEL-NAME-7Q";
const SAMPLE_SENTINEL: &str = "SID-PRIV-";

fn channel() -> Value {
    json!({
        "kind": "message", "channel_id": "files", "label": "Files",
        "source": {"endpoint": "in-dir", "protocol": "delimited_file",
            "settings": {"columns": [{"name": "sample_id"}, {"name": "value", "type": "decimal"}]}},
        "destinations": [{
            "destination_id": "out", "label": "Out", "endpoint": "out-dir",
            "protocol": "delimited_file",
            "settings": {
                "file_name": "r-{sample_id}.csv", "on_exists": "fail",
                "columns": [{"name": "sample"}, {"name": "result"}],
            },
            "mapping": {"rules": [
                {"op": "copy", "from": "col.sample_id", "to": "col.sample"},
                {"op": "copy", "from": "col.value", "to": "col.result"},
            ]},
            "delivery": {"max_attempts": 1},
        }],
    })
}

#[tokio::test]
async fn file_names_never_reach_logs_snapshots_or_error_summaries() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    let log = packie_diagnostics::init(&packie_diagnostics::LogOptions {
        dir: logs.clone(),
        filter: "debug".into(),
        retention_days: 1,
        max_file_bytes: 1 << 24,
        ring_capacity: 10_000,
        console: false,
    })
    .unwrap();
    let (inbox, outbox) = folders(dir.path());

    let store = open_store(dir.path(), None);
    let mut h = Harness::with_options(options_with_store(system_transports(), &store));
    h.apply(document(
        vec![folder("in-dir", &inbox), folder("out-dir", &outbox)],
        vec![channel()],
    ))
    .await;
    std::fs::write(
        inbox.join(format!("{SOURCE_SENTINEL}.csv")),
        format!("sample_id,value\r\n{SAMPLE_SENTINEL}1,4.5\r\n{SAMPLE_SENTINEL}2,6\r\n"),
    )
    .unwrap();

    let deliveries = until("one delivered, one failed", || {
        let records = deliveries_of(records(&store));
        let states: Vec<_> = records.iter().map(|d| d.state).collect();
        (states.contains(&DeliveryState::Acknowledged) && states.contains(&DeliveryState::Failed))
            .then_some(records)
    })
    .await;
    assert!(outbox.join(format!("r-{SAMPLE_SENTINEL}1.csv")).exists());
    // Let the source retry the move once more, then flush the log files.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let view = h.registry.snapshot();
    let snapshot = serde_json::to_string(&(&view.channels, &view.endpoints)).unwrap();
    let errors = destination_errors(&view);
    assert_eq!(deliveries.len(), 2);
    h.registry.shutdown(Duration::from_millis(200)).await;
    drop(log);
    let emitted = read_logs(&logs);

    // The paths ran: a written file, a failed write, a failed move.
    assert!(emitted.contains("file.written"), "no file.written event");
    assert!(
        emitted.contains("file.consume_failed"),
        "no file.consume_failed event"
    );
    assert!(
        errors.iter().any(|e| e.contains("the file of delivery")),
        "the failed write's summary: {errors:?}"
    );
    for sentinel in [SOURCE_SENTINEL, SAMPLE_SENTINEL] {
        assert!(!emitted.contains(sentinel), "log events carry {sentinel}");
        assert!(
            !snapshot.contains(sentinel),
            "the snapshot carries {sentinel}"
        );
        for error in &errors {
            assert!(
                !error.contains(sentinel),
                "an error summary carries {sentinel}: {error}"
            );
        }
    }
}

/// The source and destination folders. The second row's destination file exists already
/// (`on_exists: fail`), so its write fails; `processed` is a file, so the committed source
/// file cannot be moved.
fn folders(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let inbox = root.join("in");
    let outbox = root.join("out");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&outbox).unwrap();
    std::fs::write(outbox.join(format!("r-{SAMPLE_SENTINEL}2.csv")), "taken\n").unwrap();
    std::fs::write(inbox.join("processed"), "not a directory\n").unwrap();
    (inbox, outbox)
}

/// The destinations' last errors: how a failed write's persisted summary reaches the operator.
fn destination_errors(view: &packie_engine::RegistrySnapshot) -> Vec<String> {
    view.channels
        .iter()
        .filter_map(|c| c.message.as_ref())
        .flat_map(|m| m.destinations.iter())
        .filter_map(|d| d.last_error.as_ref().map(|e| e.summary.clone()))
        .collect()
}

/// Everything written to the log files.
fn read_logs(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect()
}
