use crate::{Column, ColumnType, Error, PathError, RecordMode, Template};

/// A validated value and its type; the lexical text is never normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    text: String,
    column_type: ColumnType,
}
impl Value {
    /// The original text, exactly as read or set.
    pub fn as_str(&self) -> &str {
        &self.text
    }
    /// The column type this value was validated against.
    pub fn column_type(&self) -> &ColumnType {
        &self.column_type
    }
    /// The boolean a `Boolean` value spells; `None` for other types and for empty text.
    pub fn as_boolean(&self) -> Option<bool> {
        match &self.column_type {
            ColumnType::Boolean { true_literal, .. } if &self.text == true_literal => Some(true),
            ColumnType::Boolean { false_literal, .. } if &self.text == false_literal => Some(false),
            _ => None,
        }
    }
}
/// Named field with its validation schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub(crate) column: Column,
    pub(crate) value: Value,
}
impl Field {
    /// The configured column name.
    pub fn name(&self) -> &str {
        &self.column.name
    }
    /// The field's value.
    pub fn value(&self) -> &Value {
        &self.value
    }
}
/// Ordered named fields; order follows the template's output positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub(crate) fields: Vec<Field>,
}
/// Complete file in row order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    /// The records in row order.
    pub records: Vec<Record>,
}
/// Transformation access independent of any internal PACK-IE crate.
pub trait RecordModel {
    /// The text at `path` (`col.<name>` or zero-based `col[<index>]`), if the path exists.
    fn get(&self, path: &str) -> Option<&str>;
    /// Replaces the text at `path`; the record is unchanged on failure.
    fn set(&mut self, path: &str, text: &str) -> Result<(), PathError>;
}
impl Record {
    /// Values are supplied in template declaration order, before output sorting.
    pub fn new(template: &Template, values: &[&str]) -> Result<Self, Error> {
        let mut record = Self::empty(template)?;
        if values.len() != template.columns.len() {
            return Err(Error::Ragged {
                line: 1,
                column: values.len().min(template.columns.len()) + 1,
                expected: template.columns.len(),
                actual: values.len(),
            });
        }
        for (column, text) in template.columns.iter().zip(values) {
            record
                .set_column(&column.name, text)
                .map_err(|_| Error::InvalidValue {
                    line: 1,
                    column: column.name.clone(),
                })?;
        }
        Ok(record)
    }
    /// Empty mapping target; required values may be populated with `set`.
    pub fn empty(template: &Template) -> Result<Self, Error> {
        template.check()?;
        Ok(Self::blank(template))
    }
    /// Empty record for a template that has already been checked.
    pub(crate) fn blank(template: &Template) -> Self {
        Self {
            fields: template
                .ordered_columns()
                .into_iter()
                .map(|c| Field {
                    column: c.clone(),
                    value: Value {
                        text: String::new(),
                        column_type: c.column_type.clone(),
                    },
                })
                .collect(),
        }
    }
    /// The fields in output position order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
    /// The text at `path`; see [`RecordModel::get`].
    pub fn get(&self, path: &str) -> Option<&str> {
        RecordModel::get(self, path)
    }
    /// Replaces the text at `path`; see [`RecordModel::set`].
    pub fn set(&mut self, path: &str, text: &str) -> Result<(), PathError> {
        RecordModel::set(self, path, text)
    }
    /// The text of the column called `name`.
    pub(crate) fn column_text(&self, name: &str) -> Option<&str> {
        let field = self.fields.iter().find(|f| f.name() == name)?;
        Some(field.value.as_str())
    }
    pub(crate) fn set_column(&mut self, name: &str, text: &str) -> Result<(), PathError> {
        let index = self.column_index(name).ok_or(PathError::UnknownPath)?;
        self.set_at(index, text)
    }
    fn column_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name() == name)
    }
    fn index(&self, path: &str) -> Option<usize> {
        if let Some(name) = path.strip_prefix("col.") {
            return self.column_index(name);
        }
        let index = path.strip_prefix("col[")?.strip_suffix(']')?;
        if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        index
            .parse::<usize>()
            .ok()
            .filter(|i| *i < self.fields.len())
    }
    fn set_at(&mut self, index: usize, text: &str) -> Result<(), PathError> {
        let field = &mut self.fields[index];
        if !valid_text(&field.column, text) {
            return Err(PathError::InvalidValue {
                column: field.column.name.clone(),
            });
        }
        field.value.text = text.into();
        Ok(())
    }
}
impl RecordModel for Record {
    fn get(&self, path: &str) -> Option<&str> {
        Some(self.fields.get(self.index(path)?)?.value.as_str())
    }
    fn set(&mut self, path: &str, text: &str) -> Result<(), PathError> {
        let index = self.index(path).ok_or(PathError::UnknownPath)?;
        self.set_at(index, text)
    }
}
impl Document {
    /// Borrow messages in source order. Empty files yield no messages in record or
    /// group mode, and one empty message in file mode. Grouping compares exact text.
    pub fn messages<'a>(&'a self, template: &Template) -> Result<Vec<&'a [Record]>, Error> {
        template.check()?;
        match template.record_mode {
            RecordMode::File => return Ok(vec![&self.records]),
            RecordMode::OnePerLine => return Ok(self.records.chunks(1).collect()),
            RecordMode::Grouped => {}
        }
        let group = template.group_by.as_deref().unwrap_or_default();
        if self.records.iter().any(|r| r.column_text(group).is_none()) {
            return Err(Error::MissingColumn {
                line: 1,
                column: group.to_owned(),
            });
        }
        Ok(self
            .records
            .chunk_by(|a, b| a.column_text(group) == b.column_text(group))
            .collect())
    }
}
pub(crate) fn valid_text(column: &Column, text: &str) -> bool {
    if text.is_empty() {
        return !column.required;
    }
    match &column.column_type {
        ColumnType::String => true,
        ColumnType::Integer => digits(unsigned(text)),
        ColumnType::Decimal => {
            let text = unsigned(text);
            match text.split_once('.') {
                Some((whole, fraction)) => digits(whole) && digits(fraction),
                None => digits(text),
            }
        }
        ColumnType::Date | ColumnType::Time | ColumnType::Datetime => {
            crate::temporal::valid_value(column, text)
        }
        ColumnType::Boolean {
            true_literal,
            false_literal,
        } => text == true_literal || text == false_literal,
    }
}
fn unsigned(text: &str) -> &str {
    text.strip_prefix(['+', '-']).unwrap_or(text)
}
fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}
