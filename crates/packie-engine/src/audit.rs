//! The engine's side of the audit log (`docs/spec/audit.md`, requirements P2 and P6): which
//! requests are audited, with what, and the fail-closed rule.
//!
//! The log itself is an [`AuditLog`] the host supplies through
//! [`RunOptions::audit`](crate::RunOptions::audit) (the `packie` binary passes the persistent
//! one). An audited command is recorded as `requested` before it runs; when that record
//! cannot be written the command is refused and nothing happens. Its outcome is recorded
//! after it ran. Content views (bodies, parsed fields) and identifier searches are audited
//! the same way, so content never leaves the engine unrecorded.

use std::sync::{Arc, Mutex, PoisonError};

use packie_config::Bootstrap;
use packie_core::Actor;
use packie_ipc::audit::{
    AuditAction, AuditExportChunk, AuditExportRequest, AuditObjectType, AuditOutcome, AuditPage,
    AuditQuery, AuditVerification,
};
use packie_ipc::messages::{BodySelector, ResendTargets};
use packie_ipc::{ChannelCommand, Request};
use serde_json::{Value, json};

/// One audit event as the engine hands it to the log.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditRecord {
    /// Who: the OS account behind the request, or [`Actor::system`].
    pub actor: Actor,
    /// What.
    pub action: AuditAction,
    /// On which object, with its id.
    pub object: Option<(AuditObjectType, String)>,
    /// `requested` before the command runs, then how it ended.
    pub outcome: AuditOutcome,
    /// A JSON object of ids, versions, counts, codes and the operator's own text (reasons,
    /// search terms); never message bodies.
    pub details: Value,
}

/// The audit log could not record an event; audited commands must be refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the audit log cannot be written: {0}")]
pub struct AuditUnavailable(pub String);

/// The persistent, append-only audit log the engine records to and serves.
pub trait AuditLog: Send + Sync {
    /// Appends `record`; `Err` when it cannot be stored (the caller refuses the command).
    fn record(&self, record: &AuditRecord) -> Result<u64, AuditUnavailable>;

    /// A page of events, newest first.
    fn query(&self, query: &AuditQuery) -> Result<AuditPage, AuditUnavailable>;

    /// One export chunk, oldest first.
    fn export(&self, request: &AuditExportRequest) -> Result<AuditExportChunk, AuditUnavailable>;

    /// Walks the hash chain.
    fn verify(&self) -> Result<AuditVerification, AuditUnavailable>;

    /// Whether the log can currently be written; `false` raises `audit_unavailable`.
    fn available(&self) -> bool {
        true
    }

    /// Checks now, without recording an event, that the log can be written (for requests
    /// that rely on an earlier audit event, such as export continuations). Default: the
    /// last observed health.
    fn check_writable(&self) -> Result<(), AuditUnavailable> {
        if self.available() {
            Ok(())
        } else {
            Err(AuditUnavailable("the audit log is unavailable".into()))
        }
    }
}

/// Opens the audit log of an engine started from this bootstrap (`<state>/audit`, kept
/// `audit.retention_days`).
pub type AuditOpener = fn(&Bootstrap) -> Result<Arc<dyn AuditLog>, AuditUnavailable>;

/// An in-memory log for tests and in-process hosts: records are kept and can be read back
/// with [`MemoryAudit::records`]; nothing persists and queries answer `unavailable`.
#[derive(Debug, Default)]
pub struct MemoryAudit {
    records: Mutex<Vec<AuditRecord>>,
}

impl MemoryAudit {
    /// The opener for [`RunOptions::audit`](crate::RunOptions::audit).
    pub fn open(_bootstrap: &Bootstrap) -> Result<Arc<dyn AuditLog>, AuditUnavailable> {
        Ok(Arc::new(Self::default()))
    }

    /// Everything recorded so far, oldest first.
    #[must_use]
    pub fn records(&self) -> Vec<AuditRecord> {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn not_persistent() -> AuditUnavailable {
    AuditUnavailable("this engine keeps its audit log in memory only".into())
}

impl AuditLog for MemoryAudit {
    fn record(&self, record: &AuditRecord) -> Result<u64, AuditUnavailable> {
        let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        records.push(record.clone());
        Ok(u64::try_from(records.len()).unwrap_or(u64::MAX))
    }

    fn query(&self, _query: &AuditQuery) -> Result<AuditPage, AuditUnavailable> {
        Err(not_persistent())
    }

    fn export(&self, _request: &AuditExportRequest) -> Result<AuditExportChunk, AuditUnavailable> {
        Err(not_persistent())
    }

    fn verify(&self) -> Result<AuditVerification, AuditUnavailable> {
        Err(not_persistent())
    }
}

/// An audited request's action, the object it concerns and its details.
pub(crate) type Audited = (AuditAction, Option<(AuditObjectType, String)>, Value);

/// What an audited request is, or `None` when the request is not audited (snapshots, logs,
/// plain message lists, ...).
pub(crate) fn audited(request: &Request) -> Option<Audited> {
    use AuditObjectType as Object;
    let entry = match request {
        Request::ValidateConfig { .. } => (AuditAction::ConfigValidate, None, json!({})),
        Request::VerifyConfigVersion {
            config_version,
            note,
        } => (
            AuditAction::ConfigVerify,
            Some((Object::ConfigVersion, config_version.to_string())),
            json!({ "note": note }),
        ),
        Request::PutConfig {
            base_version, note, ..
        } => config_apply(*base_version, note.as_deref()),
        Request::ChannelCommand {
            channel_id,
            command,
        } => {
            let action = match command {
                ChannelCommand::Pause => AuditAction::ChannelPause,
                ChannelCommand::Resume => AuditAction::ChannelResume,
                ChannelCommand::Restart => AuditAction::ChannelRestart,
            };
            (
                action,
                Some((Object::Channel, channel_id.to_string())),
                json!({}),
            )
        }
        Request::SetSecret { name, .. } => (
            AuditAction::SecretSet,
            Some((Object::Secret, name.clone())),
            json!({}),
        ),
        Request::DeleteSecret { name } => (
            AuditAction::SecretDelete,
            Some((Object::Secret, name.clone())),
            json!({}),
        ),
        Request::ExportAudit(export) if export.after_seq.is_none() => (
            AuditAction::AuditExport,
            Some((Object::Audit, "audit".into())),
            json!({ "format": export.format }),
        ),
        Request::CreateBackup { directory } => (
            AuditAction::BackupCreate,
            Some((
                Object::Backup,
                directory
                    .clone()
                    .unwrap_or_else(|| "backup.directory".into()),
            )),
            json!({ "scheduled": false }),
        ),
        Request::VerifyAudit => (
            AuditAction::AuditVerify,
            Some((Object::Audit, "audit".into())),
            json!({}),
        ),
        Request::TestEndpoint(test) => (
            AuditAction::EndpointTest,
            None,
            json!({ "kind": test.kind, "target": crate::checks::probe_target(test) }),
        ),
        Request::TestNotification => (AuditAction::NotificationTest, None, json!({})),
        Request::RecordClientExport { kind, rows } => (
            AuditAction::ClientExport,
            None,
            json!({ "kind": kind, "rows": rows }),
        ),
        other => return message_action(other),
    };
    Some(entry)
}

fn config_apply(base_version: u64, note: Option<&str>) -> Audited {
    let action = if note.is_some_and(|n| n.starts_with("rollback of")) {
        AuditAction::ConfigRollback
    } else {
        AuditAction::ConfigApply
    };
    (
        action,
        None,
        json!({ "base_version": base_version, "note": note }),
    )
}

/// Message commands, content views and identifier searches.
fn message_action(request: &Request) -> Option<Audited> {
    use AuditObjectType as Object;
    let entry = match request {
        Request::Resend(resend) => (
            AuditAction::MessageResend,
            Some((Object::Message, resend.message_id.to_string())),
            json!({
                "destinations": match &resend.destinations {
                    ResendTargets::All => json!("all"),
                    ResendTargets::Destinations(ids) => json!(ids),
                },
                "mode": resend.mode,
                "confirm_duplicate_risk": resend.confirm_duplicate_risk,
                "confirm_superseded": resend.confirm_superseded, "reason": resend.reason,
            }),
        ),
        Request::CloseDelivery(close) => (
            AuditAction::DeliveryClose,
            Some((Object::Delivery, close.delivery_id.to_string())),
            json!({ "reason": close.reason }),
        ),
        Request::GetMessageBody(body) => {
            let which = match body.which {
                BodySelector::Original => json!({ "body": "original" }),
                BodySelector::Transformed { delivery_id } => {
                    json!({ "body": "transformed", "delivery_id": delivery_id })
                }
            };
            (
                AuditAction::MessageBodyView,
                Some((Object::Message, body.message_id.to_string())),
                which,
            )
        }
        Request::GetMessageFields { message_id } => (
            AuditAction::MessageFieldsView,
            Some((Object::Message, message_id.to_string())),
            json!({}),
        ),
        Request::GetMessages(query) => return search(query),
        // The first chunk stands for the whole export; the count is in the outcome record.
        Request::ExportMessages(export) if export.offset == 0 => (
            AuditAction::MessageExport,
            None,
            json!({
                "filter": export.filter, "format": export.format,
                "include_bodies": export.include_bodies,
            }),
        ),
        Request::VerifyTransmission(query) => (
            AuditAction::TransmissionVerify,
            Some((Object::Channel, query.channel_id.to_string())),
            json!({ "since": query.since, "until": query.until }),
        ),
        _ => return None,
    };
    Some(entry)
}

/// A message search is audited when it looks for an identifier or text (who looked for
/// which patient or sample); browsing by channel, state or time is not.
fn search(query: &packie_ipc::messages::MessageQuery) -> Option<Audited> {
    let filter = &query.filter;
    if filter.identifier.is_none() && filter.text.is_none() && filter.message_id_prefix.is_none() {
        return None;
    }
    let details = json!({
        "identifier": filter.identifier.as_ref().map(|i| &i.value),
        "identifier_prefix": filter.identifier.as_ref().map(|i| i.prefix),
        "text": filter.text, "message_id_prefix": filter.message_id_prefix,
        "channels": filter.channels, "since": filter.since, "until": filter.until,
    });
    Some((AuditAction::MessageSearch, None, details))
}

#[cfg(test)]
mod tests {
    use packie_ipc::messages::{IdentifierFilter, MessageFilter, MessageQuery};
    use packie_ipc::reports::ExportRequest;

    use super::*;

    #[test]
    fn a_resend_records_its_destinations_as_a_plain_list() {
        let resend = |destinations| {
            let request = Request::Resend(packie_ipc::messages::ResendRequest {
                message_id: packie_core::MessageId::generate(),
                destinations,
                mode: packie_ipc::messages::ResendMode::default(),
                confirm_duplicate_risk: false,
                confirm_superseded: false,
                reason: None,
            });
            audited(&request).unwrap().2
        };
        let lis: packie_core::ConnectorId = "lis".parse().unwrap();
        assert_eq!(
            resend(ResendTargets::Destinations(vec![lis]))["destinations"],
            json!(["lis"])
        );
        assert_eq!(resend(ResendTargets::All)["destinations"], json!("all"));
    }

    #[test]
    fn searches_exports_and_views_are_audited_but_browsing_is_not() {
        let browse = Request::GetMessages(Box::default());
        assert!(audited(&browse).is_none());
        let search = Request::GetMessages(Box::new(MessageQuery {
            filter: MessageFilter {
                identifier: Some(IdentifierFilter {
                    value: "MRN-00000".into(),
                    prefix: false,
                }),
                ..MessageFilter::default()
            },
            ..MessageQuery::default()
        }));
        let (action, _, details) = audited(&search).unwrap();
        assert_eq!(action, AuditAction::MessageSearch);
        assert_eq!(details["identifier"], "MRN-00000");

        let first = ExportRequest {
            filter: MessageFilter::default(),
            format: packie_ipc::reports::ExportFormat::Csv,
            include_bodies: true,
            offset: 0,
            export_id: None,
        };
        let (action, _, details) = audited(&Request::ExportMessages(first.clone())).unwrap();
        assert_eq!(action, AuditAction::MessageExport);
        assert_eq!(details["include_bodies"], true);
        let later = ExportRequest {
            offset: 100,
            ..first
        };
        assert!(audited(&Request::ExportMessages(later)).is_none());

        let fields = Request::GetMessageFields {
            message_id: packie_core::MessageId::generate(),
        };
        assert_eq!(audited(&fields).unwrap().0, AuditAction::MessageFieldsView);
        let record = Request::GetMessage {
            message_id: packie_core::MessageId::generate(),
        };
        assert!(audited(&record).is_none());
    }
}
