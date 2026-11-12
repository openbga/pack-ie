//! Mappings (configuration.md §8.2): compile the JSON document, run it over a source
//! model into a target model.
//!
//! ```json
//! { "tables": { "flags": { "H": "H", "L": "L", "N": "N" } },
//!   "rules": [
//!     { "op": "constant", "value": "ORU^R01", "to": "MSH.9" },
//!     { "op": "copy", "from": "P.3", "to": "PID.3", "required": true },
//!     { "op": "for_each", "over": "R", "into": "OBX", "rules": [
//!       { "op": "sequence", "to": "$.1" },
//!       { "op": "lookup", "from": "$.7", "table": "flags", "to": "$.8", "on_missing": "skip" } ] } ] }
//! ```
//!
//! Rules run top to bottom. An empty result writes nothing: `copy` of an empty source
//! with no `default`, `concat` of only empty parts, `datetime` of an empty source,
//! `lookup` with `skip` and `calculate` with an empty operand leave the target field unset
//! (`constant` and `sequence` always write). `calculate` is in [`crate::formula`], the
//! `steps` that may follow any `from` in [`crate::steps`]. `append` adds one occurrence
//! only when every rule in it writes, so a calculated result whose inputs are missing adds
//! nothing.
//!
//! Validation is strict: an unknown field in a rule is a problem, `default` is accepted
//! only with `on_missing: "default"` (and required there), and `for_each` and `append`
//! cannot be nested because a `$.` path would no longer say which occurrence it addresses.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::datetime::{DateFormat, Reformat, zone};
use crate::error::Problem;
use crate::filter::{Cx, compile_expr};
use crate::json::{Obj, Problems};
use crate::model::Schema;
use crate::path::{FieldRef, Path, Scope, Syntax};
use crate::require::{Require, compile_require};
use crate::rules::{
    Datetime, Kind, Lookup, LoopTarget, OnMissing, Output, Part, Rule, Source, When, Write,
};
use crate::steps::compile_steps;
use crate::tables::CodeTable;

/// Largest value of `sequence.start`.
const MAX_SEQUENCE_START: u64 = 999_999_999;

/// A compiled mapping: its code tables and ordered rules.
#[derive(Debug, Clone)]
pub struct Mapping {
    pub(crate) tables: BTreeMap<String, CodeTable>,
    pub(crate) rules: Vec<Rule>,
    /// `require_single_group`: refuse a source with several patient, order or specimen
    /// groups before any rule runs.
    pub(crate) single_group: bool,
    /// `require`: preconditions on the source, checked before any rule.
    pub(crate) require: Vec<Require>,
}

impl Mapping {
    /// Compiles a mapping document. `from` paths are checked against `source`, `to` paths
    /// against `target`; `at` is the JSON path of the document, used in problems.
    ///
    /// # Errors
    /// Every problem found, each with its JSON path.
    pub fn compile(
        value: &Value,
        source: &Schema,
        target: &Schema,
        at: &str,
    ) -> Result<Self, Vec<Problem>> {
        let mut compiler = Compiler {
            source,
            target,
            tables: BTreeMap::new(),
            problems: Problems::default(),
        };
        let mapping = compiler.mapping(value, at);
        compiler.problems.finish(mapping)
    }

    /// The code tables, by name.
    #[must_use]
    pub const fn tables(&self) -> &BTreeMap<String, CodeTable> {
        &self.tables
    }

    /// Number of top-level rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the mapping has no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Where a rule sits: the full JSON path for problems and the mapping-relative path for
/// runtime errors.
struct Loc {
    at: String,
    rel: String,
}

impl Loc {
    fn key(&self, key: &str) -> Self {
        Self {
            at: format!("{}.{key}", self.at),
            rel: if self.rel.is_empty() {
                key.to_owned()
            } else {
                format!("{}.{key}", self.rel)
            },
        }
    }

    fn idx(&self, index: usize) -> Self {
        Self {
            at: format!("{}[{index}]", self.at),
            rel: format!("{}[{index}]", self.rel),
        }
    }
}

pub(crate) struct Compiler<'a> {
    pub(crate) source: &'a Schema,
    pub(crate) target: &'a Schema,
    tables: BTreeMap<String, CodeTable>,
    pub(crate) problems: Problems,
}

/// Where a rule sits, which decides what a `$.` path addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ctx {
    /// Top level or inside `when`: no `$.` paths.
    Top,
    /// Inside `for_each`: `$.` is the input occurrence in `from`, the output in `to`.
    Loop,
    /// Inside `for_each` mode `repeat` of this field: a `$.` `to` addresses the field without
    /// a repetition.
    Repeat(usize),
    /// Inside `append`: every `to` is a `$.` path of the new occurrence; `from` is absolute.
    Append,
}

/// Whether a `$.` `to` path may be written in `ctx`: anywhere a `$.` path is valid, except
/// that inside mode `repeat` it must address the repeated field without a repetition.
fn repeat_target(path: &Path, ctx: Ctx) -> bool {
    match ctx {
        Ctx::Repeat(field) => {
            path.field() == &FieldRef::Number(field) && path.repetition().is_none()
        }
        _ => true,
    }
}

/// The `op` names and the fields each accepts.
fn op_fields(op: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match op {
        "copy" => (
            "copy",
            &["op", "from", "steps", "to", "default", "required"],
        ),
        "constant" => ("constant", &["op", "value", "to"]),
        "lookup" => (
            "lookup",
            &[
                "op",
                "from",
                "steps",
                "table",
                "to",
                "on_missing",
                "default",
                "ignore_case",
            ],
        ),
        "concat" => ("concat", &["op", "parts", "to"]),
        "datetime" => (
            "datetime",
            &[
                "op",
                "from",
                "steps",
                "to",
                "input",
                "output",
                "from_timezone",
                "to_timezone",
            ],
        ),
        "when" => ("when", &["op", "condition", "then", "else"]),
        "for_each" => ("for_each", &["op", "over", "into", "mode", "rules"]),
        "sequence" => ("sequence", &["op", "to", "start"]),
        "calculate" => (
            "calculate",
            &[
                "op",
                "formula",
                "to",
                "scale",
                "rounding",
                "comparator",
                "on_invalid",
                "default",
                "required",
                "notation",
            ],
        ),
        "append" => ("append", &["op", "into", "rules"]),
        _ => return None,
    })
}

impl<'a> Compiler<'a> {
    fn mapping(&mut self, value: &Value, at: &str) -> Option<Mapping> {
        let object = Obj::new(
            value,
            at,
            &["tables", "require_single_group", "require", "rules"],
            &mut self.problems,
        )?;
        let single_group = object.boolean("require_single_group", false, &mut self.problems);
        let require = object.value("require").map_or(Some(Vec::new()), |value| {
            compile_require(
                value,
                self.source,
                &object.at("require"),
                &mut self.problems,
            )
        });
        if let Some(tables) = object.value("tables") {
            self.compile_tables(tables, &object.at("tables"));
        }
        let rules = object.required_array("rules", &mut self.problems)?;
        let loc = Loc {
            at: at.to_owned(),
            rel: String::new(),
        }
        .key("rules");
        let rules = self.rules(rules, &loc, None, Ctx::Top);
        Some(Mapping {
            tables: std::mem::take(&mut self.tables),
            rules,
            single_group,
            require: require?,
        })
    }

    fn compile_tables(&mut self, value: &Value, at: &str) {
        let Some(tables) = value.as_object() else {
            self.problems.add(at, "must be an object of code tables");
            return;
        };
        for (name, entries) in tables {
            let table_at = format!("{at}.{name}");
            let Some(entries) = entries.as_object() else {
                self.problems
                    .add(table_at, "a code table is an object of code to text");
                continue;
            };
            let mut pairs = Vec::new();
            for (code, replacement) in entries {
                match replacement.as_str() {
                    Some(text) => pairs.push((code.clone(), text.to_owned())),
                    None => self
                        .problems
                        .add(format!("{table_at}.{code}"), "must be a string"),
                }
            }
            self.tables.insert(name.clone(), CodeTable::new(pairs));
        }
    }

    fn rules(&mut self, values: &[Value], loc: &Loc, top: Option<usize>, ctx: Ctx) -> Vec<Rule> {
        values
            .iter()
            .enumerate()
            .filter_map(|(i, value)| self.rule(value, &loc.idx(i), top.unwrap_or(i), ctx))
            .collect()
    }

    fn rule(&mut self, value: &Value, loc: &Loc, top: usize, ctx: Ctx) -> Option<Rule> {
        if !value.is_object() {
            self.problems.add(&loc.at, "a rule must be an object");
            return None;
        }
        let Some(op) = value.get("op").and_then(Value::as_str) else {
            self.problems
                .add(format!("{}.op", loc.at), "is required and must be a string");
            return None;
        };
        let Some((op, allowed)) = op_fields(op) else {
            self.problems.add(
                format!("{}.op", loc.at),
                "unknown op; use copy, constant, lookup, concat, datetime, calculate, when, \
                 for_each, append or sequence",
            );
            return None;
        };
        let object = Obj::new(value, &loc.at, allowed, &mut self.problems)?;
        let kind = match op {
            "when" => self.when(&object, loc, top, ctx),
            "for_each" => self.for_each(&object, loc, top, ctx),
            "append" => self.append(&object, loc, top, ctx),
            _ => self.write_rule(op, &object, ctx),
        }?;
        Some(Rule {
            op,
            location: loc.rel.clone(),
            top,
            kind,
        })
    }

    /// The schema `from` paths (`write` false) or `to` paths (`write` true) belong to.
    const fn schema(&self, write: bool) -> &'a Schema {
        if write { self.target } else { self.source }
    }

    /// A `from` (source) or `to` (target) path field.
    fn path(&mut self, object: &Obj<'_>, key: &str, write: bool, ctx: Ctx) -> Option<Path> {
        let text = object.required_string(key, &mut self.problems)?;
        let in_loop = matches!(ctx, Ctx::Loop | Ctx::Repeat(_)) || (write && ctx == Ctx::Append);
        match self.schema(write).document_path(text, write, in_loop) {
            Ok(path) if write && path.scope() == Scope::Current && !repeat_target(&path, ctx) => {
                let Ctx::Repeat(field) = ctx else { return None };
                self.problems.add(
                    object.at(key),
                    format!(
                        "inside mode `repeat`, a `$.` to is a path of field {field} without a \
                         repetition, for example $.{field}.4"
                    ),
                );
                None
            }
            Ok(path) if write && ctx == Ctx::Append && path.scope() != Scope::Current => {
                self.problems.add(
                    object.at(key),
                    "inside append, `to` is a `$.` path of the new occurrence",
                );
                None
            }
            Ok(path) => Some(path),
            Err(message) => {
                self.problems.add(object.at(key), message);
                None
            }
        }
    }

    /// The `from` path of `object` and its optional `steps`.
    pub(crate) fn source(&mut self, object: &Obj<'_>, ctx: Ctx) -> Option<Source> {
        let path = self.path(object, "from", false, ctx);
        let steps = compile_steps(object, &mut self.problems);
        Some(Source {
            path: path?,
            steps: steps?,
        })
    }

    /// A rule that writes one text to `to`.
    fn write_rule(&mut self, op: &str, object: &Obj<'_>, ctx: Ctx) -> Option<Kind> {
        let output = match op {
            "copy" => self.copy(object, ctx),
            "constant" => object
                .required_scalar("value", &mut self.problems)
                .map(Output::Constant),
            "lookup" => self.lookup(object, ctx),
            "concat" => self.concat(object, ctx),
            "datetime" => self.datetime(object, ctx),
            "calculate" => self
                .calculate(object, ctx)
                .map(|calculate| Output::Calculate(Box::new(calculate))),
            _ => self.sequence(object, ctx),
        };
        let to = self.path(object, "to", true, ctx);
        let sn =
            op == "calculate" && object.value("notation").and_then(Value::as_str) == Some("sn");
        let to = to.filter(|to| self.check_sn(to, &object.at("to"), sn));
        Some(Kind::Write(Box::new(Write {
            to: to?,
            output: output?,
        })))
    }

    fn copy(&mut self, object: &Obj<'_>, ctx: Ctx) -> Option<Output> {
        let from = self.source(object, ctx);
        let default = object.scalar("default", &mut self.problems);
        let required = object.boolean("required", false, &mut self.problems);
        Some(Output::Copy {
            from: from?,
            default,
            required,
        })
    }

    fn lookup(&mut self, object: &Obj<'_>, ctx: Ctx) -> Option<Output> {
        let from = self.source(object, ctx);
        let ignore_case = object.boolean("ignore_case", false, &mut self.problems);
        let table = self.table_name(object, ignore_case);
        let on_missing = self.on_missing(object);
        Some(Output::Lookup(Box::new(Lookup {
            from: from?,
            table: table?,
            on_missing: on_missing?,
            ignore_case,
        })))
    }

    fn table_name(&mut self, object: &Obj<'_>, ignore_case: bool) -> Option<String> {
        let name = object.required_string("table", &mut self.problems)?;
        let at = object.at("table");
        let Some(table) = self.tables.get(name) else {
            let known: Vec<&str> = self.tables.keys().map(String::as_str).collect();
            self.problems.add(
                at,
                format!(
                    "unknown table `{name}`; defined tables: [{}]",
                    known.join(", ")
                ),
            );
            return None;
        };
        if ignore_case && table.is_case_ambiguous() {
            self.problems.add(
                at,
                format!("table `{name}` has codes that differ only by case; they cannot be matched ignoring case"),
            );
            return None;
        }
        Some(name.to_owned())
    }

    fn on_missing(&mut self, object: &Obj<'_>) -> Option<OnMissing> {
        let mode = object
            .string("on_missing", &mut self.problems)
            .unwrap_or("error");
        let default = object.scalar("default", &mut self.problems);
        let at = object.at("default");
        match (mode, default) {
            ("default", Some(text)) => Some(OnMissing::Default(text)),
            ("default", None) => {
                self.problems
                    .add(at, "is required with on_missing `default`");
                None
            }
            (_, Some(_)) => {
                self.problems
                    .add(at, "is only used with on_missing `default`");
                None
            }
            ("error", None) => Some(OnMissing::Error),
            ("keep", None) => Some(OnMissing::Keep),
            ("skip", None) => Some(OnMissing::Skip),
            _ => {
                self.problems.add(
                    object.at("on_missing"),
                    "must be one of error, keep, default, skip",
                );
                None
            }
        }
    }

    fn concat(&mut self, object: &Obj<'_>, ctx: Ctx) -> Option<Output> {
        let at = object.at("parts");
        let parts = object.required_array("parts", &mut self.problems)?;
        if parts.is_empty() {
            self.problems.add(at, "needs at least one part");
            return None;
        }
        let compiled: Vec<Option<Part>> = parts
            .iter()
            .enumerate()
            .map(|(i, part)| self.part(part, &format!("{at}[{i}]"), ctx))
            .collect();
        compiled
            .into_iter()
            .collect::<Option<_>>()
            .map(Output::Concat)
    }

    fn part(&mut self, value: &Value, at: &str, ctx: Ctx) -> Option<Part> {
        let object = Obj::new(value, at, &["from", "steps", "text"], &mut self.problems)?;
        match (object.has("from"), object.has("text")) {
            (true, false) => self.source(&object, ctx).map(Part::From),
            (false, true) if object.has("steps") => {
                self.problems
                    .add(object.at("steps"), "steps only follow a `from`");
                None
            }
            (false, true) => object
                .required_scalar("text", &mut self.problems)
                .map(Part::Text),
            _ => {
                self.problems
                    .add(at, "a part has exactly one of `from` and `text`");
                None
            }
        }
    }

    fn datetime(&mut self, object: &Obj<'_>, ctx: Ctx) -> Option<Output> {
        let from = self.source(object, ctx);
        let input = self.format(object, "input");
        let output = self.format(object, "output");
        let from_zone = self.zone(object, "from_timezone");
        let to_zone = self.zone(object, "to_timezone");
        Some(Output::Datetime(Box::new(Datetime {
            from: from?,
            reformat: Reformat {
                input: input?,
                output: output?,
                from_zone: from_zone?,
                to_zone: to_zone?,
            },
        })))
    }

    fn format(&mut self, object: &Obj<'_>, key: &str) -> Option<DateFormat> {
        let text = object.required_string(key, &mut self.problems)?;
        match DateFormat::parse(text) {
            Ok(format) => Some(format),
            Err(error) => {
                self.problems.add(
                    object.at(key),
                    format!(
                        "invalid date format at position {}: {}",
                        error.position, error.reason
                    ),
                );
                None
            }
        }
    }

    /// `Some(None)` when absent, `Some(Some(zone))` when valid, `None` after a problem.
    fn zone(&mut self, object: &Obj<'_>, key: &str) -> Option<Option<jiff::tz::TimeZone>> {
        if !object.has(key) {
            return Some(None);
        }
        let name = object.string(key, &mut self.problems)?;
        if let Some(found) = zone(name) {
            return Some(Some(found));
        }
        self.problems.add(
            object.at(key),
            format!("`{name}` is not a time zone known to this system"),
        );
        None
    }

    fn when(&mut self, object: &Obj<'_>, loc: &Loc, top: usize, ctx: Ctx) -> Option<Kind> {
        if ctx == Ctx::Append {
            self.problems
                .add(object.at("op"), "append holds only rules that write");
            return None;
        }
        let condition = if let Some(value) = object.value("condition") {
            let in_loop = matches!(ctx, Ctx::Loop | Ctx::Repeat(_));
            let mut cx = Cx::new(self.source, in_loop, &mut self.problems);
            compile_expr(value, &mut cx, &object.at("condition"), 0)
        } else {
            self.problems.add(object.at("condition"), "is required");
            None
        };
        let then = object
            .required_array("then", &mut self.problems)
            .map(|rules| self.rules(rules, &loc.key("then"), Some(top), ctx));
        let otherwise = match object.value("else") {
            None => Some(Vec::new()),
            Some(Value::Array(rules)) => Some(self.rules(rules, &loc.key("else"), Some(top), ctx)),
            Some(_) => {
                self.problems
                    .add(object.at("else"), "must be an array of rules");
                None
            }
        };
        Some(Kind::When(Box::new(When {
            condition: condition?,
            then: then?,
            otherwise: otherwise?,
        })))
    }

    fn for_each(&mut self, object: &Obj<'_>, loc: &Loc, top: usize, ctx: Ctx) -> Option<Kind> {
        if ctx != Ctx::Top {
            self.problems
                .add(&loc.at, "for_each cannot be inside for_each or append");
            return None;
        }
        let over = self.record(object, "over", false);
        let target = self.loop_target(object, over.as_deref());
        let ctx = match &target {
            Some(LoopTarget::Repeat(path)) => match path.field() {
                FieldRef::Number(field) => Ctx::Repeat(*field),
                FieldRef::Name(_) => Ctx::Loop,
            },
            _ => Ctx::Loop,
        };
        let rules = object
            .required_array("rules", &mut self.problems)
            .map(|rules| self.rules(rules, &loc.key("rules"), Some(top), ctx));
        Some(Kind::ForEach {
            over: over?,
            target: target?,
            rules: rules?,
        })
    }

    /// `for_each.mode` with its `into`: `create` (a new record per input occurrence),
    /// `update` (the target's copy of the input occurrence: a same-protocol HL7 or ASTM
    /// target, which starts as a copy of the source, and `into` equal to `over`) or `repeat`
    /// (one repetition of the field path `into` per input occurrence: an HL7 or ASTM target).
    fn loop_target(&mut self, object: &Obj<'_>, over: Option<&str>) -> Option<LoopTarget> {
        let mode = object.string("mode", &mut self.problems);
        let at = object.at("mode");
        match mode {
            None if object.has("mode") => None,
            None | Some("create") => self.record(object, "into", true).map(LoopTarget::Create),
            Some("repeat") => {
                if !matches!(self.target.syntax(), Syntax::Hl7 | Syntax::Astm) {
                    self.problems
                        .add(at, "mode `repeat` needs an HL7 or ASTM target");
                    return None;
                }
                let into = self.path(object, "into", true, Ctx::Top)?;
                if into.repetition().is_some() || into.component().is_some() {
                    self.problems.add(
                        object.at("into"),
                        "with mode `repeat`, into is a field path such as O.5",
                    );
                    return None;
                }
                Some(LoopTarget::Repeat(into))
            }
            Some("update") => {
                let into = self.record(object, "into", true);
                let copied = self.source.syntax() == self.target.syntax()
                    && matches!(self.source.syntax(), Syntax::Hl7 | Syntax::Astm);
                if !copied {
                    self.problems.add(
                        at,
                        "mode `update` needs an HL7 or ASTM target of the source protocol",
                    );
                    return None;
                }
                let into = into?;
                if over.is_some_and(|over| over != into) {
                    self.problems
                        .add(at, "mode `update` needs `into` equal to `over`");
                    return None;
                }
                Some(LoopTarget::Update(into))
            }
            Some(_) => {
                self.problems
                    .add(at, "must be one of create, update, repeat");
                None
            }
        }
    }

    fn append(&mut self, object: &Obj<'_>, loc: &Loc, top: usize, ctx: Ctx) -> Option<Kind> {
        if ctx != Ctx::Top {
            self.problems
                .add(&loc.at, "append cannot be inside for_each or append");
            return None;
        }
        let into = self.record(object, "into", true);
        let rules = object.required_array("rules", &mut self.problems)?;
        if rules.is_empty() {
            self.problems
                .add(object.at("rules"), "needs at least one rule");
            return None;
        }
        let compiled = self.rules(rules, &loc.key("rules"), Some(top), Ctx::Append);
        (compiled.len() == rules.len()).then_some(())?;
        Some(Kind::Append {
            into: into?,
            rules: compiled,
        })
    }

    fn record(&mut self, object: &Obj<'_>, key: &str, write: bool) -> Option<String> {
        let name = object.required_string(key, &mut self.problems)?;
        match self.schema(write).check_record(name, write) {
            Ok(()) => Some(name.to_owned()),
            Err(message) => {
                self.problems.add(object.at(key), message);
                None
            }
        }
    }

    fn sequence(&mut self, object: &Obj<'_>, ctx: Ctx) -> Option<Output> {
        if ctx == Ctx::Top {
            self.problems.add(
                object.at("op"),
                "sequence is only valid inside for_each or append",
            );
        }
        let start = object.value("start").map_or(Some(1), |value| {
            let start = value.as_u64().filter(|n| *n <= MAX_SEQUENCE_START);
            if start.is_none() {
                self.problems.add(
                    object.at("start"),
                    format!("must be an integer from 0 to {MAX_SEQUENCE_START}"),
                );
            }
            start
        });
        (ctx != Ctx::Top).then_some(())?;
        start.map(Output::Sequence)
    }
}
