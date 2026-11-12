//! The `calculate` rule (configuration.md §8.2): an exact formula over message fields and
//! constants, rounded once to the rule's scale.
//!
//! ```json
//! { "op": "calculate", "formula": { "mul": [ { "from": "$.4" }, "0.0555" ] },
//!   "to": "$.5", "scale": 2 }
//! ```
//!
//! A formula is a constant decimal, an operand (`{"from": P}`, or `{"find": R, "where": E,
//! "from": "$.n"}` for the first occurrence of `R` where `E` holds), or one operator
//! (`add`, `sub`, `mul`, `div`, `min`, `max` over 2–16 formulas, `abs` of one). Operand
//! text may carry a comparator (`<5`): with `comparator: "carry"` (the default) the result
//! keeps it, reversed when the direction reverses, dropped by an exact zero factor, and its
//! number is rounded so the bound stays true (up for `<`, down for `>`). With
//! `notation: "sn"` operands and result are HL7 structured numeric fields (`<^5`). An empty
//! operand gives an empty result (nothing written); a text that is not a number, a zero
//! divisor or a value out of range goes to `on_invalid`. `find` in a message with more than
//! one patient, order or specimen group is a transform error: its first match could belong
//! to another group than the result.

use std::borrow::Cow;

use serde_json::Value;

use crate::decimal::Decimal;
use crate::error::Reason;
use crate::exact::{CalcError, Exact, Rounding};
use crate::filter::{Cx, Expr, compile_expr};
use crate::json::{Obj, scalar_text};
use crate::mapping::{Compiler, Ctx};
use crate::model::{Model, read, single_group};
use crate::path::{Frame, Path, Scope, Syntax};
use crate::rules::Source;
use crate::steps::TooLong;

/// Deepest operator nesting.
const MAX_DEPTH: usize = 8;
/// Most constants, operands and operators in one formula.
const MAX_NODES: usize = 64;
/// Most formulas in one operator list.
const MAX_ITEMS: usize = 16;
/// Largest integer a JSON number carries exactly everywhere (2^53 − 1).
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Largest `scale`.
const MAX_SCALE: u64 = 12;

const FORMULA_FORMS: &str = "a formula is a decimal, {\"from\": …}, {\"find\": …} or one of \
add, sub, mul, div, min, max, abs";

/// A compiled `calculate` rule, without its `to`.
#[derive(Debug, Clone)]
pub(crate) struct Calculate {
    pub(crate) formula: Formula,
    pub(crate) scale: Option<u32>,
    pub(crate) rounding: Rounding,
    pub(crate) reading: Reading,
    pub(crate) on_invalid: OnInvalid,
    pub(crate) required: bool,
}

/// How operand text is read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Reading {
    /// `comparator: "carry"`.
    pub(crate) carry: bool,
    /// `notation: "sn"`: operands and result are HL7 structured numeric fields.
    pub(crate) sn: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum OnInvalid {
    Error,
    Skip,
    /// Write the formula's one operand as read.
    Keep,
    Default(String),
}

#[derive(Debug, Clone)]
pub(crate) enum Formula {
    Constant(Exact),
    From(Source),
    Find(Box<Find>),
    List(Op, Vec<Formula>),
    Abs(Box<Formula>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
}

/// `{"find": record, "where": condition, "from": $.path}`.
#[derive(Debug, Clone)]
pub(crate) struct Find {
    record: String,
    condition: Expr,
    from: Source,
}

/// A comparator carried by a value: the true value is below or above the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    Lt,
    Le,
    Gt,
    Ge,
}

impl Bound {
    const ALL: [(&'static str, Self); 4] = [
        ("<=", Self::Le),
        (">=", Self::Ge),
        ("<", Self::Lt),
        (">", Self::Gt),
    ];

    const fn flip(self) -> Self {
        match self {
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Gt => Self::Lt,
            Self::Ge => Self::Le,
        }
    }

    const fn text(self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }
}

/// An intermediate value with the comparator it carries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Num {
    value: Exact,
    bound: Option<Bound>,
}

/// Why a formula has no value.
#[derive(Debug)]
pub(crate) enum Stop<'f> {
    /// An operand is empty or missing, or a `find` matched nothing.
    Empty(&'f Path),
    /// Goes to `on_invalid`; the path is the operand's, or `None` for the rule's `to`.
    Invalid(Reason, Option<&'f Path>),
    /// A transform error whatever `on_invalid` says.
    Fatal(Reason, &'f Path),
}

impl Num {
    /// The comparator (empty when none) and the number rounded to `scale`. A carried
    /// comparator's number is rounded away from the claim, so the bound is never stronger
    /// than the computed one.
    pub(crate) fn parts(
        self,
        scale: Option<u32>,
        rounding: Rounding,
    ) -> Result<(&'static str, String), Reason> {
        let rounding = match self.bound {
            None => rounding,
            Some(Bound::Lt | Bound::Le) => Rounding::Ceiling,
            Some(Bound::Gt | Bound::Ge) => Rounding::Floor,
        };
        let number = self.value.to_text(scale, rounding).map_err(reason)?;
        Ok((self.bound.map_or("", Bound::text), number))
    }
}

const fn reason(error: CalcError) -> Reason {
    match error {
        CalcError::OutOfRange => Reason::OutOfRange,
        CalcError::DivisionByZero => Reason::DivisionByZero,
    }
}

fn invalid<'f>(error: CalcError) -> Stop<'f> {
    Stop::Invalid(reason(error), None)
}

impl Formula {
    /// Evaluates the formula over `model`; `input` resolves `$.` operand paths.
    pub(crate) fn eval(
        &self,
        model: &dyn Model,
        input: Option<Frame<'_>>,
        reading: Reading,
    ) -> Result<Num, Stop<'_>> {
        match self {
            Self::Constant(value) => Ok(Num {
                value: *value,
                bound: None,
            }),
            Self::From(source) => operand(model, source, input, reading),
            Self::Find(find) => {
                if !single_group(model) {
                    return Err(Stop::Fatal(Reason::MultipleGroups, &find.from.path));
                }
                match find.locate(model) {
                    Some(frame) => operand(model, &find.from, Some(frame), reading),
                    None => Err(Stop::Empty(&find.from.path)),
                }
            }
            Self::Abs(inner) => {
                let num = inner.eval(model, input, reading)?;
                if num.bound.is_some() {
                    return Err(Stop::Invalid(Reason::Comparator, None));
                }
                Ok(Num {
                    value: num.value.abs().map_err(invalid)?,
                    bound: None,
                })
            }
            Self::List(op, items) => {
                let mut values = items.iter().map(|item| item.eval(model, input, reading));
                let first = values.next().unwrap_or(Ok(Num {
                    value: Exact::ZERO,
                    bound: None,
                }))?;
                values.try_fold(first, |acc, next| combine(*op, acc, next?))
            }
        }
    }

    /// The text of the formula's single operand (for `on_invalid: "keep"`): after its
    /// steps, or with `sn` the whole field unchanged. `None` without an operand or a match.
    ///
    /// # Errors
    /// [`TooLong`] from the operand's steps (the caller never asks after that failure).
    pub(crate) fn operand_text<'m>(
        &self,
        model: &'m dyn Model,
        input: Option<Frame<'_>>,
        sn: bool,
    ) -> Result<Option<Cow<'m, str>>, TooLong> {
        let text = |source: &Source, frame: Option<Frame<'_>>| {
            if sn {
                Ok(read(model, &source.path, frame).unwrap_or(Cow::Borrowed("")))
            } else {
                source_text(model, source, frame)
            }
        };
        match self {
            Self::Constant(_) => Ok(None),
            Self::From(source) => text(source, input).map(Some),
            Self::Find(find) => find
                .locate(model)
                .map(|frame| text(&find.from, Some(frame)))
                .transpose(),
            Self::Abs(inner) => inner.operand_text(model, input, sn),
            Self::List(_, items) => items
                .iter()
                .find_map(|item| item.operand_text(model, input, sn).transpose())
                .transpose(),
        }
    }
}

impl Find {
    /// The first occurrence of the record where the condition holds.
    fn locate<'f>(&'f self, model: &dyn Model) -> Option<Frame<'f>> {
        (0..model.count(&self.record))
            .map(|occurrence| Frame {
                record: &self.record,
                occurrence,
            })
            .find(|frame| self.condition.eval(model, Some(*frame)))
    }
}

/// The text at a source, after its steps; missing is empty.
///
/// # Errors
/// [`TooLong`] when a step would exceed its size bound.
pub(crate) fn source_text<'m>(
    model: &'m dyn Model,
    source: &Source,
    frame: Option<Frame<'_>>,
) -> Result<Cow<'m, str>, TooLong> {
    source
        .steps
        .apply(read(model, &source.path, frame).unwrap_or(Cow::Borrowed("")))
}

/// Reads one operand: text with an optional leading comparator, or an HL7 structured
/// numeric field (comparator in component 1, number in component 2; steps apply to the
/// number).
fn operand<'f>(
    model: &dyn Model,
    source: &'f Source,
    frame: Option<Frame<'_>>,
    reading: Reading,
) -> Result<Num, Stop<'f>> {
    let path = &source.path;
    let not_numeric = Stop::Invalid(Reason::NotNumeric, Some(path));
    let too_long = |_| Stop::Invalid(Reason::StepsTooLong, Some(path));
    if !reading.sn {
        let text = source_text(model, source, frame).map_err(too_long)?;
        let text = text.trim();
        if text.is_empty() {
            return Err(Stop::Empty(path));
        }
        let (bound, digits) = Bound::ALL
            .iter()
            .filter(|_| reading.carry)
            .find_map(|(prefix, bound)| text.strip_prefix(prefix).map(|rest| (Some(*bound), rest)))
            .unwrap_or((None, text));
        return Ok(Num {
            value: exact(digits, path)?,
            bound,
        });
    }
    let component =
        |n: usize| read(model, &path.component_path(n), frame).unwrap_or(Cow::Borrowed(""));
    if !component(3).is_empty() || !component(4).is_empty() {
        return Err(not_numeric);
    }
    let comparator = component(1);
    let bound = match comparator.trim() {
        "" | "=" => None,
        text => match Bound::ALL.iter().find(|(prefix, _)| *prefix == text) {
            Some((_, bound)) if reading.carry => Some(*bound),
            _ => return Err(not_numeric),
        },
    };
    let number = source.steps.apply(component(2)).map_err(too_long)?;
    let number = number.trim();
    match (number.is_empty(), bound) {
        (true, None) => Err(Stop::Empty(path)),
        (true, Some(_)) => Err(not_numeric),
        (false, _) => Ok(Num {
            value: exact(number, path)?,
            bound,
        }),
    }
}

/// A plain decimal operand.
fn exact<'f>(digits: &str, path: &'f Path) -> Result<Exact, Stop<'f>> {
    let decimal = Decimal::parse(digits).ok_or(Stop::Invalid(Reason::NotNumeric, Some(path)))?;
    Exact::from_decimal(&decimal).map_err(|_| Stop::Invalid(Reason::OutOfRange, Some(path)))
}

/// `acc op next`, keeping a single comparator where the operation allows it.
fn combine<'f>(op: Op, acc: Num, next: Num) -> Result<Num, Stop<'f>> {
    let comparator = Stop::Invalid(Reason::Comparator, None);
    if acc.bound.is_some() && next.bound.is_some() {
        return Err(comparator);
    }
    let (a, b) = (acc.value, next.value);
    // For `mul` and `div`: the comparator of the bounded side, reversed when the other
    // factor is negative and dropped when it is zero (`<5 × 0` is exactly 0).
    let scaled = match (acc.bound, next.bound) {
        (Some(bound), None) if !b.is_zero() => {
            Some(if b.is_negative() { bound.flip() } else { bound })
        }
        (None, Some(bound)) if !a.is_zero() => {
            Some(if a.is_negative() { bound.flip() } else { bound })
        }
        _ => None,
    };
    let (value, bound) = match op {
        Op::Add => (a.add(b), acc.bound.or(next.bound)),
        Op::Sub => (a.sub(b), acc.bound.or(next.bound.map(Bound::flip))),
        Op::Mul => (a.mul(b), scaled),
        Op::Div if next.bound.is_some() => return Err(comparator),
        Op::Div => (a.div(b), scaled),
        Op::Min | Op::Max if acc.bound.is_some() || next.bound.is_some() => return Err(comparator),
        Op::Min | Op::Max => {
            let less = a.compare(b).map_err(invalid)?.is_lt();
            let value = if less == (op == Op::Min) { a } else { b };
            (Ok(value), None)
        }
    };
    Ok(Num {
        value: value.map_err(invalid)?,
        bound,
    })
}

/// The shape of a formula being compiled, for the limits and for `on_invalid: "keep"`.
#[derive(Default)]
struct Shape {
    nodes: usize,
    operands: usize,
    divides: bool,
    /// `notation: "sn"`: operand paths must stop at a field or repetition.
    sn: bool,
}

impl Compiler<'_> {
    /// With `notation: "sn"`, a path must address a field or a repetition. `true` when
    /// the path is usable.
    pub(crate) fn check_sn(&mut self, path: &Path, at: &str, sn: bool) -> bool {
        let usable = !sn || path.component().is_none();
        if !usable {
            self.problems.add(
                at,
                "with notation `sn` the path addresses a field or a repetition, not a component",
            );
        }
        usable
    }

    /// Compiles a `calculate` rule's fields other than `to`.
    pub(crate) fn calculate(&mut self, object: &Obj<'_>, ctx: Ctx) -> Option<Calculate> {
        let notation = self.choice(object, "notation", &["text", "sn"]);
        let sn = notation == Some("sn");
        if sn && (self.source.syntax() != Syntax::Hl7 || self.target.syntax() != Syntax::Hl7) {
            self.problems.add(
                object.at("notation"),
                "`sn` needs an HL7 source and an HL7 target",
            );
        }
        let mut shape = Shape {
            sn,
            ..Shape::default()
        };
        let formula = match object.value("formula") {
            Some(value) => self.formula(value, &object.at("formula"), ctx, 1, &mut shape),
            None => {
                self.problems.add(object.at("formula"), "is required");
                None
            }
        };
        let formula = formula.filter(|_| shape.nodes <= MAX_NODES);
        let scale = self.scale(object, shape.divides);
        let rounding = self.rounding(object, scale);
        let comparator = self.choice(object, "comparator", &["carry", "reject"]);
        let on_invalid = self.on_invalid(object, shape.operands);
        let required = object.boolean("required", false, &mut self.problems);
        Some(Calculate {
            formula: formula?,
            scale: scale?,
            rounding: rounding?,
            reading: Reading {
                carry: comparator? == "carry",
                sn,
            },
            on_invalid: on_invalid?,
            required,
        })
    }

    /// `Some(None)` without a scale, `None` after a problem.
    fn scale(&mut self, object: &Obj<'_>, divides: bool) -> Option<Option<u32>> {
        let Some(value) = object.value("scale") else {
            if divides {
                self.problems
                    .add(object.at("scale"), "is required when the formula divides");
                return None;
            }
            return Some(None);
        };
        let scale = value
            .as_u64()
            .filter(|n| *n <= MAX_SCALE)
            .and_then(|n| u32::try_from(n).ok());
        if scale.is_none() {
            self.problems.add(
                object.at("scale"),
                format!("must be an integer from 0 to {MAX_SCALE}"),
            );
        }
        scale.map(Some)
    }

    fn rounding(&mut self, object: &Obj<'_>, scale: Option<Option<u32>>) -> Option<Rounding> {
        if object.has("rounding") && scale == Some(None) {
            self.problems
                .add(object.at("rounding"), "is only used with `scale`");
            return None;
        }
        Some(
            match self.choice(object, "rounding", &["half_up", "half_even", "truncate"])? {
                "half_even" => Rounding::HalfEven,
                "truncate" => Rounding::Truncate,
                _ => Rounding::HalfUp,
            },
        )
    }

    /// An optional string field that must be one of `allowed`; the first is the default.
    fn choice(
        &mut self,
        object: &Obj<'_>,
        key: &str,
        allowed: &[&'static str],
    ) -> Option<&'static str> {
        if !object.has(key) {
            return allowed.first().copied();
        }
        let text = object.string(key, &mut self.problems)?;
        let found = allowed.iter().copied().find(|a| *a == text);
        if found.is_none() {
            self.problems.add(
                object.at(key),
                format!("must be one of {}", allowed.join(", ")),
            );
        }
        found
    }

    fn on_invalid(&mut self, object: &Obj<'_>, operands: usize) -> Option<OnInvalid> {
        let mode = self.choice(object, "on_invalid", &["error", "skip", "keep", "default"]);
        let default = match object.value("default") {
            Some(Value::Number(_)) => object
                .value("default")
                .and_then(|value| self.exact_text(value, &object.at("default"))),
            _ => object.scalar("default", &mut self.problems),
        };
        if object.has("default") && default.is_none() {
            // Its problem is already reported.
            return None;
        }
        match (mode?, default) {
            ("default", Some(text)) => Some(OnInvalid::Default(text)),
            ("default", None) => {
                self.problems.add(
                    object.at("default"),
                    "is required with on_invalid `default`",
                );
                None
            }
            (_, Some(_)) => {
                self.problems.add(
                    object.at("default"),
                    "is only used with on_invalid `default`",
                );
                None
            }
            ("keep", None) if operands != 1 => {
                self.problems.add(
                    object.at("on_invalid"),
                    "`keep` needs a formula with exactly one `from` or `find` operand",
                );
                None
            }
            ("keep", None) => Some(OnInvalid::Keep),
            ("skip", None) => Some(OnInvalid::Skip),
            _ => Some(OnInvalid::Error),
        }
    }

    fn formula(
        &mut self,
        value: &Value,
        at: &str,
        ctx: Ctx,
        depth: usize,
        shape: &mut Shape,
    ) -> Option<Formula> {
        if depth > MAX_DEPTH {
            self.problems.add(
                at,
                format!("formulas are nested too deeply (at most {MAX_DEPTH} levels)"),
            );
            return None;
        }
        shape.nodes += 1;
        if shape.nodes == MAX_NODES + 1 {
            self.problems
                .add(at, format!("a formula has at most {MAX_NODES} parts"));
        }
        match value {
            Value::String(_) | Value::Number(_) => self.constant_decimal(value, at),
            Value::Object(map) if map.contains_key("find") => {
                shape.operands += 1;
                self.find(value, at, shape.sn)
            }
            Value::Object(map) if map.contains_key("from") => {
                shape.operands += 1;
                let object = Obj::new(value, at, &["from", "steps"], &mut self.problems)?;
                let source = self.source(&object, ctx)?;
                self.check_sn(&source.path, &object.at("from"), shape.sn)
                    .then_some(Formula::From(source))
            }
            Value::Object(map) if map.len() == 1 => {
                let (key, inner) = map.iter().next()?;
                self.operator((key, inner), &format!("{at}.{key}"), ctx, depth, shape)
            }
            _ => {
                self.problems.add(at, FORMULA_FORMS);
                None
            }
        }
    }

    fn constant_decimal(&mut self, value: &Value, at: &str) -> Option<Formula> {
        let decimal = self.exact_text(value, at)?;
        let decimal = Decimal::parse(&decimal);
        let Some(decimal) = decimal else {
            self.problems.add(at, "must be a decimal number");
            return None;
        };
        match Exact::from_decimal(&decimal) {
            Ok(exact) => Some(Formula::Constant(exact)),
            Err(_) => {
                self.problems.add(at, "the constant has too many digits");
                None
            }
        }
    }

    /// The text of a string, or of a JSON number that arrived exactly: an integer within
    /// ±(2^53 − 1). Any other number may have been rounded by JSON parsing (as binary
    /// floating point, as in a browser) before the engine saw it, so it must be a string.
    fn exact_text(&mut self, value: &Value, at: &str) -> Option<String> {
        if let Value::Number(number) = value {
            let safe = number
                .as_i64()
                .is_some_and(|n| n.unsigned_abs() <= MAX_SAFE_INTEGER);
            if !safe {
                self.problems.add(
                    at,
                    "write a fractional or large constant as a decimal string, for example \"0.0555\"",
                );
                return None;
            }
        }
        scalar_text(value)
    }

    fn operator(
        &mut self,
        (key, inner): (&str, &Value),
        at: &str,
        ctx: Ctx,
        depth: usize,
        shape: &mut Shape,
    ) -> Option<Formula> {
        let op = match key {
            "abs" => {
                let inner = self.formula(inner, at, ctx, depth + 1, shape)?;
                return Some(Formula::Abs(Box::new(inner)));
            }
            "add" => Op::Add,
            "sub" => Op::Sub,
            "mul" => Op::Mul,
            "div" => Op::Div,
            "min" => Op::Min,
            "max" => Op::Max,
            _ => {
                self.problems
                    .add(at, format!("unknown operator; {FORMULA_FORMS}"));
                return None;
            }
        };
        shape.divides |= op == Op::Div;
        let list = match inner.as_array() {
            Some(list) if (2..=MAX_ITEMS).contains(&list.len()) => list,
            _ => {
                self.problems
                    .add(at, format!("must be an array of 2 to {MAX_ITEMS} formulas"));
                return None;
            }
        };
        let items: Vec<Option<Formula>> = list
            .iter()
            .enumerate()
            .map(|(i, item)| self.formula(item, &format!("{at}[{i}]"), ctx, depth + 1, shape))
            .collect();
        items
            .into_iter()
            .collect::<Option<_>>()
            .map(|items| Formula::List(op, items))
    }

    /// `{"find": R, "where": E, "from": "$.n", "steps": […]}`; `$.` addresses the candidate.
    fn find(&mut self, value: &Value, at: &str, sn: bool) -> Option<Formula> {
        let object = Obj::new(
            value,
            at,
            &["find", "where", "from", "steps"],
            &mut self.problems,
        )?;
        let record = object
            .required_string("find", &mut self.problems)
            .and_then(|name| match self.source.check_record(name, false) {
                Ok(()) => Some(name.to_owned()),
                Err(message) => {
                    self.problems.add(object.at("find"), message);
                    None
                }
            });
        let condition = match object.value("where") {
            Some(value) => {
                let mut cx = Cx::new(self.source, true, &mut self.problems);
                compile_expr(value, &mut cx, &object.at("where"), 0)
            }
            None => {
                self.problems.add(object.at("where"), "is required");
                None
            }
        };
        let from = self.source(&object, Ctx::Loop);
        let from = from.filter(|source| {
            let current = source.path.scope() == Scope::Current;
            if !current {
                self.problems.add(
                    object.at("from"),
                    "inside find, `from` is a `$.` path of the found occurrence",
                );
            }
            current && self.check_sn(&source.path, &object.at("from"), sn)
        });
        Some(Formula::Find(Box::new(Find {
            record: record?,
            condition: condition?,
            from: from?,
        })))
    }
}
