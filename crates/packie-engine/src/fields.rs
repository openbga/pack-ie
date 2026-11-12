//! The parsed fields of a stored message's original (`GetMessage`): every non-empty leaf
//! by the path filters and mappings use (`docs/spec/configuration.md` §8), in message
//! order. Values are message content and are never logged; only paths are, at debug level.

use std::collections::HashMap;

use packie_core::{ProtocolKind, hex_dump_bytes};
use packie_ipc::messages::{
    BodyFormat, BodyRequest, BodySelector, FieldValue, MAX_FIELDS, MessageRecord, ParsedFields,
};
use packie_transform::{AstmModel, Hl7Model, Model, Path};

use crate::registry::ChannelRegistry;

const NO_LIST: &str = "Fields are listed for HL7 and ASTM messages.";
const NO_BODY: &str = "The original body is not available in full.";

/// Repetitions, each a list of components, each a list of subcomponent texts.
type Tree<'a> = Vec<Vec<Vec<&'a str>>>;

/// The fields of `record`'s original, or why they are not listed.
pub(crate) async fn of(registry: &dyn ChannelRegistry, record: &MessageRecord) -> ParsedFields {
    let protocol = record.summary.protocol;
    if !matches!(protocol, ProtocolKind::Hl7 | ProtocolKind::Astm) {
        return unavailable(NO_LIST.to_owned());
    }
    match original(registry, record).await {
        Ok(bytes) => of_bytes(protocol, &bytes),
        Err(_) => unavailable(NO_BODY.to_owned()),
    }
}

/// The fields of the original `bytes` of a message of `protocol` (HL7 or ASTM).
pub(crate) fn of_bytes(protocol: ProtocolKind, bytes: &[u8]) -> ParsedFields {
    let mut collector = Collector::default();
    let parsed = match protocol {
        ProtocolKind::Hl7 => Hl7Model::parse(bytes)
            .map(|model| hl7_fields(&model, &mut collector))
            .is_ok(),
        ProtocolKind::Astm => packie_astm::parse_message_records(bytes)
            .map(|records| astm_fields(&records, &mut collector))
            .is_ok(),
        _ => return unavailable(NO_LIST.to_owned()),
    };
    if !parsed {
        let name = if protocol == ProtocolKind::Hl7 {
            "HL7"
        } else {
            "ASTM"
        };
        return unavailable(format!(
            "The original does not parse as {name}; see the message's reason."
        ));
    }
    ParsedFields {
        fields: collector.fields,
        truncated: collector.truncated,
        unavailable: None,
    }
}

fn unavailable(reason: String) -> ParsedFields {
    ParsedFields {
        fields: Vec::new(),
        truncated: false,
        unavailable: Some(reason),
    }
}

/// The complete original bytes, or why the store cannot give them.
pub(crate) async fn original(
    registry: &dyn ChannelRegistry,
    record: &MessageRecord,
) -> Result<Vec<u8>, String> {
    let body = registry
        .message_body(BodyRequest {
            message_id: record.summary.message_id,
            which: BodySelector::Original,
            format: BodyFormat::Hex,
        })
        .await
        .map_err(|e| e.to_string())?;
    let incomplete = || "the stored body is incomplete".to_owned();
    if body.truncated {
        return Err(incomplete());
    }
    let bytes = hex_dump_bytes(&body.content).ok_or_else(incomplete)?;
    if u64::try_from(bytes.len()).ok() == Some(body.byte_length) {
        Ok(bytes)
    } else {
        Err(incomplete())
    }
}

/// Collects verified fields up to [`MAX_FIELDS`].
#[derive(Default)]
struct Collector {
    fields: Vec<FieldValue>,
    truncated: bool,
}

impl Collector {
    /// Lists `path` when `value` is not empty and `model` reads the same value at the path
    /// (so a filter or mapping can use it).
    fn push(&mut self, model: &dyn Model, path: String, value: &str) {
        if value.is_empty() || self.truncated {
            return;
        }
        let reads_back = Path::parse(model.syntax(), &path)
            .ok()
            .and_then(|parsed| model.get(&parsed).map(|read| read == value))
            .unwrap_or(false);
        if !reads_back {
            tracing::debug!(path = %path, "field not listed: the path does not read back its value");
            return;
        }
        if self.fields.len() >= MAX_FIELDS {
            self.truncated = true;
            return;
        }
        self.fields.push(FieldValue {
            path,
            value: value.to_owned(),
        });
    }
}

/// Segment, record or field name with the occurrence suffix, none for the first.
fn with_occurrence(name: &str, occurrence: usize) -> String {
    if occurrence == 0 {
        name.to_owned()
    } else {
        format!("{name}[{occurrence}]")
    }
}

fn hl7_fields(model: &Hl7Model, out: &mut Collector) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for segment in model.message().segments() {
        let count = seen.entry(segment.name()).or_default();
        let name = with_occurrence(segment.name(), *count);
        *count += 1;
        // MSH.1 and MSH.2 are the delimiters.
        let skip = if segment.name() == "MSH" { 2 } else { 0 };
        for (index, field) in segment.fields().iter().enumerate().skip(skip) {
            let tree = hl7_tree(field);
            leaves(model, &format!("{name}.{}", index + 1), &tree, out);
        }
        if out.truncated {
            return;
        }
    }
}

fn hl7_tree(field: &packie_hl7::Field) -> Tree<'_> {
    field
        .repetitions()
        .iter()
        .map(|repetition| repetition.components().iter().map(hl7_subs).collect())
        .collect()
}

fn hl7_subs(component: &packie_hl7::Component) -> Vec<&str> {
    component
        .subcomponents()
        .iter()
        .map(|sub| sub.value().unwrap_or_default())
        .collect()
}

fn astm_tree(field: &packie_astm::Field) -> Tree<'_> {
    field
        .repeats()
        .iter()
        .map(|repeat| {
            let components = repeat.components();
            components.iter().map(|c| vec![c.decoded()]).collect()
        })
        .collect()
}

fn astm_fields(records: &[packie_astm::Record], out: &mut Collector) {
    let model = AstmModel::from_records(records);
    let mut seen: HashMap<u8, usize> = HashMap::new();
    for record in records {
        let Some(kind) = record.raw_bytes().first().copied() else {
            continue;
        };
        let count = seen.entry(kind).or_default();
        let name = with_occurrence(&char::from(kind).to_string(), *count);
        *count += 1;
        // H.2 holds the delimiters; field 1 is the record type and is not in `fields()`.
        let skip = usize::from(kind == b'H');
        for (index, field) in record.fields().iter().enumerate().skip(skip) {
            let tree = astm_tree(field);
            leaves(&model, &format!("{name}.{}", index + 2), &tree, out);
        }
        if out.truncated {
            return;
        }
    }
}

/// Lists the leaves of one field under `prefix`, the deepest level that the field has:
/// a repetition of one component of one subcomponent is addressed by the field itself
/// (with `[n]` when there are several repetitions), a component of one subcomponent by
/// `.component`, otherwise `.component.subcomponent`; repetition 0 is left out when a
/// component follows.
fn leaves(model: &dyn Model, prefix: &str, repetitions: &Tree, out: &mut Collector) {
    let several = repetitions.len() > 1;
    for (r, components) in repetitions.iter().enumerate() {
        if let [subs] = components.as_slice()
            && let [value] = subs.as_slice()
        {
            let path = if several {
                format!("{prefix}[{r}]")
            } else {
                prefix.to_owned()
            };
            out.push(model, path, value);
            continue;
        }
        let repetition = if r == 0 {
            String::new()
        } else {
            format!("[{r}]")
        };
        for (c, subs) in components.iter().enumerate() {
            let base = format!("{prefix}{repetition}.{}", c + 1);
            if let [value] = subs.as_slice() {
                out.push(model, base, value);
                continue;
            }
            for (s, value) in subs.iter().enumerate() {
                out.push(model, format!("{base}.{}", s + 1), value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use packie_transform::Syntax;

    const ORU: &str = "MSH|^~\\&|LAB|FAC|LIS|FAC2|20260101120000||ORU^R01|CTRL-00001|P|2.5.1\r\
PID|1||MRN-00000^^^HOSP~ALT-11^^^OTH||TEST^PATIENT||19700101|F\r\
OBR|1|||GLU^Glucose^L\r\
OBX|1|NM|GLU^Glucose^L||5.5|mmol/L|3.9-6.1|N\r\
OBX|2|ST|NOTE||a&b^c~d\r";

    const ASTM: &str = "H|\\^&|||LAB^1.0\r\
P|1||PAT-00001||TEST^PATIENT\r\
O|1|SMP-00001||^^^GLU\r\
R|1|^^^GLU|5.5|mmol/L\r\
R|2|^^^NA^Sodium|140|mmol/L\r\
L|1|N\r";

    fn listed(protocol: ProtocolKind, text: &str) -> ParsedFields {
        of_bytes(protocol, text.as_bytes())
    }

    fn value<'a>(fields: &'a ParsedFields, path: &str) -> Option<&'a str> {
        fields
            .fields
            .iter()
            .find(|f| f.path == path)
            .map(|f| f.value.as_str())
    }

    #[test]
    fn hl7_paths_follow_the_transform_syntax() {
        let fields = listed(ProtocolKind::Hl7, ORU);
        assert_eq!(fields.unavailable, None);
        assert!(!fields.truncated);
        let expected = [
            ("MSH.3", "LAB"),
            ("MSH.9.1", "ORU"),
            ("MSH.9.2", "R01"),
            ("MSH.10", "CTRL-00001"),
            ("PID.1", "1"),
            ("PID.3.1", "MRN-00000"),
            ("PID.3.4", "HOSP"),
            ("PID.3[1].1", "ALT-11"),
            ("PID.3[1].4", "OTH"),
            ("PID.5.2", "PATIENT"),
            ("OBR.4.2", "Glucose"),
            ("OBX.5", "5.5"),
            ("OBX.6", "mmol/L"),
            ("OBX[1].5.1.1", "a"),
            ("OBX[1].5.1.2", "b"),
            ("OBX[1].5.2", "c"),
            ("OBX[1].5[1]", "d"),
        ];
        for (path, text) in expected {
            assert_eq!(value(&fields, path), Some(text), "{path}");
        }
        // Delimiters and empty values are not listed; order is message order.
        assert_eq!(value(&fields, "MSH.1"), None);
        assert_eq!(value(&fields, "MSH.2"), None);
        assert_eq!(value(&fields, "MSH.8"), None);
        assert_eq!(fields.fields[0].path, "MSH.3");
        assert!(fields.fields.iter().all(|f| !f.value.is_empty()));
    }

    #[test]
    fn astm_paths_follow_the_transform_syntax() {
        let fields = listed(ProtocolKind::Astm, ASTM);
        assert_eq!(fields.unavailable, None);
        let expected = [
            ("H.5.1", "LAB"),
            ("H.5.2", "1.0"),
            ("P.2", "1"),
            ("P.4", "PAT-00001"),
            ("P.6.1", "TEST"),
            ("P.6.2", "PATIENT"),
            ("O.3", "SMP-00001"),
            ("O.5.4", "GLU"),
            ("R.3.4", "GLU"),
            ("R.4", "5.5"),
            ("R[1].3.4", "NA"),
            ("R[1].3.5", "Sodium"),
            ("R[1].4", "140"),
            ("L.2", "1"),
            ("L.3", "N"),
        ];
        for (path, text) in expected {
            assert_eq!(value(&fields, path), Some(text), "{path}");
        }
        assert_eq!(value(&fields, "H.2"), None);
        assert_eq!(value(&fields, "R.1"), None);
    }

    #[test]
    fn every_listed_path_reads_back_through_the_model() {
        let hl7 = Hl7Model::parse(ORU.as_bytes()).unwrap();
        for f in listed(ProtocolKind::Hl7, ORU).fields {
            let path = Path::parse(Syntax::Hl7, &f.path).unwrap();
            assert_eq!(
                hl7.get(&path).as_deref(),
                Some(f.value.as_str()),
                "{}",
                f.path
            );
        }
        let astm = AstmModel::parse(ASTM.as_bytes()).unwrap();
        for f in listed(ProtocolKind::Astm, ASTM).fields {
            let path = Path::parse(Syntax::Astm, &f.path).unwrap();
            assert_eq!(
                astm.get(&path).as_deref(),
                Some(f.value.as_str()),
                "{}",
                f.path
            );
        }
    }

    #[test]
    fn listing_stops_at_the_maximum() {
        let mut text = ORU.to_owned();
        for n in 0..MAX_FIELDS {
            text.push_str(&format!("NTE|{n}|L|note {n}\r"));
        }
        let fields = listed(ProtocolKind::Hl7, &text);
        assert_eq!(fields.fields.len(), MAX_FIELDS);
        assert!(fields.truncated);
        assert_eq!(fields.unavailable, None);
        assert_eq!(fields.fields[0].path, "MSH.3");
    }

    #[test]
    fn exactly_the_maximum_is_not_truncated() {
        let header = "MSH|^~\\&|LAB|FAC|LIS|FAC2|20260101120000||ORU^R01|CTRL-00001|P|2.5.1\r";
        // MSH lists 10 fields; every NTE lists 1.
        let mut text = header.to_owned();
        for n in 0..(MAX_FIELDS - 10) {
            text.push_str(&format!("NTE|{}\r", n + 1));
        }
        let fields = listed(ProtocolKind::Hl7, &text);
        assert_eq!(fields.fields.len(), MAX_FIELDS);
        assert!(!fields.truncated);
    }

    #[test]
    fn unparseable_input_is_unavailable_without_content() {
        for (protocol, name) in [(ProtocolKind::Hl7, "HL7"), (ProtocolKind::Astm, "ASTM")] {
            let fields = listed(protocol, "not a TEST^PATIENT message");
            assert!(fields.fields.is_empty());
            assert!(!fields.truncated);
            assert_eq!(
                fields.unavailable.as_deref(),
                Some(
                    format!("The original does not parse as {name}; see the message's reason.")
                        .as_str()
                )
            );
        }
    }

    #[test]
    fn other_protocols_have_no_field_list() {
        let fields = of_bytes(ProtocolKind::Au, b"anything");
        assert_eq!(fields.unavailable.as_deref(), Some(NO_LIST));
    }
}
