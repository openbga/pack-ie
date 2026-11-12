//! The error every command returns to the window, as `{ "kind": ..., ... }` JSON.

use serde::Serialize;
use packie_ipc::{ClientError, IpcError};

/// Why a command failed. The window branches on `kind`.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CommandError {
    /// The engine answered with an error; `error` carries its code, message and field errors.
    #[error("{error}")]
    Ipc {
        /// The engine's answer.
        error: IpcError,
    },
    /// There is no connection to the engine, or it broke during the request.
    #[error("{message}")]
    Disconnected {
        /// Names the endpoint and the cause.
        message: String,
    },
    /// Anything else: a timeout, a misuse of a command, a failed child process.
    #[error("{message}")]
    Internal {
        /// Operator-facing explanation.
        message: String,
    },
}

impl CommandError {
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    pub(crate) fn disconnected(message: impl Into<String>) -> Self {
        Self::Disconnected {
            message: message.into(),
        }
    }

    /// Maps a failed engine call; `endpoint` names where the engine was expected.
    pub(crate) fn from_client(error: ClientError, endpoint: &str) -> Self {
        match error {
            ClientError::Server(error) => Self::Ipc { error },
            ClientError::UnexpectedResponse => Self::internal(error.to_string()),
            ClientError::Connect(_) | ClientError::Frame(_) | ClientError::Closed => {
                Self::disconnected(format!(
                    "the connection to the engine at {endpoint} broke: {error}"
                ))
            }
        }
    }

    /// True when the connection itself failed, not just the request.
    pub(crate) fn is_connection_failure(error: &ClientError) -> bool {
        matches!(
            error,
            ClientError::Connect(_) | ClientError::Frame(_) | ClientError::Closed
        )
    }
}

#[cfg(test)]
mod tests {
    use packie_ipc::ErrorCode;

    use super::*;

    #[test]
    fn serializes_with_a_kind_tag() {
        let ipc = CommandError::from_client(
            ClientError::Server(IpcError::new(ErrorCode::Conflict, "stale")),
            "/tmp/x",
        );
        let json = serde_json::to_value(&ipc).unwrap();
        assert_eq!(json["kind"], "ipc");
        assert_eq!(json["error"]["code"], "conflict");
        assert_eq!(json["error"]["message"], "stale");
        assert!(json["error"]["field_errors"].is_array());

        let closed = CommandError::from_client(ClientError::Closed, "/tmp/x");
        let json = serde_json::to_value(&closed).unwrap();
        assert_eq!(json["kind"], "disconnected");
        assert!(json["message"].as_str().unwrap().contains("/tmp/x"));

        let json = serde_json::to_value(CommandError::internal("boom")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "internal", "message": "boom"})
        );
    }
}
