//! The problem report of validation: [`Problem`], its codes and severities, and the
//! [`ValidationErrors`] of a rejected document.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Whether a problem rejects the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProblemSeverity {
    /// The document is rejected.
    Error,
    /// Accepted; the operator should look.
    Warning,
}

/// Stable problem codes (§9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProblemCode {
    /// Type, range, enumeration, unknown field, missing required field.
    Schema,
    /// An id repeated within its scope.
    DuplicateId,
    /// A channel references an endpoint that does not exist.
    UnknownEndpoint,
    /// An enabled channel references a disabled endpoint.
    EndpointDisabled,
    /// A bridge endpoint is not a stream, or a protocol does not suit the endpoint kind.
    EndpointKindNotAllowed,
    /// A bridge's `endpoint_a` equals `endpoint_b`.
    SameEndpoint,
    /// Two enabled endpoints use the same serial port or overlapping listener addresses.
    PortConflict,
    /// `wait_for_listener_then_pair` without a `tcp_listener` endpoint.
    ListenerStrategy,
    /// `receiver` missing for `receiver_then_sender`, or set with another strategy.
    ReceiverRequired,
    /// A non-loopback listener without an allowlist or `allow_any_peer`.
    AllowlistRequired,
    /// An `allowed_peer_cidrs` entry is not valid CIDR notation.
    InvalidCidr,
    /// A retry number out of range (or a multiplier with more than 4 decimal places), or
    /// `max_delay_ms < initial_delay_ms`.
    RetryInvalid,
    /// `record_messages` with framer `none`.
    TapInvalid,
    /// `ack_required` above what the protocol can prove.
    AckLevelUnreachable,
    /// A pass-through destination (`mapping: null`) with another protocol than the source.
    PassthroughProtocol,
    /// A mapping that is not well formed.
    MappingInvalid,
    /// A filter expression that is not well formed.
    FilterInvalid,
    /// A secret field holds a literal instead of `{"secret": name}`.
    SecretInline,
    /// A referenced secret does not resolve now (warning; checked by the engine).
    SecretUnresolved,
    /// More than one enabled channel references the same serial or listener endpoint.
    SharedExclusiveEndpoint,
    /// A message channel or recording tap retention without either limit.
    RetentionUnbounded,
}

/// One problem with a document, at a field path such as `channels[1].connection.receiver`.
/// An empty path means the document as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Problem {
    /// Error or warning.
    pub severity: ProblemSeverity,
    /// Stable code.
    pub code: ProblemCode,
    /// Field path.
    pub path: String,
    /// What is wrong, for the operator.
    pub message: String,
}

impl Problem {
    /// An error.
    pub fn error(code: ProblemCode, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: ProblemSeverity::Error,
            code,
            path: path.into(),
            message: message.into(),
        }
    }

    /// A warning.
    pub fn warning(code: ProblemCode, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: ProblemSeverity::Warning,
            ..Self::error(code, path, message)
        }
    }

    /// Whether this rejects the document.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == ProblemSeverity::Error
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.severity == ProblemSeverity::Warning {
            f.write_str("warning: ")?;
        }
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// Every problem of a rejected document (at least one error).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct ValidationErrors(Vec<Problem>);

impl ValidationErrors {
    /// All problems, errors and warnings.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.0
    }

    /// All problems, by value.
    #[must_use]
    pub fn into_problems(self) -> Vec<Problem> {
        self.0
    }
}

impl From<Vec<Problem>> for ValidationErrors {
    fn from(problems: Vec<Problem>) -> Self {
        Self(problems)
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.0.iter().filter(|p| p.is_error()).count();
        write!(
            f,
            "{count} configuration error{}",
            if count == 1 { "" } else { "s" }
        )?;
        for problem in &self.0 {
            write!(f, "\n  {problem}")?;
        }
        Ok(())
    }
}
