//! The message model contract and the name checks that validation runs against it.

use std::borrow::Cow;

use crate::au::{HEADER_FIELDS, TEST_FIELDS};
use crate::error::{ModelError, PathError};
use crate::path::{ASTM_RECORD_HINT, FieldRef, Frame, Path, Scope, Syntax, record_name_ok};

/// A parsed protocol message that filters read and mappings read and write.
///
/// Paths handed to a model are absolute ([`Scope::Root`]) and parsed in the model's own
/// [`Syntax`]; the engine resolves `$.` paths before it calls the model. A model never
/// panics on a path it does not understand: `get` answers `None` and `set` returns an
/// error.
pub trait Model {
    /// The syntax of the paths this model understands.
    fn syntax(&self) -> Syntax;

    /// The text at `path`, or `None` when the path does not exist in this message. A path
    /// that stops above the leaf yields the text with its structural separators
    /// (`MSH.9` is `ORU^R01`).
    fn get(&self, path: &Path) -> Option<Cow<'_, str>>;

    /// Writes `value` at `path`, creating the records and fields it needs.
    ///
    /// # Errors
    /// [`ModelError`] when the model is read-only, the field is fixed, or the value cannot
    /// be represented.
    fn set(&mut self, path: &Path, value: &str) -> Result<(), ModelError>;

    /// The number of occurrences of the segment, record, `test` or `col` named `record`.
    fn count(&self, record: &str) -> usize;

    /// Appends a new, empty occurrence of `record` and returns its zero-based index.
    ///
    /// # Errors
    /// [`ModelError::ReadOnly`] or [`ModelError::CannotCreate`].
    fn create(&mut self, record: &str) -> Result<usize, ModelError>;
}

/// The text at `path` in `model`; a `$.` path is resolved against `frame`.
pub(crate) fn read<'m>(
    model: &'m dyn Model,
    path: &Path,
    frame: Option<Frame<'_>>,
) -> Option<Cow<'m, str>> {
    let path = path.within(frame)?;
    model.get(&path)
}

/// Writes `value` at `path` in `model`; a `$.` path is resolved against `frame`.
pub(crate) fn write(
    model: &mut dyn Model,
    path: &Path,
    value: &str,
    frame: Option<Frame<'_>>,
) -> Result<(), ModelError> {
    let path = path.within(frame).ok_or(ModelError::WrongSyntax)?;
    model.set(&path, value)
}

/// Whether the message holds at most one patient, order and specimen group (HL7 `PID`,
/// `OBR`, `SPM`; ASTM `P`, `O`). `find`, `append` and `require_single_group` address the
/// whole message, so they are only unambiguous then. AU and delimited messages are one
/// sample.
pub(crate) fn single_group(model: &dyn Model) -> bool {
    repeated_group(model).is_none()
}

/// The first group record that occurs more than once, in the order `PID`, `OBR`, `SPM`
/// (HL7) or `P`, `O` (ASTM).
pub(crate) fn repeated_group(model: &dyn Model) -> Option<&'static str> {
    let records: &[&'static str] = match model.syntax() {
        Syntax::Hl7 => &["PID", "OBR", "SPM"],
        Syntax::Astm => &["P", "O"],
        Syntax::Au | Syntax::Delimited => &[],
    };
    records
        .iter()
        .copied()
        .find(|record| model.count(record) > 1)
}

/// The names a protocol model knows, used to validate paths when a document is validated
/// ("unknown segment, field or column names are errors", configuration.md §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    syntax: Syntax,
    columns: Vec<String>,
}

impl Schema {
    /// HL7 v2: any three-character segment name; field numbers are not bounded.
    #[must_use]
    pub const fn hl7() -> Self {
        Self {
            syntax: Syntax::Hl7,
            columns: Vec::new(),
        }
    }

    /// ASTM E1394 records H, P, O, R, C, Q, M, L.
    #[must_use]
    pub const fn astm() -> Self {
        Self {
            syntax: Syntax::Astm,
            columns: Vec::new(),
        }
    }

    /// AU sample records (read-only).
    #[must_use]
    pub const fn au() -> Self {
        Self {
            syntax: Syntax::Au,
            columns: Vec::new(),
        }
    }

    /// Delimited files with the connector's column names.
    #[must_use]
    pub fn delimited(columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            syntax: Syntax::Delimited,
            columns: columns.into_iter().map(Into::into).collect(),
        }
    }

    /// The path syntax of this schema.
    #[must_use]
    pub const fn syntax(&self) -> Syntax {
        self.syntax
    }

    /// Parses `text` for this schema's syntax (syntax only; see [`Schema::check`]).
    ///
    /// # Errors
    /// [`PathError`] with the position of the problem.
    pub fn parse(&self, text: &str) -> Result<Path, PathError> {
        Path::parse(self.syntax, text)
    }

    /// Parses and checks a path written in a document: for a target (`write`) the path must
    /// be writable, and `$.` paths are accepted only `in_loop`.
    ///
    /// # Errors
    /// A sentence saying what is wrong with the path.
    pub(crate) fn document_path(
        &self,
        text: &str,
        write: bool,
        in_loop: bool,
    ) -> Result<Path, String> {
        let path = self.parse(text).map_err(|error| error.to_string())?;
        if path.scope() == Scope::Current && !in_loop {
            return Err("`$.` paths are only valid inside for_each".to_owned());
        }
        self.check(&path, write)?;
        Ok(path)
    }

    /// Checks that a parsed path names things this model has. With `write` the path must
    /// also be writable.
    ///
    /// # Errors
    /// A sentence saying what is unknown or fixed.
    pub fn check(&self, path: &Path, write: bool) -> Result<(), String> {
        if path.syntax() != self.syntax {
            return Err("the path is written for another protocol".to_owned());
        }
        if write {
            self.check_writable(path)?;
        }
        match (&self.syntax, path.field()) {
            (Syntax::Au, FieldRef::Name(name)) => check_au_name(path, name),
            (Syntax::Delimited, FieldRef::Name(name)) => self.check_column(name),
            _ => Ok(()),
        }
    }

    /// Checks a record or segment name used by `over` (read) or `into` (write).
    ///
    /// # Errors
    /// A sentence saying why the name is not usable.
    pub fn check_record(&self, name: &str, write: bool) -> Result<(), String> {
        if !record_name_ok(self.syntax, name) {
            return Err(match self.syntax {
                Syntax::Hl7 => "expected a three-character segment name such as OBX",
                Syntax::Astm => ASTM_RECORD_HINT,
                Syntax::Au => "AU messages have one repeating record, `test`",
                Syntax::Delimited => "delimited messages have one repeating record, `col`",
            }
            .to_owned());
        }
        if write {
            self.check_creatable(name)?;
        }
        Ok(())
    }

    fn check_writable(&self, path: &Path) -> Result<(), String> {
        match (self.syntax, path.record(), path.field()) {
            (Syntax::Au, ..) => Err("AU messages are read-only".to_owned()),
            (Syntax::Hl7, Some("MSH"), FieldRef::Number(1 | 2)) => {
                Err("MSH.1 and MSH.2 are fixed by the encoding characters".to_owned())
            }
            (Syntax::Astm, record, FieldRef::Number(n)) => match (record, n) {
                (_, 1) => Err("field 1 is the record identifier and cannot be written".to_owned()),
                (Some("H"), 2) => Err("H.2 holds the delimiters and cannot be written".to_owned()),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }

    fn check_creatable(&self, name: &str) -> Result<(), String> {
        match (self.syntax, name) {
            (Syntax::Au, _) => Err("AU messages are read-only".to_owned()),
            (Syntax::Hl7, "MSH") => Err("a message has one MSH".to_owned()),
            (Syntax::Astm, "H" | "L") => {
                Err("an ASTM message has one H and one L record".to_owned())
            }
            _ => Ok(()),
        }
    }

    fn check_column(&self, name: &str) -> Result<(), String> {
        if self.columns.iter().any(|c| c == name) {
            Ok(())
        } else {
            Err(format!("unknown column `{name}`"))
        }
    }
}

fn check_au_name(path: &Path, name: &str) -> Result<(), String> {
    let known: &[&str] = if path.scope() == Scope::Current || path.record().is_some() {
        &TEST_FIELDS
    } else {
        &HEADER_FIELDS
    };
    if known.contains(&name) {
        Ok(())
    } else {
        Err(format!(
            "unknown AU field `{name}`; known: {}",
            known.join(", ")
        ))
    }
}
