//! Placeholder substitution (scenarios.md §3).
//!
//! `${name}` is replaced in every string value of `engine`, `actors` and `steps`. A string
//! that is exactly one placeholder with a numeric value becomes a JSON number; anything
//! else is textual interpolation. An unknown placeholder is an error naming its location.

use std::collections::HashMap;

use serde_json::Value;

use crate::scenario::Scenario;

/// A placeholder's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceholderValue {
    /// Substituted as a number when the placeholder is the whole string.
    Number(u64),
    /// Text.
    Text(String),
}

impl PlaceholderValue {
    fn text(&self) -> String {
        match self {
            Self::Number(n) => n.to_string(),
            Self::Text(s) => s.clone(),
        }
    }
}

/// The values of one run, by placeholder name (`port.a`, `folder.out`, `state_dir`).
#[derive(Debug, Clone, Default)]
pub struct Placeholders {
    values: HashMap<String, PlaceholderValue>,
}

impl Placeholders {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a numeric value.
    pub fn set_number(&mut self, name: impl Into<String>, value: u64) {
        self.values
            .insert(name.into(), PlaceholderValue::Number(value));
    }

    /// Adds a text value.
    pub fn set_text(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.values
            .insert(name.into(), PlaceholderValue::Text(value.into()));
    }

    /// Placeholders with dummy values for every name the scenario declares, so a file can
    /// be checked without allocating anything.
    #[must_use]
    pub fn dry(scenario: &Scenario) -> Self {
        let mut p = Self::new();
        for name in &scenario.resources.ports {
            p.set_number(format!("port.{name}"), 1);
        }
        for name in &scenario.resources.serial_pairs {
            p.set_text(
                format!("serial.{name}.engine"),
                format!("/dry/serial/{name}.engine"),
            );
            p.set_text(
                format!("serial.{name}.peer"),
                format!("/dry/serial/{name}.peer"),
            );
        }
        for name in &scenario.resources.folders {
            p.set_text(format!("folder.{name}"), format!("/dry/folder/{name}"));
        }
        for (name, actor) in &scenario.actors {
            if actor.get("kind").and_then(Value::as_str) == Some("tcp_server") {
                p.set_number(format!("actor.{name}.port"), 1);
                p.set_text(format!("actor.{name}.host"), "127.0.0.1");
            }
        }
        p.set_text("state_dir", "/dry/state");
        p.set_text("scenario.name", scenario.name.clone());
        p
    }

    /// Substitutes inside `value`; `location` names it in errors.
    pub fn substitute(&self, value: &Value, location: &str) -> Result<Value, String> {
        match value {
            Value::String(s) => self.substitute_str(s, location),
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| self.substitute(v, &format!("{location}[{i}]")))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| {
                    self.substitute(v, &format!("{location}.{k}"))
                        .map(|v| (k.clone(), v))
                })
                .collect::<Result<_, _>>()
                .map(Value::Object),
            other => Ok(other.clone()),
        }
    }

    fn substitute_str(&self, text: &str, location: &str) -> Result<Value, String> {
        if !text.contains("${") {
            return Ok(Value::String(text.to_owned()));
        }
        let lookup = |name: &str| {
            self.values
                .get(name)
                .ok_or_else(|| format!("{location}: unknown placeholder `${{{name}}}`"))
        };
        // Whole-string placeholder: keep the number.
        if let Some(inner) = text.strip_prefix("${").and_then(|r| r.strip_suffix('}'))
            && !inner.contains("${")
            && !inner.contains('}')
        {
            return Ok(match lookup(inner)? {
                PlaceholderValue::Number(n) => Value::from(*n),
                PlaceholderValue::Text(t) => Value::String(t.clone()),
            });
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                return Err(format!("{location}: unterminated placeholder in `{text}`"));
            };
            out.push_str(&lookup(&after[..end])?.text());
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(Value::String(out))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sample() -> Placeholders {
        let mut p = Placeholders::new();
        p.set_number("port.a", 4242);
        p.set_text("state_dir", "/tmp/x");
        p
    }

    #[test]
    fn whole_string_numbers_become_json_numbers() {
        let v = sample()
            .substitute(&json!({"port": "${port.a}"}), "t")
            .unwrap();
        assert_eq!(v, json!({"port": 4242}));
    }

    #[test]
    fn interpolation_stays_text() {
        let v = sample()
            .substitute(&json!(["127.0.0.1:${port.a}", "${state_dir}/x"]), "t")
            .unwrap();
        assert_eq!(v, json!(["127.0.0.1:4242", "/tmp/x/x"]));
    }

    #[test]
    fn unknown_placeholders_name_their_location() {
        let err = sample()
            .substitute(&json!({"a": [{"b": "${port.zzz}"}]}), "steps[3]")
            .unwrap_err();
        assert!(err.contains("steps[3].a[0].b"), "{err}");
        assert!(err.contains("port.zzz"), "{err}");
    }
}
