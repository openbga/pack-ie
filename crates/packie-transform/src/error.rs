//! Error types. None of them carries message content: they name rules, paths and
//! positions, never the values a message held (architecture invariant 6).

use std::fmt;

use thiserror::Error;

/// Why a path text is not valid, and where.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid path at position {position}: {reason}")]
pub struct PathError {
    /// Byte offset in the path text where the problem was found (the text length for
    /// a path that ends too early).
    pub position: usize,
    /// What is wrong there.
    pub reason: &'static str,
}

impl PathError {
    pub(crate) const fn new(position: usize, reason: &'static str) -> Self {
        Self { position, reason }
    }
}

/// A model could not complete a read-modify operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ModelError {
    /// The model is read-only (AU messages are only ever received).
    #[error("this message model is read-only")]
    ReadOnly,
    /// The addressed value is fixed (HL7 MSH-1 and MSH-2, ASTM record identifier and
    /// the H-record delimiter field).
    #[error("this field cannot be written")]
    Immutable,
    /// The path was written for another protocol's syntax.
    #[error("the path does not belong to this message model")]
    WrongSyntax,
    /// The record or segment cannot be created (unknown name, or a fixed record).
    #[error("this record cannot be created")]
    CannotCreate,
    /// The value cannot be represented in the protocol (control characters in ASTM, for
    /// example).
    #[error("the value cannot be represented in this protocol")]
    Unrepresentable,
}

/// One problem found while validating a filter or mapping document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// JSON path of the offending value, for example
    /// `channels[2].destinations[0].mapping.rules[1].from`.
    pub path: String,
    /// What is wrong, without echoing configuration secrets (there are none here) or
    /// message data.
    pub message: String,
}

impl Problem {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Why a rule failed while a mapping ran (configuration.md §8.2, message-channels.md §6).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Reason {
    /// A `required` source field was empty or missing.
    #[error("required value is missing")]
    RequiredMissing,
    /// A `lookup` found no entry and `on_missing` is `error`.
    #[error("no entry in code table `{table}`")]
    LookupMissing {
        /// The table name from the configuration.
        table: String,
    },
    /// A `datetime` input does not match its format.
    #[error("value does not match the input date format")]
    DatetimeInput,
    /// A `datetime` output needs a component the input does not have.
    #[error("input has no {component} for the output format")]
    DatetimeMissing {
        /// The missing component (`year`, `month`, `day`, `hour`, `minute`, `second`).
        component: &'static str,
    },
    /// A `datetime` conversion produced a time that does not exist.
    #[error("the time cannot be converted between the time zones")]
    DatetimeZone,
    /// A `calculate` operand is not a decimal number.
    #[error("value is not a decimal number")]
    NotNumeric,
    /// A `calculate` divisor is zero.
    #[error("division by zero")]
    DivisionByZero,
    /// A `calculate` operand or result does not fit the exact representation.
    #[error("the value is outside the supported range")]
    OutOfRange,
    /// A `calculate` comparator (`<5`) cannot be carried through the formula.
    #[error("a comparator value cannot be used here")]
    Comparator,
    /// `find` or `append` met a message with more than one patient, order or specimen
    /// group, where the first match or the appended record could belong to another group.
    #[error("the message holds more than one patient, order or specimen group")]
    MultipleGroups,
    /// A text step would make the value longer than its size bound (65,535 bytes or the
    /// field's length before the steps, whichever is larger).
    #[error("the text steps would make the value longer than allowed")]
    StepsTooLong,
    /// A `require` `count` condition does not hold.
    #[error("the message has {found} occurrences; required {}", count_range(*.min, *.max))]
    Count {
        /// Occurrences in the message.
        found: usize,
        /// The condition's `min`.
        min: Option<usize>,
        /// The condition's `max`.
        max: Option<usize>,
    },
    /// A `require` `same` condition does not hold.
    #[error("the occurrences hold different values")]
    Differ,
    /// The target model rejected the write.
    #[error("{0}")]
    Write(#[from] ModelError),
}

/// `1 to 18`, `at most 18` or `at least 1`.
fn count_range(min: Option<usize>, max: Option<usize>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("{min} to {max}"),
        (None, Some(max)) => format!("at most {max}"),
        (Some(min), None) => format!("at least {min}"),
        (None, None) => "any number".to_owned(),
    }
}

/// A rule failed while a mapping ran. The delivery for the destination is then created
/// `failed` with reason `transform_error` (message-channels.md §6); this error names the
/// rule and path but never the value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("rule {rule_path} ({op}) at path {path}: {reason}")]
pub struct TransformError {
    /// Zero-based index of the top-level rule that failed (the enclosing rule for a rule
    /// inside `when` or `for_each`).
    pub rule: usize,
    /// Full location of the failing rule, for example `rules[2].rules[1]`.
    pub rule_path: String,
    /// The rule's `op`.
    pub op: &'static str,
    /// The path text from the rule that was being read or written.
    pub path: String,
    /// What went wrong.
    pub reason: Reason,
}
