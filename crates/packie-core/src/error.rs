//! Errors for parsing the values defined in this crate.

/// Text that is not a valid core value. Messages never echo the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Not a name matching `^[a-z0-9][a-z0-9._-]{0,63}$`.
    #[error(
        "not a valid id (lowercase letters, digits, '.', '_' or '-', starting with a letter or digit, at most 64)"
    )]
    Name,
    /// Not a UUIDv7 in canonical lowercase hyphenated form.
    #[error("not a valid identifier (expected a lowercase UUIDv7)")]
    Id,
    /// Not an RFC 3339 timestamp with an offset.
    #[error("not a valid timestamp (expected RFC 3339, for example 2026-11-06T12:00:00Z)")]
    Timestamp,
    /// Not one of the enum's names.
    #[error("not a known value")]
    Enum,
}
