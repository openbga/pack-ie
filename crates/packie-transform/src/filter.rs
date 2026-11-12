//! Filter expressions (configuration.md §8.1): a boolean over message fields.
//!
//! ```json
//! { "all": [
//!     { "path": "MSH.9.1", "op": "equals", "value": "ORU" },
//!     { "not": { "path": "PID.3", "op": "not_exists" } },
//!     { "any": [ { "path": "OBX.3.1", "op": "in", "value": ["GLU", "K"] },
//!                { "path": "OBX.5", "op": "gt", "value": "100" } ] } ] }
//! ```
//!
//! | Operator | True when the field text … | `value` |
//! | --- | --- | --- |
//! | `equals`, `not_equals` | is / is not the value | text |
//! | `in`, `not_in` | is / is not one of the values | array of text |
//! | `starts_with`, `contains` | starts with / contains the value | text |
//! | `matches` | contains a match of the regular expression (anchor with `^` and `$`) | pattern, at most 200 characters, linear-time engine |
//! | `exists`, `not_exists` | is not empty / is empty | none |
//! | `gt`, `gte`, `lt`, `lte` | compares to the value as exact decimals | number |
//!
//! A missing field is empty text. It satisfies `not_exists`, `not_equals` and `not_in`;
//! text operators see the empty string (so `equals ""` is true for it); the numeric
//! operators are false because empty text is not a number, as is any other non-numeric
//! text (`<5`, `1e3`). `ignore_case` folds both sides of the text operators. A text
//! value may be written as a JSON string or a number. Empty `all`, `any`, `in` and
//! `not_in` lists are rejected: they would silently pass or drop every message.
//!
//! Evaluation never fails and never panics; compilation reports every problem with its
//! JSON path.

use std::borrow::Cow;

use regex::{Regex, RegexBuilder};
use serde_json::Value;

use crate::decimal::Decimal;
use crate::error::Problem;
use crate::json::{Obj, Problems, scalar_text};
use crate::model::{Model, Schema, read};
use crate::path::{Frame, Path};

/// Deepest nesting of `all`, `any` and `not`.
const MAX_DEPTH: usize = 32;
/// Longest regular expression, in characters.
const MAX_PATTERN_CHARS: usize = 200;
/// Compiled-size cap for a regular expression.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

const OPERATORS: &str = "equals, not_equals, in, not_in, starts_with, contains, matches, \
exists, not_exists, gt, gte, lt, lte";

/// One compiled filter expression.
#[derive(Debug, Clone)]
pub struct Filter {
    expr: Expr,
}

/// An ordered list of filters that must all hold (the channel `filters` or a
/// destination's `filters`).
#[derive(Debug, Clone, Default)]
pub struct Filters {
    filters: Vec<Filter>,
}

impl Filter {
    /// Compiles one expression; `at` is its JSON path, used in problems. Paths are checked
    /// against `schema`, the model of the message being filtered.
    ///
    /// # Errors
    /// Every problem found, each with its JSON path.
    pub fn compile(value: &Value, schema: &Schema, at: &str) -> Result<Self, Vec<Problem>> {
        let mut problems = Problems::default();
        let expr = compile_expr(value, &mut Cx::new(schema, false, &mut problems), at, 0);
        problems.finish(expr).map(|expr| Self { expr })
    }

    /// Whether the message satisfies the expression.
    #[must_use]
    pub fn matches(&self, model: &dyn Model) -> bool {
        self.expr.eval(model, None)
    }
}

impl Filters {
    /// Compiles a list; `at` is the JSON path of the list (`channels[0].filters`).
    /// Problems from every entry are returned together.
    ///
    /// # Errors
    /// Every problem found in every entry.
    pub fn compile(values: &[Value], schema: &Schema, at: &str) -> Result<Self, Vec<Problem>> {
        let mut problems = Vec::new();
        let mut filters = Vec::new();
        for (i, value) in values.iter().enumerate() {
            match Filter::compile(value, schema, &format!("{at}[{i}]")) {
                Ok(filter) => filters.push(filter),
                Err(found) => problems.extend(found),
            }
        }
        if problems.is_empty() {
            Ok(Self { filters })
        } else {
            Err(problems)
        }
    }

    /// Whether every filter holds, in order; an empty list passes.
    #[must_use]
    pub fn passes(&self, model: &dyn Model) -> bool {
        self.filters.iter().all(|filter| filter.matches(model))
    }

    /// Number of filters.
    #[must_use]
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Whether the list is empty (everything passes).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Expr {
    Compare(Comparison),
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Not(Box<Expr>),
}

#[derive(Debug, Clone)]
pub(crate) struct Comparison {
    path: Path,
    test: Test,
    ignore_case: bool,
}

#[derive(Debug, Clone)]
enum Test {
    Equals(String),
    NotEquals(String),
    In(Vec<String>),
    NotIn(Vec<String>),
    StartsWith(String),
    Contains(String),
    Matches(Regex),
    Exists,
    NotExists,
    Gt(Decimal),
    Gte(Decimal),
    Lt(Decimal),
    Lte(Decimal),
}

impl Expr {
    pub(crate) fn eval(&self, model: &dyn Model, frame: Option<Frame<'_>>) -> bool {
        match self {
            Self::Compare(comparison) => comparison.eval(model, frame),
            Self::All(items) => items.iter().all(|e| e.eval(model, frame)),
            Self::Any(items) => items.iter().any(|e| e.eval(model, frame)),
            Self::Not(inner) => !inner.eval(model, frame),
        }
    }
}

impl Comparison {
    fn eval(&self, model: &dyn Model, frame: Option<Frame<'_>>) -> bool {
        let text = read(model, &self.path, frame).unwrap_or(Cow::Borrowed(""));
        self.test.holds(&text, self.ignore_case)
    }
}

impl Test {
    fn holds(&self, text: &str, ignore_case: bool) -> bool {
        match self {
            Self::Exists => !text.is_empty(),
            Self::NotExists => text.is_empty(),
            Self::Gt(v) => Decimal::parse(text).is_some_and(|n| n > *v),
            Self::Gte(v) => Decimal::parse(text).is_some_and(|n| n >= *v),
            Self::Lt(v) => Decimal::parse(text).is_some_and(|n| n < *v),
            Self::Lte(v) => Decimal::parse(text).is_some_and(|n| n <= *v),
            Self::Matches(regex) => regex.is_match(text),
            text_test => {
                let folded;
                let text = if ignore_case {
                    folded = text.to_lowercase();
                    folded.as_str()
                } else {
                    text
                };
                text_test.holds_text(text)
            }
        }
    }

    /// The text operators; values were folded at compile time when `ignore_case` is set.
    fn holds_text(&self, text: &str) -> bool {
        match self {
            Self::Equals(v) => text == v,
            Self::NotEquals(v) => text != v,
            Self::In(values) => values.iter().any(|v| v == text),
            Self::NotIn(values) => values.iter().all(|v| v != text),
            Self::StartsWith(v) => text.starts_with(v.as_str()),
            Self::Contains(v) => text.contains(v.as_str()),
            Self::Matches(_)
            | Self::Exists
            | Self::NotExists
            | Self::Gt(_)
            | Self::Gte(_)
            | Self::Lt(_)
            | Self::Lte(_) => false,
        }
    }
}

pub(crate) struct Cx<'a> {
    schema: &'a Schema,
    /// Whether `$.` paths are allowed (inside `for_each`).
    in_loop: bool,
    problems: &'a mut Problems,
}

impl<'a> Cx<'a> {
    pub(crate) fn new(schema: &'a Schema, in_loop: bool, problems: &'a mut Problems) -> Self {
        Self {
            schema,
            in_loop,
            problems,
        }
    }
}

/// Compiles an expression. `None` means problems were recorded for it.
pub(crate) fn compile_expr(value: &Value, cx: &mut Cx<'_>, at: &str, depth: usize) -> Option<Expr> {
    if depth > MAX_DEPTH {
        cx.problems.add(at, "expressions are nested too deeply");
        return None;
    }
    let Some(map) = value.as_object() else {
        cx.problems.add(at, "an expression must be an object");
        return None;
    };
    for key in ["all", "any", "not"] {
        if map.contains_key(key) {
            return compile_combination(key, value, cx, at, depth);
        }
    }
    compile_comparison(value, cx, at)
}

fn compile_combination(
    key: &'static str,
    value: &Value,
    cx: &mut Cx<'_>,
    at: &str,
    depth: usize,
) -> Option<Expr> {
    let object = Obj::new(value, at, &[key], cx.problems)?;
    let inner = object.value(key)?;
    if key == "not" {
        let expr = compile_expr(inner, cx, &object.at(key), depth + 1)?;
        return Some(Expr::Not(Box::new(expr)));
    }
    let Some(list) = inner.as_array() else {
        cx.problems
            .add(object.at(key), "must be an array of expressions");
        return None;
    };
    if list.is_empty() {
        cx.problems
            .add(object.at(key), "needs at least one expression");
        return None;
    }
    let mut items = Vec::new();
    let mut ok = true;
    for (i, item) in list.iter().enumerate() {
        match compile_expr(item, cx, &format!("{}[{i}]", object.at(key)), depth + 1) {
            Some(expr) => items.push(expr),
            None => ok = false,
        }
    }
    ok.then(|| {
        if key == "all" {
            Expr::All(items)
        } else {
            Expr::Any(items)
        }
    })
}

fn compile_comparison(value: &Value, cx: &mut Cx<'_>, at: &str) -> Option<Expr> {
    let object = Obj::new(
        value,
        at,
        &["path", "op", "value", "ignore_case"],
        cx.problems,
    )?;
    let path = comparison_path(&object, cx);
    let op = object.required_string("op", cx.problems);
    let ignore_case = object.boolean("ignore_case", false, cx.problems);
    let test = compile_test(op?, &object, cx, ignore_case);
    Some(Expr::Compare(Comparison {
        path: path?,
        test: test?,
        ignore_case,
    }))
}

fn comparison_path(object: &Obj<'_>, cx: &mut Cx<'_>) -> Option<Path> {
    let text = object.required_string("path", cx.problems)?;
    match cx.schema.document_path(text, false, cx.in_loop) {
        Ok(path) => Some(path),
        Err(message) => {
            cx.problems.add(object.at("path"), message);
            None
        }
    }
}

fn compile_test(op: &str, object: &Obj<'_>, cx: &mut Cx<'_>, ignore_case: bool) -> Option<Test> {
    let fold = |text: String| {
        if ignore_case {
            text.to_lowercase()
        } else {
            text
        }
    };
    match op {
        "exists" | "not_exists" => {
            if object.has("value") {
                cx.problems
                    .add(object.at("value"), format!("`{op}` takes no value"));
            }
            Some(if op == "exists" {
                Test::Exists
            } else {
                Test::NotExists
            })
        }
        "equals" => text_value(object, cx).map(|v| Test::Equals(fold(v))),
        "not_equals" => text_value(object, cx).map(|v| Test::NotEquals(fold(v))),
        "starts_with" => text_value(object, cx).map(|v| Test::StartsWith(fold(v))),
        "contains" => text_value(object, cx).map(|v| Test::Contains(fold(v))),
        "in" => list_value(object, cx, &fold).map(Test::In),
        "not_in" => list_value(object, cx, &fold).map(Test::NotIn),
        "matches" => regex_value(object, cx, ignore_case).map(Test::Matches),
        "gt" => decimal_value(object, cx).map(Test::Gt),
        "gte" => decimal_value(object, cx).map(Test::Gte),
        "lt" => decimal_value(object, cx).map(Test::Lt),
        "lte" => decimal_value(object, cx).map(Test::Lte),
        _ => {
            cx.problems.add(
                object.at("op"),
                format!("unknown operator; use one of {OPERATORS}"),
            );
            None
        }
    }
}

fn text_value(object: &Obj<'_>, cx: &mut Cx<'_>) -> Option<String> {
    object.required_scalar("value", cx.problems)
}

fn list_value(
    object: &Obj<'_>,
    cx: &mut Cx<'_>,
    fold: &dyn Fn(String) -> String,
) -> Option<Vec<String>> {
    let at = object.at("value");
    let array = object.required_array("value", cx.problems)?;
    if array.is_empty() {
        cx.problems.add(at, "needs at least one value");
        return None;
    }
    let mut values = Vec::new();
    for (i, item) in array.iter().enumerate() {
        match scalar_text(item) {
            Some(text) => values.push(fold(text)),
            None => cx
                .problems
                .add(format!("{at}[{i}]"), "must be a string or a number"),
        }
    }
    (values.len() == array.len()).then_some(values)
}

fn regex_value(object: &Obj<'_>, cx: &mut Cx<'_>, ignore_case: bool) -> Option<Regex> {
    let at = object.at("value");
    let pattern = object.required_string("value", cx.problems)?;
    if pattern.chars().count() > MAX_PATTERN_CHARS {
        cx.problems.add(
            at,
            format!("a pattern is at most {MAX_PATTERN_CHARS} characters"),
        );
        return None;
    }
    match RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
    {
        Ok(regex) => Some(regex),
        Err(error) => {
            let detail = error.to_string();
            let detail = detail.lines().last().unwrap_or("").trim();
            let detail = detail.strip_prefix("error: ").unwrap_or(detail).to_owned();
            cx.problems
                .add(at, format!("not a usable regular expression: {detail}"));
            None
        }
    }
}

fn decimal_value(object: &Obj<'_>, cx: &mut Cx<'_>) -> Option<Decimal> {
    let text = object.required_scalar("value", cx.problems)?;
    let decimal = Decimal::parse(&text);
    if decimal.is_none() {
        cx.problems.add(
            object.at("value"),
            "must be a decimal number such as 5 or -0.25",
        );
    }
    decimal
}
