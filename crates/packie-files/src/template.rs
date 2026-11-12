use crate::Error;
use std::collections::HashSet;

/// File encoding (Latin-1 is ISO-8859-1, not Windows-1252).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Charset {
    /// UTF-8; a leading BOM is stripped when reading.
    #[default]
    Utf8,
    /// ISO-8859-1: every byte maps to the code point of the same value.
    Latin1,
    /// 7-bit ASCII; bytes above 127 are rejected.
    Ascii,
}
/// Output row terminator. Readers accept all three independently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineEnding {
    /// `\r\n`.
    #[default]
    Crlf,
    /// `\n`.
    Lf,
    /// `\r`.
    Cr,
}
/// How records are partitioned into messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RecordMode {
    /// Every record is its own message.
    #[default]
    OnePerLine,
    /// Contiguous records with equal `group_by` text form one message.
    Grouped,
    /// The whole file is one message.
    File,
}
/// Transport metadata only; no file-system behavior is implemented.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnExists {
    /// Add a suffix to the new file's name.
    #[default]
    Suffix,
    /// Replace the existing file.
    Overwrite,
    /// Append to the existing file.
    Append,
    /// Fail the write.
    Fail,
}
/// A field's validated type. All variants preserve the input text exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ColumnType {
    /// Any text.
    #[default]
    String,
    /// Optional sign and ASCII digits, of any length.
    Integer,
    /// Optional sign, ASCII digits and an optional decimal point with digits on both sides.
    Decimal,
    /// A calendar date in the column's `format`.
    Date,
    /// A time of day in the column's `format`.
    Time,
    /// A date and time in the column's `format`.
    Datetime,
    /// One of two exact, case-sensitive literals.
    Boolean {
        /// Text meaning true.
        true_literal: String,
        /// Text meaning false.
        false_literal: String,
    },
}
/// One configured field; `position` is one-based, `header` defaults to `name`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Column {
    /// Unique field name, addressed as `col.<name>`.
    pub name: String,
    /// Header text to match or emit; `name` when absent.
    pub header: Option<String>,
    /// One-based source and output position; the position after the preceding column when absent.
    pub position: Option<u32>,
    /// Value type.
    pub column_type: ColumnType,
    /// Date/time tokens for `Date`, `Time` and `Datetime` columns.
    pub format: Option<String>,
    /// Whether an empty value is invalid.
    pub required: bool,
}
/// Serde-free equivalent of the delimited protocol settings; see crate docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Field delimiter: comma, semicolon, pipe or tab.
    pub delimiter: String,
    /// Quote character; `None` disables quoting.
    pub quote: Option<String>,
    /// Whether the first row (after skipped lines) is a header row.
    pub header_row: bool,
    /// Physical source lines to skip before parsing; reading only.
    pub skip_lines: u32,
    /// File encoding.
    pub charset: Charset,
    /// Output row terminator.
    pub line_ending: LineEnding,
    /// Whether the writer emits a UTF-8 BOM.
    pub write_bom: bool,
    /// The configured fields, in declaration order.
    pub columns: Vec<Column>,
    /// How records are partitioned into messages.
    pub record_mode: RecordMode,
    /// Column whose text groups records in `Grouped` mode; absent in other modes.
    pub group_by: Option<String>,
    /// Largest file `read` accepts and `write` produces, in bytes; at most
    /// [`Template::MAX_FILE_BYTES`].
    pub max_file_bytes: u64,
    /// Transport metadata: output file name.
    pub file_name: Option<String>,
    /// Transport metadata: behavior when the output file exists.
    pub on_exists: OnExists,
}
impl Template {
    /// The largest `max_file_bytes` a template may set.
    pub const MAX_FILE_BYTES: u64 = 1_073_741_824;
}
impl Default for Template {
    fn default() -> Self {
        Self {
            delimiter: ",".into(),
            quote: Some("\"".into()),
            header_row: true,
            skip_lines: 0,
            charset: Charset::Utf8,
            line_ending: LineEnding::Crlf,
            write_bom: false,
            columns: Vec::new(),
            record_mode: RecordMode::OnePerLine,
            group_by: None,
            max_file_bytes: 20 * 1024 * 1024,
            file_name: None,
            on_exists: OnExists::Suffix,
        }
    }
}
/// One template validation problem; paths use configuration field names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Configuration field, for example `columns[1].position`.
    pub path: String,
    /// What is wrong with it.
    pub message: String,
}
impl Template {
    /// Collect every configuration problem in stable field order.
    pub fn validate(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut add = |path: &str, message: &str| {
            problems.push(Problem {
                path: path.into(),
                message: message.into(),
            });
        };
        if !matches!(self.delimiter.as_str(), "," | ";" | "|" | "\t") {
            add("delimiter", "must be comma, semicolon, pipe or tab");
        }
        if let Some(q) = &self.quote
            && (q.chars().count() != 1
                || q.contains(['\r', '\n', '\0', '\u{feff}'])
                || q == &self.delimiter)
        {
            add(
                "quote",
                "must be one character distinct from delimiter, newline, NUL and BOM",
            );
        }
        if self.skip_lines > 1000 {
            add("skip_lines", "must be at most 1000");
        }
        if !(1024..=Self::MAX_FILE_BYTES).contains(&self.max_file_bytes) {
            add("max_file_bytes", "must be 1024–1073741824");
        }
        if self.write_bom && self.charset != Charset::Utf8 {
            add("write_bom", "requires utf8");
        }
        if self.columns.is_empty() {
            add("columns", "must contain at least one column");
        }
        if self
            .file_name
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.contains(['/', '\\']))
        {
            add("file_name", "must be a file name without directories");
        }
        self.validate_columns(&mut problems);
        let group_ok = match (&self.record_mode, &self.group_by) {
            (RecordMode::Grouped, Some(name)) => self.columns.iter().any(|c| &c.name == name),
            (RecordMode::Grouped, None) => false,
            (_, group) => group.is_none(),
        };
        if !group_ok {
            problems.push(Problem {
                path: "group_by".into(),
                message: "grouped mode requires a known column; other modes require none".into(),
            });
        }
        problems
    }
    fn validate_columns(&self, problems: &mut Vec<Problem>) {
        let mut names = HashSet::new();
        let mut positions = HashSet::new();
        let mut headers = HashSet::new();
        let mut next = 1_u64;
        for (i, c) in self.columns.iter().enumerate() {
            let mut add = |field: &str, message: &str| {
                problems.push(Problem {
                    path: format!("columns[{i}].{field}"),
                    message: message.into(),
                });
            };
            if !name_ok(&c.name) {
                add("name", "must match ^[a-z][a-z0-9_]{0,40}$");
            }
            if !names.insert(&c.name) {
                add("name", "duplicate name");
            }
            let position = c.position.map_or(next, u64::from);
            if position == 0 || position > u64::from(u32::MAX) {
                add("position", "must be a positive u32");
            }
            if !positions.insert(position) {
                add("position", "duplicate position");
            }
            next = position + 1;
            if self.header_row
                && !headers.insert(c.header.as_ref().unwrap_or(&c.name).to_lowercase())
            {
                add("header", "duplicate case-insensitive header");
            }
            if matches!(
                c.column_type,
                ColumnType::Date | ColumnType::Time | ColumnType::Datetime
            ) && !crate::temporal::valid_format(c)
            {
                add(
                    "format",
                    "invalid format or missing required date/time components",
                );
            }
            if let ColumnType::Boolean {
                true_literal,
                false_literal,
            } = &c.column_type
                && (true_literal.is_empty()
                    || false_literal.is_empty()
                    || true_literal == false_literal)
            {
                add("type", "boolean literals must be nonempty and distinct");
            }
        }
    }
    /// Fails with every validation problem when the template is invalid.
    pub(crate) fn check(&self) -> Result<(), Error> {
        let problems = self.validate();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(Error::Template(problems))
        }
    }
    /// The delimiter character; a checked template always has one.
    pub(crate) fn delimiter_char(&self) -> Result<char, Error> {
        self.delimiter.chars().next().ok_or(Error::Serialize)
    }
    pub(crate) fn quote_char(&self) -> Option<char> {
        self.quote.as_deref().and_then(|s| s.chars().next())
    }
    pub(crate) fn positions(&self) -> Vec<usize> {
        let mut next = 1;
        self.columns
            .iter()
            .map(|c| {
                let p = c.position.map_or(next, |p| p as usize);
                next = p.saturating_add(1);
                p.saturating_sub(1)
            })
            .collect()
    }
    pub(crate) fn ordered_columns(&self) -> Vec<&Column> {
        let positions = self.positions();
        let mut columns: Vec<_> = self.columns.iter().enumerate().collect();
        columns.sort_by_key(|(i, _)| positions[*i]);
        columns.into_iter().map(|(_, c)| c).collect()
    }
}
fn name_ok(name: &str) -> bool {
    (1..=41).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
