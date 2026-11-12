//! Filters and mappings over parsed protocol messages
//! (`docs/spec/configuration.md` §8, `docs/spec/message-channels.md` §6).
//!
//! The crate is pure: no I/O, no clock, no configuration types. Filters and mappings are
//! the JSON values the runtime document stores; this crate compiles them, reports every
//! problem with its JSON path, and runs them over messages.
//!
//! # The model
//!
//! A [`Model`] is a parsed message that can be read and written by [`Path`]. One adapter
//! exists per protocol:
//!
//! | Adapter | Wraps | Writable | Created from |
//! | --- | --- | --- | --- |
//! | [`Hl7Model`] | `packie_hl7::Message` | yes | [`Hl7Model::parse`], [`Hl7Model::from_message`], [`Hl7Model::new`] |
//! | [`AstmModel`] | `packie_astm::Record`s | yes | [`AstmModel::parse`], [`AstmModel::from_records`], [`AstmModel::new`] |
//! | [`AuModel`] | `packie_au::SampleResults` | no | [`AuModel::from_results`] |
//! | [`RecordModel`] | rows of named columns | yes | [`RecordModel::from_rows`], [`RecordModel::new`] |
//!
//! [`RecordModel`] stands on its own: the delimited-file codec (`packie-files`) hands its
//! rows over as `(column, text)` pairs, so this crate does not depend on it.
//!
//! # Path syntax
//!
//! One parser ([`Path::parse`]) with the syntax of each model ([`Syntax`]). Occurrences
//! and repetitions count from 0 and default to 0; fields, components and subcomponents
//! count from 1. A path that stops above the leaf yields the text with its separators
//! (`MSH.9` is `ORU^R01`).
//!
//! | Protocol | Shape | Examples |
//! | --- | --- | --- |
//! | HL7 | `SEG[occ].field[rep].component.subcomponent` | `MSH.9`, `PID.3[0].1`, `OBX[2].5` |
//! | ASTM | `R[occ].field[rep].component` | `R.4`, `R[2].3.4`, `O.3`, `P.3` |
//! | AU | `name` or `test[occ].name` | `sample_id`, `test[0].value` |
//! | Delimited | `col[occ].name` | `col.sample_id`, `col[1].result` |
//!
//! * **HL7.** Segment names are three characters. `MSH` has one occurrence and its
//!   fields are numbered as in the standard (`MSH.1` is the field separator, `MSH.2` the
//!   encoding characters, `MSH.9` the message type); both are read-only. Reading is the
//!   codec's decoded text. Writing a value that contains `~`, `^` or `&` below the
//!   addressed level stores them as structure (so `ORU^R01` written to `MSH.9` is two
//!   components); write to a subcomponent path (`PID.5.1.1`) to store a delimiter
//!   character as data.
//! * **ASTM.** Records are one flat list, so `R[2]` is the third result of the message
//!   wherever it sits under its patient and order. Fields are numbered as in ASTM E1394,
//!   where field 1 is the record type: `R.1` is `R`, `R.2` the sequence number, `R.3` the
//!   test id (`^^^GLU`, so `R.3.4` is `GLU`), `R.4` the value, `R.7` the abnormal flag,
//!   `R.13` the completion time; `P.3` and `P.4` the patient ids, `P.6` the name; `O.3`
//!   the specimen id. `R.1` and `H.2` (the delimiters) cannot be written. A new record is
//!   inserted before `L`.
//! * **AU.** Header fields are `kind`, `unit_no`, `rack`, `cup`, `sample_type`,
//!   `sample_no`, `sample_id`, `reserved`, `sex`, `age`, `month`, `info1` to `info6`;
//!   each result item is `test[n]` with `code`, `value` and `flags`. Padding spaces are
//!   trimmed. AU messages are read-only; no mapping can target them.
//! * **Delimited.** `col.<name>` is a column of the first row; a grouped message has
//!   several rows, `col[1].<name>` and `for_each` over `col`.
//! * **Inside `for_each`** a path may begin with `$.` and omit the record: `$.3.1` in an
//!   HL7 or ASTM loop, `$.value` in an AU loop, `$.sample_id` in a delimited loop. In
//!   `from` and in conditions it addresses the current input occurrence, in `to` the
//!   output occurrence the loop just created.
//!
//! Path errors carry the byte position of the problem ([`PathError`]).
//!
//! # Filters
//!
//! [`Filter`] and [`Filters`] implement §8.1: `equals`, `not_equals`, `in`, `not_in`,
//! `starts_with`, `contains`, `matches` (linear-time regular expressions of at most 200
//! characters), `exists`, `not_exists`, `gt`, `gte`, `lt`, `lte` (exact decimals) with
//! `all`, `any`, `not`. Evaluation is infallible and never panics: a missing field is
//! empty text. See the [`Filter`] documentation for the table of semantics.
//!
//! # Mappings
//!
//! [`Mapping`] implements §8.2: ordered rules `copy`, `constant`, `lookup` (code tables
//! with `error`, `keep`, `default`, `skip` on a miss; optional `ignore_case`), `concat`,
//! `datetime` (format tokens `yyyy yy MM dd HH mm ss`, IANA time zones), `calculate`
//! (exact decimal formulas over fields and constants, one final rounding, comparators such
//! as `<5` carried truthfully, HL7 structured numerics), `when`, `for_each` (creating new
//! occurrences, updating the target's copy of each, or writing the repetitions of one
//! field), `append` (one record added only
//! when every rule in it writes, for calculated results) and `sequence`. Every `from` may be
//! followed by text `steps` (trim, strip leading characters, split, slice, replace, case).
//! No floating point is used anywhere. A mapping runs over a source [`Model`] into a target
//! [`Model`] that the caller supplies, and stops at the first failing rule with a
//! [`TransformError`] that names the rule, its path and the reason, never a value.
//! [`CodeTable`]s belong to the mapping that defines them, so they are versioned with the
//! channel configuration.
//!
//! # How the pipeline uses it
//!
//! Compile when a configuration is applied, run for every message. Compiled filters and
//! mappings are immutable and can be shared between messages and threads that only read
//! them; keep them with the `config_version` they were compiled from.
//!
//! 1. **Apply.** For the channel `filters` call [`Filters::compile`] with the source
//!    protocol's [`Schema`]; for each destination compile its `filters` (also against the
//!    source schema, because they run on the source message) and its `mapping` with
//!    [`Mapping::compile`] (source schema, destination schema). Report every [`Problem`]
//!    as `filter_invalid` or `mapping_invalid`; its `path` is already the JSON path when
//!    the document path is passed as `at`. A delimited connector's [`Schema`] lists its
//!    column names.
//! 2. **Per message.** Wrap the parsed message in its adapter. Evaluate
//!    [`Filters::passes`] for the channel, then for each enabled destination (a false
//!    result is `filtered`, not an error). For a destination with a mapping, build the
//!    target: an empty model with the generated header fields (for HL7 `Hl7Model::new()`
//!    with MSH-3..7 and MSH-10..12 from the connector settings, a new control id and the
//!    current time; for ASTM `AstmModel::new()`; for files `RecordModel::new()`), or a
//!    clone of the source model when the protocols are equal. Call [`Mapping::apply`],
//!    then encode: [`Hl7Model::encode`], [`AstmModel::encode`], or the rows of
//!    [`RecordModel::rows`] in the connector's column order.
//! 3. **Failure.** `Err(TransformError)` is the `transform_error` of
//!    message-channels.md §6: create the delivery `failed` with the error's `Display` text
//!    as its summary (it names rule and path, not data) and discard the half-written
//!    target. Re-running after the mapping is corrected is a fresh `apply` on a fresh
//!    target.
//!
//! ```
//! use serde_json::json;
//! use packie_transform::{Filters, Hl7Model, Mapping, Schema};
//!
//! let schema = Schema::hl7();
//! let filters = Filters::compile(
//!     &[json!({"path": "MSH.9.1", "op": "equals", "value": "ORU"})],
//!     &schema,
//!     "channels[0].filters",
//! )
//! .expect("valid filters");
//! let mapping = Mapping::compile(
//!     &json!({"rules": [
//!         {"op": "constant", "value": "ORU^R01", "to": "MSH.9"},
//!         {"op": "for_each", "over": "OBX", "into": "OBX", "rules": [
//!             {"op": "sequence", "to": "$.1"},
//!             {"op": "copy", "from": "$.5", "to": "$.5"}]}]}),
//!     &schema,
//!     &schema,
//!     "channels[0].destinations[0].mapping",
//! )
//! .expect("valid mapping");
//!
//! let source = Hl7Model::parse(
//!     b"MSH|^~\\&|A|B|C|D|20261012120000||ORU^R01|M1|P|2.5.1\rOBX|9|NM|GLU||5.5\r",
//! )?;
//! assert!(filters.passes(&source));
//! let mut target = Hl7Model::new();
//! mapping.apply(&source, &mut target)?;
//! assert_eq!(
//!     target.encode(),
//!     b"MSH|^~\\&|||||||ORU^R01\rOBX|1||||5.5\r".to_vec()
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod apply;
mod astm;
mod au;
mod datetime;
mod decimal;
mod error;
mod exact;
mod filter;
mod formula;
mod hl7;
mod json;
mod mapping;
mod model;
mod path;
mod record;
mod require;
mod rules;
mod steps;
mod tables;

pub use astm::AstmModel;
pub use au::AuModel;
pub use error::{ModelError, PathError, Problem, Reason, TransformError};
pub use filter::{Filter, Filters};
pub use hl7::Hl7Model;
pub use mapping::Mapping;
pub use model::{Model, Schema};
pub use path::{FieldRef, Path, Scope, Syntax};
pub use record::RecordModel;
pub use tables::CodeTable;
