use crate::{Charset, Problem};
use thiserror::Error;

/// Codec errors never contain file contents. Lines and columns are one-based;
/// syntax columns are field numbers, typed-value columns are configured names.
#[derive(Debug, Error)]
pub enum Error {
    /// The template failed validation; every problem is listed.
    #[error("invalid template: {0:?}")]
    Template(Vec<Problem>),
    /// The file read, or the output written, exceeds the template's `max_file_bytes`.
    #[error("file is {actual} bytes; limit is {limit}")]
    FileTooLarge {
        /// Size of the file.
        actual: u64,
        /// The configured limit.
        limit: u64,
    },
    /// The bytes are not valid in the template's charset.
    #[error("invalid {charset:?} encoding at byte {offset}")]
    Encoding {
        /// The charset that failed to decode.
        charset: Charset,
        /// Byte offset of the first invalid byte.
        offset: usize,
    },
    /// A quote appears where it is not allowed, or text follows a closing quote.
    #[error("malformed quoting at line {line}, column {column}")]
    Syntax {
        /// Physical line of the offending field.
        line: usize,
        /// One-based field number.
        column: usize,
    },
    /// The file ends inside a quoted field.
    #[error("truncated quoted field at line {line}, column {column}")]
    Truncated {
        /// Physical line of the offending field.
        line: usize,
        /// One-based field number.
        column: usize,
    },
    /// A row has a different number of fields than the first row.
    #[error("ragged row at line {line}, column {column}: expected {expected} fields, got {actual}")]
    Ragged {
        /// Physical line of the row.
        line: usize,
        /// One-based number of the first missing or surplus field.
        column: usize,
        /// Field count the row should have.
        expected: usize,
        /// Field count the row has.
        actual: usize,
    },
    /// A configured column or the group-by column is absent.
    #[error("missing column {column} at line {line}")]
    MissingColumn {
        /// Line of the header or row that lacks the column.
        line: usize,
        /// Configured column name.
        column: String,
    },
    /// The header row has two fields that match case-insensitively.
    #[error("ambiguous header at line {line}, column {column}")]
    DuplicateHeader {
        /// Physical line of the header row.
        line: usize,
        /// One-based field number of the second occurrence.
        column: usize,
    },
    /// A value is empty but required, or does not match its column type.
    #[error("invalid value at line {line}, column {column}")]
    InvalidValue {
        /// Physical line of the row (the record number when writing).
        line: usize,
        /// Configured column name.
        column: String,
    },
    /// A field needs quotes but the template disables quoting.
    #[error("quoting disabled for field requiring quotes at row {line}, column {column}")]
    QuotingDisabled {
        /// One-based output row, header included.
        line: usize,
        /// One-based field number.
        column: usize,
    },
    /// The output contains a character the charset cannot encode.
    #[error("output cannot represent character in {charset:?}")]
    Unrepresentable {
        /// The output charset.
        charset: Charset,
    },
    /// The output cannot be built, for example a row wider than the writer allows.
    #[error("cannot serialize delimited output")]
    Serialize,
}

/// Path/set failure, with no field values in diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathError {
    /// The path is not `col.<name>` or `col[<index>]` of an existing field.
    #[error("unknown column path")]
    UnknownPath,
    /// The text is not valid for the column; the record is unchanged.
    #[error("invalid value for column {column}")]
    InvalidValue {
        /// Configured column name.
        column: String,
    },
}
