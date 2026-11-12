//! Shell-style name patterns for `run-all --filter`: `*` matches any run of characters
//! (including none), `?` exactly one; everything else matches itself. The whole name must
//! match, case-sensitively.

use regex::Regex;

/// A compiled pattern.
#[derive(Debug, Clone)]
pub struct Glob {
    pattern: String,
    regex: Regex,
}

impl Glob {
    /// Compiles `pattern`.
    pub fn new(pattern: &str) -> Result<Self, String> {
        let mut expr = String::from("^");
        for c in pattern.chars() {
            match c {
                '*' => expr.push_str(".*"),
                '?' => expr.push('.'),
                c => expr.push_str(&regex::escape(&c.to_string())),
            }
        }
        expr.push('$');
        let regex = Regex::new(&expr).map_err(|e| format!("invalid filter `{pattern}`: {e}"))?;
        Ok(Self {
            pattern: pattern.to_owned(),
            regex,
        })
    }

    /// Whether `name` matches the whole pattern.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        self.regex.is_match(name)
    }

    /// The pattern as written.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(p: &str) -> Glob {
        Glob::new(p).unwrap()
    }

    #[test]
    fn stars_and_question_marks_match_whole_names() {
        assert!(glob("bridge-*").matches("bridge-tap-hl7"));
        assert!(glob("*-hl7").matches("bridge-tap-hl7"));
        assert!(glob("*").matches(""));
        assert!(glob("msg-?uplicate").matches("msg-duplicate"));
        assert!(!glob("bridge").matches("bridge-tap-hl7"), "whole name only");
        assert!(!glob("Bridge-*").matches("bridge-x"), "case-sensitive");
    }

    #[test]
    fn regex_characters_are_literal() {
        assert!(glob("a.b").matches("a.b"));
        assert!(!glob("a.b").matches("axb"));
        assert!(glob("a+(b)|c[").matches("a+(b)|c["));
        assert_eq!(glob("x*").pattern(), "x*");
    }
}
