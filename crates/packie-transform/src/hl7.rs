//! HL7 v2 adapter over [`packie_hl7::Message`].
//!
//! Reading returns the codec's decoded text, so a field or component that has several
//! parts carries its structural separators (`MSH.9` is `ORU^R01`). Writing is symmetric:
//! a value written to a field, repetition or component path is split on the separators
//! that lie below that level (`~`, `^`, `&`), so `constant "ORU^R01" → MSH.9` produces
//! two components and an ASTM `^^^GLU` produces three empty components and one filled.
//! To store a delimiter character as data, write to the subcomponent path (`PID.5.1.1`),
//! which is always literal and escaped on the wire.

use std::borrow::Cow;
use std::fmt;

use packie_hl7::{EncodingCharacters, Message};

use crate::error::ModelError;
use crate::model::Model;
use crate::path::{FieldRef, Path, Syntax, record_name_ok};

/// An HL7 v2 message addressed by `SEG[occ].field[rep].component.subcomponent` paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hl7Model {
    message: Message,
}

impl Default for Hl7Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Hl7Model {
    /// An empty message with just `MSH`, standard delimiters (`|^~\&`) and no other
    /// fields: the starting point of a mapping that builds the whole message.
    #[must_use]
    pub fn new() -> Self {
        Self {
            message: Message::new(EncodingCharacters::default()),
        }
    }

    /// Parses message bytes.
    ///
    /// # Errors
    /// The codec's structural error.
    pub fn parse(bytes: &[u8]) -> Result<Self, packie_hl7::Error> {
        Message::parse(bytes).map(Self::from_message)
    }

    /// Wraps an existing message, for example a skeleton the pipeline built.
    #[must_use]
    pub const fn from_message(message: Message) -> Self {
        Self { message }
    }

    /// The underlying message.
    #[must_use]
    pub const fn message(&self) -> &Message {
        &self.message
    }

    /// The wire bytes: the original bytes when nothing was written, otherwise the
    /// canonical CR-terminated form.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        self.message.encode()
    }

    fn write_leaf(&mut self, path: &Path, levels: Levels, value: &str) -> Result<(), ModelError> {
        let text = codec_path(path, levels).ok_or(ModelError::WrongSyntax)?;
        self.message.set(&text, value).map_err(map_error)
    }
}

fn map_error(error: packie_hl7::Error) -> ModelError {
    match error {
        packie_hl7::Error::ImmutableEncodingCharacters => ModelError::Immutable,
        _ => ModelError::Unrepresentable,
    }
}

/// The levels below a field a read or write is addressed at.
#[derive(Debug, Clone, Copy)]
struct Levels {
    repetition: Option<usize>,
    component: Option<usize>,
    subcomponent: Option<usize>,
}

impl Levels {
    /// The levels the path itself names.
    const fn of(path: &Path) -> Self {
        Self {
            repetition: path.repetition(),
            component: path.component(),
            subcomponent: path.subcomponent(),
        }
    }
}

impl fmt::Display for Levels {
    /// The codec's suffix: `[rep].comp.sub`, each part only when present.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(repetition) = self.repetition {
            write!(f, "[{repetition}]")?;
        }
        if let Some(component) = self.component {
            write!(f, ".{component}")?;
        }
        if let Some(subcomponent) = self.subcomponent {
            write!(f, ".{subcomponent}")?;
        }
        Ok(())
    }
}

/// The codec's own path text, `SEG(n)-field[rep].comp.sub`, for the field `path` names at
/// `levels`; `None` when `path` is not an HL7 segment path.
fn codec_path(path: &Path, levels: Levels) -> Option<String> {
    let (Some(record), FieldRef::Number(field)) = (path.record(), path.field()) else {
        return None;
    };
    if !path.is_absolute_in(Syntax::Hl7) {
        return None;
    }
    Some(format!(
        "{record}({})-{field}{levels}",
        path.occurrence() + 1
    ))
}

impl Model for Hl7Model {
    fn syntax(&self) -> Syntax {
        Syntax::Hl7
    }

    fn get(&self, path: &Path) -> Option<Cow<'_, str>> {
        let codec_path = packie_hl7::Path::parse(&codec_path(path, Levels::of(path))?).ok()?;
        self.message.get_path(&codec_path).map(Cow::Borrowed)
    }

    fn set(&mut self, path: &Path, value: &str) -> Result<(), ModelError> {
        let encoding = self.message.encoding();
        let separators = [
            char::from(encoding.repetition()),
            char::from(encoding.component()),
            char::from(encoding.subcomponent()),
        ];
        let levels_below = match (path.repetition(), path.component(), path.subcomponent()) {
            (_, _, Some(_)) => 3,
            (_, Some(_), None) => 2,
            (Some(_), None, None) => 1,
            (None, None, None) => 0,
        };
        // Separators that act as structure at this level: those strictly below it.
        let active = &separators[levels_below.min(2)..];
        if levels_below == 3 || !value.contains(active) {
            return self.write_leaf(path, Levels::of(path), value);
        }
        self.set_structured(path, value, separators)
    }

    fn count(&self, record: &str) -> usize {
        self.message
            .segments()
            .iter()
            .filter(|segment| segment.name() == record)
            .count()
    }

    fn create(&mut self, record: &str) -> Result<usize, ModelError> {
        if record == "MSH" || !record_name_ok(Syntax::Hl7, record) {
            return Err(ModelError::CannotCreate);
        }
        let index = self.count(record);
        // The codec appends a segment when a field of a new occurrence is written.
        self.message
            .set(&format!("{record}({})-1", index + 1), "")
            .map_err(map_error)?;
        Ok(index)
    }
}

impl Hl7Model {
    /// Writes a value that carries separators below the addressed level: the addressed
    /// node is cleared, then every leaf is written at subcomponent level.
    fn set_structured(
        &mut self,
        path: &Path,
        value: &str,
        separators: [char; 3],
    ) -> Result<(), ModelError> {
        let node = Levels {
            subcomponent: None,
            ..Levels::of(path)
        };
        self.write_leaf(path, node, "")?;
        for (repetition, component, subcomponent, text) in leaves(path, value, separators) {
            let leaf = Levels {
                repetition: Some(repetition),
                component: Some(component),
                subcomponent: Some(subcomponent),
            };
            self.write_leaf(path, leaf, text)?;
        }
        Ok(())
    }
}

/// The leaves of `value` as `(repetition, component, subcomponent, text)`, the first
/// two zero-based and one-based as the codec path wants them.
fn leaves<'v>(
    path: &Path,
    value: &'v str,
    separators: [char; 3],
) -> Vec<(usize, usize, usize, &'v str)> {
    let [repetition_sep, component_sep, subcomponent_sep] = separators;
    let fixed_repetition = path.repetition().or_else(|| path.component().map(|_| 0));
    let repetitions: Vec<&str> = if path.repetition().is_none() && path.component().is_none() {
        value.split(repetition_sep).collect()
    } else {
        vec![value]
    };
    let mut out = Vec::new();
    for (r, repetition) in repetitions.into_iter().enumerate() {
        let components: Vec<(usize, &str)> = match path.component() {
            Some(component) => vec![(component, repetition)],
            None => repetition
                .split(component_sep)
                .enumerate()
                .map(|(c, text)| (c + 1, text))
                .collect(),
        };
        for (component, text) in components {
            out.extend(
                text.split(subcomponent_sep)
                    .enumerate()
                    .map(|(s, leaf)| (fixed_repetition.unwrap_or(r), component, s + 1, leaf)),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORU: &[u8] = b"MSH|^~\\&|APP|FAC|LIS|LAB|20261012120000||ORU^R01|M1|P|2.5.1\rPID|1||SYN-1^^^HOSP~SYN-2||DOE^JANE^Q\rOBX|1|NM|GLU^Glucose|1|5.5|mmol/L|||||F\rOBX|2|ST|NOTE||a \\T\\ b\r";

    fn p(text: &str) -> Path {
        Path::parse(Syntax::Hl7, text).expect("valid path")
    }

    fn model() -> Hl7Model {
        Hl7Model::parse(ORU).expect("synthetic message parses")
    }

    #[test]
    fn reads_fields_components_repetitions_and_occurrences() {
        let m = model();
        assert_eq!(m.get(&p("MSH.9")).as_deref(), Some("ORU^R01"));
        assert_eq!(m.get(&p("MSH.9.2")).as_deref(), Some("R01"));
        assert_eq!(m.get(&p("PID.3")).as_deref(), Some("SYN-1^^^HOSP~SYN-2"));
        assert_eq!(m.get(&p("PID.3[0]")).as_deref(), Some("SYN-1^^^HOSP"));
        assert_eq!(m.get(&p("PID.3[1].1")).as_deref(), Some("SYN-2"));
        assert_eq!(m.get(&p("PID.3[0].4")).as_deref(), Some("HOSP"));
        assert_eq!(m.get(&p("OBX[1].5")).as_deref(), Some("a & b"));
        assert_eq!(m.get(&p("OBX.5")).as_deref(), Some("5.5"));
        assert_eq!(m.get(&p("OBX[2].5")), None);
        assert_eq!(m.get(&p("ZZZ.1")), None);
        assert_eq!(m.count("OBX"), 2);
        assert_eq!(m.count("NTE"), 0);
    }

    #[test]
    fn paths_of_other_syntaxes_and_scopes_read_nothing() {
        let m = model();
        let astm = Path::parse(Syntax::Astm, "R.3").expect("valid");
        assert_eq!(m.get(&astm), None);
        assert_eq!(m.get(&p("$.3")), None);
    }

    #[test]
    fn writes_split_structure_below_the_addressed_level() {
        let mut m = Hl7Model::new();
        m.set(&p("MSH.9"), "ORU^R01").expect("write");
        m.set(&p("PID.3"), "A^^^H~B").expect("write");
        m.set(&p("PID.5.1"), "x&y").expect("write");
        m.set(&p("PID.6.1.1"), "a^b~c&d").expect("write");
        m.set(&p("PID.7[1]"), "p^q").expect("write");
        assert_eq!(m.get(&p("MSH.9.1")).as_deref(), Some("ORU"));
        assert_eq!(m.get(&p("PID.3[1].1")).as_deref(), Some("B"));
        assert_eq!(m.get(&p("PID.3[0].4")).as_deref(), Some("H"));
        assert_eq!(m.get(&p("PID.5.1.2")).as_deref(), Some("y"));
        assert_eq!(m.get(&p("PID.6.1.1")).as_deref(), Some("a^b~c&d"));
        assert_eq!(m.get(&p("PID.7[1].2")).as_deref(), Some("q"));
        assert_eq!(
            m.encode(),
            b"MSH|^~\\&|||||||ORU^R01\rPID|||A^^^H~B||x&y|a\\S\\b\\R\\c\\T\\d|~p^q\r".to_vec()
        );
    }

    #[test]
    fn rewriting_a_field_clears_its_previous_structure() {
        let mut m = model();
        m.set(&p("PID.3"), "ONLY").expect("write");
        assert_eq!(m.get(&p("PID.3")).as_deref(), Some("ONLY"));
        m.set(&p("PID.5"), "A^B^C").expect("write");
        m.set(&p("PID.5"), "Z^Y").expect("write");
        assert_eq!(m.get(&p("PID.5")).as_deref(), Some("Z^Y"));
    }

    #[test]
    fn create_appends_numbered_segments_and_skips_msh() {
        let mut m = Hl7Model::new();
        assert_eq!(m.create("OBX"), Ok(0));
        assert_eq!(m.create("OBX"), Ok(1));
        m.set(&p("OBX[1].5"), "7").expect("write");
        assert_eq!(m.count("OBX"), 2);
        assert_eq!(m.encode(), b"MSH|^~\\&\rOBX|\rOBX|||||7\r".to_vec());
        assert_eq!(m.create("MSH"), Err(ModelError::CannotCreate));
        assert_eq!(m.create("obx"), Err(ModelError::CannotCreate));
    }

    #[test]
    fn msh_delimiters_cannot_be_written() {
        let mut m = Hl7Model::new();
        assert_eq!(m.set(&p("MSH.1"), "x"), Err(ModelError::Immutable));
        assert_eq!(m.set(&p("MSH.2"), "x"), Err(ModelError::Immutable));
        assert_eq!(
            m.set(&Path::parse(Syntax::Astm, "R.3").expect("valid"), "x"),
            Err(ModelError::WrongSyntax)
        );
    }

    #[test]
    fn untouched_messages_encode_to_their_original_bytes() {
        assert_eq!(model().encode(), ORU.to_vec());
    }
}
