//! Input preconditions (configuration.md §8.2 `require`): checks on the source message that
//! run before any rule, so a mapping that assumes a shape (one patient, at most 18 tests, one
//! specimen id) refuses other messages instead of misattributing them.
//!
//! ```json
//! [ { "count": "OBR", "min": 1, "max": 18 }, { "same": "OBR.3.1" } ]
//! ```

use std::borrow::Cow;

use serde_json::Value;

use crate::error::{Reason, TransformError};
use crate::json::{Obj, Problems};
use crate::model::{Model, Schema};
use crate::path::{Path, Scope};

/// Largest `count.min` and `count.max`.
const MAX_COUNT: u64 = 65_535;

/// One compiled precondition.
#[derive(Debug, Clone)]
pub(crate) enum Require {
    /// The number of occurrences of `record` is within `min..=max`.
    Count {
        record: String,
        min: Option<usize>,
        max: Option<usize>,
    },
    /// Every occurrence of the path's record holds the same text there.
    Same(Path),
}

impl Require {
    /// Checks the source; `index` is the condition's position, for the error.
    pub(crate) fn check(&self, source: &dyn Model, index: usize) -> Result<(), TransformError> {
        let (path, reason) = match self {
            Self::Count { record, min, max } => {
                let found = source.count(record);
                let low = min.unwrap_or(0);
                if found >= low && max.is_none_or(|max| found <= max) {
                    return Ok(());
                }
                let reason = Reason::Count {
                    found,
                    min: *min,
                    max: *max,
                };
                (record.as_str(), reason)
            }
            Self::Same(path) => {
                let record = path.record().unwrap_or_default();
                let text = |occurrence| {
                    source
                        .get(&path.at_occurrence(occurrence))
                        .unwrap_or(Cow::Borrowed(""))
                };
                let first = text(0);
                if (1..source.count(record)).all(|occurrence| text(occurrence) == first) {
                    return Ok(());
                }
                (path.as_str(), Reason::Differ)
            }
        };
        Err(TransformError {
            rule: 0,
            rule_path: format!("require[{index}]"),
            op: "require",
            path: path.to_owned(),
            reason,
        })
    }
}

/// Compiles the `require` list against the source schema.
pub(crate) fn compile_require(
    value: &Value,
    schema: &Schema,
    at: &str,
    problems: &mut Problems,
) -> Option<Vec<Require>> {
    let Some(list) = value.as_array() else {
        problems.add(at, "must be an array of conditions");
        return None;
    };
    let compiled: Vec<Option<Require>> = list
        .iter()
        .enumerate()
        .map(|(i, item)| compile_one(item, schema, &format!("{at}[{i}]"), problems))
        .collect();
    compiled.into_iter().collect()
}

fn compile_one(
    value: &Value,
    schema: &Schema,
    at: &str,
    problems: &mut Problems,
) -> Option<Require> {
    let keys: &[&str] = match value.as_object() {
        Some(map) if map.contains_key("count") => &["count", "min", "max"],
        Some(map) if map.contains_key("same") => &["same"],
        _ => {
            problems.add(at, "a condition is {\"count\": …} or {\"same\": …}");
            return None;
        }
    };
    let object = Obj::new(value, at, keys, problems)?;
    if object.has("same") {
        return same(&object, schema, problems);
    }
    let record = object.required_string("count", problems).and_then(|name| {
        match schema.check_record(name, false) {
            Ok(()) => Some(name.to_owned()),
            Err(message) => {
                problems.add(object.at("count"), message);
                None
            }
        }
    });
    let min = bound(&object, "min", problems);
    let max = bound(&object, "max", problems);
    let (min, max) = (min?, max?);
    match (min, max) {
        (None, None) => {
            problems.add(at, "needs `min`, `max` or both");
            None
        }
        (Some(low), Some(high)) if low > high => {
            problems.add(object.at("min"), "must not be greater than `max`");
            None
        }
        _ => Some(Require::Count {
            record: record?,
            min,
            max,
        }),
    }
}

fn same(object: &Obj<'_>, schema: &Schema, problems: &mut Problems) -> Option<Require> {
    let text = object.required_string("same", problems)?;
    let message = match schema.parse(text) {
        Err(error) => error.to_string(),
        Ok(path) if path.scope() == Scope::Current => {
            "`$.` paths are only valid inside for_each".to_owned()
        }
        Ok(path) if path.record().is_none() => {
            "the path addresses no repeating record or segment".to_owned()
        }
        Ok(path) if occurrence_written(&path) => {
            "write the path without an occurrence; every occurrence is compared".to_owned()
        }
        Ok(path) => match schema.check(&path, false) {
            Ok(()) => return Some(Require::Same(path)),
            Err(message) => message,
        },
    };
    problems.add(object.at("same"), message);
    None
}

/// Whether the path text names an occurrence (`OBR[1].3`), which `same` does not take.
fn occurrence_written(path: &Path) -> bool {
    path.record()
        .is_some_and(|record| path.as_str().starts_with(&format!("{record}[")))
}

/// An optional `min`/`max` from 0 to [`MAX_COUNT`]; `Some(None)` when absent.
fn bound(object: &Obj<'_>, key: &str, problems: &mut Problems) -> Option<Option<usize>> {
    let Some(value) = object.value(key) else {
        return Some(None);
    };
    let number = value
        .as_u64()
        .filter(|n| *n <= MAX_COUNT)
        .and_then(|n| usize::try_from(n).ok());
    if number.is_none() {
        problems.add(
            object.at(key),
            format!("must be an integer from 0 to {MAX_COUNT}"),
        );
    }
    number.map(Some)
}
