//! Answers IPC requests from the engine's shared state.

use std::sync::Arc;

use packie_config::{ConfigDiff, ConfigError, Problem, RuntimeConfig, SecretError};
use packie_core::Timestamp;
use packie_core::{Actor, ActorOrigin};
use packie_diagnostics::{LogFilter, TapRecvError, TapViewer};
use packie_ipc::audit::{AuditAction, AuditObjectType, AuditOutcome};
use packie_ipc::messages::{MAX_REASON, RejectionCode};
use packie_ipc::{
    Capability, ClientInfo, ErrorCode, Event, Handler, IpcError, Request, Response, TestHook, Topic,
};
use tokio::sync::{broadcast, mpsc};

use crate::audit::{self, AuditRecord, AuditUnavailable, Audited};
use crate::checks;
use crate::engine::{ENGINE_VERSION, Shared, record_logged};
use crate::exports::Chunk;
use crate::registry::{ChannelRegistry, RegistryError};

/// Events buffered per subscription inside the engine.
const SUBSCRIPTION_BUFFER: usize = 64;

pub(crate) struct EngineHandler {
    shared: Arc<Shared>,
}

impl EngineHandler {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// Validate, check the base version, save as the next version, apply.
    async fn put_config(
        &self,
        document: serde_json::Value,
        base_version: u64,
        note: Option<String>,
        actor: &Actor,
    ) -> Result<Response, IpcError> {
        let shared = &self.shared;
        let note = required_note(note.as_deref(), "apply a configuration")?;
        let _guard = shared.apply_lock.lock().await;
        let parsed = match parse(document) {
            Ok((config, warnings)) => self.checked(config, warnings).await,
            Err(error) => Err(error),
        };
        let (mut config, warnings) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                shared.rejected();
                tracing::warn!(
                    event = "config.rejected",
                    errors = error.field_errors.iter().filter(|p| p.is_error()).count(),
                    "Configuration rejected; the active version is unchanged"
                );
                shared.snapshot_now.notify_one();
                return Err(error);
            }
        };
        let active = shared.active();
        if base_version != active.config_version {
            return Err(IpcError {
                reason_code: Some(RejectionCode::VersionConflict),
                ..IpcError::new(
                    ErrorCode::Conflict,
                    format!(
                        "Configuration changed elsewhere: the document was edited from version \
                         {base_version} but version {} is active. Reload and apply again.",
                        active.config_version
                    ),
                )
            });
        }
        config.config_version = active.config_version + 1;
        config.generated_at = Timestamp::now();
        shared.status().pending = Some(config.config_version);
        if let Err(error) = shared.save(&config, note, actor) {
            shared.status().pending = None;
            return Err(internal(error));
        }
        let diff = ConfigDiff::between(&active, &config);
        let config = Arc::new(config);
        shared.set_active(Arc::clone(&config));
        let report = shared.registry.apply(&config, &diff).await;
        let result = shared.applied(&report);
        if let Err(error) = shared
            .store
            .record_outcomes(config.config_version, &report.channels)
        {
            tracing::warn!(
                event = "config.save_failed",
                config_version = config.config_version,
                %error,
                "The apply outcomes could not be saved with the version"
            );
        }
        shared.snapshot_now.notify_one();
        let count = |change| diff.channels_with(change).count();
        tracing::info!(
            event = "config.applied",
            config_version = config.config_version,
            result = ?result,
            added = count(packie_config::Change::Added),
            restarted = count(packie_config::Change::Changed),
            updated = count(packie_config::Change::Updated),
            removed = count(packie_config::Change::Removed),
            stopped = count(packie_config::Change::Stopped),
            "Configuration version {} applied",
            config.config_version
        );
        Ok(Response::ConfigApplied {
            config_version: config.config_version,
            result,
            outcomes: report.channels,
            diff,
            warnings,
        })
    }

    /// Records that `actor` verified a saved version (change control, decision 16).
    async fn verify_version(
        &self,
        config_version: u64,
        note: &str,
        actor: &Actor,
    ) -> Result<Response, IpcError> {
        let note = required_note(Some(note), "verify a version")?;
        let _guard = self.shared.apply_lock.lock().await;
        let version = self
            .shared
            .store
            .verify(config_version, actor, note)
            .map_err(version_error)?;
        Ok(Response::ConfigVersionVerified {
            version: Box::new(version),
        })
    }

    /// Adds the registry's checks (mappings, filters) and unresolved-secret warnings to a
    /// parsed document; any error rejects it.
    async fn checked(
        &self,
        config: RuntimeConfig,
        mut problems: Vec<Problem>,
    ) -> Result<(RuntimeConfig, Vec<Problem>), IpcError> {
        problems.extend(self.shared.registry.validate(&config));
        problems.extend(self.secret_warnings(&config).await);
        if problems.iter().any(Problem::is_error) {
            return Err(IpcError::invalid_config(problems));
        }
        Ok((config, problems))
    }

    /// `secret_unresolved` warnings against the names the secret store holds.
    async fn secret_warnings(&self, config: &RuntimeConfig) -> Vec<Problem> {
        let secrets = Arc::clone(&self.shared.secrets);
        let names = tokio::task::spawn_blocking(move || secrets.names()).await;
        match names {
            Ok(Ok(names)) => packie_config::unresolved_secrets(config, &names),
            Ok(Err(error)) => {
                tracing::warn!(event = "secrets.unavailable", %error, "Cannot list secrets to check references");
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    async fn validate(&self, document: serde_json::Value) -> Response {
        let parsed = match parse(document) {
            Ok((config, warnings)) => self.checked(config, warnings).await,
            Err(error) => Err(error),
        };
        match parsed {
            Ok((config, problems)) => Response::Validation {
                problems,
                diff: ConfigDiff::between(&self.shared.active(), &config),
            },
            Err(error) => Response::Validation {
                problems: error.field_errors,
                diff: ConfigDiff::default(),
            },
        }
    }

    /// Secret writes and listing, on a blocking thread (key stores may block).
    async fn secrets(&self, request: Request) -> Result<Response, IpcError> {
        let secrets = Arc::clone(&self.shared.secrets);
        let answer = tokio::task::spawn_blocking(move || match request {
            Request::SetSecret { name, value } => secrets
                .set(&name, &packie_config::Secret::new(value))
                .map(|()| Response::Done),
            // The stores remove silently; a name that is not stored is reported as such.
            Request::DeleteSecret { name } => match secrets.names() {
                Ok(names) if !names.contains(&name) => Err(SecretError::Missing(name)),
                _ => secrets.delete(&name).map(|()| Response::Done),
            },
            _ => secrets.names().map(|names| Response::Secrets { names }),
        })
        .await
        .map_err(|_| IpcError::new(ErrorCode::Internal, "The secret store task stopped."))?;
        answer.map_err(|error| match error {
            SecretError::InvalidName(_) => IpcError::new(ErrorCode::BadRequest, error.to_string()),
            SecretError::Missing(name) => IpcError::new(
                ErrorCode::NotFound,
                format!("No secret {name} is stored; nothing was removed."),
            ),
            other => IpcError::new(ErrorCode::Unavailable, other.to_string()),
        })
    }

    async fn command(
        &self,
        channel: packie_core::ChannelId,
        command: packie_ipc::ChannelCommand,
    ) -> Result<Response, IpcError> {
        let _guard = self.shared.apply_lock.lock().await;
        let result = self.shared.registry.command(channel, command).await;
        result.map_err(registry_error)?;
        Ok(Response::Done)
    }

    /// Message queries and the resend and close commands.
    /// The transmission report; a channel neither configured nor with stored messages is
    /// `not_found`.
    async fn transmission(
        &self,
        query: &packie_ipc::reports::TransmissionQuery,
    ) -> Result<Response, IpcError> {
        let registry = self.shared.registry.as_ref();
        let configured = self.shared.active().channel(query.channel_id).is_some();
        let known = crate::reports::reportable(registry, configured, query.channel_id)
            .await
            .map_err(registry_error)?;
        if !known {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                format!(
                    "No channel {} in the active configuration, and no message from it is \
                     stored.",
                    query.channel_id
                ),
            ));
        }
        let report = crate::reports::transmission(registry, query)
            .await
            .map_err(registry_error)?;
        Ok(Response::Transmission(Box::new(report)))
    }

    async fn messages(&self, request: Request) -> Result<Response, IpcError> {
        let registry = &self.shared.registry;
        match request {
            Request::GetMessages(query) => {
                query
                    .check()
                    .map_err(|text| IpcError::new(ErrorCode::BadRequest, text))?;
                let list = registry
                    .query_messages(*query)
                    .await
                    .map_err(registry_error)?;
                Ok(Response::Messages(list))
            }
            Request::GetMessage { message_id } => Ok(Response::Message {
                message: Box::new(registry.message(message_id).await.map_err(registry_error)?),
            }),
            Request::ExportMessages(export) => Ok(Response::MessageExport(
                crate::reports::export(registry.as_ref(), &export)
                    .await
                    .map_err(registry_error)?,
            )),
            Request::VerifyTransmission(query) => self.transmission(&query).await,
            Request::GetMessageFields { message_id } => {
                let record = registry.message(message_id).await.map_err(registry_error)?;
                Ok(Response::MessageFields(Box::new(
                    crate::fields::of(registry.as_ref(), &record).await,
                )))
            }
            Request::GetMessageBody(body) => Ok(Response::MessageBody(
                registry.message_body(body).await.map_err(registry_error)?,
            )),
            Request::Resend(resend) => {
                if !resend.has_valid_reason() {
                    return Err(IpcError::rejected(
                        RejectionCode::ReasonRequired,
                        "A resend reason, when given, has 1 to 500 characters.",
                    ));
                }
                if !resend.confirm_superseded {
                    refuse_superseded(registry.as_ref(), resend.message_id).await?;
                }
                let _guard = self.shared.apply_lock.lock().await;
                let result = registry.resend(resend).await;
                Ok(Response::Resent(result.map_err(registry_error)?))
            }
            Request::CloseDelivery(close) => {
                if !close.has_valid_reason() {
                    return Err(IpcError::rejected(
                        RejectionCode::ReasonRequired,
                        "Give a reason of 1 to 500 characters for closing the delivery.",
                    ));
                }
                let _guard = self.shared.apply_lock.lock().await;
                let result = registry.close_delivery(close).await;
                Ok(Response::DeliveryClosed {
                    delivery: Box::new(result.map_err(registry_error)?),
                })
            }
            _ => Err(IpcError::new(ErrorCode::Internal, "Misrouted request.")),
        }
    }

    fn test_hook(&self, hook: TestHook) -> Result<Response, IpcError> {
        let Some(hooks) = &self.shared.test_hooks else {
            return Err(IpcError::new(
                ErrorCode::NotImplemented,
                "Test hooks are disabled; set test_hooks in the bootstrap document.",
            ));
        };
        tracing::warn!(event = "test_hook", hook = ?hook, "Test hook invoked");
        match hook {
            TestHook::HoldStoreCommits => hooks.hold_commits(true),
            TestHook::ReleaseStoreCommits => hooks.hold_commits(false),
            TestHook::Crash => {
                // Answer first, then end the process without any shutdown work.
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    std::process::exit(CRASH_EXIT_CODE);
                });
            }
        }
        Ok(Response::Done)
    }

    fn hello(&self, client: &str, from: &ClientInfo) -> Response {
        tracing::debug!(
            event = "ipc.hello",
            client,
            operator_account = %from.actor.name,
            origin = from.actor.origin.as_str(),
            "IPC client identified"
        );
        let shared = &self.shared;
        let mut capabilities = Vec::new();
        if shared.test_hooks.is_some() {
            capabilities.push(Capability::TestHooks);
        }
        Response::Hello {
            engine_version: ENGINE_VERSION.to_owned(),
            protocol_version: packie_ipc::PROTOCOL_VERSION,
            node_id: shared.bootstrap.node.node_id.clone(),
            node_name: shared.bootstrap.node.display_name.clone(),
            capabilities,
            actor: Some(Box::new(from.actor.clone())),
        }
    }
}

/// The actor of a request that reached the handler without a connection (in-process
/// callers and tests): no OS account is known.
fn unattributed() -> Actor {
    Actor {
        name: "unknown".to_owned(),
        uid_or_sid: String::new(),
        origin: ActorOrigin::Unknown,
        pid: None,
    }
}

/// The one audit event of a connection refused because its OS identity cannot be read:
/// actor `unknown`, outcome `rejected`, the OS error in `reason` (acceptance L-03).
fn access_refused(reason: &str) -> AuditRecord {
    AuditRecord {
        actor: unattributed(),
        action: AuditAction::AccessRefused,
        object: None,
        outcome: AuditOutcome::Rejected,
        details: serde_json::json!({ "reason": reason }),
    }
}

/// Exit code of the `crash` test hook, so a runner can tell it from a real failure.
const CRASH_EXIT_CODE: i32 = 86;

/// The `operator.command` name of a request, for those that are operator commands.
fn command_name(request: &Request) -> Option<&'static str> {
    Some(match request {
        Request::PutConfig { .. } => "apply_config",
        Request::ValidateConfig { .. } => "validate_config",
        Request::ChannelCommand { command, .. } => match command {
            packie_ipc::ChannelCommand::Pause => "pause_channel",
            packie_ipc::ChannelCommand::Resume => "resume_channel",
            packie_ipc::ChannelCommand::Restart => "restart_channel",
        },
        Request::Resend(_) => "resend",
        Request::CloseDelivery(_) => "close_delivery",
        Request::VerifyConfigVersion { .. } => "verify_config_version",
        Request::SetSecret { .. } => "set_secret",
        Request::DeleteSecret { .. } => "delete_secret",
        Request::TestEndpoint(_) => "test_endpoint",
        Request::TestNotification => "test_notification",
        _ => return None,
    })
}

/// Refuses to resend a message that a newer revision supersedes, naming the newer message:
/// resending it may reissue a corrected result (requirement P10 in `docs/requirements.md`).
async fn refuse_superseded(
    registry: &dyn ChannelRegistry,
    message_id: packie_core::MessageId,
) -> Result<(), IpcError> {
    let record = registry.message(message_id).await.map_err(registry_error)?;
    let Some(newer) = record.revised_by else {
        return Ok(());
    };
    let received = registry
        .message(newer)
        .await
        .map(|m| format!(", received {}", m.summary.received_at))
        .unwrap_or_default();
    Err(IpcError::rejected(
        RejectionCode::Superseded,
        format!(
            "A newer revision of this message exists ({newer}{received}). Resending this older \
             version may reissue a corrected result; confirm that it is intended to resend it \
             anyway."
        ),
    ))
}

/// The operator's note, trimmed: 1 to [`MAX_REASON`] characters, or a `reason_required`
/// rejection naming what it is for.
fn required_note<'a>(note: Option<&'a str>, purpose: &str) -> Result<&'a str, IpcError> {
    let note = note.map(str::trim).unwrap_or_default();
    if (1..=MAX_REASON).contains(&note.chars().count()) {
        return Ok(note);
    }
    Err(IpcError::rejected(
        RejectionCode::ReasonRequired,
        format!("A note of 1 to {MAX_REASON} characters saying why is required to {purpose}."),
    ))
}

/// `accepted`, `rejected` (a precondition or the document failed) or `failed`.
fn outcome_of(result: &Result<Response, IpcError>) -> &'static str {
    match result {
        Ok(Response::Validation { problems, .. }) if problems.iter().any(Problem::is_error) => {
            "rejected"
        }
        Ok(_) => "accepted",
        Err(error) => match error.code {
            ErrorCode::BadRequest
            | ErrorCode::InvalidConfig
            | ErrorCode::Conflict
            | ErrorCode::NotFound
            | ErrorCode::Rejected => "rejected",
            ErrorCode::NotImplemented | ErrorCode::Unavailable | ErrorCode::Internal => "failed",
        },
    }
}

/// Logs one operator command (`docs/spec/diagnostics.md` §7.3): `command`, `result`
/// (`accepted`, `rejected`, `failed`) and `origin`. Free-text reasons are logged by length
/// only; validation counts as rejected when the document has errors.
fn log_command(
    command: &str,
    actor: &Actor,
    reason_length: Option<usize>,
    result: &Result<Response, IpcError>,
) {
    let outcome = outcome_of(result);
    let detail = result.as_ref().err().map_or("", |e| e.message.as_str());
    let origin = actor.origin.as_str();
    tracing::info!(
        event = "operator.command",
        command,
        result = outcome,
        operator_account = %actor.name,
        uid_or_sid = %actor.uid_or_sid,
        origin,
        reason_length,
        "Operator command {command} by {} ({origin}): {outcome} {detail}",
        actor.name
    );
}

pub(crate) fn registry_error(error: RegistryError) -> IpcError {
    match error {
        RegistryError::NotFound(id) => IpcError {
            reason_code: Some(RejectionCode::NotFound),
            ..IpcError::new(
                ErrorCode::NotFound,
                format!("No channel {id} in the active configuration."),
            )
        },
        RegistryError::Rejected { code, message } => IpcError::rejected(code, message),
        RegistryError::NotImplemented(message) => IpcError::new(ErrorCode::NotImplemented, message),
        RegistryError::BadRequest(message) => IpcError::new(ErrorCode::BadRequest, message),
        RegistryError::Failed(message) => IpcError::new(ErrorCode::Internal, message),
    }
}

fn parse(
    document: serde_json::Value,
) -> Result<(RuntimeConfig, Vec<packie_config::Problem>), IpcError> {
    let config = RuntimeConfig::from_value(document)
        .map_err(|e| IpcError::invalid_config(e.into_problems()))?;
    let warnings = packie_config::validate(&config).unwrap_or_default();
    Ok((config, warnings))
}

/// A saved version that is not in the history is `not_found`; anything else is internal.
fn version_error(error: ConfigError) -> IpcError {
    match error {
        ConfigError::UnknownVersion(v) => IpcError::new(
            ErrorCode::NotFound,
            format!("Version {v} is not in the history."),
        ),
        error => internal(error),
    }
}

fn internal(error: ConfigError) -> IpcError {
    tracing::error!(event = "config.save_failed", %error, "Could not save the configuration");
    IpcError::new(
        ErrorCode::Internal,
        "The configuration could not be saved; nothing was applied. See the engine log.",
    )
}

impl EngineHandler {
    /// Answers `request` from `client`, logging operator commands with the client's account.
    async fn dispatch(&self, request: Request, client: &ClientInfo) -> Result<Response, IpcError> {
        let command = command_name(&request);
        let reason_length = match &request {
            Request::CloseDelivery(close) => Some(close.reason.chars().count()),
            Request::Resend(resend) => resend.reason.as_ref().map(|r| r.chars().count()),
            _ => None,
        };
        let export = Chunk::of(&request);
        if let Some(chunk) = &export {
            let writable = chunk.is_first() || self.shared.audit.check_writable().is_ok();
            self.shared.exports.admit(chunk, &client.actor, writable)?;
        }
        let audited = audit::audited(&request);
        if let Some(entry) = &audited {
            self.record_requested(&client.actor, entry)?;
        }
        let mut result = self.answer(request, client).await;
        if let Some(chunk) = export {
            self.shared
                .exports
                .answered(chunk, &client.actor, &mut result);
        }
        if let Some(command) = command {
            log_command(command, &client.actor, reason_length, &result);
        }
        if let Some(entry) = audited {
            self.record_outcome(&client.actor, entry, &result);
        }
        result
    }

    /// Records that `actor` asked for an audited action; refuses it when the log cannot.
    fn record_requested(
        &self,
        actor: &Actor,
        (action, object, details): &Audited,
    ) -> Result<(), IpcError> {
        let record = AuditRecord {
            actor: actor.clone(),
            action: *action,
            object: object.clone(),
            outcome: AuditOutcome::Requested,
            details: details.clone(),
        };
        self.shared
            .audit
            .record(&record)
            .map(drop)
            .map_err(|error| {
                tracing::error!(
                    event = "audit.write_failed",
                    action = action.as_str(),
                    error_summary = %error,
                    "Refused {}: {error}",
                    action.as_str()
                );
                IpcError::new(
                    ErrorCode::Unavailable,
                    format!(
                        "The audit log cannot be written, so this was not done: {error}. Check \
                     disk space and the state directory's audit folder."
                    ),
                )
            })
    }

    /// Records how an audited action ended; a failure here is logged (the action ran).
    fn record_outcome(
        &self,
        actor: &Actor,
        (action, object, mut details): Audited,
        result: &Result<Response, IpcError>,
    ) {
        let outcome = match outcome_of(result) {
            "accepted" => AuditOutcome::Succeeded,
            "rejected" => AuditOutcome::Rejected,
            _ => AuditOutcome::Failed,
        };
        if let Some(map) = details.as_object_mut() {
            match result {
                Err(error) => {
                    map.insert("error_code".into(), serde_json::json!(error.code));
                }
                Ok(Response::MessageExport(chunk)) => {
                    map.insert("count".into(), serde_json::json!(chunk.total));
                }
                Ok(Response::Transmission(report)) => {
                    map.insert("messages".into(), serde_json::json!(report.messages));
                }
                Ok(Response::EndpointTested(report)) => {
                    map.insert("ok".into(), serde_json::json!(report.ok));
                }
                Ok(Response::NotificationTested { results }) => {
                    let results: Vec<_> = results
                        .iter()
                        .map(|r| serde_json::json!({ "target": r.target, "ok": r.ok }))
                        .collect();
                    map.insert("results".into(), serde_json::json!(results));
                }
                Ok(_) => {}
            }
        }
        let record = AuditRecord {
            actor: actor.clone(),
            action,
            object,
            outcome,
            details,
        };
        record_logged(self.shared.audit.as_ref(), &record);
    }

    /// A backup on request, on a blocking thread.
    async fn create_backup(
        &self,
        directory: Option<String>,
        actor: &Actor,
    ) -> Result<Response, IpcError> {
        let backups = Arc::clone(&self.shared.backups);
        let actor = actor.clone();
        let summary =
            tokio::task::spawn_blocking(move || backups.create(directory.as_deref(), &actor))
                .await
                .map_err(|_| IpcError::new(ErrorCode::Internal, "The backup task stopped."))??;
        Ok(Response::BackupCreated(Box::new(summary)))
    }

    /// The audit log's own requests: review, export and chain verification.
    fn audit_request(&self, request: Request) -> Result<Response, IpcError> {
        let audit = &self.shared.audit;
        let unavailable =
            |e: AuditUnavailable| IpcError::new(ErrorCode::Unavailable, e.to_string());
        match request {
            Request::GetAuditEvents(query) => audit
                .query(&query)
                .map(Response::AuditEvents)
                .map_err(unavailable),
            Request::ExportAudit(export) => audit
                .export(&export)
                .map(Response::AuditExport)
                .map_err(unavailable),
            _ => audit
                .verify()
                .map(|v| Response::AuditVerification(Box::new(v)))
                .map_err(unavailable),
        }
    }

    async fn answer(&self, request: Request, from: &ClientInfo) -> Result<Response, IpcError> {
        let shared = &self.shared;
        match request {
            Request::Hello { client } => Ok(self.hello(&client, from)),
            Request::GetSnapshot => Ok(Response::Snapshot {
                snapshot: Box::new(shared.current()),
            }),
            Request::GetConfig => Ok(Response::Config {
                document: Box::new((*shared.active()).clone()),
            }),
            Request::GetConfigHistory => Ok(Response::ConfigHistory {
                versions: shared.store.history().map_err(internal)?,
            }),
            Request::GetConfigVersion { config_version } => {
                let document = shared.store.load(config_version).map_err(version_error)?;
                Ok(Response::Config {
                    document: Box::new(document),
                })
            }
            Request::ValidateConfig { document } => Ok(self.validate(document).await),
            Request::PutConfig {
                document,
                base_version,
                note,
            } => {
                self.put_config(document, base_version, note, &from.actor)
                    .await
            }
            Request::VerifyConfigVersion {
                config_version,
                note,
            } => {
                self.verify_version(config_version, &note, &from.actor)
                    .await
            }
            Request::GetAlerts {
                include_cleared,
                since,
            } => Ok(Response::Alerts {
                alerts: shared.alerts().history(include_cleared, since),
            }),
            Request::SetSecret { .. } | Request::DeleteSecret { .. } | Request::ListSecrets => {
                self.secrets(request).await
            }
            Request::GetLogs { filter } => Ok(Response::Logs {
                events: shared.ring.query(&filter),
            }),
            Request::ChannelCommand {
                channel_id,
                command,
            } => self.command(channel_id, command).await,
            Request::GetMessages(_)
            | Request::GetMessage { .. }
            | Request::GetMessageFields { .. }
            | Request::ExportMessages(_)
            | Request::VerifyTransmission(_)
            | Request::GetMessageBody(_)
            | Request::Resend(_)
            | Request::CloseDelivery(_) => self.messages(request).await,
            Request::GetAuditEvents(_) | Request::ExportAudit(_) | Request::VerifyAudit => {
                self.audit_request(request)
            }
            Request::CreateBackup { directory } => self.create_backup(directory, &from.actor).await,
            Request::BackupStatus => Ok(Response::BackupStatus(Box::new(shared.backups.status()))),
            Request::ListSerialPorts => checks::serial_ports(shared).await,
            Request::TestEndpoint(test) => checks::test_endpoint(shared, test).await,
            Request::TestNotification => Ok(checks::test_notification(shared).await),
            // The audit record written before this answer is the whole of the request.
            Request::RecordClientExport { .. } => Ok(Response::Done),
            Request::TestHook { hook } => self.test_hook(hook),
            Request::Subscribe { .. } | Request::Unsubscribe { .. } => Err(IpcError::new(
                ErrorCode::BadRequest,
                "Subscriptions are handled by the server.",
            )),
        }
    }
}

impl Handler for EngineHandler {
    async fn handle(&self, request: Request) -> Result<Response, IpcError> {
        self.dispatch(request, &ClientInfo::new(unattributed()))
            .await
    }

    async fn handle_from(
        &self,
        request: Request,
        client: &ClientInfo,
    ) -> Result<Response, IpcError> {
        self.dispatch(request, client).await
    }

    fn refused(&self, reason: &str) {
        record_logged(self.shared.audit.as_ref(), &access_refused(reason));
    }

    fn subscribe(&self, topic: Topic) -> Result<mpsc::Receiver<Event>, IpcError> {
        self.subscribe_from(topic, &ClientInfo::new(unattributed()))
    }

    fn subscribe_from(
        &self,
        topic: Topic,
        client: &ClientInfo,
    ) -> Result<mpsc::Receiver<Event>, IpcError> {
        let shared = &self.shared;
        match topic {
            Topic::Snapshots => {
                let first = shared.latest();
                Ok(forward(
                    Some(first),
                    shared.snapshots.subscribe(),
                    |snapshot| {
                        Some(Event::Snapshot {
                            snapshot: Box::new((*snapshot).clone()),
                        })
                    },
                ))
            }
            Topic::Logs { filter } => {
                let filter = LogFilter {
                    limit: None,
                    ..filter
                };
                Ok(forward(None, shared.ring.subscribe(), move |event| {
                    filter.matches(&event).then_some(Event::Log { event })
                }))
            }
            Topic::Alerts => Ok(forward(None, shared.alert_events.subscribe(), |alert| {
                Some(Event::Alert { alert })
            })),
            Topic::Traffic { channel_id } => self.traffic(channel_id, &client.actor),
        }
    }
}

impl EngineHandler {
    /// A live traffic transcript is a content view: audited as `traffic_view` before it
    /// opens, and refused when the audit log cannot record it.
    fn traffic(
        &self,
        channel_id: packie_core::ChannelId,
        actor: &Actor,
    ) -> Result<mpsc::Receiver<Event>, IpcError> {
        let entry: Audited = (
            AuditAction::TrafficView,
            Some((AuditObjectType::Channel, channel_id.to_string())),
            serde_json::json!({}),
        );
        self.record_requested(actor, &entry)?;
        let opened = self.open_traffic(channel_id);
        let outcome = opened
            .as_ref()
            .map(|_| Response::Done)
            .map_err(Clone::clone);
        self.record_outcome(actor, entry, &outcome);
        opened
    }

    fn open_traffic(
        &self,
        channel_id: packie_core::ChannelId,
    ) -> Result<mpsc::Receiver<Event>, IpcError> {
        if self.shared.active().channel(channel_id).is_none() {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                format!("No channel {channel_id} in the active configuration."),
            ));
        }
        let viewer =
            self.shared.tap.subscribe(channel_id).map_err(|busy| {
                IpcError::rejected(RejectionCode::TranscriptInUse, busy.to_string())
            })?;
        Ok(forward_traffic(viewer))
    }
}

/// Feeds a traffic subscription from the channel's tap viewer. Ends, freeing the channel
/// for another viewer, when the subscriber goes away or the tap is removed.
fn forward_traffic(mut viewer: TapViewer) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel(SUBSCRIPTION_BUFFER);
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                received = viewer.recv() => match received {
                    Ok(segment) => Event::Traffic { segment },
                    Err(TapRecvError::Lagged(missed)) => Event::Lagged { missed },
                    Err(TapRecvError::Closed) => return,
                },
                () = tx.closed() => return,
            };
            if tx.send(event).await.is_err() {
                return;
            }
        }
    });
    rx
}

/// Feeds a subscription from a broadcast stream, optionally starting with `first`. Ends
/// when the subscriber goes away or the stream closes; a slow subscriber gets `lagged`.
fn forward<T, F>(
    first: Option<T>,
    mut source: broadcast::Receiver<T>,
    map: F,
) -> mpsc::Receiver<Event>
where
    T: Clone + Send + 'static,
    F: Fn(T) -> Option<Event> + Send + 'static,
{
    let (tx, rx) = mpsc::channel(SUBSCRIPTION_BUFFER);
    tokio::spawn(async move {
        let mut next = first.map(Ok);
        loop {
            let item = match next.take() {
                Some(item) => item,
                None => source.recv().await,
            };
            let event = match item {
                Ok(value) => map(value),
                Err(broadcast::error::RecvError::Lagged(missed)) => Some(Event::Lagged { missed }),
                Err(broadcast::error::RecvError::Closed) => return,
            };
            let Some(event) = event else { continue };
            if tx.send(event).await.is_err() {
                return;
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance L-03: a connection refused for an unreadable OS identity is one event,
    /// attributed to `unknown`, rejected, with the OS error.
    #[test]
    fn a_refused_connection_is_one_unattributed_access_refused_event() {
        let audit = crate::MemoryAudit::default();
        record_logged(
            &audit,
            &access_refused("the peer's credentials cannot be read: permission denied"),
        );
        let records = audit.records();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(
            (record.action, record.outcome),
            (AuditAction::AccessRefused, AuditOutcome::Rejected)
        );
        assert_eq!(
            (record.actor.name.as_str(), record.actor.uid_or_sid.as_str()),
            ("unknown", "")
        );
        assert_eq!(record.object, None);
        assert_eq!(
            record.details["reason"],
            "the peer's credentials cannot be read: permission denied"
        );
    }
}
