//! Running a compiled mapping.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::error::{Reason, TransformError};
use crate::formula::{Calculate, OnInvalid, Stop, source_text};
use crate::mapping::Mapping;
use crate::model::{Model, repeated_group, single_group, write};
use crate::path::{Frame, Path, Scope};
use crate::rules::{Kind, Lookup, LoopTarget, OnMissing, Output, Part, Rule, Source, Write};
use crate::tables::CodeTable;

impl Mapping {
    /// Runs the rules in order, reading `source` and writing `target`.
    ///
    /// `target` is the message the output starts from: an empty model with the generated
    /// header fields (what the pipeline builds for the destination's protocol, for example
    /// `Hl7Model::new()` with MSH fields set), or a clone of the source model when source
    /// and target protocols are the same. Rules only add or overwrite what they address.
    ///
    /// With `require_single_group`, a source holding more than one patient, order or
    /// specimen group fails before any rule runs; so does a source that does not meet a
    /// `require` condition (`rule_path` `require[i]`, `op` `require`).
    ///
    /// # Errors
    /// The first rule that fails ([`TransformError`]), or the `require_single_group`
    /// precondition (`rule` 0, `rule_path` `require_single_group`, `op` `mapping`, `path` the
    /// repeated record). `target` may then be partly written and must be discarded: a failed
    /// mapping produces no output.
    pub fn apply(&self, source: &dyn Model, target: &mut dyn Model) -> Result<(), TransformError> {
        if let Some(record) = repeated_group(source).filter(|_| self.single_group) {
            return Err(TransformError {
                rule: 0,
                rule_path: "require_single_group".to_owned(),
                op: "mapping",
                path: record.to_owned(),
                reason: Reason::MultipleGroups,
            });
        }
        for (index, condition) in self.require.iter().enumerate() {
            condition.check(source, index)?;
        }
        let mut run = Run {
            tables: &self.tables,
            source,
            target,
        };
        run.rules(&self.rules, None)
    }
}

/// The `for_each` iteration in progress: the source occurrence being read, the target
/// occurrence just created, and the zero-based iteration count.
#[derive(Clone, Copy)]
struct Loop<'a> {
    input: Frame<'a>,
    output: Frame<'a>,
    counter: u64,
    /// Mode `repeat`: the repetition of the output field that `$.` `to` paths write.
    repetition: Option<usize>,
}

struct Run<'a> {
    tables: &'a BTreeMap<String, CodeTable>,
    source: &'a dyn Model,
    target: &'a mut dyn Model,
}

/// What a writing rule writes at its `to`.
enum Text<'r> {
    /// The text, split on the message's separators below the addressed level.
    Plain(Cow<'r, str>),
    /// An HL7 structured numeric value: the field becomes comparator `^` number.
    Sn(&'static str, String),
}

type Produced<'r> = Result<Option<Text<'r>>, TransformError>;

/// `Some(Plain)` unless `text` is empty: an empty result writes nothing.
fn plain(text: Cow<'_, str>) -> Option<Text<'_>> {
    (!text.is_empty()).then_some(Text::Plain(text))
}

impl<'a> Run<'a> {
    fn rules(&mut self, rules: &[Rule], frame: Option<Loop<'_>>) -> Result<(), TransformError> {
        rules.iter().try_for_each(|rule| self.rule(rule, frame))
    }

    fn rule(&mut self, rule: &Rule, frame: Option<Loop<'_>>) -> Result<(), TransformError> {
        match &rule.kind {
            Kind::Write(write) => {
                let Write { to, output } = &**write;
                let input = frame.map(|f| f.input);
                let counter = frame.map_or(0, |f| f.counter);
                let Some(text) = self.output(rule, output, to, input, counter)? else {
                    return Ok(());
                };
                match frame {
                    Some(Loop {
                        output,
                        repetition: Some(repetition),
                        ..
                    }) if to.scope() == Scope::Current => {
                        let to = to
                            .resolved(output.record, output.occurrence)
                            .with_repetition(repetition);
                        self.put(rule, &to, &text, None)
                    }
                    _ => self.put(rule, to, &text, frame.map(|f| f.output)),
                }
            }
            Kind::When(when) => {
                let holds = when.condition.eval(self.source, frame.map(|f| f.input));
                self.rules(if holds { &when.then } else { &when.otherwise }, frame)
            }
            Kind::ForEach {
                over,
                target,
                rules,
            } => self.for_each(rule, over, target, rules),
            Kind::Append { into, rules } => self.append(rule, into, rules),
        }
    }

    /// The text a writing rule produces; `None` writes nothing. `input` resolves `$.` in
    /// `from` paths; `counter` is what `sequence` adds to its start.
    fn output<'r>(
        &self,
        rule: &Rule,
        output: &'r Output,
        to: &Path,
        input: Option<Frame<'_>>,
        counter: u64,
    ) -> Produced<'r>
    where
        'a: 'r,
    {
        let text = match output {
            Output::Copy {
                from,
                default,
                required,
            } => {
                let text = self.read(rule, from, input)?;
                if !text.is_empty() {
                    return Ok(plain(text));
                }
                match default {
                    Some(default) if !default.is_empty() => Cow::Borrowed(default.as_str()),
                    _ if *required => {
                        return Err(fail(rule, from.path.as_str(), Reason::RequiredMissing));
                    }
                    _ => return Ok(None),
                }
            }
            // `constant` writes even empty text (it clears the field).
            Output::Constant(value) => return Ok(Some(Text::Plain(Cow::Borrowed(value)))),
            Output::Lookup(lookup) => return self.lookup(rule, lookup, input),
            Output::Concat(parts) => Cow::Owned(self.concat(rule, parts, input)?),
            Output::Datetime(datetime) => {
                let text = self.read(rule, &datetime.from, input)?;
                if text.is_empty() {
                    return Ok(None);
                }
                match datetime.reformat.apply(&text) {
                    Ok(output) => Cow::Owned(output),
                    Err(reason) => return Err(fail(rule, datetime.from.path.as_str(), reason)),
                }
            }
            Output::Calculate(calculate) => return self.calculate(rule, calculate, to, input),
            Output::Sequence(start) => Cow::Owned(start.saturating_add(counter).to_string()),
        };
        Ok(plain(text))
    }

    fn lookup<'r>(&self, rule: &Rule, lookup: &'r Lookup, input: Option<Frame<'_>>) -> Produced<'r>
    where
        'a: 'r,
    {
        let key = self.read(rule, &lookup.from, input)?;
        let found = self
            .tables
            .get(&lookup.table)
            .and_then(|table| table.lookup(&key, lookup.ignore_case));
        let output: Cow<'r, str> = match (found, &lookup.on_missing) {
            (Some(replacement), _) => Cow::Borrowed(replacement),
            (None, OnMissing::Error) => {
                return Err(fail(
                    rule,
                    lookup.from.path.as_str(),
                    Reason::LookupMissing {
                        table: lookup.table.clone(),
                    },
                ));
            }
            (None, OnMissing::Keep) => key,
            (None, OnMissing::Default(default)) => Cow::Borrowed(default),
            (None, OnMissing::Skip) => return Ok(None),
        };
        Ok(plain(output))
    }

    fn concat(
        &self,
        rule: &Rule,
        parts: &[Part],
        input: Option<Frame<'_>>,
    ) -> Result<String, TransformError> {
        let mut text = String::new();
        for part in parts {
            match part {
                Part::From(source) => text.push_str(&self.read(rule, source, input)?),
                Part::Text(literal) => text.push_str(literal),
            }
        }
        Ok(text)
    }

    fn calculate<'r>(
        &self,
        rule: &Rule,
        calculate: &'r Calculate,
        to: &Path,
        input: Option<Frame<'_>>,
    ) -> Produced<'r>
    where
        'a: 'r,
    {
        let sn = calculate.reading.sn;
        let outcome = calculate
            .formula
            .eval(self.source, input, calculate.reading)
            .and_then(|num| {
                num.parts(calculate.scale, calculate.rounding)
                    .map_err(|reason| Stop::Invalid(reason, None))
            });
        let (reason, path) = match outcome {
            Ok((comparator, number)) if sn => return Ok(Some(Text::Sn(comparator, number))),
            Ok((comparator, number)) => {
                return Ok(Some(Text::Plain(Cow::Owned(format!(
                    "{comparator}{number}"
                )))));
            }
            Err(Stop::Empty(path)) if calculate.required => {
                return Err(fail(rule, path.as_str(), Reason::RequiredMissing));
            }
            Err(Stop::Empty(_)) => return Ok(None),
            Err(Stop::Fatal(reason, path)) => return Err(fail(rule, path.as_str(), reason)),
            Err(Stop::Invalid(reason, path)) => (reason, path.unwrap_or(to)),
        };
        let text: Cow<'r, str> = match &calculate.on_invalid {
            OnInvalid::Error => return Err(fail(rule, path.as_str(), reason)),
            OnInvalid::Skip => return Ok(None),
            OnInvalid::Default(text) => Cow::Borrowed(text),
            // The operand text is what the steps failed to produce: `keep` has nothing to
            // write, and running the steps again would only fail again.
            OnInvalid::Keep if reason == Reason::StepsTooLong => {
                return Err(fail(rule, path.as_str(), reason));
            }
            OnInvalid::Keep => calculate
                .formula
                .operand_text(self.source, input, sn)
                .map_err(|_| fail(rule, path.as_str(), Reason::StepsTooLong))?
                .unwrap_or_default(),
        };
        Ok(plain(text))
    }

    /// Runs `rules` once per `over` occurrence of the source, writing a new `into`
    /// occurrence, with `update` the target's copy of the same occurrence, or with `repeat`
    /// repetition *i* of one field.
    fn for_each(
        &mut self,
        rule: &Rule,
        over: &str,
        target: &LoopTarget,
        rules: &[Rule],
    ) -> Result<(), TransformError> {
        for input in 0..self.source.count(over) {
            let (output, repetition) = match target {
                LoopTarget::Create(into) => {
                    let occurrence = self
                        .target
                        .create(into)
                        .map_err(|error| fail(rule, into, Reason::Write(error)))?;
                    (
                        Frame {
                            record: into,
                            occurrence,
                        },
                        None,
                    )
                }
                LoopTarget::Update(into) => (
                    Frame {
                        record: into,
                        occurrence: input,
                    },
                    None,
                ),
                LoopTarget::Repeat(field) => (
                    Frame {
                        record: field.record().unwrap_or_default(),
                        occurrence: field.occurrence(),
                    },
                    Some(input),
                ),
            };
            let frame = Loop {
                input: Frame {
                    record: over,
                    occurrence: input,
                },
                output,
                counter: u64::try_from(input).unwrap_or(u64::MAX),
                repetition,
            };
            self.rules(rules, Some(frame))?;
        }
        Ok(())
    }

    /// Adds one `into` occurrence when every rule produces a text; otherwise nothing. A
    /// target with several groups is refused: the record would land under the last one.
    fn append(&mut self, rule: &Rule, into: &str, rules: &[Rule]) -> Result<(), TransformError> {
        if !single_group(self.target) {
            return Err(fail(rule, into, Reason::MultipleGroups));
        }
        // `create` appends, so the new occurrence's number is the current count.
        let counter = u64::try_from(self.target.count(into)).unwrap_or(u64::MAX);
        let mut writes = Vec::with_capacity(rules.len());
        for inner in rules {
            // Validation admits only writing rules here.
            let Kind::Write(write) = &inner.kind else {
                continue;
            };
            let Write { to, output } = &**write;
            match self.output(inner, output, to, None, counter)? {
                Some(text) => writes.push((inner, to, text)),
                None => return Ok(()),
            }
        }
        let occurrence = self
            .target
            .create(into)
            .map_err(|error| fail(rule, into, Reason::Write(error)))?;
        let frame = Frame {
            record: into,
            occurrence,
        };
        for (inner, to, text) in writes {
            self.put(inner, to, &text, Some(frame))?;
        }
        Ok(())
    }

    /// The source text of `from` after its steps; missing is empty. A step over its size
    /// bound is a transform error at the `from` path, whatever the rule's fallbacks say.
    fn read(
        &self,
        rule: &Rule,
        from: &Source,
        input: Option<Frame<'_>>,
    ) -> Result<Cow<'a, str>, TransformError> {
        source_text(self.source, from, input)
            .map_err(|_| fail(rule, from.path.as_str(), Reason::StepsTooLong))
    }

    /// Writes a rule's text; a structured numeric value replaces the field with its
    /// comparator and number components.
    fn put(
        &mut self,
        rule: &Rule,
        to: &Path,
        text: &Text<'_>,
        frame: Option<Frame<'_>>,
    ) -> Result<(), TransformError> {
        match text {
            Text::Plain(text) => self.write(rule, to, text, frame),
            Text::Sn(comparator, number) => {
                self.write(rule, to, "", frame)?;
                if !comparator.is_empty() {
                    self.write(rule, &to.component_path(1), comparator, frame)?;
                }
                self.write(rule, &to.component_path(2), number, frame)
            }
        }
    }

    fn write(
        &mut self,
        rule: &Rule,
        path: &Path,
        value: &str,
        frame: Option<Frame<'_>>,
    ) -> Result<(), TransformError> {
        write(self.target, path, value, frame)
            .map_err(|error| fail(rule, path.as_str(), Reason::Write(error)))
    }
}

fn fail(rule: &Rule, path: &str, reason: Reason) -> TransformError {
    TransformError {
        rule: rule.top,
        rule_path: rule.location.clone(),
        op: rule.op,
        path: path.to_owned(),
        reason,
    }
}
