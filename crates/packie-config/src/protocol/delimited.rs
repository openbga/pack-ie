//! Delimited file settings (`docs/spec/configuration.md` §7.4).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Charset;
use crate::defaults::yes;

/// Destination line ending.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    /// `\r\n`.
    #[default]
    Crlf,
    /// `\n`.
    Lf,
}

/// How rows become messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RecordMode {
    /// Each row is one message.
    #[default]
    OnePerLine,
    /// Consecutive rows with equal `group_by` values form one message.
    Grouped,
}

/// Type of a delimited column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    /// Text.
    #[default]
    String,
    /// Whole number.
    Integer,
    /// Exact decimal, kept as text; never floating point.
    Decimal,
    /// Date, per `format`.
    Date,
    /// Date and time, per `format`.
    Datetime,
}

/// One delimited column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// `^[a-z][a-z0-9_]{0,40}$`, unique; addressed as `col.<name>`. `control_id`,
    /// `sample_id` and `patient_id` fill the message identifiers.
    pub name: String,
    /// Header text to match (case-insensitive) with `header_row`; default `name`.
    #[serde(default)]
    pub header: Option<String>,
    /// 1-based position without `header_row`, and destination column order; default next.
    #[serde(default)]
    pub position: Option<u32>,
    /// Default `string`.
    #[serde(default, rename = "type")]
    pub column_type: ColumnType,
    /// Required for `date`/`datetime`: tokens `yyyy yy MM dd HH mm ss` and separators.
    #[serde(default)]
    pub format: Option<String>,
    /// Source: an empty value makes the message `invalid`. Default `false`.
    #[serde(default)]
    pub required: bool,
}

/// Delimited file settings (§7.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DelimitedSettings {
    /// `,` `;` `|` or a tab. Default `,`.
    #[serde(default = "comma")]
    pub delimiter: String,
    /// One character, or `null` for no quoting. Default `"`.
    #[serde(default = "double_quote")]
    pub quote: Option<String>,
    /// Source: the first line names the columns; destination: write a header. Default
    /// `true`.
    #[serde(default = "yes")]
    pub header_row: bool,
    /// Leading lines ignored on read; 0–1000.
    #[serde(default)]
    pub skip_lines: u32,
    /// Default `utf8`; a UTF-8 BOM is accepted on read.
    #[serde(default)]
    pub charset: Charset,
    /// Destination only. Default `crlf`.
    #[serde(default)]
    pub line_ending: LineEnding,
    /// Destination only. Default `false`.
    #[serde(default)]
    pub write_bom: bool,
    /// At least one.
    pub columns: Vec<Column>,
    /// Default `one_per_line`.
    #[serde(default)]
    pub record_mode: RecordMode,
    /// Required for `grouped`: a column name.
    #[serde(default)]
    pub group_by: Option<String>,
    /// Source: larger files are not read. Destination: larger mapped files are not
    /// written (`transform_error`). 1024–1073741824. Default 20 MiB.
    #[serde(default = "mib_20")]
    pub max_file_bytes: u64,
    /// Destination only, required: template with `{message_id}`, `{control_id}`,
    /// `{sample_id}`, `{date}`, `{time}`, `{seq}`; no path separators.
    #[serde(default)]
    pub file_name: Option<String>,
    /// Destination only. Default `suffix`.
    #[serde(default)]
    pub on_exists: OnExists,
}

/// What a file write does when the target name already exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum OnExists {
    /// Write `<stem>-1.<ext>`, `<stem>-2.<ext>`, ... instead.
    #[default]
    Suffix,
    /// Replace the existing file.
    Overwrite,
    /// Append to the existing file.
    Append,
    /// Fail the write.
    Fail,
}

const fn mib_20() -> u64 {
    20 * 1024 * 1024
}
fn comma() -> String {
    ",".to_owned()
}
#[allow(clippy::unnecessary_wraps)]
fn double_quote() -> Option<String> {
    Some("\"".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delimited_quote_can_be_turned_off() {
        let json = r#"{"quote":null,"columns":[{"name":"sample_id"}]}"#;
        let d: DelimitedSettings = serde_json::from_str(json).unwrap();
        assert_eq!((d.quote, d.delimiter.as_str()), (None, ","));
        let json = r#"{"columns":[{"name":"n","type":"decimal"}]}"#;
        let d: DelimitedSettings = serde_json::from_str(json).unwrap();
        assert_eq!(d.quote.as_deref(), Some("\""));
        assert_eq!(d.columns[0].column_type, ColumnType::Decimal);
    }
}
