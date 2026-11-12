//! The event to record: who did what to which object, and how it ended.

use packie_ipc::audit::{AuditAction, AuditObjectType, AuditOrigin, AuditOutcome};

use crate::{Actor, ActorOrigin, Error, Result, chain};

/// Largest accepted `details`, as canonical JSON.
pub const MAX_DETAILS: usize = 4096;

/// Longest accepted actor name, uid or SID, and object id, in bytes.
const MAX_TEXT: usize = 256;

/// The account running this process, for CLI commands that work without an engine
/// (offline backup and restore), with origin `cli`: the uid (Unix) or the SID of the
/// process token (Windows), never environment variables.
///
/// # Errors
///
/// [`Error::Unavailable`] when the OS cannot say (Windows); the command must not run
/// unattributed.
pub fn this_process() -> Result<Actor> {
    #[cfg(unix)]
    let (name, uid_or_sid) = {
        let uid = nix::unistd::getuid();
        let name = nix::unistd::User::from_uid(uid)
            .ok()
            .flatten()
            .map_or_else(|| uid.to_string(), |user| user.name);
        (name, uid.to_string())
    };
    #[cfg(windows)]
    let (name, uid_or_sid) = packie_ipc::this_process_account().map_err(Error::Unavailable)?;
    Ok(Actor {
        name,
        uid_or_sid,
        origin: ActorOrigin::Cli,
        pid: Some(std::process::id()),
    })
}

/// The `origin` column of an actor: `sim` and `unknown` clients are `other`.
#[must_use]
pub const fn origin_of(actor: &Actor) -> AuditOrigin {
    match actor.origin {
        ActorOrigin::Console => AuditOrigin::Console,
        ActorOrigin::Cli => AuditOrigin::Cli,
        ActorOrigin::System => AuditOrigin::Engine,
        ActorOrigin::Sim | ActorOrigin::Unknown => AuditOrigin::Other,
    }
}

/// One event to record.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Who, and from which kind of client (the `origin` column, [`origin_of`]).
    pub actor: Actor,
    /// What.
    pub action: AuditAction,
    /// On which object.
    pub object: Option<(AuditObjectType, String)>,
    /// How it ended. Default `succeeded`.
    pub outcome: AuditOutcome,
    /// A JSON object of identifiers, versions, counts, codes and text the operator entered;
    /// never message bodies. At most [`MAX_DETAILS`] bytes.
    pub details: serde_json::Value,
}

impl Entry {
    /// A `succeeded` event without object or details.
    #[must_use]
    pub fn new(actor: Actor, action: AuditAction) -> Self {
        Self {
            actor,
            action,
            object: None,
            outcome: AuditOutcome::Succeeded,
            details: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// With the object it concerns.
    #[must_use]
    pub fn object(mut self, kind: AuditObjectType, id: impl Into<String>) -> Self {
        self.object = Some((kind, id.into()));
        self
    }

    /// With an outcome.
    #[must_use]
    pub fn outcome(mut self, outcome: AuditOutcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// With details (a JSON object).
    #[must_use]
    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    /// The canonical details text, after checking the entry's limits.
    pub(crate) fn checked_details(&self) -> Result<String> {
        if !self.details.is_object() {
            return Err(Error::invalid("details must be a JSON object"));
        }
        let text = chain::canonical_json(&self.details);
        if text.len() > MAX_DETAILS {
            return Err(Error::invalid(format!(
                "details exceed {MAX_DETAILS} bytes"
            )));
        }
        if self.actor.name.is_empty()
            || self.actor.name.len() > MAX_TEXT
            || self.actor.uid_or_sid.len() > MAX_TEXT
        {
            return Err(Error::invalid(format!(
                "the actor name must be 1 to {MAX_TEXT} bytes, its uid or SID at most {MAX_TEXT}"
            )));
        }
        if let Some((_, id)) = &self.object
            && (id.is_empty() || id.len() > MAX_TEXT)
        {
            return Err(Error::invalid(format!(
                "object id must be 1 to {MAX_TEXT} bytes"
            )));
        }
        Ok(text)
    }
}
