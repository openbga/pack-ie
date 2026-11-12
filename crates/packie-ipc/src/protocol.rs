//! The messages exchanged over the management interface.
//!
//! The client sends [`ClientFrame`]s, each a [`Request`] with an id it chooses. The engine
//! answers every request with one [`ServerFrame::Response`] or [`ServerFrame::Error`]
//! carrying the same id, and pushes [`ServerFrame::Event`]s for the subscriptions the
//! client opened. All enums are tagged by `type` with `snake_case` names.

use packie_config::{ChannelOutcome, ConfigDiff, Problem, RuntimeConfig, VersionInfo};
pub use packie_core::ChannelCommand;
use packie_core::Timestamp;
use packie_core::{ChannelId, MessageId};
use packie_diagnostics::{Alert, ApplyResult, EngineSnapshot, LogEvent, LogFilter, TapSegment};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::audit::{
    AuditExportChunk, AuditExportRequest, AuditPage, AuditQuery, AuditVerification,
};
use crate::backup::{BackupStatusReport, BackupSummary};
use crate::messages::{
    BodyRequest, CloseRequest, DeliveryRecord, MessageBody, MessageList, MessageQuery,
    MessageRecord, RejectionCode, ResendRequest, ResendResult,
};

/// Version of this protocol; bumped on incompatible changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// One request from a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ClientFrame {
    /// Chosen by the client; echoed in the response.
    pub id: u64,
    /// What the client asks for.
    pub request: Request,
}

/// What a client can ask for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Identify both sides. Answered with [`Response::Hello`].
    Hello {
        /// Client name and version, for logs.
        client: String,
    },
    /// The current snapshot. Answered with [`Response::Snapshot`].
    GetSnapshot,
    /// The active runtime document. Answered with [`Response::Config`].
    GetConfig,
    /// Saved versions. Answered with [`Response::ConfigHistory`].
    GetConfigHistory,
    /// One saved version. Answered with [`Response::Config`].
    GetConfigVersion {
        /// The version.
        config_version: u64,
    },
    /// Sign off a saved version as verified after the change, with the operator's note (1 to
    /// 500 characters, such as a validation record number). Answered with
    /// [`Response::ConfigVersionVerified`].
    VerifyConfigVersion {
        /// The version.
        #[ts(type = "number")]
        config_version: u64,
        /// The reference.
        note: String,
    },
    /// Validate a document without applying it. Answered with [`Response::Validation`].
    ValidateConfig {
        /// The document, as JSON.
        #[ts(as = "RuntimeConfig")]
        document: serde_json::Value,
    },
    /// Validate, save as the next version and apply. Answered with
    /// [`Response::ConfigApplied`]; an invalid document fails with `invalid_config`, a stale
    /// `base_version` with `conflict` (`version_conflict`), and both leave the active
    /// version alone. The document's own `config_version` is ignored.
    PutConfig {
        /// The document, as JSON.
        #[ts(as = "RuntimeConfig")]
        document: serde_json::Value,
        /// The active version the document was edited from.
        base_version: u64,
        /// Why, 1 to 500 characters; required (change control), shown in the version
        /// history.
        #[serde(default)]
        note: Option<String>,
    },
    /// Active alerts and, optionally, cleared ones (kept 30 days). Answered with
    /// [`Response::Alerts`].
    GetAlerts {
        /// Also list cleared alerts.
        #[serde(default)]
        include_cleared: bool,
        /// Only alerts seen at or after this time.
        #[serde(default)]
        since: Option<Timestamp>,
    },
    /// Store a secret value in the secret provider. Answered with [`Response::Done`]. The
    /// value is never logged or returned.
    SetSecret {
        /// `^[a-z0-9][a-z0-9_.-]{0,62}$`.
        name: String,
        /// The value.
        value: String,
    },
    /// Remove a secret. Answered with [`Response::Done`].
    DeleteSecret {
        /// The secret's name.
        name: String,
    },
    /// The names of stored secrets (never values). Answered with [`Response::Secrets`].
    ListSecrets,
    /// Recent log events. Answered with [`Response::Logs`].
    GetLogs {
        /// Which events.
        filter: LogFilter,
    },
    /// Start a stream of events. Answered with [`Response::Subscribed`]; events follow as
    /// [`ServerFrame::Event`] with that subscription id.
    Subscribe {
        /// What to stream.
        topic: Topic,
    },
    /// Stop a stream. Answered with [`Response::Done`].
    Unsubscribe {
        /// The subscription id.
        subscription: u64,
    },
    /// Pause, resume or restart a channel. Answered with [`Response::Done`].
    ChannelCommand {
        /// The channel.
        channel_id: ChannelId,
        /// What to do.
        command: ChannelCommand,
    },
    /// A page of stored messages. Answered with [`Response::Messages`].
    GetMessages(Box<MessageQuery>),
    /// One message with its deliveries and attempts. Answered with [`Response::Message`].
    GetMessage {
        /// The message.
        message_id: MessageId,
    },
    /// The original's parsed fields, an explicit content view (audited). Answered with
    /// [`Response::MessageFields`].
    GetMessageFields {
        /// The message.
        message_id: MessageId,
    },
    /// The original or a transformed body. Answered with [`Response::MessageBody`].
    GetMessageBody(BodyRequest),
    /// One chunk of an export of the messages a filter selects (audited with the filter and
    /// count; bodies only when asked for, a content view). Answered with
    /// [`Response::MessageExport`].
    ExportMessages(crate::reports::ExportRequest),
    /// The transmission verification report of one channel and period (audited). Answered
    /// with [`Response::Transmission`].
    VerifyTransmission(crate::reports::TransmissionQuery),
    /// Resend a message to some or all destinations. Answered with [`Response::Resent`];
    /// destinations whose preconditions fail are listed with their rejection.
    Resend(ResendRequest),
    /// Close a delivery with a reason. Answered with [`Response::DeliveryClosed`].
    CloseDelivery(CloseRequest),
    /// A page of the audit log, newest first. Answered with [`Response::AuditEvents`].
    GetAuditEvents(AuditQuery),
    /// One chunk of an audit export, oldest first; the first chunk is audited. Answered
    /// with [`Response::AuditExport`].
    ExportAudit(AuditExportRequest),
    /// Walk the audit hash chain (audited). Answered with [`Response::AuditVerification`].
    VerifyAudit,
    /// Take a consistent backup while the engine runs (audited). Answered with
    /// [`Response::BackupCreated`].
    CreateBackup {
        /// An absolute directory that does not exist or is empty; absent for a new
        /// directory under `backup.directory`.
        #[serde(default)]
        directory: Option<String>,
    },
    /// The schedule, the newest attempts and staleness. Answered with
    /// [`Response::BackupStatus`].
    BackupStatus,
    /// The serial ports the operating system reports, with the configured endpoints that
    /// name each. Answered with [`Response::SerialPorts`].
    ListSerialPorts,
    /// Probe an endpoint definition without sending data (audited). Answered with
    /// [`Response::EndpointTested`]; an invalid definition fails with `invalid_config`.
    TestEndpoint(crate::checks::EndpointTest),
    /// Send a test message to every configured notification target (audited). Answered
    /// with [`Response::NotificationTested`].
    TestNotification,
    /// Record that the client is about to write an export it built itself (audited as
    /// `client_export`; refused with `unavailable` when the audit log cannot record it, in
    /// which case the client writes nothing). Answered with [`Response::Done`].
    RecordClientExport {
        /// What is exported.
        kind: ClientExportKind,
        /// Rows (versions, for a change report) in the export.
        rows: u32,
    },
    /// A scenario test hook; only when the bootstrap sets `test_hooks` (advertised as the
    /// `test_hooks` capability in [`Response::Hello`]). Answered with [`Response::Done`].
    TestHook {
        /// What to do.
        hook: TestHook,
    },
}

/// An export a client builds itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ClientExportKind {
    /// The console's configuration change report (versions, notes, sign-offs, outcomes).
    ChangeReport,
}

/// Scenario test hooks (`docs/spec/scenarios.md` §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TestHook {
    /// The store writer accepts requests but completes no commit until released.
    HoldStoreCommits,
    /// Let held commits finish.
    ReleaseStoreCommits,
    /// End the process at once: no drain, no flush, no graceful close. The answer is sent
    /// first; the connection then drops.
    Crash,
}

/// Optional engine features a client may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// [`Request::TestHook`] is enabled.
    TestHooks,
}

/// A stream a client can subscribe to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Topic {
    /// [`Event::Snapshot`] every publication interval, starting with the current one.
    Snapshots,
    /// [`Event::Log`] for each new event that matches; `limit` is ignored.
    Logs {
        /// Which events.
        filter: LogFilter,
    },
    /// [`Event::Alert`] whenever an alert is raised or cleared.
    Alerts,
    /// [`Event::Traffic`] for each segment of one channel's traffic while subscribed.
    Traffic {
        /// The channel.
        channel_id: ChannelId,
    },
}

/// A successful answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// To [`Request::Hello`].
    Hello {
        /// Engine version.
        engine_version: String,
        /// [`PROTOCOL_VERSION`] of the engine.
        protocol_version: u32,
        /// Node id.
        node_id: String,
        /// Node name.
        node_name: String,
        /// Optional features enabled in this engine.
        capabilities: Vec<Capability>,
        /// The OS account the engine attributes this connection's actions to.
        #[serde(default)]
        actor: Option<Box<packie_core::Actor>>,
    },
    /// To [`Request::GetSnapshot`].
    Snapshot {
        /// The snapshot.
        snapshot: Box<EngineSnapshot>,
    },
    /// To [`Request::GetConfig`] and [`Request::GetConfigVersion`].
    Config {
        /// The document.
        document: Box<RuntimeConfig>,
    },
    /// To [`Request::GetConfigHistory`].
    ConfigHistory {
        /// Saved versions, newest first.
        versions: Vec<VersionInfo>,
    },
    /// To [`Request::VerifyConfigVersion`]: the version with its sign-offs.
    ConfigVersionVerified {
        /// The version.
        version: Box<VersionInfo>,
    },
    /// To [`Request::ValidateConfig`].
    Validation {
        /// Every problem; the document is valid when none is an error.
        problems: Vec<Problem>,
        /// What applying it would change; empty (no channels or endpoints) when the
        /// document has errors.
        diff: ConfigDiff,
    },
    /// To [`Request::PutConfig`].
    ConfigApplied {
        /// The new active version.
        config_version: u64,
        /// `applied`, or `partial` when a channel failed.
        result: ApplyResult,
        /// Per-channel outcomes.
        outcomes: Vec<ChannelOutcome>,
        /// What changed.
        diff: ConfigDiff,
        /// Warnings of the document.
        warnings: Vec<Problem>,
    },
    /// To [`Request::GetAlerts`].
    Alerts {
        /// Active alerts newest first, then cleared ones newest first.
        alerts: Vec<Alert>,
    },
    /// To [`Request::ListSecrets`].
    Secrets {
        /// Names, sorted.
        names: Vec<String>,
    },
    /// To [`Request::GetLogs`].
    Logs {
        /// Matching events, oldest first.
        events: Vec<LogEvent>,
    },
    /// To [`Request::Subscribe`].
    Subscribed {
        /// Id of the new subscription.
        subscription: u64,
    },
    /// To [`Request::GetMessages`].
    Messages(MessageList),
    /// To [`Request::GetMessage`].
    Message {
        /// The message.
        message: Box<MessageRecord>,
    },
    /// To [`Request::GetMessageFields`].
    MessageFields(Box<crate::messages::ParsedFields>),
    /// To [`Request::ExportMessages`].
    MessageExport(crate::reports::ExportChunk),
    /// To [`Request::VerifyTransmission`].
    Transmission(Box<crate::reports::TransmissionReport>),
    /// To [`Request::GetMessageBody`].
    MessageBody(MessageBody),
    /// To [`Request::Resend`].
    Resent(ResendResult),
    /// To [`Request::CloseDelivery`].
    DeliveryClosed {
        /// The delivery, now `cancelled`.
        delivery: Box<DeliveryRecord>,
    },
    /// To [`Request::GetAuditEvents`].
    AuditEvents(AuditPage),
    /// To [`Request::ExportAudit`].
    AuditExport(AuditExportChunk),
    /// To [`Request::VerifyAudit`].
    AuditVerification(Box<AuditVerification>),
    /// To [`Request::CreateBackup`].
    BackupCreated(Box<BackupSummary>),
    /// To [`Request::BackupStatus`].
    BackupStatus(Box<BackupStatusReport>),
    /// To [`Request::ListSerialPorts`].
    SerialPorts {
        /// Sorted by name.
        ports: Vec<crate::checks::SerialPort>,
    },
    /// To [`Request::TestEndpoint`].
    EndpointTested(crate::checks::ProbeReport),
    /// To [`Request::TestNotification`].
    NotificationTested {
        /// One result per configured target; empty when none is configured.
        results: Vec<crate::checks::NotificationResult>,
    },
    /// To requests that return nothing.
    Done,
}

/// Why a request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The frame or request is malformed.
    BadRequest,
    /// The configuration document is invalid; see `field_errors`.
    InvalidConfig,
    /// The document was edited from a version that is no longer active.
    Conflict,
    /// The channel, message, delivery, version or subscription does not exist.
    NotFound,
    /// An operator command's precondition failed; see `reason_code`.
    Rejected,
    /// This build of the engine cannot do it.
    NotImplemented,
    /// The engine cannot serve the request now (for example, shutting down).
    Unavailable,
    /// An unexpected failure; the engine log has details.
    Internal,
}

/// A failed answer. Never contains message bodies or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
#[error("{message}")]
pub struct IpcError {
    /// Stable code to branch on.
    pub code: ErrorCode,
    /// Operator-facing explanation.
    pub message: String,
    /// Every problem of the document, for `invalid_config`.
    pub field_errors: Vec<Problem>,
    /// Stable reason, for `rejected` (and `conflict`, `invalid_config`).
    pub reason_code: Option<RejectionCode>,
}

impl IpcError {
    /// An error without field errors.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field_errors: Vec::new(),
            reason_code: None,
        }
    }

    /// `rejected` with a stable reason code.
    pub fn rejected(reason: RejectionCode, message: impl Into<String>) -> Self {
        Self {
            reason_code: Some(reason),
            ..Self::new(ErrorCode::Rejected, message)
        }
    }

    /// `not_implemented` with the standard text for features this build lacks.
    #[must_use]
    pub fn not_implemented(what: &str) -> Self {
        Self::new(
            ErrorCode::NotImplemented,
            format!("{what} is not available in this build of the engine."),
        )
    }

    /// `invalid_config` with every field problem.
    #[must_use]
    pub fn invalid_config(errors: Vec<Problem>) -> Self {
        let count = errors.iter().filter(|p| p.is_error()).count();
        Self {
            code: ErrorCode::InvalidConfig,
            message: format!(
                "The configuration has {count} error{}; nothing was applied.",
                if count == 1 { "" } else { "s" }
            ),
            field_errors: errors,
            reason_code: Some(RejectionCode::InvalidConfig),
        }
    }
}

/// A frame from the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    /// A successful answer to request `id`.
    Response {
        /// The request id.
        id: u64,
        /// The answer.
        result: Response,
    },
    /// A failed answer to request `id` (0 when the request could not be read).
    Error {
        /// The request id.
        id: u64,
        /// What went wrong.
        error: IpcError,
    },
    /// An event of subscription `subscription`.
    Event {
        /// The subscription id.
        subscription: u64,
        /// The event.
        event: Event,
    },
}

/// One item of a subscription stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A new snapshot.
    Snapshot {
        /// The snapshot.
        snapshot: Box<EngineSnapshot>,
    },
    /// A new log event.
    Log {
        /// The event.
        event: LogEvent,
    },
    /// An alert was raised (`cleared_at` absent) or cleared.
    Alert {
        /// The alert.
        alert: Alert,
    },
    /// A traffic segment.
    Traffic {
        /// The segment.
        segment: TapSegment,
    },
    /// The client fell behind and `missed` events of this stream were dropped.
    Lagged {
        /// Number of dropped events.
        missed: u64,
    },
}
