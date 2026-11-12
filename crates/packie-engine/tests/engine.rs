//! The engine end to end over IPC with the no-op registry. One test per process, because
//! logging is global.
#![cfg(test)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use packie_core::{ChannelState, StopReason};
use serde_json::json;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use packie_engine::{
    AuditLog, AuditRecord, AuditUnavailable, MemoryAudit, NoopRegistry, RunOptions,
};
use packie_ipc::audit::{AuditAction, AuditOutcome};
use packie_ipc::{ClientError, ErrorCode, Event, IpcClient, Request, Response, Topic};

/// The audit log of this test's engine: records in memory, and fails while `failing` is set.
#[derive(Default)]
struct TestAudit {
    records: MemoryAudit,
    failing: AtomicBool,
}

impl AuditLog for TestAudit {
    fn record(&self, record: &AuditRecord) -> Result<u64, AuditUnavailable> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(AuditUnavailable("disk full (test)".into()));
        }
        self.records.record(record)
    }

    fn query(
        &self,
        query: &packie_ipc::audit::AuditQuery,
    ) -> Result<packie_ipc::audit::AuditPage, AuditUnavailable> {
        self.records.query(query)
    }

    fn export(
        &self,
        request: &packie_ipc::audit::AuditExportRequest,
    ) -> Result<packie_ipc::audit::AuditExportChunk, AuditUnavailable> {
        self.records.export(request)
    }

    fn verify(&self) -> Result<packie_ipc::audit::AuditVerification, AuditUnavailable> {
        self.records.verify()
    }
}

fn test_audit() -> &'static Arc<TestAudit> {
    static AUDIT: OnceLock<Arc<TestAudit>> = OnceLock::new();
    AUDIT.get_or_init(Arc::default)
}

fn open_test_audit(
    _bootstrap: &packie_config::Bootstrap,
) -> Result<Arc<dyn AuditLog>, AuditUnavailable> {
    Ok(Arc::clone(test_audit()) as Arc<dyn AuditLog>)
}

async fn audits_commands_and_fails_closed(client: &IpcClient) {
    let records = test_audit().records.records();
    let has = |action: AuditAction, outcome: AuditOutcome| {
        records
            .iter()
            .any(|r| r.action == action && r.outcome == outcome)
    };
    assert!(has(AuditAction::EngineStart, AuditOutcome::Succeeded));
    assert!(has(AuditAction::ConfigApply, AuditOutcome::Requested));
    assert!(has(AuditAction::ConfigApply, AuditOutcome::Succeeded));
    assert!(has(AuditAction::ConfigValidate, AuditOutcome::Rejected));
    assert!(has(AuditAction::SecretSet, AuditOutcome::Requested));
    #[cfg(unix)]
    {
        let apply = records
            .iter()
            .find(|r| r.action == AuditAction::ConfigApply)
            .unwrap();
        assert!(!apply.actor.uid_or_sid.is_empty() && !apply.actor.is_system());
    }

    refuses_commands_it_cannot_audit(client).await;
}

/// A backup on request, written while the engine runs, audited and visible in the status.
async fn takes_backups_on_request(client: &IpcClient, dir: &Path) {
    let target = dir.join("backups").join("manual");
    let request = Request::CreateBackup {
        directory: Some(target.to_string_lossy().into_owned()),
    };
    let Response::BackupCreated(summary) = client.request(request).await.unwrap() else {
        panic!("expected a backup");
    };
    assert!(summary.files > 0);
    assert!(target.join("manifest.json").is_file());
    assert!(target.join("config").is_dir());
    let Response::BackupStatus(status) = client.request(Request::BackupStatus).await.unwrap()
    else {
        panic!("expected the backup status");
    };
    assert!(status.last_success.is_some_and(|a| a.ok));
    assert!(!status.stale);
    assert!(
        test_audit()
            .records
            .records()
            .iter()
            .any(|r| r.action == AuditAction::BackupCreate && r.outcome == AuditOutcome::Succeeded)
    );
    // Without a directory and without `backup.directory` the request explains itself.
    let error = server_error(
        client
            .request(Request::CreateBackup { directory: None })
            .await,
    );
    assert_eq!(error.code, ErrorCode::BadRequest);
}

/// Change control: an apply needs a note, versions record who applied and verified them.
async fn keeps_the_change_control_record(client: &IpcClient) {
    let active = config(client).await["config_version"].as_u64().unwrap();
    let put = Request::PutConfig {
        document: config(client).await,
        base_version: active,
        note: Some("   ".into()),
    };
    let error = server_error(client.request(put).await);
    assert_eq!(
        error.reason_code,
        Some(packie_ipc::messages::RejectionCode::ReasonRequired)
    );

    let verify = Request::VerifyConfigVersion {
        config_version: active,
        note: "VAL-2026-017".into(),
    };
    let Response::ConfigVersionVerified { version } = client.request(verify).await.unwrap() else {
        panic!("expected the verified version");
    };
    assert_eq!(version.verifications[0].note, "VAL-2026-017");
    let Response::ConfigHistory { versions } =
        client.request(Request::GetConfigHistory).await.unwrap()
    else {
        panic!("expected history");
    };
    let newest = &versions[0];
    assert_eq!(newest.config_version, active);
    assert!(newest.applied_by.as_ref().is_some_and(|a| !a.is_system()));
    assert_eq!(newest.verifications.len(), 1);
    assert!(!newest.outcomes.is_empty(), "what the apply did is kept");
    assert!(
        test_audit()
            .records
            .records()
            .iter()
            .any(|r| r.action == AuditAction::ConfigVerify && r.outcome == AuditOutcome::Succeeded)
    );
}

/// Fail closed: when the audit log cannot be written, the command is not run.
async fn refuses_commands_it_cannot_audit(client: &IpcClient) {
    test_audit().failing.store(true, Ordering::SeqCst);
    let before = config(client).await["config_version"].clone();
    let mut document = config(client).await;
    document["channels"][1]["label"] = json!("Not applied");
    let put = Request::PutConfig {
        document,
        base_version: before.as_u64().unwrap(),
        note: Some("must not apply".into()),
    };
    let error = server_error(client.request(put).await);
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(error.message.contains("audit log"), "{}", error.message);
    // Reading the snapshot is not audited and still works.
    assert!(client.request(Request::GetSnapshot).await.is_ok());
    let export = refuses_content_it_cannot_audit(client).await;
    test_audit().failing.store(false, Ordering::SeqCst);
    assert_eq!(config(client).await["config_version"], before);
    records_client_exports(client, export).await;
}

/// Once the audit log works, a client-built export is recorded with its kind and size.
async fn records_client_exports(client: &IpcClient, export: Request) {
    assert_eq!(client.request(export).await.unwrap(), Response::Done);
    let recorded = test_audit().records.records();
    let last = recorded.last().unwrap();
    assert_eq!(
        (last.action, last.outcome),
        (AuditAction::ClientExport, AuditOutcome::Succeeded)
    );
    assert_eq!(last.details["kind"], "change_report");
    assert_eq!(last.details["rows"], 3);
}

/// A live transcript and a client-built export are content: refused while the audit log
/// is down. Returns the export request, to repeat once the log works.
async fn refuses_content_it_cannot_audit(client: &IpcClient) -> Request {
    let channel = config(client).await["channels"][0]["channel_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let Err(ClientError::Server(error)) = client
        .subscribe(Topic::Traffic {
            channel_id: channel,
        })
        .await
    else {
        panic!("a transcript must not open while the audit log is down");
    };
    assert_eq!(error.code, ErrorCode::Unavailable);
    let export = Request::RecordClientExport {
        kind: packie_ipc::ClientExportKind::ChangeReport,
        rows: 3,
    };
    let refused = server_error(client.request(export.clone()).await);
    assert_eq!(refused.code, ErrorCode::Unavailable);
    export
}

fn write_bootstrap(dir: &Path) -> std::path::PathBuf {
    let sample = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/runtime.json"
    ));
    // As the simulator does: the runtime document is the state directory's runtime.json.
    std::fs::create_dir_all(dir.join("state")).unwrap();
    std::fs::copy(sample, dir.join("state/runtime.json")).unwrap();
    // A file secret provider, so the test never touches the OS key store.
    std::fs::write(dir.join("secrets.json"), "{}").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(dir.join("secrets.json"), mode).unwrap();
    }
    let ipc = if cfg!(windows) {
        format!(r"\\.\pipe\packie-engine-test-{}", std::process::id())
    } else {
        dir.join("state/packie.sock").to_string_lossy().into_owned()
    };
    let bootstrap = json!({
        "node": {"node_id": "test-node", "display_name": "Test node"},
        "paths": {"state_directory": "state", "log_directory": "logs"},
        "ipc": {"path": ipc},
        "runtime": {"versions_kept": 5},
        "secrets": {"provider": "file", "file": "secrets.json"},
    });
    let path = dir.join("bootstrap.json");
    std::fs::write(&path, bootstrap.to_string()).unwrap();
    path
}

async fn connect(endpoint: &str) -> IpcClient {
    for _ in 0..100 {
        if let Ok(client) = IpcClient::connect(endpoint).await {
            return client;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("engine did not start serving {endpoint}");
}

async fn config(client: &IpcClient) -> serde_json::Value {
    match client.request(Request::GetConfig).await.unwrap() {
        Response::Config { document } => serde_json::to_value(document).unwrap(),
        other => panic!("unexpected {other:?}"),
    }
}

fn server_error(result: Result<Response, ClientError>) -> packie_ipc::IpcError {
    match result {
        Err(ClientError::Server(error)) => error,
        other => panic!("expected a server error, got {other:?}"),
    }
}

async fn start_engine(
    bootstrap: &Path,
) -> (
    IpcClient,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), packie_engine::EngineError>>,
) {
    let endpoint = packie_config::Bootstrap::load(bootstrap)
        .unwrap()
        .ipc_endpoint();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let shutdown = async move {
        let _ = stopped.await;
    };
    let bootstrap = bootstrap.to_owned();
    let engine = tokio::spawn(async move {
        let registry =
            |_: &packie_engine::EngineContext| -> Arc<dyn packie_engine::ChannelRegistry> {
                Arc::new(NoopRegistry::new())
            };
        let options = RunOptions {
            audit: open_test_audit,
            ..RunOptions::default()
        };
        packie_engine::run(&bootstrap, options, registry, shutdown).await
    });
    (connect(&endpoint).await, stop, engine)
}

async fn starts_from_the_seed(client: &IpcClient) -> packie_core::ChannelId {
    // The seed became version 1; every enabled channel is stopped, not implemented.
    let Response::Snapshot { snapshot } = client.request(Request::GetSnapshot).await.unwrap()
    else {
        panic!("expected a snapshot");
    };
    assert_eq!(snapshot.node.config.active_version, Some(1));
    assert_eq!(snapshot.channels.len(), 2);
    assert!(
        snapshot
            .channels
            .iter()
            .all(|c| c.state == ChannelState::Stopped
                && c.stop_reason == Some(StopReason::NotImplemented))
    );
    assert_eq!(snapshot.node.channel_counts.stopped, 2);

    snapshot.channels[0].id
}

async fn rejects_invalid_documents(client: &IpcClient) {
    // An invalid document is rejected with field paths and changes nothing.
    let mut invalid = config(client).await;
    invalid["endpoints"][1]["settings"]["port"] = json!(0);
    // A receiver is only meaningful with `receiver_then_sender`.
    invalid["channels"][0]["connection"]["connect_strategy"] = json!("parallel");
    let put = Request::PutConfig {
        document: invalid,
        base_version: 1,
        note: Some("invalid on purpose".into()),
    };
    let error = server_error(client.request(put).await);
    assert_eq!(error.code, ErrorCode::InvalidConfig);
    let paths: Vec<&str> = error.field_errors.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "endpoints[1].settings.port",
            "channels[0].connection.receiver"
        ]
    );
    assert_eq!(config(client).await["config_version"], 1);

    // Validation without applying reports the same errors.
    let mut unknown = config(client).await;
    unknown["channels"][1]["source"]["settings"]["ack_mode"] = json!("enhanced");
    let Response::Validation { problems, diff } = client
        .request(Request::ValidateConfig { document: unknown })
        .await
        .unwrap()
    else {
        panic!("expected validation");
    };
    assert!(diff.channels.is_empty() && diff.endpoints.is_empty());
    assert_eq!(problems[0].path, "channels[1].source.settings.ack_mode");
}

async fn applies_valid_documents(client: &IpcClient) {
    // A valid change applies as version 2: the bridge restarts, the message channel's
    // label applies in place.
    let mut changed = config(client).await;
    changed["channels"][0]["connection"]["pairing_timeout_ms"] = json!(1000);
    changed["channels"][1]["label"] = json!("Renamed results");
    let mut snapshots = client.subscribe(Topic::Snapshots).await.unwrap();
    let put = Request::PutConfig {
        document: changed.clone(),
        base_version: 1,
        note: Some("shorter pairing".into()),
    };
    let Response::ConfigApplied {
        config_version,
        result,
        outcomes,
        ..
    } = client.request(put).await.unwrap()
    else {
        panic!("expected applied");
    };
    assert_eq!(config_version, 2);
    assert_eq!(result, packie_diagnostics::ApplyResult::Applied);
    let outcomes: Vec<_> = outcomes.into_iter().map(|o| o.outcome).collect();
    assert_eq!(
        outcomes,
        [
            packie_config::ApplyOutcome::Restarted,
            packie_config::ApplyOutcome::Updated
        ]
    );

    // Re-submitting from the stale base version conflicts.
    let stale = Request::PutConfig {
        document: changed,
        base_version: 1,
        note: Some("stale".into()),
    };
    let error = server_error(client.request(stale).await);
    assert_eq!(error.code, ErrorCode::Conflict);

    // Subscribers see the new version.
    let mut seen = None;
    while seen != Some(2) {
        match tokio::time::timeout(Duration::from_secs(5), snapshots.next())
            .await
            .unwrap()
        {
            Some(Event::Snapshot { snapshot }) => seen = snapshot.node.config.active_version,
            other => panic!("unexpected {other:?}"),
        }
    }
}

async fn serves_history_logs_and_commands(client: &IpcClient, channel: packie_core::ChannelId) {
    // History and logs.
    let Response::ConfigHistory { versions } =
        client.request(Request::GetConfigHistory).await.unwrap()
    else {
        panic!("expected history");
    };
    let numbers: Vec<u64> = versions.iter().map(|v| v.config_version).collect();
    assert_eq!(numbers, [2, 1]);
    assert_eq!(versions[0].note.as_deref(), Some("shorter pairing"));
    let filter = packie_diagnostics::LogFilter {
        event: Some("config.applied".into()),
        ..Default::default()
    };
    let Response::Logs { events } = client.request(Request::GetLogs { filter }).await.unwrap()
    else {
        panic!("expected logs");
    };
    assert_eq!(events.len(), 1);

    // Operator commands are logged with command, result and origin.
    let filter = packie_diagnostics::LogFilter {
        event: Some("operator.command".into()),
        ..Default::default()
    };
    let Response::Logs { events } = client.request(Request::GetLogs { filter }).await.unwrap()
    else {
        panic!("expected logs");
    };
    let fields = |e: &packie_diagnostics::LogEvent| {
        (
            e.fields.get("command").cloned(),
            e.fields.get("result").cloned(),
            e.fields.get("origin").cloned(),
        )
    };
    // Attributed to this process's OS account, read from the connection.
    #[cfg(unix)]
    {
        let uid = std::process::Command::new("id").arg("-u").output().unwrap();
        let uid = String::from_utf8(uid.stdout).unwrap().trim().to_owned();
        assert!(
            events
                .iter()
                .all(|e| e.fields.get("uid_or_sid") == Some(&uid)
                    && e.fields
                        .get("operator_account")
                        .is_some_and(|a| !a.is_empty())),
            "{events:?}"
        );
    }
    let commands: Vec<_> = events.iter().map(fields).collect();
    let entry = |command: &str, result: &str| {
        (
            Some(command.to_owned()),
            Some(result.to_owned()),
            Some("unknown".to_owned()),
        )
    };
    assert!(
        commands.contains(&entry("apply_config", "accepted")),
        "{commands:?}"
    );
    assert!(
        commands.contains(&entry("apply_config", "rejected")),
        "{commands:?}"
    );
    assert!(
        commands.contains(&entry("validate_config", "rejected")),
        "{commands:?}"
    );

    let error = server_error(
        client
            .request(Request::ChannelCommand {
                channel_id: channel,
                command: packie_ipc::ChannelCommand::Pause,
            })
            .await,
    );
    assert_eq!(error.code, ErrorCode::NotImplemented);
}

async fn manages_secrets_and_alerts(client: &IpcClient) {
    let set = Request::SetSecret {
        name: "partner-sftp".into(),
        value: "hunter2".into(),
    };
    assert_eq!(client.request(set).await.unwrap(), Response::Done);
    let Response::Secrets { names } = client.request(Request::ListSecrets).await.unwrap() else {
        panic!("expected names");
    };
    assert_eq!(names, ["partner-sftp"]);
    let delete = Request::DeleteSecret {
        name: "partner-sftp".into(),
    };
    assert_eq!(
        client.request(delete.clone()).await.unwrap(),
        Response::Done
    );
    // Deleting a name that is not stored says so instead of claiming it was removed.
    assert_eq!(
        server_error(client.request(delete).await).code,
        ErrorCode::NotFound
    );
    let bad = Request::SetSecret {
        name: "Bad Name".into(),
        value: "x".into(),
    };
    assert_eq!(
        server_error(client.request(bad).await).code,
        ErrorCode::BadRequest
    );

    let alerts = Request::GetAlerts {
        include_cleared: true,
        since: None,
    };
    assert!(matches!(
        client.request(alerts).await.unwrap(),
        Response::Alerts { .. }
    ));
}

async fn hello_names_the_os_account(client: &IpcClient) {
    let Response::Hello {
        capabilities,
        actor,
        ..
    } = client
        .request(Request::Hello {
            client: "packie-cli test".into(),
        })
        .await
        .unwrap()
    else {
        panic!("expected hello");
    };
    assert!(capabilities.is_empty());
    let actor = actor.expect("the engine says who it attributes actions to");
    assert_eq!(actor.origin, packie_core::ActorOrigin::Cli);
    assert!(!actor.name.is_empty() && !actor.uid_or_sid.is_empty());
    assert_eq!(actor.pid, Some(std::process::id()));
}

async fn messages_and_hooks_are_not_available_yet(client: &IpcClient) {
    use packie_ipc::messages::{CloseRequest, MessageQuery, RejectionCode};
    hello_names_the_os_account(client).await;

    let query = Request::GetMessages(Box::default());
    assert_eq!(
        server_error(client.request(query).await).code,
        ErrorCode::NotImplemented
    );
    let mut short = MessageQuery::default();
    short.filter.text = Some("ab".into());
    let error = server_error(client.request(Request::GetMessages(Box::new(short))).await);
    assert_eq!(error.code, ErrorCode::BadRequest);
    let message = Request::GetMessage {
        message_id: packie_core::MessageId::generate(),
    };
    assert_eq!(
        server_error(client.request(message).await).code,
        ErrorCode::NotImplemented
    );

    let close = |reason: &str| {
        Request::CloseDelivery(CloseRequest {
            delivery_id: packie_core::DeliveryId::generate(),
            reason: reason.into(),
        })
    };
    let error = server_error(client.request(close("")).await);
    assert_eq!(
        (error.code, error.reason_code),
        (ErrorCode::Rejected, Some(RejectionCode::ReasonRequired))
    );
    let error = server_error(client.request(close("Checked with the LIS")).await);
    assert_eq!(error.code, ErrorCode::NotImplemented);

    let hook = Request::TestHook {
        hook: packie_ipc::TestHook::HoldStoreCommits,
    };
    assert_eq!(
        server_error(client.request(hook).await).code,
        ErrorCode::NotImplemented
    );
}

async fn one_transcript_viewer_per_channel(endpoint: &str, channel: packie_core::ChannelId) {
    let topic = Topic::Traffic {
        channel_id: channel,
    };
    let first = connect(endpoint).await;
    let _watching = first.subscribe(topic.clone()).await.unwrap();
    let second = connect(endpoint).await;
    let Err(ClientError::Server(error)) = second.subscribe(topic.clone()).await else {
        panic!("a second viewer must be refused");
    };
    assert_eq!(
        error.reason_code,
        Some(packie_ipc::messages::RejectionCode::TranscriptInUse)
    );
    assert_eq!(error.message, "Transcript in use by another client.");

    // When the first client goes away its viewer slot frees up.
    drop(first);
    let mut freed = false;
    for _ in 0..50 {
        if second.subscribe(topic.clone()).await.is_ok() {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(freed, "the slot was not released");

    // Each opening is an attributed content view: requested, then how it ended.
    let views: Vec<AuditOutcome> = test_audit()
        .records
        .records()
        .iter()
        .filter(|r| r.action == AuditAction::TrafficView)
        .inspect(|r| assert_eq!(r.object.as_ref().unwrap().1, channel.to_string()))
        .map(|r| r.outcome)
        .collect();
    assert!(views.contains(&AuditOutcome::Rejected), "{views:?}");
    assert!(views.contains(&AuditOutcome::Succeeded), "{views:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_snapshots_and_applies_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let bootstrap = write_bootstrap(dir.path());
    let loaded = packie_config::Bootstrap::load(&bootstrap).unwrap();
    let (client, stop, engine) = start_engine(&bootstrap).await;
    let client = &client;

    let channel = starts_from_the_seed(client).await;
    rejects_invalid_documents(client).await;
    applies_valid_documents(client).await;
    serves_history_logs_and_commands(client, channel).await;
    messages_and_hooks_are_not_available_yet(client).await;
    manages_secrets_and_alerts(client).await;
    one_transcript_viewer_per_channel(&loaded.ipc_endpoint(), channel).await;
    audits_commands_and_fails_closed(client).await;
    keeps_the_change_control_record(client).await;
    takes_backups_on_request(client, dir.path()).await;

    stop.send(()).unwrap();
    engine.await.unwrap().unwrap();
    assert!(
        loaded
            .config_dir()
            .join("runtime.v0000000002.json")
            .exists()
    );
    let log_files = std::fs::read_dir(dir.path().join("logs")).unwrap().count();
    assert_eq!(log_files, 1);
}
