//! Template and column builders shared by the test binaries; each binary uses a subset.
#![allow(dead_code)]

use packie_files::{Column, ColumnType, Template};

pub(crate) fn column(name: &str, column_type: ColumnType) -> Column {
    Column {
        name: name.into(),
        column_type,
        ..Column::default()
    }
}

/// A `Date`, `Time` or `Datetime` column with its format tokens.
pub(crate) fn temporal(name: &str, column_type: ColumnType, format: &str) -> Column {
    Column {
        format: Some(format.into()),
        ..column(name, column_type)
    }
}

pub(crate) fn boolean(true_literal: &str, false_literal: &str) -> ColumnType {
    ColumnType::Boolean {
        true_literal: true_literal.into(),
        false_literal: false_literal.into(),
    }
}

/// Default template with exactly these columns.
pub(crate) fn with_columns(columns: Vec<Column>) -> Template {
    Template {
        columns,
        ..Template::default()
    }
}

/// Default template with string columns of these names.
pub(crate) fn strings(names: &[&str]) -> Template {
    with_columns(
        names
            .iter()
            .map(|name| column(name, ColumnType::String))
            .collect(),
    )
}
