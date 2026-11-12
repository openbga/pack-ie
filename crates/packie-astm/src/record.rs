use std::str;

use crate::ascii::is_printable;
use crate::escape::decode_escapes;
use crate::frame::CR;
use crate::record_model::{Component, Delimiters, Field, Record, RecordError, RecordType, Repeat};

/// Parses CR-separated message bytes using delimiters from the leading header.
pub fn parse_message_records(bytes: &[u8]) -> Result<Vec<Record>, RecordError> {
    if bytes.is_empty() {
        return Err(RecordError::EmptyRecord);
    }
    if bytes.last() != Some(&CR) {
        return Err(RecordError::MissingRecordTerminator);
    }
    let content = &bytes[..bytes.len() - 1];
    let mut raw_records = content.split(|byte| *byte == CR);
    let header = raw_records.next().ok_or(RecordError::EmptyRecord)?;
    if header.is_empty() {
        return Err(RecordError::EmptyRecord);
    }
    let delimiters = Delimiters::from_header(header)?;
    let mut records = vec![parse_record(header, delimiters)?];
    for raw in raw_records {
        if raw.is_empty() {
            return Err(RecordError::EmptyRecord);
        }
        records.push(parse_record(raw, delimiters)?);
    }
    Ok(records)
}

/// Parses one unterminated record using an already established delimiter set.
pub fn parse_record(raw: &[u8], delimiters: Delimiters) -> Result<Record, RecordError> {
    let Some(record_byte) = raw.first().copied() else {
        return Err(RecordError::EmptyRecord);
    };
    if let Some(offset) = raw.iter().position(|byte| !is_printable(*byte)) {
        return Err(RecordError::DisallowedByte { offset });
    }
    let record_type = RecordType::parse(record_byte)?;
    if raw.get(1).is_some_and(|byte| *byte != delimiters.field()) {
        return Err(RecordError::InvalidDelimiters);
    }
    let raw_text = str::from_utf8(raw).map_err(|_| RecordError::DisallowedByte { offset: 0 })?;
    let fields = raw
        .split(|byte| *byte == delimiters.field())
        .skip(1)
        .enumerate()
        .map(|(index, field)| {
            if record_type == RecordType::Header && index == 0 {
                Ok(literal_field(field))
            } else {
                parse_field(field, delimiters)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Record {
        kind: record_type,
        raw: raw_text.to_owned(),
        fields,
    })
}

fn literal_field(raw: &[u8]) -> Field {
    let text = ascii_string(raw);
    Field {
        raw: text.clone(),
        decoded: text.clone(),
        repeats: vec![Repeat {
            raw: text.clone(),
            components: vec![Component {
                raw: text.clone(),
                decoded: text,
            }],
        }],
    }
}

fn parse_field(raw: &[u8], delimiters: Delimiters) -> Result<Field, RecordError> {
    let repeats = raw
        .split(|byte| *byte == delimiters.repeat())
        .map(|repeat| parse_repeat(repeat, delimiters))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Field {
        raw: ascii_string(raw),
        decoded: decode_escapes(raw, delimiters)?,
        repeats,
    })
}

fn parse_repeat(raw: &[u8], delimiters: Delimiters) -> Result<Repeat, RecordError> {
    let components = raw
        .split(|byte| *byte == delimiters.component())
        .map(|component| {
            Ok(Component {
                raw: ascii_string(component),
                decoded: decode_escapes(component, delimiters)?,
            })
        })
        .collect::<Result<Vec<_>, RecordError>>()?;
    Ok(Repeat {
        raw: ascii_string(raw),
        components,
    })
}

fn ascii_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn custom_delimiters_and_escapes_decode_without_losing_raw_text() {
        let bytes = b"H!@$%!!!SYNTH-ANALYZER@2.0!!!!!!!P!LIS2-A2!20261012120000\rC!1!I!alpha%F%beta%S%gamma%R%delta%E%%X21%!G\rL!1!N\r";
        let records = parse_message_records(bytes).expect("synthetic message parses");
        assert_eq!(
            records[0].raw(),
            "H!@$%!!!SYNTH-ANALYZER@2.0!!!!!!!P!LIS2-A2!20261012120000"
        );
        let comment = records[1].as_comment().expect("comment view exists");
        assert_eq!(comment.text(), Some("alpha!beta$gamma@delta%!"));
        assert_eq!(
            records[1].field(3).expect("comment text field").raw(),
            "alpha%F%beta%S%gamma%R%delta%E%%X21%"
        );
    }

    #[test]
    fn result_value_remains_lexical_text() {
        let record = parse_record(
            b"R|1|^^^SYN-TEST|<0.0100|ng/mL|0.0000-1.0000|L||F||SYN-OP||20261012115959|SYN-INST",
            Delimiters::default(),
        )
        .expect("synthetic result parses");
        let result = record.as_result().expect("result view exists");
        assert_eq!(result.value_text(), Some("<0.0100"));
        assert_eq!(result.reference_range_text(), Some("0.0000-1.0000"));
        assert_eq!(result.completion_timestamp_text(), Some("20261012115959"));
    }

    fn typed_records() -> Vec<Record> {
        let bytes = b"H|\\^&|||SYNTH-ANALYZER^1.0|||||||P|LIS2-A2|20261012120000\rP|1|SYN-PRACTICE|SYN-LAB\rO|1|SYN-0001|SYN-INST-0001|^^^SYN-TEST|R||||||N||||URINE\rR|1|^^^SYN-TEST|<0.0100|ng/mL|0.0000-1.0000|L||F||SYN-OP||20261012115959|SYN-INST\rC|1|I|Synthetic comment|G\rL|1|N\r";
        parse_message_records(bytes).expect("synthetic message parses")
    }

    #[test]
    fn typed_header_accessors_use_standard_positions() {
        let records = typed_records();
        let header = records[0].as_header().expect("header view exists");
        assert_eq!(header.sender_name(), Some("SYNTH-ANALYZER"));
        assert_eq!(header.sender_version(), Some("1.0"));
        assert_eq!(header.processing_id(), Some("P"));
        assert_eq!(header.version(), Some("LIS2-A2"));
        assert_eq!(header.timestamp_text(), Some("20261012120000"));
    }

    #[test]
    fn typed_patient_and_order_accessors_use_standard_positions() {
        let records = typed_records();
        let patient = records[1].as_patient().expect("patient view exists");
        assert_eq!(patient.practice_assigned_id(), Some("SYN-PRACTICE"));
        assert_eq!(patient.laboratory_assigned_id(), Some("SYN-LAB"));

        let order = records[2].as_order().expect("order view exists");
        assert_eq!(order.specimen_id(), Some("SYN-0001"));
        assert_eq!(order.instrument_specimen_id(), Some("SYN-INST-0001"));
        assert_eq!(order.priority(), Some("R"));
        assert_eq!(order.action_code(), Some("N"));
        assert_eq!(order.specimen_descriptor(), Some("URINE"));
    }

    #[test]
    fn typed_result_accessor_preserves_lexical_fields() {
        let records = typed_records();
        let result = records[3].as_result().expect("result view exists");
        assert_eq!(result.value_text(), Some("<0.0100"));
        assert_eq!(result.units(), Some("ng/mL"));
        assert_eq!(result.reference_range_text(), Some("0.0000-1.0000"));
        assert_eq!(result.abnormal_flags(), Some("L"));
        assert_eq!(result.result_status(), Some("F"));
        assert_eq!(result.operator(), Some("SYN-OP"));
        assert_eq!(result.completion_timestamp_text(), Some("20261012115959"));
        assert_eq!(result.instrument_id(), Some("SYN-INST"));
    }

    #[test]
    fn typed_comment_accessors_use_standard_positions() {
        let records = typed_records();
        let comment = records[4].as_comment().expect("comment view exists");
        assert_eq!(comment.source(), Some("I"));
        assert_eq!(comment.text(), Some("Synthetic comment"));
        assert_eq!(comment.comment_type(), Some("G"));
    }

    #[test]
    fn malformed_escape_is_rejected() {
        assert_eq!(
            parse_record(b"C|1|I|bad&X1&", Delimiters::default()),
            Err(RecordError::MalformedEscape)
        );
    }

    #[test]
    fn empty_record_inside_message_is_rejected() {
        assert_eq!(
            parse_message_records(b"H|\\^&|||SYNTH-ANALYZER^1.0\r\rL|1|N\r"),
            Err(RecordError::EmptyRecord)
        );
    }

    proptest! {
        #[test]
        fn arbitrary_record_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
            let _ = parse_record(&bytes, Delimiters::default());
            let _ = parse_message_records(&bytes);
        }
    }
}
