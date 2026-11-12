//! Folder sources and sinks in a temporary directory.
#![cfg(test)]

mod common;

use std::time::Duration;

use packie_config::OnExists;
use packie_transport::Transports;
use serde_json::json;

use common::{endpoint, merged, names, transports};

fn folder(dir: &std::path::Path, extra: &serde_json::Value) -> packie_config::EndpointConfig {
    let base = json!({
        "kind": "folder", "path": dir, "stable_seconds": 1, "poll_interval_seconds": 1,
    });
    endpoint(merged(base, extra))
}

#[tokio::test(flavor = "multi_thread")]
async fn complete_files_are_units_and_are_moved_when_consumed() {
    let dir = tempfile::tempdir().unwrap();
    let config = folder(dir.path(), &json!({"file_pattern": "*.csv"}));
    let mut source = transports().file_source(&config).unwrap();

    std::fs::write(dir.path().join("ignored.txt"), "x").unwrap();
    std::fs::write(dir.path().join("partial.csv.tmp"), "x").unwrap();
    std::fs::write(dir.path().join("b.csv"), "sample_id\nS-2\n").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(dir.path().join("a.csv"), "sample_id\nS-1\n").unwrap();

    let started = std::time::Instant::now();
    let first = tokio::time::timeout(Duration::from_secs(10), source.next())
        .await
        .unwrap()
        .unwrap();
    // Not complete before `stable_seconds`; oldest modified first.
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert_eq!(first.name, "b.csv");
    assert_eq!(first.bytes, b"sample_id\nS-2\n");
    source.consumed(&first).await.unwrap();
    assert!(dir.path().join("processed/b.csv").exists());
    assert!(!dir.path().join("b.csv").exists());

    let second = tokio::time::timeout(Duration::from_secs(10), source.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.name, "a.csv");
}

/// Appends a character three times, 400 ms apart.
fn grow(path: &std::path::Path) {
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(400));
        let mut text = std::fs::read_to_string(path).unwrap();
        text.push('+');
        std::fs::write(path, text).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_growing_file_waits_and_left_files_are_not_returned_twice() {
    let dir = tempfile::tempdir().unwrap();
    let config = folder(dir.path(), &json!({"on_consumed": "leave", "mode": "poll"}));
    let mut source = transports().file_source(&config).unwrap();
    let path = dir.path().join("results.txt");
    std::fs::write(&path, "part 1").unwrap();

    let writer = std::thread::spawn({
        let path = path.clone();
        move || grow(&path)
    });
    let unit = tokio::time::timeout(Duration::from_secs(10), source.next())
        .await
        .unwrap()
        .unwrap();
    writer.join().unwrap();
    assert_eq!(unit.bytes, b"part 1+++");
    source.consumed(&unit).await.unwrap();
    assert!(path.exists());
    let again = tokio::time::timeout(Duration::from_millis(2500), source.next()).await;
    assert!(again.is_err(), "a left file came back");
}

#[tokio::test]
async fn sink_writes_atomically_with_on_exists() {
    let dir = tempfile::tempdir().unwrap();
    let sink = transports()
        .file_sink(&folder(dir.path(), &json!({})))
        .unwrap();
    assert_eq!(
        sink.write("orders.csv", b"one", OnExists::Suffix)
            .await
            .unwrap(),
        "orders.csv"
    );
    assert_eq!(
        sink.write("orders.csv", b"two", OnExists::Suffix)
            .await
            .unwrap(),
        "orders-1.csv"
    );
    let error = sink
        .write("orders.csv", b"x", OnExists::Fail)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "write failed: the output file already exists (on_exists fail)"
    );
    sink.write("orders.csv", b"+", OnExists::Append)
        .await
        .unwrap();
    sink.write("orders-1.csv", b"new", OnExists::Overwrite)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("orders.csv")).unwrap(),
        b"one+"
    );
    assert_eq!(
        std::fs::read(dir.path().join("orders-1.csv")).unwrap(),
        b"new"
    );
    let leftovers: Vec<_> = names(dir.path())
        .into_iter()
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert!(
        sink.write("../escape", b"x", OnExists::Fail)
            .await
            .unwrap_err()
            .persistent
    );
}

#[tokio::test]
async fn planted_temporaries_are_never_reused_or_followed() {
    let dir = tempfile::tempdir().unwrap();
    let victim = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(victim.path(), "precious").unwrap();
    // The names the previous design used as temporaries.
    std::fs::write(dir.path().join(".a.csv.part"), "planted").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(victim.path(), dir.path().join(".b.csv.part")).unwrap();

    let sink = transports()
        .file_sink(&folder(dir.path(), &json!({})))
        .unwrap();
    for name in ["a.csv", "b.csv"] {
        sink.write(name, b"new", OnExists::Fail).await.unwrap();
        let again = sink.write(name, b"new", OnExists::Fail).await.unwrap_err();
        assert_eq!(
            again.cause,
            "the output file already exists (on_exists fail)"
        );
        sink.write(name, b"newer", OnExists::Overwrite)
            .await
            .unwrap();
        assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), b"newer");
    }
    assert_eq!(std::fs::read(victim.path()).unwrap(), b"precious");
    assert_eq!(
        std::fs::read(dir.path().join(".a.csv.part")).unwrap(),
        b"planted"
    );
    let left: Vec<_> = names(dir.path())
        .into_iter()
        .filter(|n| n.starts_with('.'))
        .collect();
    #[cfg(unix)]
    assert_eq!(left, [".a.csv.part", ".b.csv.part"]);
    #[cfg(not(unix))]
    assert_eq!(left, [".a.csv.part"]);
}

type SharedSink = std::sync::Arc<Box<dyn packie_transport::FileSink>>;

fn body(n: usize) -> String {
    format!("writer {n}\n").repeat(5000)
}

/// Eight writers of `name` at the same time.
async fn race(
    sink: &SharedSink,
    name: &'static str,
    on_exists: OnExists,
) -> Vec<Result<String, packie_transport::LinkError>> {
    let tasks: Vec<_> = (0..8)
        .map(|n| {
            let sink = sink.clone();
            tokio::spawn(async move { sink.write(name, body(n).as_bytes(), on_exists).await })
        })
        .collect();
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await.unwrap());
    }
    results
}

fn is_one_intact_body(path: std::path::PathBuf) -> bool {
    let text = std::fs::read_to_string(path).unwrap();
    (0..8).any(|n| text == body(n))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writes_of_one_name_do_not_corrupt_each_other() {
    for mode in ["atomic_rename", "direct"] {
        let dir = tempfile::tempdir().unwrap();
        let config = folder(dir.path(), &json!({"write_mode": mode}));
        let sink: SharedSink = std::sync::Arc::new(transports().file_sink(&config).unwrap());

        // `suffix`: every writer gets a file of its own.
        let mut written: Vec<String> = race(&sink, "r.csv", OnExists::Suffix)
            .await
            .into_iter()
            .map(Result::unwrap)
            .collect();
        written.sort();
        written.dedup();
        assert_eq!(written.len(), 8, "{mode}: {written:?}");
        let mut contents: Vec<_> = written
            .iter()
            .map(|n| std::fs::read_to_string(dir.path().join(n)).unwrap())
            .collect();
        contents.sort();
        let mut expected: Vec<_> = (0..8).map(body).collect();
        expected.sort();
        assert_eq!(contents, expected, "{mode}");
        assert_eq!(names(dir.path()).len(), 8, "{mode}: no temporaries left");

        // `fail`: exactly one writer wins the name.
        let results = race(&sink, "only.csv", OnExists::Fail).await;
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1, "{mode}");
        assert!(is_one_intact_body(dir.path().join("only.csv")), "{mode}");

        // `overwrite`: all succeed and the file is one intact body.
        let results = race(&sink, "last.csv", OnExists::Overwrite).await;
        assert!(results.iter().all(Result::is_ok), "{mode}");
        assert!(is_one_intact_body(dir.path().join("last.csv")), "{mode}");
        assert_eq!(names(dir.path()).len(), 10, "{mode}: no temporaries left");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn writes_never_go_through_a_symlink() {
    for mode in ["atomic_rename", "direct"] {
        let dir = tempfile::tempdir().unwrap();
        let victim = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(victim.path(), "precious").unwrap();
        let link = dir.path().join("out.csv");
        std::os::unix::fs::symlink(victim.path(), &link).unwrap();
        let sink = transports()
            .file_sink(&folder(dir.path(), &json!({"write_mode": mode})))
            .unwrap();

        // Appending refuses the link in both modes.
        let error = sink
            .write("out.csv", b"+", OnExists::Append)
            .await
            .unwrap_err();
        assert!(error.persistent, "{mode}");
        assert!(error.cause.contains("symbolic link"), "{mode}: {error}");
        // `fail` and `suffix` see a taken name and leave the link alone.
        assert!(sink.write("out.csv", b"x", OnExists::Fail).await.is_err());
        assert_eq!(
            sink.write("out.csv", b"x", OnExists::Suffix).await.unwrap(),
            "out-1.csv"
        );
        assert_eq!(std::fs::read(victim.path()).unwrap(), b"precious", "{mode}");

        // `overwrite` never writes through it: direct mode refuses, atomic mode swaps the
        // link for a regular file.
        let overwritten = sink.write("out.csv", b"new", OnExists::Overwrite).await;
        assert_eq!(std::fs::read(victim.path()).unwrap(), b"precious", "{mode}");
        if mode == "direct" {
            assert!(overwritten.unwrap_err().persistent);
            assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        } else {
            overwritten.unwrap();
            assert!(link.symlink_metadata().unwrap().file_type().is_file());
            assert_eq!(std::fs::read(&link).unwrap(), b"new");
        }
    }
}

#[tokio::test]
async fn fail_and_suffix_never_replace_an_existing_file() {
    for mode in ["atomic_rename", "direct"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("keep.csv"), "original").unwrap();
        let sink = transports()
            .file_sink(&folder(dir.path(), &json!({"write_mode": mode})))
            .unwrap();
        let error = sink
            .write("keep.csv", b"new", OnExists::Fail)
            .await
            .unwrap_err();
        assert_eq!(
            error.cause, "the output file already exists (on_exists fail)",
            "{mode}"
        );
        assert_eq!(
            sink.write("keep.csv", b"new", OnExists::Suffix)
                .await
                .unwrap(),
            "keep-1.csv"
        );
        assert_eq!(
            std::fs::read(dir.path().join("keep.csv")).unwrap(),
            b"original",
            "{mode}"
        );
        assert_eq!(names(dir.path()), ["keep-1.csv", "keep.csv"], "{mode}");
    }
}

#[tokio::test]
async fn files_over_the_limit_are_returned_unread_and_consumed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.csv"), [b'x'; 50]).unwrap();
    let mut source = transports()
        .file_source(&folder(dir.path(), &json!({"mode": "poll"})))
        .unwrap();
    source.set_max_file_bytes(10);
    let unit = tokio::time::timeout(Duration::from_secs(10), source.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unit.name, "big.csv");
    assert_eq!(unit.too_large, Some(50));
    assert!(unit.bytes.is_empty());
    source.consumed(&unit).await.unwrap();
    assert!(dir.path().join("processed/big.csv").exists());

    // At the limit exactly, the file is read.
    std::fs::write(dir.path().join("fits.csv"), [b'y'; 10]).unwrap();
    let unit = tokio::time::timeout(Duration::from_secs(10), source.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unit.too_large, None);
    assert_eq!(unit.bytes, [b'y'; 10]);
}

/// File names may carry identifiers, so errors about them never repeat the name.
#[tokio::test]
async fn write_errors_do_not_repeat_the_file_name() {
    const SENTINEL: &str = "MRN-00000-SENTINEL";
    let name = format!("{SENTINEL}.csv");
    for mode in ["atomic_rename", "direct"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(&name), "original").unwrap();
        let sink = transports()
            .file_sink(&folder(dir.path(), &json!({"write_mode": mode})))
            .unwrap();
        let error = sink.write(&name, b"new", OnExists::Fail).await.unwrap_err();
        assert!(!error.to_string().contains(SENTINEL), "{mode}: {error}");
        let refused = sink
            .write(&format!("../{name}"), b"new", OnExists::Overwrite)
            .await
            .unwrap_err();
        assert!(!refused.to_string().contains(SENTINEL), "{mode}: {refused}");
    }
}
