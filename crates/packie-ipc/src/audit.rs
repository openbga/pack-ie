//! The audit log over IPC (`docs/spec/audit.md`): the event record, the review query, the
//! export in chunks and the chain verification report.
//!
//! An event names who (the OS account of the IPC peer), from where (`origin`), what
//! (`action` on an object) and how it ended (`outcome`). `details` holds identifiers,
//! versions, counts, codes and text the operator entered (notes, reasons, a searched
//! identifier), never message bodies.

use std::fmt;
use std::str::FromStr;

use packie_core::{ParseError, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Default page size of [`AuditQuery`].
pub const DEFAULT_AUDIT_PAGE: u32 = 100;
/// Largest page size of [`AuditQuery`].
pub const MAX_AUDIT_PAGE: u32 = 1000;

macro_rules! audit_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS,
        )]
        pub enum $name {
            $( $(#[$vmeta])* #[serde(rename = $text)] $variant, )+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The serialized text.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ParseError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(ParseError::Enum),
                }
            }
        }
    };
}

audit_enum! {
    /// Where an audited action came from.
    pub enum AuditOrigin {
        /// The desktop console.
        Console => "console",
        /// The `packie` command line.
        Cli => "cli",
        /// The engine itself (start, stop, schedules, retention).
        Engine => "engine",
        /// Another local client (the simulator, an unidentified client).
        Other => "other",
    }
}

impl AuditOrigin {
    /// The origin of an IPC client by its self-declared name label (`console`, `cli`,
    /// anything else is `other`).
    #[must_use]
    pub fn of_client(label: &str) -> Self {
        match label {
            "console" => Self::Console,
            "cli" => Self::Cli,
            _ => Self::Other,
        }
    }
}

audit_enum! {
    /// What was done. The catalog is closed; `docs/spec/audit.md` §3 lists the details
    /// each action carries.
    pub enum AuditAction {
        /// A runtime document was validated.
        ConfigValidate => "config_validate",
        /// A runtime document was saved as the next version and applied.
        ConfigApply => "config_apply",
        /// An earlier version was applied again.
        ConfigRollback => "config_rollback",
        /// A version was signed off as verified.
        ConfigVerify => "config_verify",
        /// A channel was paused.
        ChannelPause => "channel_pause",
        /// A channel was resumed.
        ChannelResume => "channel_resume",
        /// A channel was restarted.
        ChannelRestart => "channel_restart",
        /// A message was resent.
        MessageResend => "message_resend",
        /// A delivery was closed.
        DeliveryClose => "delivery_close",
        /// A secret value was stored (the name only is recorded).
        SecretSet => "secret_set",
        /// A secret was removed.
        SecretDelete => "secret_delete",
        /// An original or transformed body was shown.
        MessageBodyView => "message_body_view",
        /// A message's parsed fields were shown.
        MessageFieldsView => "message_fields_view",
        /// Messages were searched by identifier, text or id prefix (the filter is recorded).
        MessageSearch => "message_search",
        /// Messages were exported.
        MessageExport => "message_export",
        /// A transmission verification report was produced.
        TransmissionVerify => "transmission_verify",
        /// Audit events were exported.
        AuditExport => "audit_export",
        /// The audit chain was verified.
        AuditVerify => "audit_verify",
        /// A backup was created.
        BackupCreate => "backup_create",
        /// A backup was restored into the state directory.
        BackupRestore => "backup_restore",
        /// A channel's live traffic transcript was opened (a content view).
        TrafficView => "traffic_view",
        /// A client wrote an export it built itself, such as the console's change report.
        ClientExport => "client_export",
        /// An endpoint definition was probed (kind and settings summary are recorded).
        EndpointTest => "endpoint_test",
        /// A test message was sent to the notification targets.
        NotificationTest => "notification_test",
        /// The engine started.
        EngineStart => "engine_start",
        /// The engine stopped.
        EngineStop => "engine_stop",
        /// A management connection or command was refused for lack of authorization.
        AccessRefused => "access_refused",
        /// The chain head was published to the engine log (daily and at shutdown).
        AuditAnchor => "audit_anchor",
        /// The current audit file was sealed at the turn of the year.
        AuditSeal => "audit_seal",
        /// Sealed audit files past retention were removed.
        AuditPrune => "audit_prune",
    }
}

audit_enum! {
    /// The kind of object an action concerns.
    pub enum AuditObjectType {
        /// A channel id.
        Channel => "channel",
        /// A message id.
        Message => "message",
        /// A delivery id.
        Delivery => "delivery",
        /// A configuration version number.
        ConfigVersion => "config_version",
        /// A secret name.
        Secret => "secret",
        /// A backup directory.
        Backup => "backup",
        /// The audit log (an audit file name).
        Audit => "audit",
        /// The engine (its node id).
        Engine => "engine",
    }
}

audit_enum! {
    /// How an action ended.
    pub enum AuditOutcome {
        /// A command is about to run; its result follows as a second event.
        Requested => "requested",
        /// It was done.
        Succeeded => "succeeded",
        /// A precondition or validation refused it; nothing changed.
        Rejected => "rejected",
        /// It failed while running.
        Failed => "failed",
    }
}

/// One audit event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AuditEvent {
    /// Position in the chain, from 1, without gaps.
    pub seq: u64,
    /// When it was recorded (UTC).
    pub at: Timestamp,
    /// The OS account's numeric id (Unix uid) or SID (Windows); empty for the engine.
    pub actor_uid: String,
    /// The OS account name; `system` for the engine.
    pub actor_name: String,
    /// Where it came from.
    pub origin: AuditOrigin,
    /// What was done.
    pub action: AuditAction,
    /// The kind of object.
    pub object_type: Option<AuditObjectType>,
    /// The object's id (channel id, message id, version number, ...).
    pub object_id: Option<String>,
    /// How it ended.
    pub outcome: AuditOutcome,
    /// Identifiers, versions, counts and codes; never message content.
    #[ts(type = "Record<string, unknown>")]
    pub details: serde_json::Value,
    /// The previous event's `hash` (64 zeros before the first event).
    pub prev_hash: String,
    /// SHA-256 over `prev_hash` and this event's canonical encoding, hex.
    pub hash: String,
}

/// Which audit events. Every field narrows the result; all are optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuditFilter {
    /// Recorded at or after.
    #[serde(default)]
    pub since: Option<Timestamp>,
    /// Recorded before.
    #[serde(default)]
    pub until: Option<Timestamp>,
    /// The account: its name or uid/SID, exactly.
    #[serde(default)]
    pub actor: Option<String>,
    /// The action.
    #[serde(default)]
    pub action: Option<AuditAction>,
    /// The origin.
    #[serde(default)]
    pub origin: Option<AuditOrigin>,
    /// The outcome.
    #[serde(default)]
    pub outcome: Option<AuditOutcome>,
    /// The object kind.
    #[serde(default)]
    pub object_type: Option<AuditObjectType>,
    /// The object id, exactly (with or without `object_type`).
    #[serde(default)]
    pub object_id: Option<String>,
}

/// A page of the audit log, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuditQuery {
    /// Which events.
    #[serde(default)]
    pub filter: AuditFilter,
    /// Only events with a lower `seq` (the previous page's `next_before_seq`).
    #[serde(default)]
    pub before_seq: Option<u64>,
    /// Events to return, 1 to [`MAX_AUDIT_PAGE`]. Default [`DEFAULT_AUDIT_PAGE`].
    #[serde(default = "default_page")]
    pub limit: u32,
}

impl Default for AuditQuery {
    fn default() -> Self {
        Self {
            filter: AuditFilter::default(),
            before_seq: None,
            limit: DEFAULT_AUDIT_PAGE,
        }
    }
}

const fn default_page() -> u32 {
    DEFAULT_AUDIT_PAGE
}

impl AuditQuery {
    /// Checks the page size; the text is for the operator.
    pub fn check(&self) -> Result<(), String> {
        if (1..=MAX_AUDIT_PAGE).contains(&self.limit) {
            Ok(())
        } else {
            Err(format!("limit must be from 1 to {MAX_AUDIT_PAGE}"))
        }
    }
}

/// Answer to [`AuditQuery`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AuditPage {
    /// Matching events, newest first.
    pub events: Vec<AuditEvent>,
    /// Pass as `before_seq` for the next page; absent on the last page.
    pub next_before_seq: Option<u64>,
}

/// Export file format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AuditExportFormat {
    /// RFC 4180 CSV with a header row in the first chunk.
    #[default]
    Csv,
    /// One JSON [`AuditEvent`] per line.
    JsonLines,
}

/// Export matching events oldest first, one chunk per request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuditExportRequest {
    /// Which events.
    #[serde(default)]
    pub filter: AuditFilter,
    /// The format.
    #[serde(default)]
    pub format: AuditExportFormat,
    /// Continue after this `seq` (the previous chunk's `next_after_seq`); absent for the
    /// first chunk, which is the one the engine audits.
    #[serde(default)]
    pub after_seq: Option<u64>,
    /// The first chunk's `export_id`, required with every later chunk: only the first chunk
    /// is audited, so a continuation must belong to an export that was.
    #[serde(default)]
    pub export_id: Option<String>,
}

/// One chunk of an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuditExportChunk {
    /// The format of `content`.
    pub format: AuditExportFormat,
    /// The text to append to the export file.
    pub content: String,
    /// Events in this chunk.
    pub rows: u32,
    /// Pass as `after_seq` for the next chunk; absent when the export is complete.
    pub next_after_seq: Option<u64>,
    /// Pass with every later chunk of this export; absent after the last one.
    #[serde(default)]
    pub export_id: Option<String>,
}

/// What a chain verification found wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AuditProblemKind {
    /// An event's stored hash differs from the hash of its content: it was edited.
    HashMismatch,
    /// An event's `prev_hash` is not the previous event's hash: events were removed,
    /// inserted or reordered.
    BrokenLink,
    /// `seq` does not continue the previous event's: events were removed or reordered.
    SequenceGap,
    /// The oldest kept event neither starts the chain nor follows a recorded prune.
    UnexplainedStart,
    /// A file could not be read or is not an audit file.
    Unreadable,
}

/// One verification problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuditProblem {
    /// The audit file, by name.
    pub file: String,
    /// The event concerned, when there is one.
    pub seq: Option<u64>,
    /// What is wrong.
    pub kind: AuditProblemKind,
    /// Operator-facing explanation.
    pub message: String,
}

/// Result of walking the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuditVerification {
    /// No problem found.
    pub ok: bool,
    /// Audit files walked (sealed years and the current file).
    pub files: u32,
    /// Events checked.
    pub events: u64,
    /// The oldest kept event.
    pub first_seq: Option<u64>,
    /// The newest event.
    pub last_seq: Option<u64>,
    /// The newest event's hash; compare it with the last `audit.anchor` log line.
    pub head_hash: Option<String>,
    /// Every problem, oldest first (at most 100).
    pub problems: Vec<AuditProblem>,
}
