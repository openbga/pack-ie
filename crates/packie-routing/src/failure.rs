//! A failure attributed to one side of a bridge, and its operator-facing summary
//! (`docs/spec/bridge-channels.md` §16).

use packie_core::{BridgeSide, ChannelId};

use crate::status::SideCell;
use packie_transport::{LinkError, Operation};

/// What ended an attempt or a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    /// The side it is attributed to.
    pub(crate) side: BridgeSide,
    /// The failed operation.
    pub(crate) operation: Operation,
    /// Short cause, never payload bytes.
    pub(crate) cause: String,
    /// Retrying alone will not fix it.
    pub(crate) persistent: bool,
}

impl Failure {
    /// A failure a retry may fix.
    pub(crate) fn new(side: BridgeSide, operation: Operation, cause: impl Into<String>) -> Self {
        Self {
            side,
            operation,
            cause: cause.into(),
            persistent: false,
        }
    }

    /// A transport failure on `side`.
    pub(crate) fn link(side: BridgeSide, error: LinkError) -> Self {
        Self {
            side,
            operation: error.operation,
            cause: error.cause,
            persistent: error.persistent,
        }
    }

    /// An internal fault (a pump that panicked, an invariant that did not hold).
    pub(crate) fn internal(side: BridgeSide, cause: impl Into<String>) -> Self {
        Self::new(
            side,
            Operation::Read,
            format!("internal error: {}", cause.into()),
        )
    }

    /// `Channel {id} endpoint {A|B} "{label}" {operation} failed: {cause}.`
    pub(crate) fn summary(&self, channel: ChannelId, cell: &SideCell) -> String {
        format!(
            "Channel {channel} endpoint {} \"{}\" {} failed: {}.",
            cell.role_label(),
            cell.label,
            self.operation,
            self.cause
        )
    }
}
