#![cfg(test)]
//! Backup, verify and restore end to end.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use packie_audit::{Audit, Entry};
use packie_backup::{Error, Layout, create, restore, run_scheduled, status, verify};
use packie_config::{BackupSchedule, BackupSettings};
use packie_core::{
    Actor, ActorOrigin, ChannelId, ConnectorId, EndpointId, MessageDirection, MessageOrigin,
    MessageState, ProtocolKind, Timestamp,
};
use packie_ipc::audit::{AuditAction, AuditObjectType};
use packie_ipc::messages::MessageQuery;
use packie_store::{Identifiers, OpenOptions, Receipt, Store};

fn operator() -> Actor {
    Actor {
        name: "jdoe".into(),
        uid_or_sid: "501".into(),
        origin: ActorOrigin::Cli,
        pid: Some(4242),
    }
}

fn spilling() -> OpenOptions {
    OpenOptions {
        inline_body_cap: 16,
        ..OpenOptions::default()
    }
}

fn quiet() -> packie_audit::Options {
    packie_audit::Options {
        background: false,
        ..packie_audit::Options::default()
    }
}

struct State {
    _dir: tempfile::TempDir,
    root: PathBuf,
    layout: Layout,
}

fn state() -> State {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    let state_dir = root.join("state");
    fs::create_dir_all(state_dir.join("config")).unwrap();
    let document = br#"{"config_version": 1}"#;
    fs::write(state_dir.join("config/runtime.v0000000001.json"), document).unwrap();
    fs::write(state_dir.join("config/runtime.v0000000001.note"), b"first").unwrap();
    fs::write(state_dir.join("runtime.json"), document).unwrap();
    let layout = Layout {
        runtime_file: state_dir.join("runtime.json"),
        state_dir,
        bootstrap_file: None,
        node_id: "lab-1".into(),
        secret_files: Vec::new(),
    };
    State {
        _dir: dir,
        root,
        layout,
    }
}

fn receipt(channel: &str) -> Receipt {
    Receipt {
        channel_id: channel.to_owned(),
        channel_label: "Analyzer results".into(),
        source_connector_id: ConnectorId::generate().to_string(),
        source_endpoint_id: EndpointId::generate().to_string(),
        source_peer: None,
        source_file: None,
        protocol: ProtocolKind::Hl7,
        direction: MessageDirection::Inbound,
        origin: MessageOrigin::Received,
        initial_state: MessageState::Received,
        identifiers: Identifiers::default(),
        received_at: Timestamp::now(),
        config_version: 1,
        duplicate_detection: false,
        destinations: Vec::new(),
    }
}

fn body(i: usize) -> Vec<u8> {
    format!("MSH|^~\\&|ANALYZER|LAB|||{i:08}||ORU^R01|{i}|P|2.5.1\r").into_bytes()
}

async fn fill(store: &Store, from: usize, count: usize) {
    let channel = ChannelId::generate().to_string();
    for i in from..from + count {
        store
            .commit_received(receipt(&channel), body(i))
            .await
            .unwrap();
    }
}

fn pause(channel: &str) -> Entry {
    Entry::new(operator(), AuditAction::ChannelPause).object(AuditObjectType::Channel, channel)
}

/// Every message of the store at `dir` with its original body readable (spilled files are
/// checked against their content hash on read).
fn readable_messages(dir: &Path) -> u64 {
    let store = Store::open(dir, spilling()).unwrap();
    let list = store
        .query_messages(&MessageQuery {
            limit: 1000,
            ..MessageQuery::default()
        })
        .unwrap();
    for message in &list.messages {
        store.body(message.message_id, None).unwrap();
    }
    list.total
}

#[tokio::test]
async fn restore_brings_back_messages_bodies_configuration_and_the_audit_chain() {
    let s = state();
    let dest = s.root.join("backups/one");
    let manifest = {
        let store = Store::open(s.layout.store_dir(), spilling()).unwrap();
        let audit = Audit::open(s.layout.audit_dir(), quiet()).unwrap();
        fill(&store, 0, 25).await;
        for channel in ["lis", "au-1", "lis"] {
            audit.record(&pause(channel)).unwrap();
        }
        create(&s.layout, &dest, &operator(), Some(&store)).unwrap()
    };
    assert_eq!(manifest.store.as_ref().unwrap().messages, 25);
    assert_eq!(manifest.store.as_ref().unwrap().body_files, 25);
    assert_eq!(manifest.audit.as_ref().unwrap().head_seq, Some(3));
    assert_eq!(manifest.config_version, Some(1));
    assert_eq!(manifest.actor, operator());
    let check = verify(&dest).unwrap();
    assert!(check.ok, "{:?}", check.problems);
    assert_eq!(check.files_checked as usize, manifest.files.len());

    // Lose the store, a body and the audit log.
    let store_dir = s.layout.store_dir();
    fs::write(store_dir.join("messages.sqlite3"), b"garbage").unwrap();
    let some_body = fs::read_dir(store_dir.join("bodies"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::remove_dir_all(&some_body).unwrap();
    fs::remove_dir_all(s.layout.audit_dir()).unwrap();
    fs::write(&s.layout.runtime_file, b"{}").unwrap();

    let restored = restore(&dest, &s.layout, &operator()).unwrap();
    assert_eq!(readable_messages(&store_dir), 25);
    assert_eq!(
        fs::read(&s.layout.runtime_file).unwrap(),
        br#"{"config_version": 1}"#
    );
    assert!(
        s.layout
            .config_dir()
            .join("runtime.v0000000001.note")
            .is_file()
    );
    assert!(
        restored
            .pre_restore
            .join("store/messages.sqlite3")
            .is_file()
    );
    assert_eq!(
        fs::read(restored.pre_restore.join("runtime.json")).unwrap(),
        b"{}"
    );

    let chain = packie_audit::verify_dir(&s.layout.audit_dir()).unwrap();
    assert!(chain.ok, "{chain:?}");
    let audit = Audit::open(s.layout.audit_dir(), quiet()).unwrap();
    let newest = &audit
        .query(&packie_ipc::audit::AuditQuery::default())
        .unwrap()
        .events;
    let event = newest
        .iter()
        .find(|e| e.action == AuditAction::BackupRestore)
        .unwrap();
    assert_eq!(event.seq, restored.audit_seq);
    assert_eq!(event.actor_name, "jdoe");
    assert_eq!(event.details["messages"], 25);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_backup_taken_under_write_load_restores_consistently() {
    let s = state();
    let store = Store::open(s.layout.store_dir(), spilling()).unwrap();
    let audit = Audit::open(s.layout.audit_dir(), quiet()).unwrap();
    fill(&store, 0, 20).await;
    let running = Arc::new(AtomicBool::new(true));
    let writers: Vec<_> = (0..3)
        .map(|w| tokio::spawn(write_until(store.clone(), Arc::clone(&running), w)))
        .collect();
    let auditor = {
        let (audit, running) = (audit.clone(), Arc::clone(&running));
        std::thread::spawn(move || record_until(&audit, &running))
    };
    let dest = s.root.join("backups/live");
    let (layout, target, snapshot) = (s.layout.clone(), dest.clone(), store.clone());
    let manifest =
        tokio::task::spawn_blocking(move || create(&layout, &target, &operator(), Some(&snapshot)))
            .await
            .unwrap()
            .unwrap();
    running.store(false, Ordering::Release);
    for writer in writers {
        writer.await.unwrap();
    }
    assert!(auditor.join().unwrap() > 0);
    let check = verify(&dest).unwrap();
    assert!(check.ok, "{:?}", check.problems);
    assert!(check.audit.unwrap().ok);

    // Restore into another computer's state directory.
    let other = state();
    let restored = restore(&dest, &other.layout, &operator()).unwrap();
    let messages = manifest.store.unwrap().messages;
    assert!(messages >= 20);
    assert_eq!(readable_messages(&other.layout.store_dir()), messages);
    assert_eq!(
        restored.audit_seq,
        manifest.audit.unwrap().head_seq.unwrap() + 1
    );
}

fn record_until(audit: &Audit, running: &AtomicBool) -> u32 {
    let mut n = 0;
    while running.load(Ordering::Acquire) {
        audit.record(&pause("lis")).unwrap();
        n += 1;
    }
    n
}

async fn write_until(store: Store, running: Arc<AtomicBool>, worker: usize) {
    let channel = ChannelId::generate().to_string();
    let mut i = 10_000 * (worker + 1);
    while running.load(Ordering::Acquire) {
        store
            .commit_received(receipt(&channel), body(i))
            .await
            .unwrap();
        i += 1;
    }
}

#[tokio::test]
async fn an_offline_backup_reads_the_closed_store() {
    let s = state();
    {
        let store = Store::open(s.layout.store_dir(), spilling()).unwrap();
        fill(&store, 0, 5).await;
        Audit::open(s.layout.audit_dir(), quiet())
            .unwrap()
            .record(&pause("lis"))
            .unwrap();
    }
    let dest = s.root.join("offline");
    let manifest = create(&s.layout, &dest, &operator(), None).unwrap();
    assert_eq!(manifest.store.unwrap().messages, 5);
    // The event plus the anchor written when the log closed.
    assert_eq!(manifest.audit.unwrap().head_seq, Some(2));
    assert!(verify(&dest).unwrap().ok);
}

/// A verified backup (with an audit log) in a fresh state directory.
fn verified_backup() -> (State, PathBuf) {
    let s = state();
    Audit::open(s.layout.audit_dir(), quiet())
        .unwrap()
        .record(&pause("lis"))
        .unwrap();
    let dest = s.root.join("b");
    create(&s.layout, &dest, &operator(), None).unwrap();
    assert!(verify(&dest).unwrap().ok);
    (s, dest)
}

#[test]
fn verification_finds_changed_missing_extra_and_incomplete_backups() {
    type Tamper = fn(&Path);
    let cases: [(&str, Tamper, &str); 5] = [
        (
            "changed",
            |d| fs::write(d.join("runtime.json"), b"{\"edited\": true}").unwrap(),
            "runtime.json changed",
        ),
        (
            "extra",
            |d| fs::write(d.join("notes.txt"), b"extra").unwrap(),
            "notes.txt is not listed",
        ),
        (
            "missing",
            |d| fs::remove_file(d.join("config/runtime.v0000000001.note")).unwrap(),
            "note is missing",
        ),
        (
            "incomplete",
            |d| fs::remove_file(d.join("manifest.json")).unwrap(),
            "incomplete",
        ),
        (
            "audit gone",
            |d| fs::remove_dir_all(d.join("audit")).unwrap(),
            "the audit copy holds no events; the manifest recorded event 2 as the last",
        ),
    ];
    for (case, tamper, expected) in cases {
        let (_s, dest) = verified_backup();
        tamper(&dest);
        let check = verify(&dest).unwrap();
        assert!(!check.ok, "{case}");
        assert!(
            check.problems.iter().any(|p| p.contains(expected)),
            "{case}: {:?}",
            check.problems
        );
    }
}

#[test]
fn restore_refuses_while_an_engine_runs_and_refuses_a_bad_backup() {
    let s = state();
    let dest = s.root.join("b");
    create(&s.layout, &dest, &operator(), None).unwrap();
    {
        let _engine = packie_host::InstanceLock::acquire(
            &s.layout.state_dir,
            &s.layout.state_dir.join("bootstrap.json"),
        )
        .unwrap();
        let refused = restore(&dest, &s.layout, &operator()).unwrap_err();
        assert!(matches!(refused, Error::EngineRunning { .. }), "{refused}");
    }
    fs::write(dest.join("runtime.json"), b"tampered").unwrap();
    let refused = restore(&dest, &s.layout, &operator()).unwrap_err();
    assert!(matches!(refused, Error::NotVerified(_)), "{refused}");
    // Nothing was moved aside.
    assert!(s.layout.config_dir().is_dir());
}

#[test]
fn a_backup_needs_a_new_or_empty_absolute_directory() {
    let s = state();
    assert!(create(&s.layout, Path::new("relative"), &operator(), None).is_err());
    let busy = s.root.join("busy");
    fs::create_dir(&busy).unwrap();
    fs::write(busy.join("x"), b"x").unwrap();
    assert!(create(&s.layout, &busy, &operator(), None).is_err());
    assert!(
        busy.join("x").is_file(),
        "an existing directory is left alone"
    );
    let empty = s.root.join("empty");
    fs::create_dir(&empty).unwrap();
    create(&s.layout, &empty, &operator(), None).unwrap();
    assert!(verify(&empty).unwrap().ok);
}

#[test]
fn the_schedule_keeps_the_newest_backups_and_the_status_tracks_staleness() {
    let s = state();
    let settings = BackupSettings {
        directory: Some(s.root.join("scheduled")),
        schedule: Some(BackupSchedule {
            daily_at: "02:30".into(),
            keep: 2,
        }),
        stale_after_hours: None,
    };
    let started = Timestamp::now();
    let fresh = status(&s.layout, &settings, started, started);
    assert!(!fresh.stale);
    assert_eq!(fresh.stale_after_hours, Some(26));
    assert!(fresh.schedule.unwrap().next_at.is_some());
    let later = started.checked_add(Duration::from_hours(27)).unwrap();
    assert!(
        status(&s.layout, &settings, started, later).stale,
        "never backed up"
    );

    for _ in 0..3 {
        run_scheduled(&s.layout, &settings, &Actor::system(), None).unwrap();
        // Scheduled directory names have one-second resolution.
        std::thread::sleep(Duration::from_millis(1100));
    }
    assert_eq!(fs::read_dir(s.root.join("scheduled")).unwrap().count(), 2);
    let report = status(&s.layout, &settings, started, Timestamp::now());
    assert!(!report.stale);
    let last = report.last_success.unwrap();
    assert!(last.ok);
    assert!(last.summary.unwrap().files > 0);
    assert_eq!(report.last_attempt.unwrap().at, last.at);
}

#[test]
fn a_secrets_file_kept_with_the_configuration_never_enters_a_backup() {
    let mut s = state();
    let config = s.layout.config_dir();
    let secrets = config.join("secrets.json");
    fs::write(&secrets, br#"{"sftp-password": "hunter2"}"#).unwrap();
    // The bootstrap names it by another spelling of the same path.
    s.layout.secret_files = vec![config.join("..").join("config").join("secrets.json")];
    let dest = s.root.join("b");
    let manifest = create(&s.layout, &dest, &operator(), None).unwrap();
    assert!(!dest.join("config/secrets.json").exists());
    assert!(
        manifest
            .files
            .iter()
            .all(|f| !f.path.ends_with("secrets.json")),
        "{:?}",
        manifest.files
    );
    assert!(
        dest.join("config/runtime.v0000000001.json").is_file(),
        "history is kept"
    );
    assert_eq!(manifest.config_version, Some(1));
    let found: Vec<_> = walk_all(&dest)
        .into_iter()
        .filter(|path| fs::read(path).unwrap().windows(7).any(|w| w == b"hunter2"))
        .collect();
    assert!(found.is_empty(), "secret value in {found:?}");
    assert!(verify(&dest).unwrap().ok);

    // Also when the runtime document setting points at the secrets file.
    s.layout.runtime_file = secrets;
    let other = s.root.join("c");
    create(&s.layout, &other, &operator(), None).unwrap();
    assert!(!other.join("runtime.json").exists());
}

fn walk_all(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

#[cfg(unix)]
#[test]
fn links_and_special_files_in_a_backup_fail_verification_and_restore() {
    let (s, dest) = verified_backup();
    let outside = s.root.join("outside.txt");
    fs::write(&outside, b"not part of the backup").unwrap();

    // An unlisted link, as the review found: it used to pass an unchanged manifest.
    std::os::unix::fs::symlink(&outside, dest.join("config/planted")).unwrap();
    let check = verify(&dest).unwrap();
    assert!(!check.ok);
    assert!(
        check
            .problems
            .iter()
            .any(|p| p.contains("config/planted is a symbolic link or special file")),
        "{:?}",
        check.problems
    );
    let refused = restore(&dest, &s.layout, &operator()).unwrap_err();
    assert!(matches!(refused, Error::NotVerified(_)), "{refused}");
    assert!(
        s.layout
            .config_dir()
            .join("runtime.v0000000001.json")
            .is_file(),
        "nothing was moved aside"
    );
    fs::remove_file(dest.join("config/planted")).unwrap();

    // A FIFO, which would also have stalled a copy.
    nix::unistd::mkfifo(&dest.join("config/pipe"), nix::sys::stat::Mode::S_IRWXU).unwrap();
    let problems = verify(&dest).unwrap().problems;
    assert!(
        problems
            .iter()
            .any(|p| p.contains("config/pipe is a symbolic link or special file")),
        "{problems:?}"
    );
    fs::remove_file(dest.join("config/pipe")).unwrap();

    // A listed file replaced by a link to identical bytes: the hash matches, the link
    // does not.
    let listed = dest.join("config/runtime.v0000000001.json");
    let copy = s.root.join("same-bytes.json");
    fs::copy(&listed, &copy).unwrap();
    fs::remove_file(&listed).unwrap();
    std::os::unix::fs::symlink(&copy, &listed).unwrap();
    let problems = verify(&dest).unwrap().problems;
    assert!(
        problems
            .iter()
            .any(|p| p.contains("config/runtime.v0000000001.json is a symbolic link")),
        "{problems:?}"
    );
    assert!(restore(&dest, &s.layout, &operator()).is_err());

    // A linked directory hides its contents from the walk but is named itself.
    fs::remove_file(&listed).unwrap();
    fs::copy(&copy, &listed).unwrap();
    assert!(verify(&dest).unwrap().ok);
    std::os::unix::fs::symlink(s.layout.config_dir(), dest.join("linked-dir")).unwrap();
    let problems = verify(&dest).unwrap().problems;
    assert!(
        problems
            .iter()
            .any(|p| p.contains("linked-dir is a symbolic link")),
        "{problems:?}"
    );
}

/// A bootstrap whose file-provider secrets live under `<state>/config`.
fn with_secrets_in_config(s: &State) -> (PathBuf, PathBuf) {
    let secrets = s.layout.config_dir().join("secrets.json");
    fs::write(&secrets, br#"{"sftp-password": "current"}"#).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&secrets, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let bootstrap = s.root.join("bootstrap.json");
    fs::write(
        &bootstrap,
        r#"{"node": {"node_id": "lab-1", "display_name": "Lab"},
            "paths": {"state_directory": "state"},
            "ipc": {"group": null},
            "secrets": {"provider": "file", "file": "state/config/secrets.json"}}"#,
    )
    .unwrap();
    (bootstrap, secrets)
}

#[test]
fn a_restore_keeps_the_secrets_file_the_bootstrap_keeps_in_config() {
    let s = state();
    let (bootstrap, secrets) = with_secrets_in_config(&s);
    let layout = Layout::of(
        &packie_config::Bootstrap::load(&bootstrap).unwrap(),
        &bootstrap,
    );
    assert_eq!(layout.secret_files, std::slice::from_ref(&secrets));
    let dest = s.root.join("b");
    create(&layout, &dest, &operator(), None).unwrap();
    assert!(!dest.join("config/secrets.json").exists());

    let restored = restore(&dest, &layout, &operator()).unwrap();
    assert_eq!(
        fs::read(&secrets).unwrap(),
        br#"{"sftp-password": "current"}"#
    );
    assert!(
        layout
            .config_dir()
            .join("runtime.v0000000001.json")
            .is_file()
    );
    assert!(!restored.pre_restore.join("config/secrets.json").exists());
    // The engine's start check of the bootstrap passes again.
    packie_config::Bootstrap::load(&bootstrap).unwrap();
}

#[test]
fn a_restore_keeps_the_current_secrets_file_over_one_an_older_backup_carries() {
    let s = state();
    let (bootstrap, secrets) = with_secrets_in_config(&s);
    let layout = Layout::of(
        &packie_config::Bootstrap::load(&bootstrap).unwrap(),
        &bootstrap,
    );
    // A backup taken without the exclusion carries the secrets file.
    let old = Layout {
        secret_files: Vec::new(),
        ..layout.clone()
    };
    let dest = s.root.join("old");
    create(&old, &dest, &operator(), None).unwrap();
    assert!(dest.join("config/secrets.json").is_file());
    fs::write(&secrets, br#"{"sftp-password": "rotated"}"#).unwrap();

    restore(&dest, &layout, &operator()).unwrap();
    assert_eq!(
        fs::read(&secrets).unwrap(),
        br#"{"sftp-password": "rotated"}"#
    );
    packie_config::Bootstrap::load(&bootstrap).unwrap();
}
