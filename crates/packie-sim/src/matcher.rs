//! Matchers and dotted paths used by assertion steps (scenarios.md §7.3).
//!
//! A bare JSON value means equality (`null` allowed). An object with exactly one operator
//! key is an operator: `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `in`, `contains`, `matches`,
//! `exists`. A path that is absent from the observed document fails every matcher except
//! `{"exists": false}`; `exists: true` also requires a non-null value.

use std::cmp::Ordering;

use regex::Regex;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// One expectation about a value.
#[derive(Debug, Clone)]
pub enum Matcher {
    /// Equal to the value.
    Eq(Value),
    /// Not equal.
    Ne(Value),
    /// Greater than (numbers).
    Gt(Value),
    /// Greater than or equal.
    Gte(Value),
    /// Less than.
    Lt(Value),
    /// Less than or equal.
    Lte(Value),
    /// One of the values.
    In(Vec<Value>),
    /// Contains the substring (strings) or the element (arrays).
    Contains(Value),
    /// The text matches the regular expression (unanchored search).
    Matches(Regex),
    /// Present and not null (`true`), or absent or null (`false`).
    Exists(bool),
}

const OPERATORS: [&str; 10] = [
    "eq", "ne", "gt", "gte", "lt", "lte", "in", "contains", "matches", "exists",
];

impl Matcher {
    /// Builds a matcher from its JSON form.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let Value::Object(map) = value else {
            return Ok(Self::Eq(value.clone()));
        };
        let mut entries = map.iter();
        let (Some((op, operand)), None) = (entries.next(), entries.next()) else {
            return Err(format!(
                "a matcher object has exactly one operator ({}), found {} keys",
                OPERATORS.join(", "),
                map.len()
            ));
        };
        let number = |what: &str| {
            if operand.is_number() {
                Ok(operand.clone())
            } else {
                Err(format!("`{what}` needs a number, found {operand}"))
            }
        };
        match op.as_str() {
            "eq" => Ok(Self::Eq(operand.clone())),
            "ne" => Ok(Self::Ne(operand.clone())),
            "gt" => Ok(Self::Gt(number("gt")?)),
            "gte" => Ok(Self::Gte(number("gte")?)),
            "lt" => Ok(Self::Lt(number("lt")?)),
            "lte" => Ok(Self::Lte(number("lte")?)),
            "in" => match operand {
                Value::Array(items) => Ok(Self::In(items.clone())),
                _ => Err("`in` needs an array".into()),
            },
            "contains" => Ok(Self::Contains(operand.clone())),
            "matches" => {
                let pattern = operand
                    .as_str()
                    .ok_or_else(|| "`matches` needs a string".to_owned())?;
                Regex::new(pattern)
                    .map(Self::Matches)
                    .map_err(|e| format!("`matches` regular expression: {e}"))
            }
            "exists" => operand
                .as_bool()
                .map(Self::Exists)
                .ok_or_else(|| "`exists` needs a boolean".to_owned()),
            other => Err(format!(
                "unknown matcher operator `{other}` (expected one of {})",
                OPERATORS.join(", ")
            )),
        }
    }

    /// Checks `actual` (`None` when the path is absent). `Err` says what was expected
    /// and what was observed.
    pub fn check(&self, actual: Option<&Value>) -> Result<(), String> {
        let present = actual.filter(|v| !v.is_null());
        if let Self::Exists(want) = self {
            return if present.is_some() == *want {
                Ok(())
            } else if *want {
                Err(format!("expected a value, observed {}", show(actual)))
            } else {
                Err(format!("expected no value, observed {}", show(actual)))
            };
        }
        let Some(actual) = actual else {
            return Err(format!(
                "expected {}, but the path is absent",
                self.describe()
            ));
        };
        let ok = match self {
            Self::Eq(want) => actual == want,
            Self::Ne(want) => actual != want,
            Self::Gt(want) => compare(actual, want) == Some(Ordering::Greater),
            Self::Gte(want) => matches!(
                compare(actual, want),
                Some(Ordering::Greater | Ordering::Equal)
            ),
            Self::Lt(want) => compare(actual, want) == Some(Ordering::Less),
            Self::Lte(want) => matches!(
                compare(actual, want),
                Some(Ordering::Less | Ordering::Equal)
            ),
            Self::In(items) => items.contains(actual),
            Self::Contains(want) => contains(actual, want),
            Self::Matches(re) => as_text(actual).is_some_and(|t| re.is_match(&t)),
            Self::Exists(_) => true,
        };
        if ok {
            Ok(())
        } else {
            Err(format!(
                "expected {}, observed {}",
                self.describe(),
                show(Some(actual))
            ))
        }
    }

    /// A short rendering for reports, such as `>= 3` or `matches /x/`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Eq(v) => v.to_string(),
            Self::Ne(v) => format!("!= {v}"),
            Self::Gt(v) => format!("> {v}"),
            Self::Gte(v) => format!(">= {v}"),
            Self::Lt(v) => format!("< {v}"),
            Self::Lte(v) => format!("<= {v}"),
            Self::In(v) => format!("in {}", Value::Array(v.clone())),
            Self::Contains(v) => format!("contains {v}"),
            Self::Matches(re) => format!("matches /{}/", re.as_str()),
            Self::Exists(true) => "a value".to_owned(),
            Self::Exists(false) => "no value".to_owned(),
        }
    }
}

impl<'de> Deserialize<'de> for Matcher {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(&value).map_err(D::Error::custom)
    }
}

fn show(value: Option<&Value>) -> String {
    value.map_or_else(|| "<absent>".to_owned(), Value::to_string)
}

fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        _ => None,
    }
}

fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    let (Value::Number(x), Value::Number(y)) = (a, b) else {
        return None;
    };
    if let (Some(x), Some(y)) = (x.as_i64(), y.as_i64()) {
        return Some(x.cmp(&y));
    }
    if let (Some(x), Some(y)) = (x.as_u64(), y.as_u64()) {
        return Some(x.cmp(&y));
    }
    x.as_f64()?.partial_cmp(&y.as_f64()?)
}

fn contains(actual: &Value, want: &Value) -> bool {
    match (actual, want) {
        (Value::String(a), Value::String(w)) => a.contains(w.as_str()),
        (Value::Array(items), _) => items.contains(want),
        _ => false,
    }
}

/// Turns the text values of log fields (which the engine records as text) into numbers
/// and booleans so `{"recovered": 0}` matches the field `"0"`.
#[must_use]
pub fn coerce_text(value: &str) -> Value {
    match serde_json::from_str::<Value>(value) {
        Ok(v @ (Value::Number(_) | Value::Bool(_))) => v,
        _ => Value::String(value.to_owned()),
    }
}

/// One step of a dotted path: a key and optional selectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    key: String,
    selectors: Vec<Selector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Selector {
    /// `[key=value]`: the array element whose field `key` has the text `value`.
    Field(String, String),
    /// `[3]`: the element at an index.
    Index(usize),
}

/// Parses `a.b[k=v].c`.
pub fn parse_path(path: &str) -> Result<Vec<Segment>, String> {
    let mut segments = Vec::new();
    let mut chars = path.chars().peekable();
    loop {
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '.' || c == '[' {
                break;
            }
            key.push(c);
            chars.next();
        }
        let mut selectors = Vec::new();
        while chars.peek() == Some(&'[') {
            chars.next();
            let mut inner = String::new();
            loop {
                match chars.next() {
                    Some(']') => break,
                    Some(c) => inner.push(c),
                    None => return Err(format!("path `{path}`: unclosed `[`")),
                }
            }
            selectors.push(match inner.split_once('=') {
                Some((k, v)) if !k.is_empty() => Selector::Field(k.to_owned(), v.to_owned()),
                _ => Selector::Index(
                    inner
                        .parse()
                        .map_err(|_| format!("path `{path}`: bad selector `[{inner}]`"))?,
                ),
            });
        }
        if key.is_empty() && selectors.is_empty() {
            return Err(format!("path `{path}` has an empty segment"));
        }
        segments.push(Segment { key, selectors });
        match chars.next() {
            None => return Ok(segments),
            Some('.') => {}
            Some(c) => return Err(format!("path `{path}`: unexpected `{c}`")),
        }
    }
}

/// Follows `path` into `root`; `None` when any step is absent.
#[must_use]
pub fn resolve<'a>(root: &'a Value, path: &[Segment]) -> Option<&'a Value> {
    let mut node = root;
    for segment in path {
        if !segment.key.is_empty() {
            node = node.get(&segment.key)?;
        }
        for selector in &segment.selectors {
            let items = node.as_array()?;
            node = match selector {
                Selector::Index(i) => items.get(*i)?,
                Selector::Field(k, v) => items
                    .iter()
                    .find(|item| item.get(k).and_then(as_text).as_deref() == Some(v.as_str()))?,
            };
        }
    }
    Some(node)
}

/// Checks every `path: matcher` pair against `root`, returning one line per mismatch.
#[must_use]
pub fn check_all<'a>(
    root: &Value,
    expectations: impl IntoIterator<Item = (&'a String, &'a Matcher)>,
) -> Vec<String> {
    let mut failures = Vec::new();
    for (path, matcher) in expectations {
        let observed = match parse_path(path) {
            Ok(segments) => resolve(root, &segments),
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        if let Err(why) = matcher.check(observed) {
            failures.push(format!("{path}: {why}"));
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn m(v: Value) -> Matcher {
        Matcher::from_value(&v).unwrap()
    }

    #[test]
    fn bare_values_are_equality_and_null_matches_null() {
        assert!(m(json!(3)).check(Some(&json!(3))).is_ok());
        assert!(m(json!(null)).check(Some(&json!(null))).is_ok());
        assert!(m(json!(null)).check(Some(&json!(1))).is_err());
        assert!(m(json!("x")).check(None).is_err(), "absent is not null");
    }

    #[test]
    fn exists_means_present_and_not_null() {
        assert!(m(json!({"exists": true})).check(Some(&json!("t"))).is_ok());
        assert!(
            m(json!({"exists": true}))
                .check(Some(&json!(null)))
                .is_err()
        );
        assert!(m(json!({"exists": false})).check(None).is_ok());
        assert!(
            m(json!({"exists": false}))
                .check(Some(&json!(null)))
                .is_ok()
        );
    }

    #[test]
    fn operators() {
        assert!(m(json!({"gte": 2})).check(Some(&json!(2))).is_ok());
        assert!(m(json!({"gt": 2})).check(Some(&json!(2))).is_err());
        assert!(
            m(json!({"in": ["a", "b"]}))
                .check(Some(&json!("b")))
                .is_ok()
        );
        assert!(
            m(json!({"contains": "ell"}))
                .check(Some(&json!("hello")))
                .is_ok()
        );
        assert!(
            m(json!({"matches": "endpoint A .* (read|write) failed"}))
                .check(Some(&json!("Channel x endpoint A \"p\" read failed: eof")))
                .is_ok()
        );
        assert!(m(json!({"ne": 1})).check(Some(&json!(2))).is_ok());
    }

    #[test]
    fn malformed_matchers_are_rejected() {
        assert!(Matcher::from_value(&json!({"nope": 1})).is_err());
        assert!(Matcher::from_value(&json!({"gt": 1, "lt": 2})).is_err());
        assert!(Matcher::from_value(&json!({"matches": "("})).is_err());
    }

    #[test]
    fn paths_select_array_elements() {
        let doc = json!({"destinations": [
            {"destination_id": "lis", "queue": {"queued": 4}},
            {"destination_id": "other", "queue": {"queued": 1}}]});
        let path = parse_path("destinations[destination_id=lis].queue.queued").unwrap();
        assert_eq!(resolve(&doc, &path), Some(&json!(4)));
        let path = parse_path("destinations[destination_id=nope].queue").unwrap();
        assert_eq!(resolve(&doc, &path), None);
        let path = parse_path("destinations[1].destination_id").unwrap();
        assert_eq!(resolve(&doc, &path), Some(&json!("other")));
    }

    #[test]
    fn log_text_fields_coerce_to_numbers() {
        assert_eq!(coerce_text("0"), json!(0));
        assert_eq!(coerce_text("true"), json!(true));
        assert_eq!(coerce_text("pause_channel"), json!("pause_channel"));
    }
}
