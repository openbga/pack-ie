//! `init` writes JSON lines to a daily file and feeds the ring. One test per process,
//! because the subscriber is global.
#![cfg(test)]

use packie_core::ChannelId;
use packie_diagnostics::{LogFilter, LogOptions, init};

#[test]
fn writes_json_lines_and_fills_the_ring() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    // A file from an earlier run today is appended to, not replaced.
    std::fs::create_dir_all(&logs).unwrap();
    // Named exactly as the logger names today's file (local date, not UTC).
    std::fs::write(
        packie_diagnostics::first_log_file_today(&logs),
        "{\"earlier\":true}\n",
    )
    .unwrap();
    let channel = ChannelId::generate();
    let handle = init(&LogOptions {
        dir: logs.clone(),
        filter: "info".into(),
        retention_days: 3,
        max_file_bytes: 1024 * 1024,
        ring_capacity: 100,
        console: false,
    })
    .unwrap();
    {
        let span = tracing::info_span!("channel", channel_id = %channel);
        let _guard = span.enter();
        tracing::info!(event = "channel.started", "Channel started");
        tracing::debug!("filtered out");
    }
    let events = handle.ring().query(&LogFilter::default());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].channel_id, Some(channel));
    assert_eq!(events[0].event.as_deref(), Some("channel.started"));
    drop(handle);

    let files: Vec<_> = std::fs::read_dir(&logs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("packie."), "{name}");
    assert_eq!(files[0].extension().and_then(|e| e.to_str()), Some("log"));
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert_eq!(text.lines().next(), Some("{\"earlier\":true}"));
    let line: serde_json::Value = serde_json::from_str(text.lines().nth(1).unwrap()).unwrap();
    assert_eq!(line["message"], "Channel started");
    assert_eq!(line["event"], "channel.started");
    assert_eq!(line["spans"][0]["channel_id"], channel.to_string());
}
