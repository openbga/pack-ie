//! Delimited-file adapter: rows of named columns, addressed as `col.<name>`.
//!
//! The model is deliberately independent of the file codec. `packie-files` produces a
//! record per row (one row per message in `one_per_line` mode, several in `grouped`
//! mode); the routing adapter copies each row's `(column name, text)` pairs into a
//! [`RecordModel`] with [`RecordModel::from_rows`], runs filters and mappings, and writes
//! the model's rows back out in the connector's column order. Decimals stay text; no
//! value is converted here.
//!
//! `col.sample_id` is the column of the first row, `col[1].sample_id` that of the second
//! (grouped files), and `for_each` may iterate `col` rows.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::error::ModelError;
use crate::model::Model;
use crate::path::{FieldRef, Path, Syntax};

type Row = BTreeMap<String, String>;

/// Rows of a delimited-file message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordModel {
    rows: Vec<Row>,
}

impl RecordModel {
    /// A message with no rows; the first write to `col.<name>` creates row 0.
    #[must_use]
    pub const fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// One row from `(column name, text)` pairs.
    #[must_use]
    pub fn from_row<N, V>(row: impl IntoIterator<Item = (N, V)>) -> Self
    where
        N: Into<String>,
        V: Into<String>,
    {
        Self::from_rows([row])
    }

    /// Several rows (a grouped message).
    #[must_use]
    pub fn from_rows<N, V, R>(rows: impl IntoIterator<Item = R>) -> Self
    where
        N: Into<String>,
        V: Into<String>,
        R: IntoIterator<Item = (N, V)>,
    {
        Self {
            rows: rows
                .into_iter()
                .map(|row| row.into_iter().map(|(n, v)| (n.into(), v.into())).collect())
                .collect(),
        }
    }

    /// The rows, each as column name to text.
    #[must_use]
    pub fn rows(&self) -> &[BTreeMap<String, String>] {
        &self.rows
    }
}

fn address(path: &Path) -> Option<&str> {
    match path.field() {
        FieldRef::Name(name) if path.is_absolute_in(Syntax::Delimited) => Some(name),
        _ => None,
    }
}

impl Model for RecordModel {
    fn syntax(&self) -> Syntax {
        Syntax::Delimited
    }

    fn get(&self, path: &Path) -> Option<Cow<'_, str>> {
        let name = address(path)?;
        self.rows
            .get(path.occurrence())?
            .get(name)
            .map(|v| Cow::Borrowed(v.as_str()))
    }

    fn set(&mut self, path: &Path, value: &str) -> Result<(), ModelError> {
        let name = address(path).ok_or(ModelError::WrongSyntax)?;
        if self.rows.len() <= path.occurrence() {
            self.rows.resize_with(path.occurrence() + 1, Row::new);
        }
        self.rows[path.occurrence()].insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn count(&self, record: &str) -> usize {
        if record == "col" { self.rows.len() } else { 0 }
    }

    fn create(&mut self, record: &str) -> Result<usize, ModelError> {
        if record != "col" {
            return Err(ModelError::CannotCreate);
        }
        self.rows.push(Row::new());
        Ok(self.rows.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Path {
        Path::parse(Syntax::Delimited, text).expect("valid path")
    }

    #[test]
    fn reads_rows_and_columns() {
        let m = RecordModel::from_rows([
            [("sample_id", "S1"), ("result", "5.5")],
            [("sample_id", "S1"), ("result", "7.0")],
        ]);
        assert_eq!(m.get(&p("col.sample_id")).as_deref(), Some("S1"));
        assert_eq!(m.get(&p("col[1].result")).as_deref(), Some("7.0"));
        assert_eq!(m.get(&p("col[2].result")), None);
        assert_eq!(m.get(&p("col.unit")), None);
        assert_eq!(m.count("col"), 2);
    }

    #[test]
    fn writes_create_rows_on_demand() {
        let mut m = RecordModel::new();
        m.set(&p("col[1].result"), "9").expect("write");
        assert_eq!(m.count("col"), 2);
        assert_eq!(m.create("col"), Ok(2));
        assert_eq!(m.create("row"), Err(ModelError::CannotCreate));
        assert_eq!(m.rows()[1].get("result").map(String::as_str), Some("9"));
        assert_eq!(
            m.set(&Path::parse(Syntax::Hl7, "PID.3").expect("valid"), "x"),
            Err(ModelError::WrongSyntax)
        );
    }
}
