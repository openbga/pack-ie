//! Export sessions. Only the first chunk of a message or audit export is audited, so every
//! later chunk must carry the `export_id` the first chunk issued: same account, same filter,
//! format and content choice. Continuations are also refused while the audit log cannot be
//! written, as audited requests are (fail closed).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use packie_core::Actor;
use packie_ipc::{ErrorCode, IpcError, Request, Response};
use serde_json::{Value, json};

/// An export unused this long is forgotten; its continuation is refused.
const IDLE_LIMIT: Duration = Duration::from_mins(30);
/// Most open exports; the least recently used is forgotten first.
const MAX_SESSIONS: usize = 64;

/// One chunk request of an export.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Chunk {
    /// What the export selects and how, without the position.
    key: Value,
    /// The first chunk (the audited one).
    first: bool,
    /// The `export_id` the request carries.
    token: Option<String>,
}

impl Chunk {
    /// The first chunk of its export (the audited one).
    pub(crate) const fn is_first(&self) -> bool {
        self.first
    }

    /// The export chunk `request` asks for, if it is one.
    pub(crate) fn of(request: &Request) -> Option<Self> {
        match request {
            Request::ExportMessages(export) => Some(Self {
                key: json!({
                    "export": "messages", "filter": export.filter, "format": export.format,
                    "include_bodies": export.include_bodies,
                }),
                first: export.offset == 0,
                token: export.export_id.clone(),
            }),
            Request::ExportAudit(export) => Some(Self {
                key: json!({ "export": "audit", "filter": export.filter, "format": export.format }),
                first: export.after_seq.is_none(),
                token: export.export_id.clone(),
            }),
            _ => None,
        }
    }
}

struct Session {
    key: Value,
    account: String,
    used: Instant,
}

/// The exports whose first chunk was answered and that have more chunks to come.
#[derive(Default)]
pub(crate) struct ExportSessions {
    sessions: Mutex<HashMap<String, Session>>,
}

impl ExportSessions {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Admits a continuation only within its audited export and while the audit log can be
    /// written; a first chunk is checked by the audit record it writes.
    pub(crate) fn admit(
        &self,
        chunk: &Chunk,
        actor: &Actor,
        audit_available: bool,
    ) -> Result<(), IpcError> {
        if chunk.first {
            return Ok(());
        }
        if !audit_available {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "The audit log cannot be written, so the export cannot continue. Check disk \
                 space and the state directory's audit folder, then export again.",
            ));
        }
        let mut sessions = self.lock();
        sessions.retain(|_, s| s.used.elapsed() < IDLE_LIMIT);
        let session = chunk.token.as_ref().and_then(|t| sessions.get_mut(t));
        match session {
            Some(s) if s.key == chunk.key && s.account == actor.uid_or_sid => {
                s.used = Instant::now();
                Ok(())
            }
            _ => Err(IpcError::new(
                ErrorCode::BadRequest,
                "This chunk does not continue an export of this account: pass the first \
                 chunk's export_id with the same filter and format, or start the export again.",
            )),
        }
    }

    /// Issues the `export_id` after a first chunk with more to come, and forgets the export
    /// after its last chunk.
    pub(crate) fn answered(
        &self,
        chunk: Chunk,
        actor: &Actor,
        result: &mut Result<Response, IpcError>,
    ) {
        let (export_id, more) = match result {
            Ok(Response::MessageExport(c)) => (&mut c.export_id, c.next_offset.is_some()),
            Ok(Response::AuditExport(c)) => (&mut c.export_id, c.next_after_seq.is_some()),
            _ => return,
        };
        let mut sessions = self.lock();
        match (chunk.first, more) {
            (true, true) => *export_id = Some(open(&mut sessions, chunk.key, actor)),
            (true, false) => {}
            (false, true) => export_id.clone_from(&chunk.token),
            (false, false) => {
                if let Some(token) = &chunk.token {
                    sessions.remove(token);
                }
            }
        }
    }
}

/// Opens a session for `key`, forgetting the least recently used one when full; returns its
/// `export_id`.
fn open(sessions: &mut HashMap<String, Session>, key: Value, actor: &Actor) -> String {
    if sessions.len() >= MAX_SESSIONS {
        let oldest = sessions
            .iter()
            .min_by_key(|(_, s)| s.used)
            .map(|(t, _)| t.clone());
        if let Some(oldest) = oldest {
            sessions.remove(&oldest);
        }
    }
    let token = uuid::Uuid::now_v7().simple().to_string();
    let session = Session {
        key,
        account: actor.uid_or_sid.clone(),
        used: Instant::now(),
    };
    sessions.insert(token.clone(), session);
    token
}

#[cfg(test)]
mod tests {
    use packie_ipc::reports::{ExportChunk, ExportRequest};

    use super::*;

    fn actor(uid: &str) -> Actor {
        Actor {
            name: "lab".into(),
            uid_or_sid: uid.into(),
            origin: packie_core::ActorOrigin::Cli,
            pid: None,
        }
    }

    fn request(offset: u32, bodies: bool, export_id: Option<String>) -> Request {
        Request::ExportMessages(ExportRequest {
            filter: packie_ipc::messages::MessageFilter::default(),
            format: packie_ipc::reports::ExportFormat::Csv,
            include_bodies: bodies,
            offset,
            export_id,
        })
    }

    /// The answer to a message export chunk.
    fn answer(next_offset: Option<u32>) -> Response {
        let chunk = ExportChunk {
            content: String::new(),
            count: 100,
            total: 300,
            next_offset,
            export_id: None,
        };
        Response::MessageExport(chunk)
    }

    fn issued(result: &Result<Response, IpcError>) -> Option<String> {
        match result {
            Ok(Response::MessageExport(c)) => c.export_id.clone(),
            _ => None,
        }
    }

    #[test]
    fn a_continuation_needs_the_first_chunks_session() {
        let sessions = ExportSessions::default();
        let lab = actor("501");
        let orphan = Chunk::of(&request(100, true, None)).unwrap();
        assert!(sessions.admit(&orphan, &lab, true).is_err());

        let first = Chunk::of(&request(0, true, None)).unwrap();
        sessions.admit(&first, &lab, true).unwrap();
        let mut opened = Ok(answer(Some(100)));
        sessions.answered(first, &lab, &mut opened);
        let token = issued(&opened).unwrap();

        let next = Chunk::of(&request(100, true, Some(token.clone()))).unwrap();
        assert!(sessions.admit(&next, &actor("502"), true).is_err());
        assert_eq!(
            sessions.admit(&next, &lab, false).unwrap_err().code,
            ErrorCode::Unavailable
        );
        let widened = Chunk::of(&request(100, false, Some(token))).unwrap();
        assert!(sessions.admit(&widened, &lab, true).is_err());
        sessions.admit(&next, &lab, true).unwrap();

        let mut last = Ok(answer(None));
        sessions.answered(next.clone(), &lab, &mut last);
        assert!(issued(&last).is_none());
        assert!(
            sessions.admit(&next, &lab, true).is_err(),
            "the export is finished"
        );
    }

    #[test]
    fn an_audit_export_continuation_is_checked_too() {
        let sessions = ExportSessions::default();
        let request = Request::ExportAudit(packie_ipc::audit::AuditExportRequest {
            filter: packie_ipc::audit::AuditFilter::default(),
            format: packie_ipc::audit::AuditExportFormat::Csv,
            after_seq: Some(0),
            export_id: None,
        });
        let chunk = Chunk::of(&request).unwrap();
        assert!(sessions.admit(&chunk, &actor("501"), true).is_err());
    }
}
