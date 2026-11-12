//! Strict reading of the configuration JSON shapes. Problems are collected, never thrown
//! at the first one, and carry the JSON path of the offending value.

use serde_json::{Map, Value};

use crate::error::Problem;

/// An object whose keys have been checked against the allowed list.
pub(crate) struct Obj<'a> {
    map: &'a Map<String, Value>,
    path: String,
}

/// Collects problems for one compilation.
#[derive(Debug, Default)]
pub(crate) struct Problems(pub(crate) Vec<Problem>);

impl Problems {
    pub(crate) fn add(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.0.push(Problem::new(path, message));
    }

    /// `compiled` when it exists and no problem was recorded, otherwise every problem.
    pub(crate) fn finish<T>(self, compiled: Option<T>) -> Result<T, Vec<Problem>> {
        match compiled {
            Some(value) if self.0.is_empty() => Ok(value),
            _ => Err(self.0),
        }
    }
}

impl<'a> Obj<'a> {
    /// `value` as an object with only `allowed` keys; every unknown key is a problem.
    /// `None` (after reporting) when `value` is not an object.
    pub(crate) fn new(
        value: &'a Value,
        path: &str,
        allowed: &[&str],
        problems: &mut Problems,
    ) -> Option<Self> {
        let Some(map) = value.as_object() else {
            problems.add(path, "must be an object");
            return None;
        };
        for key in map.keys().filter(|k| !allowed.contains(&k.as_str())) {
            problems.add(format!("{path}.{key}"), "unknown field");
        }
        Some(Self {
            map,
            path: path.to_owned(),
        })
    }

    pub(crate) fn at(&self, key: &str) -> String {
        format!("{}.{key}", self.path)
    }

    pub(crate) fn has(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub(crate) fn value(&self, key: &str) -> Option<&'a Value> {
        self.map.get(key)
    }

    /// An optional string field; a non-string is a problem.
    pub(crate) fn string(&self, key: &str, problems: &mut Problems) -> Option<&'a str> {
        let value = self.map.get(key)?;
        let text = value.as_str();
        if text.is_none() {
            problems.add(self.at(key), "must be a string");
        }
        text
    }

    /// A required string field.
    pub(crate) fn required_string(&self, key: &str, problems: &mut Problems) -> Option<&'a str> {
        self.required(key, problems, |problems| self.string(key, problems))
    }

    /// An optional text field that may also be written as a number.
    pub(crate) fn scalar(&self, key: &str, problems: &mut Problems) -> Option<String> {
        let value = self.map.get(key)?;
        let text = scalar_text(value);
        if text.is_none() {
            problems.add(self.at(key), "must be a string or a number");
        }
        text
    }

    /// A required text field that may also be written as a number.
    pub(crate) fn required_scalar(&self, key: &str, problems: &mut Problems) -> Option<String> {
        self.required(key, problems, |problems| self.scalar(key, problems))
    }

    /// A boolean field with a default.
    pub(crate) fn boolean(&self, key: &str, default: bool, problems: &mut Problems) -> bool {
        match self.map.get(key) {
            None => default,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                problems.add(self.at(key), "must be true or false");
                default
            }
        }
    }

    /// A required array field.
    pub(crate) fn required_array(
        &self,
        key: &str,
        problems: &mut Problems,
    ) -> Option<&'a Vec<Value>> {
        self.required(key, problems, |problems| {
            let array = self.map.get(key)?.as_array();
            if array.is_none() {
                problems.add(self.at(key), "must be an array");
            }
            array
        })
    }

    /// Reads a field with `read` when the key is present; a missing key is a problem.
    fn required<T>(
        &self,
        key: &str,
        problems: &mut Problems,
        read: impl FnOnce(&mut Problems) -> Option<T>,
    ) -> Option<T> {
        if self.has(key) {
            read(problems)
        } else {
            problems.add(self.at(key), "is required");
            None
        }
    }
}

/// Text of a JSON string or number.
pub(crate) fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}
