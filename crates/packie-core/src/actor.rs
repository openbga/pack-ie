//! Who performed an action: the OS account behind a management connection, or the engine.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The OS identity behind a management request (requirement P1 in `docs/requirements.md`,
/// decision 13). The engine reads it from the connection itself (peer credentials of the
/// socket, the pipe client's token); a client cannot supply or change it. Actions the engine
/// takes on its own (retention purges, reconciliation, startup import) are attributed to
/// [`Actor::system`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct Actor {
    /// The account name as the OS reports it (`jdoe`, `LAB\jdoe`), or `system`.
    pub name: String,
    /// The numeric uid (Unix) or the SID (Windows) as text; empty for the engine itself.
    pub uid_or_sid: String,
    /// The kind of client, as the client declared it in its hello: a label for review, not
    /// part of the identity.
    pub origin: ActorOrigin,
    /// The client's process id, when the OS reports it.
    pub pid: Option<u32>,
}

/// The kind of client a request came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ActorOrigin {
    /// The desktop console.
    Console,
    /// The `packie` command line.
    Cli,
    /// The simulator and test tools.
    Sim,
    /// A client that has not said what it is (no hello yet, or an unknown name).
    Unknown,
    /// The engine itself.
    System,
}

impl ActorOrigin {
    /// The `snake_case` name, as serialized and logged.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Cli => "cli",
            Self::Sim => "sim",
            Self::Unknown => "unknown",
            Self::System => "system",
        }
    }
}

impl Actor {
    /// The engine acting on its own.
    #[must_use]
    pub fn system() -> Self {
        Self {
            name: "system".to_owned(),
            uid_or_sid: String::new(),
            origin: ActorOrigin::System,
            pid: None,
        }
    }

    /// Whether this is the engine itself.
    #[must_use]
    pub fn is_system(&self) -> bool {
        self.origin == ActorOrigin::System
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_for_logs_audit_and_typescript() {
        let actor = Actor {
            name: "jdoe".into(),
            uid_or_sid: "501".into(),
            origin: ActorOrigin::Console,
            pid: Some(4242),
        };
        let json = serde_json::to_value(&actor).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"name": "jdoe", "uid_or_sid": "501", "origin": "console", "pid": 4242})
        );
        assert_eq!(serde_json::from_value::<Actor>(json).unwrap(), actor);
        assert!(Actor::system().is_system());
        assert_eq!(ActorOrigin::Unknown.as_str(), "unknown");
    }
}
