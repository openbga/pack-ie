//! Field paths (configuration.md §8). One parser, four syntaxes, one result type.
//!
//! | Syntax | Shape | Examples |
//! | --- | --- | --- |
//! | HL7 | `SEG[occ].field[rep].component.subcomponent` | `MSH.9`, `PID.3[0].1`, `OBX[2].5` |
//! | ASTM | `R[occ].field[rep].component` | `R.4`, `O[1].3`, `P.3` |
//! | AU | `name` or `test[occ].name` | `sample_id`, `test[0].value` |
//! | Delimited | `col[occ].name` | `col.sample_id`, `col[1].result` |
//!
//! Occurrences and repetitions count from 0 and default to 0; fields, components and
//! subcomponents count from 1. Inside `for_each` a path may begin with `$.` and omit the
//! record: `$.3[0].1`, `$.value`.

use std::borrow::Cow;
use std::fmt;

use crate::error::PathError;

/// Largest index in a path, matching the HL7 codec's limit.
const MAX_INDEX: usize = 65_535;

/// Why a name is not an ASTM record, for path and `for_each` validation alike.
pub(crate) const ASTM_RECORD_HINT: &str = "expected an ASTM record letter (H, P, O, R, C, Q, M, L)";

/// The path syntax of a message model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Syntax {
    /// `SEG[occ].field[rep].component.subcomponent`.
    Hl7,
    /// `R[occ].field[rep].component`, fields numbered as in ASTM E1394.
    Astm,
    /// Field names of the AU sample record.
    Au,
    /// `col[occ].name`.
    Delimited,
}

/// Whether a path starts at the message or at the current `for_each` occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Addresses the message.
    Root,
    /// Begins with `$.`: addresses the current occurrence of a `for_each`.
    Current,
}

/// The record occurrence a `$.` path addresses: the current `for_each` iteration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame<'a> {
    pub(crate) record: &'a str,
    pub(crate) occurrence: usize,
}

/// The field part of a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldRef {
    /// HL7 and ASTM: a one-based field number.
    Number(usize),
    /// AU and delimited files: a field or column name.
    Name(String),
}

/// A parsed path. Parse once with [`Path::parse`], then read or write any
/// [`Model`](crate::Model) of the same syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    syntax: Syntax,
    text: String,
    scope: Scope,
    record: Option<String>,
    occurrence: usize,
    field: FieldRef,
    repetition: Option<usize>,
    component: Option<usize>,
    subcomponent: Option<usize>,
}

impl Path {
    /// Parses `text` in the given syntax.
    ///
    /// # Errors
    /// [`PathError`] with the byte position of the first problem.
    pub fn parse(syntax: Syntax, text: &str) -> Result<Self, PathError> {
        let mut cursor = Cursor { text, pos: 0 };
        let scope = if cursor.eat_str("$.") {
            Scope::Current
        } else {
            Scope::Root
        };
        let mut path = Self {
            syntax,
            text: text.to_owned(),
            scope,
            record: None,
            occurrence: 0,
            field: FieldRef::Number(1),
            repetition: None,
            component: None,
            subcomponent: None,
        };
        match syntax {
            Syntax::Hl7 | Syntax::Astm => path.parse_numbered(&mut cursor)?,
            Syntax::Au => path.parse_au(&mut cursor)?,
            Syntax::Delimited => path.parse_delimited(&mut cursor)?,
        }
        if cursor.pos < text.len() {
            return Err(cursor.error("unexpected character"));
        }
        Ok(path)
    }

    /// The path text as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The syntax the path was parsed in.
    #[must_use]
    pub const fn syntax(&self) -> Syntax {
        self.syntax
    }

    /// Message-relative or `$.`-relative.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }

    /// The segment (HL7), record (ASTM), `test` (AU) or `col` (delimited) the path names.
    /// `None` for a `$.` path and for AU header fields.
    #[must_use]
    pub fn record(&self) -> Option<&str> {
        self.record.as_deref()
    }

    /// Zero-based occurrence of the record.
    #[must_use]
    pub const fn occurrence(&self) -> usize {
        self.occurrence
    }

    /// The field number or name.
    #[must_use]
    pub const fn field(&self) -> &FieldRef {
        &self.field
    }

    /// Zero-based repetition, when written.
    #[must_use]
    pub const fn repetition(&self) -> Option<usize> {
        self.repetition
    }

    /// One-based component, when written.
    #[must_use]
    pub const fn component(&self) -> Option<usize> {
        self.component
    }

    /// One-based subcomponent, when written.
    #[must_use]
    pub const fn subcomponent(&self) -> Option<usize> {
        self.subcomponent
    }

    /// This path made absolute for one occurrence of `record`: how `$.` paths are
    /// resolved inside `for_each`. Root paths are returned unchanged.
    #[must_use]
    pub fn resolved(&self, record: &str, occurrence: usize) -> Self {
        if self.scope == Scope::Root {
            return self.clone();
        }
        Self {
            scope: Scope::Root,
            record: Some(record.to_owned()),
            occurrence,
            ..self.clone()
        }
    }

    /// This field- or repetition-level path narrowed to one component (HL7 structured
    /// numeric reads and writes): `$.5` becomes `$.5.2`.
    pub(crate) fn component_path(&self, component: usize) -> Self {
        Self {
            text: format!("{}.{component}", self.text),
            component: Some(component),
            subcomponent: None,
            ..self.clone()
        }
    }

    /// This absolute path at another occurrence of its record (`require` `same`).
    pub(crate) fn at_occurrence(&self, occurrence: usize) -> Self {
        Self {
            occurrence,
            ..self.clone()
        }
    }

    /// This path at repetition `repetition` of its field (`for_each` mode `repeat`).
    pub(crate) fn with_repetition(&self, repetition: usize) -> Self {
        Self {
            repetition: Some(repetition),
            ..self.clone()
        }
    }

    /// Whether a model of `syntax` can address this path: parsed in that syntax and not
    /// `$.`-relative (models only ever see absolute paths).
    pub(crate) fn is_absolute_in(&self, syntax: Syntax) -> bool {
        self.syntax == syntax && self.scope == Scope::Root
    }

    /// This path as an absolute path for the model: `$.` paths are resolved against `frame`.
    /// `None` for a `$.` path outside a loop.
    pub(crate) fn within(&self, frame: Option<Frame<'_>>) -> Option<Cow<'_, Self>> {
        match (self.scope, frame) {
            (Scope::Root, _) => Some(Cow::Borrowed(self)),
            (Scope::Current, Some(f)) => Some(Cow::Owned(self.resolved(f.record, f.occurrence))),
            (Scope::Current, None) => None,
        }
    }

    fn parse_numbered(&mut self, cursor: &mut Cursor<'_>) -> Result<(), PathError> {
        if self.scope == Scope::Root {
            let start = cursor.pos;
            let name = cursor.word();
            if !record_name_ok(self.syntax, name) {
                return Err(PathError::new(
                    start,
                    match self.syntax {
                        Syntax::Astm => ASTM_RECORD_HINT,
                        _ => "expected a three-character segment name such as PID",
                    },
                ));
            }
            self.occurrence = cursor.bracket()?.unwrap_or(0);
            if name == "MSH" && self.occurrence != 0 {
                return Err(PathError::new(start, "a message has one MSH"));
            }
            self.record = Some(name.to_owned());
            cursor.expect('.', "expected `.` before the field number")?;
        }
        self.field = FieldRef::Number(cursor.number(1, "expected a field number from 1")?);
        self.repetition = cursor.bracket()?;
        if cursor.eat('.') {
            self.component = Some(cursor.number(1, "expected a component number from 1")?);
            self.subcomponent = self.subcomponent_part(cursor)?;
        }
        Ok(())
    }

    fn subcomponent_part(&self, cursor: &mut Cursor<'_>) -> Result<Option<usize>, PathError> {
        if !cursor.eat('.') {
            return Ok(None);
        }
        if self.syntax == Syntax::Astm {
            return Err(cursor.error_before("ASTM fields have no subcomponents"));
        }
        cursor
            .number(1, "expected a subcomponent number from 1")
            .map(Some)
    }

    fn parse_au(&mut self, cursor: &mut Cursor<'_>) -> Result<(), PathError> {
        if self.scope == Scope::Current {
            self.field = FieldRef::Name(cursor.name()?);
            return Ok(());
        }
        let start = cursor.pos;
        let first = cursor.name()?;
        if matches!(cursor.peek(), Some(b'[' | b'.')) {
            if first != "test" {
                return Err(PathError::new(
                    start,
                    "only `test` has occurrences; header fields are written bare",
                ));
            }
            self.record = Some(first);
            self.occurrence = cursor.bracket()?.unwrap_or(0);
            cursor.expect('.', "expected `.` before the field name")?;
            self.field = FieldRef::Name(cursor.name()?);
        } else {
            self.field = FieldRef::Name(first);
        }
        Ok(())
    }

    fn parse_delimited(&mut self, cursor: &mut Cursor<'_>) -> Result<(), PathError> {
        if self.scope == Scope::Root {
            let start = cursor.pos;
            if cursor.word() != "col" {
                return Err(PathError::new(
                    start,
                    "delimited paths start with `col` (col.<name>)",
                ));
            }
            self.record = Some("col".to_owned());
            self.occurrence = cursor.bracket()?.unwrap_or(0);
            cursor.expect('.', "expected `.` before the column name")?;
        }
        self.field = FieldRef::Name(cursor.name()?);
        Ok(())
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Whether `name` can be the record named by `over` or `into` (or a path's record) in
/// `syntax`. AU and delimited records are the fixed names `test` and `col`.
#[must_use]
pub(crate) fn record_name_ok(syntax: Syntax, name: &str) -> bool {
    let bytes = name.as_bytes();
    match syntax {
        Syntax::Hl7 => {
            bytes.len() == 3
                && bytes[0].is_ascii_uppercase()
                && bytes[1..]
                    .iter()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        }
        Syntax::Astm => matches!(
            bytes,
            [b'H' | b'P' | b'O' | b'R' | b'C' | b'Q' | b'M' | b'L']
        ),
        Syntax::Au => name == "test",
        Syntax::Delimited => name == "col",
    }
}

/// A field or column name: `^[a-z][a-z0-9_]{0,40}$`.
fn name_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() <= 41
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

struct Cursor<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn error(&self, reason: &'static str) -> PathError {
        PathError::new(self.pos, reason)
    }

    /// An error at the character just consumed (the `.` that introduced something
    /// invalid).
    fn error_before(&self, reason: &'static str) -> PathError {
        PathError::new(self.pos.saturating_sub(1), reason)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == u8::try_from(expected).ok() {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, expected: &str) -> bool {
        if self.text[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char, reason: &'static str) -> Result<(), PathError> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(self.error(reason))
        }
    }

    /// The run of ASCII letters and digits at the cursor (possibly empty).
    fn word(&mut self) -> &'a str {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_alphanumeric()) {
            self.pos += 1;
        }
        &self.text[start..self.pos]
    }

    /// A field or column name.
    fn name(&mut self) -> Result<String, PathError> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            self.pos += 1;
        }
        let name = &self.text[start..self.pos];
        if name_ok(name) {
            Ok(name.to_owned())
        } else {
            Err(PathError::new(
                start,
                "expected a name of lowercase letters, digits and `_`, at most 41 long",
            ))
        }
    }

    /// Decimal digits with a minimum value and the global maximum.
    fn number(&mut self, minimum: usize, reason: &'static str) -> Result<usize, PathError> {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        let digits = &self.text[start..self.pos];
        let value = digits
            .parse::<usize>()
            .ok()
            .filter(|value| *value >= minimum)
            .ok_or_else(|| PathError::new(start, reason))?;
        if value > MAX_INDEX {
            return Err(PathError::new(start, "index is larger than 65535"));
        }
        Ok(value)
    }

    /// An optional `[n]` with `n` from 0.
    fn bracket(&mut self) -> Result<Option<usize>, PathError> {
        if !self.eat('[') {
            return Ok(None);
        }
        let value = self.number(0, "expected an index from 0 inside `[]`")?;
        self.expect(']', "expected `]`")?;
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn parse(syntax: Syntax, text: &str) -> Path {
        Path::parse(syntax, text).expect("valid path")
    }

    fn fail(syntax: Syntax, text: &str) -> PathError {
        Path::parse(syntax, text).expect_err("invalid path")
    }

    #[test]
    fn hl7_paths() {
        let p = parse(Syntax::Hl7, "PID.3[1].4.2");
        assert_eq!(p.record(), Some("PID"));
        assert_eq!(p.field(), &FieldRef::Number(3));
        assert_eq!(p.repetition(), Some(1));
        assert_eq!(p.component(), Some(4));
        assert_eq!(p.subcomponent(), Some(2));
        let p = parse(Syntax::Hl7, "OBX[2].5");
        assert_eq!(
            (p.occurrence(), p.repetition(), p.component()),
            (2, None, None)
        );
        let p = parse(Syntax::Hl7, "MSH.9");
        assert_eq!(p.field(), &FieldRef::Number(9));
        assert_eq!(p.scope(), Scope::Root);
    }

    #[test]
    fn current_scope_paths_omit_the_record() {
        let p = parse(Syntax::Hl7, "$.3[0].1");
        assert_eq!(p.scope(), Scope::Current);
        assert_eq!(p.record(), None);
        let resolved = p.resolved("OBX", 4);
        assert_eq!(resolved.scope(), Scope::Root);
        assert_eq!((resolved.record(), resolved.occurrence()), (Some("OBX"), 4));
        assert_eq!(resolved.field(), &FieldRef::Number(3));
        let p = parse(Syntax::Au, "$.value");
        assert_eq!(p.field(), &FieldRef::Name("value".into()));
    }

    #[test]
    fn astm_au_and_delimited_paths() {
        let p = parse(Syntax::Astm, "R[3].4[1].2");
        assert_eq!(
            (p.record(), p.occurrence(), p.repetition(), p.component()),
            (Some("R"), 3, Some(1), Some(2))
        );
        assert_eq!(parse(Syntax::Au, "sample_id").record(), None);
        let p = parse(Syntax::Au, "test[2].flags");
        assert_eq!((p.record(), p.occurrence()), (Some("test"), 2));
        let p = parse(Syntax::Delimited, "col[1].sample_id");
        assert_eq!((p.record(), p.occurrence()), (Some("col"), 1));
        assert_eq!(parse(Syntax::Delimited, "col.result").occurrence(), 0);
    }

    #[test]
    fn errors_carry_positions() {
        let cases = [
            (Syntax::Hl7, "PID", 3),
            (Syntax::Hl7, "pid.3", 0),
            (Syntax::Hl7, "PID.0", 4),
            (Syntax::Hl7, "PID.3[", 6),
            (Syntax::Hl7, "PID.3[x]", 6),
            (Syntax::Hl7, "PID.3.1.1.1", 9),
            (Syntax::Hl7, "PID.3 ", 5),
            (Syntax::Hl7, "MSH[1].9", 0),
            (Syntax::Hl7, "PID.70000", 4),
            (Syntax::Hl7, "", 0),
            (Syntax::Astm, "X.1", 0),
            (Syntax::Astm, "R.3.1.1", 5),
            (Syntax::Au, "test[0]", 7),
            (Syntax::Au, "rack[0].x", 0),
            (Syntax::Delimited, "row.x", 0),
            (Syntax::Delimited, "col.", 4),
            (Syntax::Delimited, "col.Name", 4),
        ];
        for (syntax, text, position) in cases {
            assert_eq!(fail(syntax, text).position, position, "{text}");
        }
        assert!(fail(Syntax::Hl7, "PID.é").reason.contains("field number"));
    }

    proptest! {
        #[test]
        fn parser_never_panics_and_errors_stay_in_bounds(text in "\\PC{0,40}") {
            for syntax in [Syntax::Hl7, Syntax::Astm, Syntax::Au, Syntax::Delimited] {
                if let Err(error) = Path::parse(syntax, &text) {
                    prop_assert!(error.position <= text.len());
                    prop_assert!(text.is_char_boundary(error.position));
                }
            }
        }

        #[test]
        fn path_like_text_never_panics(text in "[A-Z$.\\[\\]0-9a-z_]{0,24}") {
            for syntax in [Syntax::Hl7, Syntax::Astm, Syntax::Au, Syntax::Delimited] {
                if let Ok(path) = Path::parse(syntax, &text) {
                    prop_assert_eq!(path.as_str(), text.as_str());
                    let _ = path.resolved("OBX", 3);
                }
            }
        }
    }
}
