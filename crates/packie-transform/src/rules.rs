//! Compiled mapping rules.

use crate::datetime::Reformat;
use crate::filter::Expr;
use crate::formula::Calculate;
use crate::path::Path;
use crate::steps::Steps;

/// One rule with the location it came from, for errors.
#[derive(Debug, Clone)]
pub(crate) struct Rule {
    /// The rule's `op`.
    pub(crate) op: &'static str,
    /// Location within the mapping, for example `rules[2].rules[1]`.
    pub(crate) location: String,
    /// Index of the enclosing top-level rule.
    pub(crate) top: usize,
    pub(crate) kind: Kind,
}

#[derive(Debug, Clone)]
pub(crate) enum Kind {
    /// A rule that produces one text and writes it to `to`.
    Write(Box<Write>),
    When(Box<When>),
    ForEach {
        over: String,
        target: LoopTarget,
        rules: Vec<Rule>,
    },
    /// Adds one `into` occurrence when every rule (all `Write`) produces a text.
    Append {
        into: String,
        rules: Vec<Rule>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Write {
    pub(crate) to: Path,
    pub(crate) output: Output,
}

/// Where each `for_each` iteration writes (`mode`).
#[derive(Debug, Clone)]
pub(crate) enum LoopTarget {
    /// `create`: a new occurrence of the record.
    Create(String),
    /// `update`: the target's copy of the input occurrence.
    Update(String),
    /// `repeat`: repetition *i* of this field.
    Repeat(Path),
}

/// What a writing rule produces.
#[derive(Debug, Clone)]
pub(crate) enum Output {
    Copy {
        from: Source,
        default: Option<String>,
        required: bool,
    },
    Constant(String),
    Lookup(Box<Lookup>),
    Concat(Vec<Part>),
    Datetime(Box<Datetime>),
    Calculate(Box<Calculate>),
    Sequence(u64),
}

/// A `from` path and the text steps that follow it.
#[derive(Debug, Clone)]
pub(crate) struct Source {
    pub(crate) path: Path,
    pub(crate) steps: Steps,
}

#[derive(Debug, Clone)]
pub(crate) struct Lookup {
    pub(crate) from: Source,
    pub(crate) table: String,
    pub(crate) on_missing: OnMissing,
    pub(crate) ignore_case: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum OnMissing {
    Error,
    Keep,
    Default(String),
    Skip,
}

#[derive(Debug, Clone)]
pub(crate) enum Part {
    From(Source),
    Text(String),
}

#[derive(Debug, Clone)]
pub(crate) struct Datetime {
    pub(crate) from: Source,
    pub(crate) reformat: Reformat,
}

#[derive(Debug, Clone)]
pub(crate) struct When {
    pub(crate) condition: Expr,
    pub(crate) then: Vec<Rule>,
    pub(crate) otherwise: Vec<Rule>,
}
