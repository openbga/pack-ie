//! The audit log's error type.

/// Audit operation result.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors carry metadata only, never event details.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The audit log cannot be written; audited commands must be refused.
    #[error("the audit log cannot be written: {0}")]
    Unavailable(String),
    /// The audit database failed.
    #[error("audit database error: {0}")]
    Sql(#[from] rusqlite::Error),
    /// An audit file or directory could not be read, written or removed.
    #[error("audit file I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// A stored or exported event could not be encoded or decoded as JSON.
    #[error("audit JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// An export could not be written as CSV.
    #[error("audit CSV error: {0}")]
    Csv(#[from] csv::Error),
    /// The entry or request is malformed (a caller defect).
    #[error("invalid audit entry: {0}")]
    Invalid(String),
}

impl Error {
    /// A malformed entry, request or stored value.
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
