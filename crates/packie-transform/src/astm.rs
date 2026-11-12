//! ASTM E1394 adapter over [`packie_astm::Record`].
//!
//! The model is a flat list of records. `R[2]` is the third R record of the message,
//! wherever it sits under its patient and order. Fields are numbered as in the standard,
//! where field 1 is the record type: `R.1` is `R`, `R.2` the sequence number, `R.3` the
//! universal test id, `R.4` the value, `R.7` the abnormal flag, `O.3` the specimen id,
//! `P.3` the practice-assigned patient id. Repeats (`R.3[1]`) and components (`R.3.4`)
//! follow the field.
//!
//! Reading returns decoded text with the repeat and component separators in place.
//! Writing mirrors it: a value written to a field or repeat is split on the separators
//! below that level, and every part is escaped when the message is encoded
//! ([`AstmModel::encode`]).

use std::borrow::Cow;
use std::fmt::Write as _;

use packie_astm::{Delimiters, Record, RecordError, RecordType};

use crate::error::ModelError;
use crate::model::Model;
use crate::path::{FieldRef, Path, Syntax, record_name_ok};

/// Repeats of components of decoded leaf text.
type Field = Vec<Vec<String>>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rec {
    kind: char,
    /// `fields[i]` is E1394 field `i + 2`.
    fields: Vec<Field>,
}

/// An ASTM message addressed by `R[occ].field[rep].component` paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstmModel {
    delimiters: Delimiters,
    records: Vec<Rec>,
}

impl Default for AstmModel {
    fn default() -> Self {
        Self::new()
    }
}

fn leaf(text: &str) -> Field {
    vec![vec![text.to_owned()]]
}

impl AstmModel {
    /// A message of just the header (`H|\^&`) and terminator (`L|1|N`) records, with the
    /// standard delimiters: the starting point of a mapping that builds the whole message.
    #[must_use]
    pub fn new() -> Self {
        Self {
            delimiters: Delimiters::default(),
            records: vec![
                Rec {
                    kind: 'H',
                    fields: vec![leaf("\\^&")],
                },
                Rec {
                    kind: 'L',
                    fields: vec![leaf("1"), leaf("N")],
                },
            ],
        }
    }

    /// Parses CR-terminated record bytes (the form `packie_astm` receives).
    ///
    /// # Errors
    /// The codec's record error.
    pub fn parse(bytes: &[u8]) -> Result<Self, RecordError> {
        packie_astm::parse_message_records(bytes).map(|records| Self::from_records(&records))
    }

    /// Builds the model from parsed records, keeping the header's delimiters.
    #[must_use]
    pub fn from_records(records: &[Record]) -> Self {
        let delimiters = records
            .first()
            .and_then(|header| match header.raw_bytes() {
                [b'H', field, repeat, component, escape, ..] => {
                    Delimiters::new(*field, *repeat, *component, *escape).ok()
                }
                _ => None,
            })
            .unwrap_or_default();
        Self {
            delimiters,
            records: records.iter().map(convert).collect(),
        }
    }

    /// The wire records: one line per record, each ended by CR, every part escaped.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = String::new();
        for record in &self.records {
            encode_record(record, self.delimiters, &mut out);
        }
        out.into_bytes()
    }

    fn count_kind(&self, kind: char) -> usize {
        self.records.iter().filter(|r| r.kind == kind).count()
    }

    fn index_of(&self, kind: char, occurrence: usize) -> Option<usize> {
        self.records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind == kind)
            .nth(occurrence)
            .map(|(i, _)| i)
    }

    /// The repeat and component separators of the message.
    fn separators(&self) -> (char, char) {
        (
            char::from(self.delimiters.repeat()),
            char::from(self.delimiters.component()),
        )
    }

    fn join_components(&self, components: &[String]) -> String {
        components.join(&self.separators().1.to_string())
    }

    fn join_field(&self, field: &Field) -> String {
        let repeat = self.separators().0;
        field
            .iter()
            .map(|components| self.join_components(components))
            .collect::<Vec<_>>()
            .join(&repeat.to_string())
    }

    /// Appends an empty record of `kind` (`H` first, `L` last, anything else before `L`)
    /// and returns its occurrence index.
    fn create_kind(&mut self, kind: char) -> Result<usize, ModelError> {
        let existing = self.count_kind(kind);
        let new = Rec {
            kind,
            fields: Vec::new(),
        };
        match kind {
            'H' | 'L' if existing > 0 => return Err(ModelError::CannotCreate),
            'H' => self.records.insert(0, new),
            'L' => self.records.push(new),
            _ => {
                let at = match self.records.last() {
                    Some(last) if last.kind == 'L' => self.records.len() - 1,
                    _ => self.records.len(),
                };
                self.records.insert(at, new);
            }
        }
        Ok(existing)
    }
}

fn convert(record: &Record) -> Rec {
    let kind = match record.record_type() {
        RecordType::Header => 'H',
        RecordType::Patient => 'P',
        RecordType::Order => 'O',
        RecordType::Result => 'R',
        RecordType::Comment => 'C',
        RecordType::Query => 'Q',
        RecordType::Manufacturer => 'M',
        RecordType::Terminator => 'L',
    };
    let fields = record
        .fields()
        .iter()
        .map(|field| {
            field
                .repeats()
                .iter()
                .map(|repeat| {
                    repeat
                        .components()
                        .iter()
                        .map(|c| c.decoded().to_owned())
                        .collect()
                })
                .collect()
        })
        .collect();
    Rec { kind, fields }
}

fn encode_record(record: &Rec, d: Delimiters, out: &mut String) {
    out.push(record.kind);
    for (index, field) in record.fields.iter().enumerate() {
        out.push(char::from(d.field()));
        if record.kind == 'H' && index == 0 {
            out.push_str(&field.concat().concat());
        } else {
            out.push_str(&encode_field(field, d));
        }
    }
    out.push('\r');
}

fn encode_field(field: &Field, d: Delimiters) -> String {
    let component = char::from(d.component());
    let repeat = char::from(d.repeat());
    let mut out = String::new();
    for (r, components) in field.iter().enumerate() {
        if r > 0 {
            out.push(repeat);
        }
        for (c, text) in components.iter().enumerate() {
            if c > 0 {
                out.push(component);
            }
            out.push_str(&escape(text, d));
        }
    }
    out
}

/// Delimiters become `F`, `S`, `R`, `E` escapes; anything outside printable ASCII
/// becomes a hex escape, so every string is representable.
fn escape(text: &str, d: Delimiters) -> String {
    let escape = char::from(d.escape());
    let mut out = String::with_capacity(text.len());
    let mut hex = String::new();
    for byte in text.bytes() {
        let named = [
            (d.field(), 'F'),
            (d.component(), 'S'),
            (d.repeat(), 'R'),
            (d.escape(), 'E'),
        ]
        .into_iter()
        .find(|(delimiter, _)| *delimiter == byte)
        .map(|(_, code)| code);
        if named.is_none() && !(0x20..=0x7e).contains(&byte) {
            let _ = write!(hex, "{byte:02X}");
            continue;
        }
        flush_hex(&mut out, &mut hex, escape);
        match named {
            Some(code) => {
                out.push(escape);
                out.push(code);
                out.push(escape);
            }
            None => out.push(char::from(byte)),
        }
    }
    flush_hex(&mut out, &mut hex, escape);
    out
}

fn flush_hex(out: &mut String, hex: &mut String, escape: char) {
    if !hex.is_empty() {
        out.push(escape);
        out.push('X');
        out.push_str(hex);
        out.push(escape);
        hex.clear();
    }
}

/// `(kind, field number)` of a path this model can use.
fn address(path: &Path) -> Option<(char, usize)> {
    if !path.is_absolute_in(Syntax::Astm) {
        return None;
    }
    let kind = path.record()?.chars().next()?;
    let FieldRef::Number(number) = path.field() else {
        return None;
    };
    Some((kind, *number))
}

impl Model for AstmModel {
    fn syntax(&self) -> Syntax {
        Syntax::Astm
    }

    fn get(&self, path: &Path) -> Option<Cow<'_, str>> {
        let (kind, number) = address(path)?;
        let record = &self.records[self.index_of(kind, path.occurrence())?];
        if number == 1 {
            return Some(Cow::Owned(kind.to_string()));
        }
        let field = record.fields.get(number - 2)?;
        match (path.repetition(), path.component()) {
            (None, None) => Some(match field.as_slice() {
                [only] if only.len() == 1 => Cow::Borrowed(only[0].as_str()),
                _ => Cow::Owned(self.join_field(field)),
            }),
            (repetition, None) => {
                let components = field.get(repetition.unwrap_or(0))?;
                Some(Cow::Owned(self.join_components(components)))
            }
            (repetition, Some(component)) => field
                .get(repetition.unwrap_or(0))?
                .get(component - 1)
                .map(|text| Cow::Borrowed(text.as_str())),
        }
    }

    fn set(&mut self, path: &Path, value: &str) -> Result<(), ModelError> {
        let (kind, number) = address(path).ok_or(ModelError::WrongSyntax)?;
        if number == 1 || (kind == 'H' && number == 2) {
            return Err(ModelError::Immutable);
        }
        while self.count_kind(kind) <= path.occurrence() {
            self.create_kind(kind)?;
        }
        let index = self
            .index_of(kind, path.occurrence())
            .ok_or(ModelError::CannotCreate)?;
        let (repeat, component) = self.separators();
        let record = &mut self.records[index];
        if record.fields.len() < number - 1 {
            record.fields.resize(number - 1, leaf(""));
        }
        let field = &mut record.fields[number - 2];
        match (path.repetition(), path.component()) {
            (None, None) => {
                *field = value
                    .split(repeat)
                    .map(|part| part.split(component).map(str::to_owned).collect())
                    .collect();
            }
            (repetition, None) => {
                let r = repetition.unwrap_or(0);
                grow(field, r);
                field[r] = value.split(component).map(str::to_owned).collect();
            }
            (repetition, Some(c)) => {
                let r = repetition.unwrap_or(0);
                grow(field, r);
                let components = &mut field[r];
                if components.len() < c {
                    components.resize(c, String::new());
                }
                value.clone_into(&mut components[c - 1]);
            }
        }
        Ok(())
    }

    fn count(&self, record: &str) -> usize {
        let mut chars = record.chars();
        match (chars.next(), chars.next()) {
            (Some(kind), None) => self.count_kind(kind),
            _ => 0,
        }
    }

    fn create(&mut self, record: &str) -> Result<usize, ModelError> {
        if !record_name_ok(Syntax::Astm, record) {
            return Err(ModelError::CannotCreate);
        }
        let kind = record.chars().next().ok_or(ModelError::CannotCreate)?;
        self.create_kind(kind)
    }
}

fn grow(field: &mut Field, repetition: usize) {
    if field.len() <= repetition {
        field.resize(repetition + 1, vec![String::new()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESULT: &[u8] = b"H|\\^&|||CHEM^1.0|||||||P|LIS2-A2|20261012120000\rP|1|PRAC-1|LAB-1\rO|1|SYN-0001||^^^PANEL\rR|1|^^^GLU|5.5|mmol/L|3.9-6.1|N||F||OP||20261012115959\rR|2|^^^NOTE|a&F&b&E&c\\x|\rL|1|N\r";

    fn p(text: &str) -> Path {
        Path::parse(Syntax::Astm, text).expect("valid path")
    }

    fn model() -> AstmModel {
        AstmModel::parse(RESULT).expect("synthetic message parses")
    }

    #[test]
    fn reads_fields_with_standard_numbering() {
        let m = model();
        let cases = [
            ("H.1", Some("H")),
            ("H.2", Some("\\^&")),
            ("H.5", Some("CHEM^1.0")),
            ("H.5.2", Some("1.0")),
            ("P.3", Some("PRAC-1")),
            ("O.3", Some("SYN-0001")),
            ("R.3", Some("^^^GLU")),
            ("R.3.4", Some("GLU")),
            ("R.4", Some("5.5")),
            ("R[0].7", Some("N")),
            ("R[1].4", Some("a|b&c\\x")),
            ("R[2].4", None),
            ("R.99", None),
        ];
        for (path, expected) in cases {
            assert_eq!(m.get(&p(path)).as_deref(), expected, "{path}");
        }
        assert_eq!(m.count("R"), 2);
        assert_eq!(m.count("C"), 0);
        assert_eq!(m.count("RR"), 0);
    }

    #[test]
    fn foreign_paths_read_nothing() {
        let m = model();
        assert_eq!(
            m.get(&Path::parse(Syntax::Hl7, "PID.3").expect("valid")),
            None
        );
        assert_eq!(m.get(&p("$.4")), None);
    }

    #[test]
    fn builds_a_message_between_h_and_l() {
        let mut m = AstmModel::new();
        assert_eq!(m.create("P"), Ok(0));
        assert_eq!(m.create("O"), Ok(0));
        assert_eq!(m.create("R"), Ok(0));
        m.set(&p("P.2"), "1").expect("write");
        m.set(&p("P.3"), "ID|1").expect("write");
        m.set(&p("O.5"), "^^^GLU\\^^^K").expect("write");
        m.set(&p("R[1].4.1"), "x").expect("write");
        m.set(&p("R[0].4"), "a&b").expect("write");
        assert_eq!(m.count("R"), 2);
        assert_eq!(
            String::from_utf8(m.encode()).expect("ascii"),
            "H|\\^&\rP|1|ID&F&1\rO||||^^^GLU\\^^^K\rR|||a&E&b\rR|||x\rL|1|N\r"
        );
    }

    #[test]
    fn fixed_fields_and_unknown_records_are_refused() {
        let mut m = AstmModel::new();
        assert_eq!(m.set(&p("R.1"), "x"), Err(ModelError::Immutable));
        assert_eq!(m.set(&p("H.2"), "x"), Err(ModelError::Immutable));
        assert_eq!(m.create("H"), Err(ModelError::CannotCreate));
        assert_eq!(m.create("L"), Err(ModelError::CannotCreate));
        assert_eq!(m.create("X"), Err(ModelError::CannotCreate));
        assert_eq!(
            m.set(&Path::parse(Syntax::Hl7, "PID.3").expect("valid"), "x"),
            Err(ModelError::WrongSyntax)
        );
    }

    #[test]
    fn encoding_a_parsed_message_reproduces_its_records() {
        assert_eq!(model().encode(), RESULT.to_vec());
    }

    #[test]
    fn non_ascii_and_control_text_is_hex_escaped_and_parses_back() {
        let mut m = AstmModel::new();
        m.set(&p("P.3"), "Ünï\nx").expect("write");
        let encoded = m.encode();
        let back = AstmModel::parse(&encoded).expect("encoded text parses");
        assert_eq!(back.get(&p("P.3")).as_deref(), Some("Ünï\nx"));
    }
}
