//! Pure complete-file delimited codec: [`read()`] parses bytes, [`write()`] encodes records,
//! and [`Document::messages`] partitions rows without performing I/O.
//!
//! [`Template`] mirrors `docs/spec/configuration.md` §7.4 and the engine's
//! `DelimitedSettings`, including names, types and defaults. `Column::column_type`
//! maps configuration `type`; absent `header` means `name`, absent `position` means
//! the next position after the preceding column (initially 1). Header lookup is
//! case-insensitive; explicit `header` takes precedence over `position`. The latter
//! supplies a fallback only when `header` is absent. Output is ordered by position,
//! with empty fields at gaps. `skip_lines` only affects reading. `file_name` and
//! `on_exists` are transport metadata and never interpreted by this crate.
//!
//! Extensions beyond current configuration: [`RecordMode::File`],
//! [`ColumnType::Time`], and [`ColumnType::Boolean`] with exact, case-sensitive
//! configured literals. `OnePerLine` is record mode; `Grouped` is group mode.
//! Dates use `yyyy yy MM dd HH mm ss` tokens and literal separators (including `T`);
//! `yy` means 2000–2099. Integers and decimals have optional signs and ASCII digits;
//! decimals allow a decimal point, but no exponent, whitespace, NaN or infinity.
//! Every value retains its original spelling, including leading/trailing zeroes.
//!
//! Required empty fields, malformed quoting, ragged rows and invalid typed values
//! are errors; no partial document is returned. Empty optional values are allowed
//! for every type. Blank physical lines outside quotes are ignored. A final row
//! need not end with a newline; an open quoted field at EOF is truncated input.
//! UTF-8 BOMs are stripped only with UTF-8; Latin-1 maps every byte without failure.
//! Writers reject unrepresentable text and unsafe fields when quoting is disabled.
//!
//! [`Record::new`] creates a typed row from template-order values; [`Record::empty`]
//! creates a mapping target (required values are checked at write time).
//! [`RecordModel`] exposes borrowed text for transforms. `col.name` addresses a
//! configured column; `col[0]` addresses the first configured field in output
//! position order (zero-based). Sets validate before mutation. Unknown paths fail.
//!
//! Configuration mapping (the routing layer copies these into the pure model):
//!
//! | Configuration field | Template behavior |
//! | --- | --- |
//! | `delimiter`, `quote` | Same strings; doubled-quote escaping, `None` disables quotes. |
//! | `header_row` | Match source headers, emit destination headers. |
//! | `skip_lines` | Skip physical source lines before CSV parsing. |
//! | `charset` | Map `utf8`, `latin1`, `ascii` to [`Charset`]. |
//! | `line_ending`, `write_bom` | Destination terminator and optional UTF-8 BOM. |
//! | `columns[].name` | Unique field name used in `col.<name>`. |
//! | `columns[].header`, `position` | Header selection and one-based source/output position. |
//! | `columns[].type` | [`Column::column_type`], using the matching [`ColumnType`]. |
//! | `columns[].format`, `required` | Temporal tokens and nonempty-field validation. |
//! | `record_mode`, `group_by` | [`Document::messages`] partitions contiguous rows. |
//! | `max_file_bytes` | Size limit: [`read()`] checks it before decoding, [`write()`] refuses larger output. |
//! | `file_name`, `on_exists` | Copied metadata; enforced by the transport. |
//!
//! Codec validation also rejects ambiguous duplicate positions/effective headers,
//! quote characters equal to the delimiter, CR, LF, NUL or BOM, invalid temporal
//! formats, and `write_bom` for non-UTF-8 output. Date formats require year, month,
//! day; time formats require hours and minutes; datetime requires both. Seconds
//! are optional. Each component may appear once. `file_name` may be absent because
//! source/destination role validation belongs to the configuration layer.
//!
//! ```
//! use packie_files::{Column, Template, read, write};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let template = Template {
//!     columns: vec![Column { name: "sample_id".into(), ..Column::default() }],
//!     ..Template::default()
//! };
//! let mut document = read(b"sample_id\nSYN-001\n", &template)?;
//! document.records[0].set("col.sample_id", "SYN-002")?;
//! assert_eq!(write(&document.records, &template)?, b"sample_id\r\nSYN-002\r\n");
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod codec;
mod error;
mod lexer;
mod model;
mod template;
mod temporal;

pub use codec::{read, write};
pub use error::{Error, PathError};
pub use model::{Document, Field, Record, RecordModel, Value};
pub use template::{
    Charset, Column, ColumnType, LineEnding, OnExists, Problem, RecordMode, Template,
};
