//! The SFTP file endpoint against an in-process SSH/SFTP server (`server`) on localhost.
//! Synthetic keys, user and password only.
#![cfg(test)]

#[path = "../common/mod.rs"]
mod common;
mod server;

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use packie_config::OnExists;
use packie_transport::{
    FileSink, FileSource, FileUnit, LinkError, MapSecrets, Operation, SystemTransports, Transports,
};
use russh::keys::ssh_key::LineEnding;
use serde_json::json;
use tokio::net::TcpListener;

use server::{PASSWORD, Server, fingerprint, key, transports};

fn read(path: impl AsRef<Path>) -> Vec<u8> {
    fs::read(path).unwrap()
}

async fn write(sink: &dyn FileSink, name: &str, bytes: &[u8], on_exists: OnExists) -> String {
    sink.write(name, bytes, on_exists).await.unwrap()
}

async fn write_error(
    sink: &dyn FileSink,
    name: &str,
    bytes: &[u8],
    on_exists: OnExists,
) -> LinkError {
    sink.write(name, bytes, on_exists).await.unwrap_err()
}

/// The next unit, failing the test if none arrives in `seconds`.
async fn next_within(source: &mut dyn FileSource, seconds: u64) -> FileUnit {
    tokio::time::timeout(Duration::from_secs(seconds), source.next())
        .await
        .expect("no file arrived in time")
        .unwrap()
}

/// Whether `source` stays silent for `seconds`.
async fn stays_silent(source: &mut dyn FileSource, seconds: u64) -> bool {
    tokio::time::timeout(Duration::from_secs(seconds), source.next())
        .await
        .is_err()
}

fn set_mtime(path: &Path, seconds: u64) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

#[tokio::test]
async fn password_upload_is_renamed_into_place() {
    let server = Server::start(&key(9)).await;
    let sink = server.sink(&json!({}));
    let name = write(&*sink, "r1.csv", b"id;value\n1;2\n", OnExists::Fail).await;
    assert_eq!(name, "r1.csv");
    assert_eq!(
        server.names("outbound"),
        ["r1.csv"],
        "no .part file is left"
    );
    assert_eq!(
        read(server.dir("outbound").join("r1.csv")),
        b"id;value\n1;2\n"
    );
}

#[tokio::test]
async fn existing_names_follow_on_exists() {
    let server = Server::start(&key(9)).await;
    let sink = server.sink(&json!({}));
    let out = |name: &str| read(server.dir("outbound").join(name));
    write(&*sink, "r.csv", b"one", OnExists::Suffix).await;
    assert_eq!(
        write(&*sink, "r.csv", b"two", OnExists::Suffix).await,
        "r-1.csv"
    );
    assert_eq!(
        write(&*sink, "r.csv", b"three", OnExists::Suffix).await,
        "r-2.csv"
    );

    let error = write_error(&*sink, "r.csv", b"four", OnExists::Fail).await;
    assert_eq!(error.operation, Operation::Write);
    assert!(!error.persistent);
    assert_eq!(
        error.cause,
        "the output file already exists (on_exists fail)"
    );

    assert_eq!(
        write(&*sink, "r.csv", b"new", OnExists::Overwrite).await,
        "r.csv"
    );
    assert_eq!(out("r.csv"), b"new");
    assert_eq!(
        write(&*sink, "r.csv", b"+more", OnExists::Append).await,
        "r.csv"
    );
    assert_eq!(out("r.csv"), b"new+more");
    assert_eq!(out("r-1.csv"), b"two");
    assert_eq!(server.names("outbound"), ["r-1.csv", "r-2.csv", "r.csv"]);
}

#[tokio::test]
async fn direct_mode_writes_the_final_name_and_never_replaces_without_asking() {
    let server = Server::start(&key(9)).await;
    let sink = server.sink(&json!({"write_mode": "direct"}));
    assert_eq!(
        write(&*sink, "d.csv", b"one", OnExists::Suffix).await,
        "d.csv"
    );
    assert_eq!(
        write(&*sink, "d.csv", b"two", OnExists::Suffix).await,
        "d-1.csv"
    );
    let error = write_error(&*sink, "d.csv", b"x", OnExists::Fail).await;
    assert_eq!(
        error.cause,
        "the output file already exists (on_exists fail)"
    );
    write(&*sink, "d.csv", b"x", OnExists::Overwrite).await;
    assert_eq!(read(server.dir("outbound").join("d.csv")), b"x");
    assert_eq!(server.names("outbound"), ["d-1.csv", "d.csv"]);
}

#[tokio::test]
async fn names_with_directories_are_refused_before_connecting() {
    let server = Server::start(&key(9)).await;
    let sink = server.sink(&json!({}));
    for name in ["sub/r.csv", "..\\r.csv", ""] {
        let error = write_error(&*sink, name, b"x", OnExists::Suffix).await;
        assert!(error.persistent, "{name}");
        assert_eq!(error.operation, Operation::Write);
    }
    assert!(server.names("outbound").is_empty());
}

#[tokio::test]
async fn a_wrong_host_key_fingerprint_is_a_persistent_error_showing_both() {
    let server = Server::start(&key(9)).await;
    let wrong = fingerprint(&key(8));
    let sink = server.sink(&json!({"host_key_fingerprint": wrong}));
    let error = write_error(&*sink, "r.csv", b"x", OnExists::Suffix).await;
    assert!(error.persistent);
    assert_eq!(error.operation, Operation::Connect);
    assert_eq!(
        error.cause,
        format!(
            "host key mismatch: expected {wrong}, got {}",
            server.fingerprint
        )
    );
    assert!(server.names("outbound").is_empty());
}

#[tokio::test]
async fn credential_and_network_failures_are_classified() {
    let server = Server::start(&key(9)).await;
    let endpoint = server.endpoint("/outbound", &json!({}));

    let rejected = transports("not-the-password").file_sink(&endpoint).unwrap();
    let error = write_error(&*rejected, "r.csv", b"x", OnExists::Suffix).await;
    assert!(error.persistent);
    assert_eq!(error.cause, "authentication rejected by the server");

    let no_secrets = SystemTransports::new(Arc::new(MapSecrets::default()));
    let missing = no_secrets.file_sink(&endpoint).unwrap();
    let error = write_error(&*missing, "r.csv", b"x", OnExists::Suffix).await;
    assert!(error.persistent);
    assert_eq!(error.operation, Operation::Connect);
    assert_eq!(error.cause, "secret sftp-pw is not set");

    let nowhere = server.endpoint("/does-not-exist", &json!({}));
    let sink = transports(PASSWORD).file_sink(&nowhere).unwrap();
    let error = write_error(&*sink, "r.csv", b"x", OnExists::Suffix).await;
    assert!(error.persistent, "{error}");
    assert_eq!(error.operation, Operation::Write);

    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let unreachable = server.endpoint("/outbound", &json!({"port": port}));
    let sink = transports(PASSWORD).file_sink(&unreachable).unwrap();
    let error = write_error(&*sink, "r.csv", b"x", OnExists::Suffix).await;
    assert!(!error.persistent, "{error}");
    assert_eq!(error.operation, Operation::Connect);
}

#[tokio::test]
async fn key_authentication_uses_the_secret_store() {
    let client = key(9);
    let server = Server::start(&client).await;
    let pem = client.to_openssh(LineEnding::LF).unwrap();
    let secrets = MapSecrets::new(HashMap::from([(
        "sftp-key".to_owned(),
        pem.as_str().to_owned(),
    )]));
    let auth = json!({"auth": {"method": "key", "private_key": {"secret": "sftp-key"}, "passphrase": null}});
    let endpoint = server.endpoint("/outbound", &auth);
    let sink = SystemTransports::new(Arc::new(secrets))
        .file_sink(&endpoint)
        .unwrap();
    assert_eq!(
        write(&*sink, "k.csv", b"key", OnExists::Fail).await,
        "k.csv"
    );
    assert_eq!(read(server.dir("outbound").join("k.csv")), b"key");

    let other = key(10).to_openssh(LineEnding::LF).unwrap();
    let secrets = MapSecrets::new(HashMap::from([(
        "sftp-key".to_owned(),
        other.as_str().to_owned(),
    )]));
    let sink = SystemTransports::new(Arc::new(secrets))
        .file_sink(&endpoint)
        .unwrap();
    let error = write_error(&*sink, "k.csv", b"x", OnExists::Suffix).await;
    assert!(error.persistent);
    assert_eq!(error.cause, "authentication rejected by the server");
}

#[tokio::test]
async fn a_complete_file_is_returned_and_consumed_into_processed() {
    let server = Server::start(&key(9)).await;
    let inbound = server.dir("inbound");
    let mut source = server.source(&json!({"stable_seconds": 1}));
    fs::write(inbound.join("r1.csv"), b"id;value\n1;2\n").unwrap();
    fs::write(inbound.join("r1.csv.tmp"), b"ignored").unwrap();
    set_mtime(&inbound.join("r1.csv"), 1_700_000_000);

    let unit = next_within(&mut *source, 10).await;
    assert_eq!(unit.name, "r1.csv");
    assert_eq!(unit.bytes, b"id;value\n1;2\n");
    assert_eq!(unit.modified.unix_millis(), 1_700_000_000_000);
    // Not returned again before it is consumed.
    assert!(stays_silent(&mut *source, 2).await);

    source.consumed(&unit).await.unwrap();
    assert_eq!(server.names("inbound"), ["processed", "r1.csv.tmp"]);
    assert_eq!(read(inbound.join("processed/r1.csv")), b"id;value\n1;2\n");

    // The same name arriving again is moved to a free name.
    fs::write(inbound.join("r1.csv"), b"second").unwrap();
    let again = next_within(&mut *source, 10).await;
    assert_eq!(again.bytes, b"second");
    source.consumed(&again).await.unwrap();
    assert_eq!(server.names("inbound/processed"), ["r1-1.csv", "r1.csv"]);
}

#[tokio::test]
async fn a_growing_file_is_returned_only_when_stable() {
    let server = Server::start(&key(9)).await;
    let path = server.dir("inbound").join("grow.csv");
    fs::write(&path, b"part0;").unwrap();
    let mut source = server.source(&json!({"stable_seconds": 1}));
    let writer = tokio::spawn(async move {
        for n in 1..=5 {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            write!(file, "part{n};").unwrap();
        }
    });
    let unit = next_within(&mut *source, 15).await;
    assert!(
        writer.is_finished(),
        "returned while the file was still growing"
    );
    assert_eq!(unit.bytes, b"part0;part1;part2;part3;part4;part5;");
}

#[tokio::test]
async fn oldest_first_recursive_and_the_processed_directory_is_skipped() {
    let server = Server::start(&key(9)).await;
    let inbound = server.dir("inbound");
    fs::create_dir(inbound.join("sub")).unwrap();
    fs::create_dir(inbound.join("processed")).unwrap();
    fs::write(inbound.join("a.csv"), b"a").unwrap();
    fs::write(inbound.join("b.csv"), b"b").unwrap();
    fs::write(inbound.join("sub/c.csv"), b"c").unwrap();
    fs::write(inbound.join("processed/old.csv"), b"old").unwrap();
    fs::write(inbound.join(".hidden.csv"), b"h").unwrap();
    fs::write(inbound.join("note.txt"), b"n").unwrap();
    set_mtime(&inbound.join("b.csv"), 1_700_000_000);
    set_mtime(&inbound.join("a.csv"), 1_700_000_100);
    set_mtime(&inbound.join("sub/c.csv"), 1_700_000_200);
    let mut source = server.source(&json!({"recursive": true, "file_pattern": "*.csv"}));

    // Each unit is returned without the previous one being consumed yet.
    let b = next_within(&mut *source, 10).await;
    let a = next_within(&mut *source, 10).await;
    let c = next_within(&mut *source, 10).await;
    assert_eq!(
        [&*b.name, &*a.name, &*c.name],
        ["b.csv", "a.csv", "sub/c.csv"]
    );
    for unit in [&b, &a, &c] {
        source.consumed(unit).await.unwrap();
    }
    assert_eq!(read(inbound.join("processed/sub/c.csv")), b"c");
    assert!(
        stays_silent(&mut *source, 2).await,
        "processed/old.csv must be skipped"
    );
}

#[tokio::test]
async fn delete_removes_and_leave_is_never_returned_again() {
    let server = Server::start(&key(9)).await;
    let inbound = server.dir("inbound");
    let mut deleting = server.source(&json!({"on_consumed": "delete"}));
    fs::write(inbound.join("d.csv"), b"d").unwrap();
    let unit = next_within(&mut *deleting, 10).await;
    deleting.consumed(&unit).await.unwrap();
    assert!(server.names("inbound").is_empty());

    let mut leaving = server.source(&json!({"on_consumed": "leave"}));
    fs::write(inbound.join("l.csv"), b"l").unwrap();
    let unit = next_within(&mut *leaving, 10).await;
    leaving.consumed(&unit).await.unwrap();
    assert_eq!(server.names("inbound"), ["l.csv"]);
    assert!(stays_silent(&mut *leaving, 2).await);

    // A changed file is a new file.
    fs::write(inbound.join("l.csv"), b"changed").unwrap();
    let unit = next_within(&mut *leaving, 10).await;
    assert_eq!(unit.bytes, b"changed");
}

#[tokio::test]
async fn a_source_reports_a_wrong_host_key() {
    let server = Server::start(&key(9)).await;
    let wrong = fingerprint(&key(8));
    let mut source = server.source(&json!({"host_key_fingerprint": wrong}));
    let error = source.next().await.unwrap_err();
    assert!(error.persistent);
    assert!(
        error
            .cause
            .starts_with("host key mismatch: expected SHA256:"),
        "{error}"
    );
}

#[tokio::test]
async fn planted_part_files_are_left_alone_and_uploads_use_their_own() {
    let server = Server::start(&key(9)).await;
    let outbound = server.dir("outbound");
    fs::write(outbound.join(".r.csv.part"), b"planted").unwrap();
    for mode in ["atomic_rename", "direct"] {
        let sink = server.sink(&json!({"write_mode": mode}));
        let name = format!("{mode}.csv");
        write(&*sink, &name, b"new", OnExists::Fail).await;
        write(&*sink, &name, b"newer", OnExists::Overwrite).await;
        assert_eq!(read(outbound.join(&name)), b"newer");
    }
    write(&*server.sink(&json!({})), "r.csv", b"new", OnExists::Fail).await;
    assert_eq!(read(outbound.join(".r.csv.part")), b"planted");
    assert_eq!(
        server.names("outbound"),
        [".r.csv.part", "atomic_rename.csv", "direct.csv", "r.csv"],
        "no upload temporaries are left"
    );
}

#[tokio::test]
async fn concurrent_uploads_of_one_name_each_get_a_file_of_their_own() {
    let server = Server::start(&key(9)).await;
    for mode in ["atomic_rename", "direct"] {
        let sink = server.sink(&json!({"write_mode": mode}));
        let bodies: Vec<String> = (0..4)
            .map(|n| format!("writer {n}\n").repeat(500))
            .collect();
        let upload = |n: usize| sink.write("c.csv", bodies[n].as_bytes(), OnExists::Suffix);
        let (a, b, c, d) = tokio::join!(upload(0), upload(1), upload(2), upload(3));
        let mut written: Vec<String> = [a, b, c, d].into_iter().map(Result::unwrap).collect();
        written.sort();
        written.dedup();
        assert_eq!(written.len(), 4, "{mode}: {written:?}");
        let mut contents: Vec<String> = written
            .iter()
            .map(|name| String::from_utf8(read(server.dir("outbound").join(name))).unwrap())
            .collect();
        contents.sort();
        let mut expected = bodies.clone();
        expected.sort();
        assert_eq!(contents, expected, "{mode}");
        assert_eq!(server.names("outbound").len(), 4, "{mode}: no temporaries");
        for name in written {
            fs::remove_file(server.dir("outbound").join(name)).unwrap();
        }
    }
}

#[tokio::test]
async fn files_over_the_limit_are_not_read_whatever_the_server_reports() {
    let server = Server::start(&key(9)).await;
    let inbound = server.dir("inbound");
    let mut source = server.source(&json!({}));
    source.set_max_file_bytes(10);

    // Reported over the limit by the listing and the handle: not read.
    fs::write(inbound.join("big.csv"), [b'x'; 50]).unwrap();
    let unit = next_within(&mut *source, 10).await;
    assert_eq!(unit.too_large, Some(50));
    assert!(unit.bytes.is_empty());
    source.consumed(&unit).await.unwrap();
    assert_eq!(server.names("inbound"), ["processed"]);

    // The handle reports size 0, but the read stops at the limit plus one.
    fs::write(inbound.join("liar.csv"), b"LIAR and a long body").unwrap();
    let unit = next_within(&mut *source, 10).await;
    assert_eq!(unit.too_large, Some(11));
    assert!(unit.bytes.is_empty());
    source.consumed(&unit).await.unwrap();

    // Within the limit the content is returned.
    fs::write(inbound.join("ok.csv"), [b'y'; 10]).unwrap();
    let unit = next_within(&mut *source, 10).await;
    assert_eq!(unit.too_large, None);
    assert_eq!(unit.bytes, [b'y'; 10]);
}

/// A log sink for the test binary: everything the transport logs, at every level.
#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// File names may carry sample or patient identifiers, so they never appear in the
/// transport's own log events or error texts (diagnostics.md §8).
#[tokio::test]
async fn file_names_never_reach_logs_or_errors() {
    const SENTINEL: &str = "MRN-00000-SENTINEL";
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    // Global, so events from any runtime thread are seen (the other tests of this binary log
    // into it too; none uses the sentinel). Call sites they hit first ask again.
    tracing::subscriber::set_global_default(subscriber).unwrap();
    tracing::callsite::rebuild_interest_cache();

    let server = Server::start(&key(9)).await;
    let name = format!("{SENTINEL}.csv");
    let sink = server.sink(&json!({}));
    write(&*sink, &name, b"one", OnExists::Fail).await;
    let error = write_error(&*sink, &name, b"two", OnExists::Fail).await;
    assert!(!error.to_string().contains(SENTINEL), "{error}");

    let inbound = server.dir("inbound");
    let mut source = server.source(&json!({"stable_seconds": 0}));
    fs::write(inbound.join(&name), b"id;value\n1;2\n").unwrap();
    let unit = next_within(&mut *source, 10).await;
    assert_eq!(
        unit.name, name,
        "the name is data for the caller, not for logs"
    );
    source.consumed(&unit).await.unwrap();

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("sftp.file_read"),
        "the events were captured: {logs}"
    );
    assert!(logs.contains("sftp.file_written"), "{logs}");
    assert!(logs.contains("sftp.file_consumed"), "{logs}");
    assert!(
        !logs.contains(SENTINEL),
        "a file name reached the logs:\n{logs}"
    );
}

fn secrets(password: &str) -> MapSecrets {
    MapSecrets::new(HashMap::from([("sftp-pw".to_owned(), password.to_owned())]))
}

async fn probe(server: &Server, extra: &serde_json::Value) -> packie_transport::ProbeReport {
    let config = server.endpoint("/inbound", extra);
    packie_transport::probe(&config, &secrets(PASSWORD), Duration::from_secs(10)).await
}

#[tokio::test]
async fn probe_signs_in_and_counts_without_writing_or_naming_files() {
    let server = Server::start(&key(9)).await;
    fs::write(server.dir("inbound").join("r1.csv"), "x").unwrap();
    fs::write(server.dir("inbound").join("r2.csv"), "x").unwrap();
    fs::write(server.dir("inbound").join("skip.txt"), "x").unwrap();
    fs::create_dir(server.dir("inbound").join("sub")).unwrap();
    let report = probe(&server, &json!({"file_pattern": "*.csv"})).await;
    assert!(report.ok, "{report:?}");
    let steps: Vec<_> = report.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        steps,
        [
            "credentials",
            "resolve",
            "connect",
            "authenticate",
            "open sftp",
            "list",
            "disconnect"
        ]
    );
    let list = &report.steps[5].detail;
    assert!(
        list.contains("2 files match *.csv, 1 sub-directory"),
        "{list}"
    );
    let text = format!("{report:?}");
    assert!(!text.contains("r1.csv") && !text.contains(PASSWORD));
    // Nothing was written or moved.
    assert_eq!(
        server.names("inbound"),
        ["r1.csv", "r2.csv", "skip.txt", "sub"]
    );
}

#[tokio::test]
async fn probe_reports_a_host_key_mismatch_with_both_fingerprints() {
    let server = Server::start(&key(9)).await;
    let wrong = fingerprint(&key(8));
    let report = probe(&server, &json!({"host_key_fingerprint": wrong})).await;
    assert!(!report.ok);
    let connect = report.steps.last().unwrap();
    assert_eq!(connect.name, "connect");
    assert!(connect.detail.contains(&wrong), "{connect:?}");
    assert!(connect.detail.contains(&server.fingerprint), "{connect:?}");
    assert!(report.steps.iter().all(|s| s.name != "authenticate"));
}

#[tokio::test]
async fn probe_reports_rejected_credentials_and_missing_directories() {
    let server = Server::start(&key(9)).await;
    let config = server.endpoint("/inbound", &json!({}));
    let report = packie_transport::probe(&config, &secrets("wrong"), Duration::from_secs(10)).await;
    assert!(!report.ok);
    let last = report.steps.last().unwrap();
    assert_eq!(last.name, "authenticate");
    assert!(last.detail.contains("authentication rejected"), "{last:?}");

    let config = server.endpoint("/no-such-dir", &json!({}));
    let report =
        packie_transport::probe(&config, &secrets(PASSWORD), Duration::from_secs(10)).await;
    let last = report.steps.last().unwrap();
    assert_eq!((last.name.as_str(), last.ok), ("disconnect", true));
    let list = report.steps.iter().find(|s| s.name == "list").unwrap();
    assert!(!list.ok);
    assert_eq!(list.detail, "remote path not found");
    assert!(!report.ok);
}

#[tokio::test]
async fn probe_without_the_secret_stops_before_the_network() {
    let server = Server::start(&key(9)).await;
    let config = server.endpoint("/inbound", &json!({}));
    let report =
        packie_transport::probe(&config, &MapSecrets::default(), Duration::from_secs(10)).await;
    assert!(!report.ok);
    assert_eq!(report.steps.len(), 1);
    assert_eq!(report.steps[0].name, "credentials");
}
