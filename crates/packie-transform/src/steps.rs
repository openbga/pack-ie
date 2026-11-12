//! Text steps (configuration.md §8.2): optional `steps` next to a `from`, run in order on
//! the text read before the rule uses it.
//!
//! ```json
//! [ { "step": "trim" }, { "step": "split", "separator": "/", "take": 0 },
//!   { "step": "strip_leading", "chars": "0" } ]
//! ```
//!
//! A part or slice that does not exist is empty text. The one failure is size: no step may
//! produce more than [`MIN_LIMIT`] bytes or the length of the text before the steps,
//! whichever is larger, so a chain of `replace` or case steps cannot multiply a field. The
//! growing steps (`replace`, `upper`, `lower`) compute their result's length before they
//! allocate it; the others never grow the text.

use std::borrow::Cow;

use serde_json::Value;

use crate::json::{Obj, Problems};

/// Most steps after one `from`.
const MAX_STEPS: usize = 16;
/// Longest `chars` set and `split` separator, in characters.
const MAX_SEPARATOR: usize = 16;
/// Longest `replace` text, in characters.
const MAX_REPLACE: usize = 64;
/// Largest `split.take` magnitude.
const MAX_TAKE: i64 = 100;
/// Largest `slice.start` and `slice.length`.
const MAX_SLICE: u64 = 65_535;
/// Smallest size bound of a step result, in bytes; the text's own length before the steps
/// is the bound when it is larger.
pub(crate) const MIN_LIMIT: usize = 65_535;

/// A step would produce more than the size bound; nothing was allocated for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TooLong;

const STEP_NAMES: &str = "trim, strip_leading, split, slice, replace, upper, lower";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    Trim,
    StripLeading(String),
    Split { separator: String, take: i64 },
    Slice { start: usize, length: Option<usize> },
    Replace { find: String, with: String },
    Upper,
    Lower,
}

/// The steps after one `from`; empty when none were given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Steps(Vec<Step>);

impl Steps {
    /// Runs every step on `text`.
    ///
    /// # Errors
    /// [`TooLong`] when a step's result would exceed `max(MIN_LIMIT, text.len())` bytes.
    pub(crate) fn apply<'t>(&self, text: Cow<'t, str>) -> Result<Cow<'t, str>, TooLong> {
        let limit = text.len().max(MIN_LIMIT);
        self.0
            .iter()
            .try_fold(text, |text, step| step.apply(text, limit))
    }
}

/// The byte length of `text` with every character mapped by `case`, without building it.
fn case_len<I: Iterator<Item = char>>(text: &str, case: impl Fn(char) -> I) -> Option<usize> {
    text.chars().try_fold(0_usize, |total, c| {
        case(c).try_fold(total, |total, mapped| total.checked_add(mapped.len_utf8()))
    })
}

/// `Ok(())` when a result of `len` bytes (`None`: overflowed) fits `limit`.
fn fits(len: Option<usize>, limit: usize) -> Result<(), TooLong> {
    len.filter(|len| *len <= limit).map(|_| ()).ok_or(TooLong)
}

impl Step {
    fn apply<'t>(&self, text: Cow<'t, str>, limit: usize) -> Result<Cow<'t, str>, TooLong> {
        let owned = |s: &str| Cow::Owned(s.to_owned());
        Ok(match self {
            Self::Trim => match text {
                Cow::Borrowed(s) => Cow::Borrowed(s.trim()),
                Cow::Owned(s) => owned(s.trim()),
            },
            Self::StripLeading(chars) => {
                let last = text.char_indices().last().map_or(0, |(i, _)| i);
                let start = text
                    .char_indices()
                    .find(|&(i, c)| i == last || !chars.contains(c))
                    .map_or(0, |(i, _)| i);
                owned(&text[start..])
            }
            Self::Split { separator, take } => {
                let parts: Vec<&str> = text.split(separator.as_str()).collect();
                let index = if *take < 0 {
                    usize::try_from(take.unsigned_abs())
                        .ok()
                        .and_then(|back| parts.len().checked_sub(back))
                } else {
                    usize::try_from(*take).ok()
                };
                owned(index.and_then(|i| parts.get(i)).copied().unwrap_or(""))
            }
            Self::Slice { start, length } => {
                let chars = text.chars().skip(*start);
                Cow::Owned(match length {
                    Some(length) => chars.take(*length).collect(),
                    None => chars.collect(),
                })
            }
            Self::Replace { find, with } => {
                // Matches do not overlap, so the removed bytes never exceed the text.
                let matches = text.matches(find.as_str()).count();
                let len = matches
                    .checked_mul(find.len())
                    .and_then(|removed| text.len().checked_sub(removed))
                    .zip(matches.checked_mul(with.len()))
                    .and_then(|(kept, added)| kept.checked_add(added));
                fits(len, limit)?;
                Cow::Owned(text.replace(find.as_str(), with))
            }
            Self::Upper => {
                fits(case_len(&text, char::to_uppercase), limit)?;
                Cow::Owned(text.to_uppercase())
            }
            Self::Lower => {
                // `to_lowercase` maps a final sigma by context, to a letter of the same length.
                fits(case_len(&text, char::to_lowercase), limit)?;
                Cow::Owned(text.to_lowercase())
            }
        })
    }
}

/// Compiles the optional `steps` field of `object`.
pub(crate) fn compile_steps(object: &Obj<'_>, problems: &mut Problems) -> Option<Steps> {
    let Some(value) = object.value("steps") else {
        return Some(Steps::default());
    };
    let at = object.at("steps");
    let Some(list) = value.as_array() else {
        problems.add(at, "must be an array of steps");
        return None;
    };
    if list.len() > MAX_STEPS {
        problems.add(&at, format!("at most {MAX_STEPS} steps"));
        return None;
    }
    let steps: Vec<Option<Step>> = list
        .iter()
        .enumerate()
        .map(|(i, step)| compile_step(step, &format!("{at}[{i}]"), problems))
        .collect();
    steps.into_iter().collect::<Option<_>>().map(Steps)
}

fn step_fields(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "trim" | "upper" | "lower" => &["step"],
        "strip_leading" => &["step", "chars"],
        "split" => &["step", "separator", "take"],
        "slice" => &["step", "start", "length"],
        "replace" => &["step", "find", "with"],
        _ => return None,
    })
}

fn compile_step(value: &Value, at: &str, problems: &mut Problems) -> Option<Step> {
    let Some(name) = value.get("step").and_then(Value::as_str) else {
        problems.add(format!("{at}.step"), "is required and must be a string");
        return None;
    };
    let Some(fields) = step_fields(name) else {
        problems.add(
            format!("{at}.step"),
            format!("unknown step; use {STEP_NAMES}"),
        );
        return None;
    };
    let object = Obj::new(value, at, fields, problems)?;
    let text = |key: &str, min: usize, max: usize, problems: &mut Problems| {
        let text = object.required_string(key, problems)?;
        let count = text.chars().count();
        if (min..=max).contains(&count) {
            Some(text.to_owned())
        } else {
            problems.add(
                object.at(key),
                format!("must have {min} to {max} characters"),
            );
            None
        }
    };
    match name {
        "trim" => Some(Step::Trim),
        "upper" => Some(Step::Upper),
        "lower" => Some(Step::Lower),
        "strip_leading" => text("chars", 1, MAX_SEPARATOR, problems).map(Step::StripLeading),
        "split" => {
            let separator = text("separator", 1, MAX_SEPARATOR, problems);
            let take = integer(&object, "take", -MAX_TAKE, MAX_TAKE, problems);
            Some(Step::Split {
                separator: separator?,
                take: take?,
            })
        }
        "slice" => {
            let start = count(&object, "start", 0, problems);
            let length = if object.has("length") {
                count(&object, "length", 1, problems).map(Some)
            } else {
                Some(None)
            };
            Some(Step::Slice {
                start: start?,
                length: length?,
            })
        }
        _ => {
            let find = text("find", 1, MAX_REPLACE, problems);
            let with = text("with", 0, MAX_REPLACE, problems);
            Some(Step::Replace {
                find: find?,
                with: with?,
            })
        }
    }
}

/// A required integer field from `min` to `max`.
fn integer(
    object: &Obj<'_>,
    key: &str,
    min: i64,
    max: i64,
    problems: &mut Problems,
) -> Option<i64> {
    let Some(value) = object.value(key) else {
        problems.add(object.at(key), "is required");
        return None;
    };
    let number = value.as_i64().filter(|n| (min..=max).contains(n));
    if number.is_none() {
        problems.add(
            object.at(key),
            format!("must be an integer from {min} to {max}"),
        );
    }
    number
}

/// A required `slice` count from `min` to [`MAX_SLICE`].
fn count(object: &Obj<'_>, key: &str, min: u64, problems: &mut Problems) -> Option<usize> {
    let Some(value) = object.value(key) else {
        problems.add(object.at(key), "is required");
        return None;
    };
    let number = value
        .as_u64()
        .filter(|n| (min..=MAX_SLICE).contains(n))
        .and_then(|n| usize::try_from(n).ok());
    if number.is_none() {
        problems.add(
            object.at(key),
            format!("must be an integer from {min} to {MAX_SLICE}"),
        );
    }
    number
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn steps(value: &Value) -> Result<Steps, Vec<String>> {
        let wrapper = json!({ "steps": value });
        let mut problems = Problems::default();
        let object = Obj::new(&wrapper, "r", &["steps"], &mut problems).expect("object");
        let steps = compile_steps(&object, &mut problems);
        problems
            .finish(steps)
            .map_err(|found| found.into_iter().map(|p| p.to_string()).collect())
    }

    fn run(value: Value, text: &str) -> String {
        steps(&value)
            .expect("valid steps")
            .apply(Cow::Borrowed(text))
            .expect("within the size bound")
            .into_owned()
    }

    #[test]
    fn each_step_does_one_thing() {
        let cases = [
            (json!({"step": "trim"}), "  SYN 42 \t", "SYN 42"),
            (
                json!({"step": "strip_leading", "chars": "0"}),
                "000120",
                "120",
            ),
            (json!({"step": "strip_leading", "chars": "0"}), "000", "0"),
            (
                json!({"step": "strip_leading", "chars": "#0"}),
                "#0A1",
                "A1",
            ),
            (json!({"step": "strip_leading", "chars": "0"}), "", ""),
            (
                json!({"step": "split", "separator": "/", "take": 0}),
                "GLU/2",
                "GLU",
            ),
            (
                json!({"step": "split", "separator": "##", "take": 1}),
                "S1##ISO",
                "ISO",
            ),
            (
                json!({"step": "split", "separator": "-", "take": -1}),
                "3.5-5.1",
                "5.1",
            ),
            (
                json!({"step": "split", "separator": "-", "take": 2}),
                "3.5-5.1",
                "",
            ),
            (
                json!({"step": "split", "separator": "-", "take": -3}),
                "3.5-5.1",
                "",
            ),
            (
                json!({"step": "slice", "start": 0, "length": 1}),
                "female",
                "f",
            ),
            (json!({"step": "slice", "start": 2}), "ÄÖÜX", "ÜX"),
            (json!({"step": "slice", "start": 9}), "abc", ""),
            (
                json!({"step": "replace", "find": "-", "with": ""}),
                "2026-10-12",
                "20261012",
            ),
            (json!({"step": "upper"}), "neg", "NEG"),
            (json!({"step": "lower"}), "NEG", "neg"),
        ];
        for (step, input, expected) in cases {
            assert_eq!(run(json!([step]), input), expected, "{step} on {input:?}");
        }
    }

    #[test]
    fn steps_run_in_order() {
        let chain = json!([
            {"step": "trim"},
            {"step": "slice", "start": 0, "length": 1},
            {"step": "upper"}
        ]);
        assert_eq!(run(chain, "  female"), "F");
        assert_eq!(run(json!([]), " kept "), " kept ");
    }

    fn zeros_to(width: usize) -> Value {
        json!({"step": "replace", "find": "0", "with": "0".repeat(width)})
    }

    fn sized(value: &Value, text: &str) -> Result<usize, TooLong> {
        let steps = steps(value).expect("valid steps");
        steps.apply(Cow::Borrowed(text)).map(|out| out.len())
    }

    #[test]
    fn growth_is_refused_before_the_result_is_built() {
        // Each step multiplies by 64: 64, 4,096, then 262,144 bytes would pass the bound.
        assert_eq!(sized(&json!([zeros_to(64), zeros_to(64)]), "0"), Ok(4_096));
        assert_eq!(
            sized(&Value::Array(vec![zeros_to(64); 3]), "0"),
            Err(TooLong)
        );
        // Sixteen steps would ask for 64^16 bytes; refused at the third, with no allocation.
        assert_eq!(
            sized(&Value::Array(vec![zeros_to(64); 16]), "0"),
            Err(TooLong)
        );
        // The bound itself: 1,023 × 64 = 65,472 bytes fit, 1,024 × 64 = 65,536 do not.
        assert_eq!(
            sized(&json!([zeros_to(64)]), &"0".repeat(1_023)),
            Ok(65_472)
        );
        assert_eq!(
            sized(&json!([zeros_to(64)]), &"0".repeat(1_024)),
            Err(TooLong)
        );
    }

    #[test]
    fn large_fields_may_be_processed_without_growing() {
        let field = "ab-".repeat(70_000);
        let same = json!([
            {"step": "replace", "find": "a", "with": "A"},
            {"step": "upper"},
            {"step": "replace", "find": "-", "with": ""},
            {"step": "trim"}
        ]);
        assert_eq!(sized(&same, &field), Ok(140_000));
        assert_eq!(
            sized(
                &json!([{"step": "replace", "find": "-", "with": "--"}]),
                &field
            ),
            Err(TooLong)
        );
    }

    #[test]
    fn case_changes_that_grow_are_measured_first() {
        // `ŉ` (2 bytes) upper-cases to `ʼN` (3 bytes); `İ` (2 bytes) lower-cases to 3 bytes.
        let upper = json!([{"step": "upper"}]);
        assert_eq!(sized(&upper, &"ŉ".repeat(100)), Ok(300));
        assert_eq!(sized(&upper, &"ŉ".repeat(32_768)), Err(TooLong));
        let lower = json!([{"step": "lower"}]);
        assert_eq!(sized(&lower, &"İ".repeat(32_768)), Err(TooLong));
        assert_eq!(run(json!([{"step": "upper"}]), "straße"), "STRASSE");
        assert_eq!(run(json!([{"step": "lower"}]), "ΟΔΟΣ"), "οδος");
    }

    #[test]
    fn invalid_steps_are_reported_with_paths() {
        let problems = steps(&json!([
            {"step": "pad"},
            {"step": "split", "separator": "", "take": 101},
            {"step": "slice", "start": -1, "length": 0},
            {"step": "replace", "find": "x"},
            {"step": "trim", "extra": 1},
            "trim"
        ]))
        .expect_err("invalid");
        assert_eq!(
            problems,
            [
                "r.steps[0].step: unknown step; use trim, strip_leading, split, slice, replace, upper, lower",
                "r.steps[1].separator: must have 1 to 16 characters",
                "r.steps[1].take: must be an integer from -100 to 100",
                "r.steps[2].start: must be an integer from 0 to 65535",
                "r.steps[2].length: must be an integer from 1 to 65535",
                "r.steps[3].with: is required",
                "r.steps[4].extra: unknown field",
                "r.steps[5].step: is required and must be a string",
            ]
        );
        let too_many: Vec<Value> = (0..17).map(|_| json!({"step": "trim"})).collect();
        assert_eq!(
            steps(&Value::Array(too_many)).expect_err("too many"),
            ["r.steps: at most 16 steps"]
        );
        assert_eq!(
            steps(&json!("trim")).expect_err("not a list"),
            ["r.steps: must be an array of steps"]
        );
    }
}
