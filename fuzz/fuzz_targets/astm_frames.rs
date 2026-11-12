//! ASTM low-level frames and records on arbitrary bytes: parse_frame, parse_message_records,
//! parse_record (delimiters from the first input byte) and assemble_message, plus the encoder
//! round trip for the frame text. Typed accessors are walked so lazy paths run too.
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_astm::{
    Delimiters, FrameEnd, Message, Record, assemble_message, encode_frame, encode_records,
    parse_frame, parse_message_records, parse_record,
};

fn walk(record: &Record) {
    let _ = record.sequence_number();
    for field in record.fields() {
        let _ = (field.raw(), field.decoded());
        for repeat in field.repeats() {
            let _ = repeat.raw();
            for component in repeat.components() {
                let _ = (component.raw(), component.decoded());
            }
        }
    }
    let _ = record.field(1);
    let _ = record.field(usize::MAX);
    if let Some(header) = record.as_header() {
        let _ = (
            header.sender_name(),
            header.version(),
            header.timestamp_text(),
        );
    }
    if let Some(patient) = record.as_patient() {
        let _ = (patient.sequence(), patient.laboratory_assigned_id());
    }
    if let Some(order) = record.as_order() {
        let _ = (order.specimen_id(), order.universal_test_id_components());
    }
    if let Some(result) = record.as_result() {
        let _ = (result.test_code(), result.value_text(), result.units());
        let _ = result.completion_timestamp_text();
    }
    if let Some(query) = record.as_query() {
        let _ = (query.patient_id(), query.starting_range_components());
    }
    if let Some(comment) = record.as_comment() {
        let _ = (comment.source(), comment.text(), comment.comment_type());
    }
}

fn walk_message(message: &Message) {
    walk(message.header());
    walk(message.terminator());
    message.queries().iter().for_each(walk);
    message.manufacturers().iter().for_each(walk);
    for patient in message.patients() {
        walk(patient.record());
        patient.comments().iter().for_each(walk);
        for order in patient.orders() {
            walk(order.record());
            order.comments().iter().for_each(walk);
            for result in order.results() {
                walk(result.record());
                result.comments().iter().for_each(walk);
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(frame) = parse_frame(data) {
        assert_eq!(frame.raw(), data);
        let again = encode_frame(frame.number(), frame.text(), frame.end())
            .expect("a parsed frame re-encodes");
        // Lowercase checksum digits are rejected by the parser, so the encoding is exact.
        assert_eq!(again, data);
    }

    if let Ok(records) = parse_message_records(data) {
        records.iter().for_each(walk);
        if let Ok(message) = assemble_message(records) {
            walk_message(&message);
        }
    }

    if let Some((&first, rest)) = data.split_first() {
        let delimiters = match first & 3 {
            0 => Ok(Delimiters::default()),
            1 => Delimiters::new(b'|', b'\\', b'^', b'&'),
            2 => Delimiters::new(b'!', b'@', b'#', b'$'),
            _ => Delimiters::new(rest.first().copied().unwrap_or(0), b'~', b'^', b'&'),
        };
        if let Ok(delimiters) = delimiters
            && let Ok(record) = parse_record(rest, delimiters)
        {
            walk(&record);
        }
    }

    // Any record text that frames must frame to valid frames that parse back.
    let records: Vec<Vec<u8>> = data
        .split(|byte| *byte == b'\r')
        .map(<[u8]>::to_vec)
        .collect();
    if let Ok(frames) = encode_records(&records) {
        let mut text = Vec::new();
        for raw in &frames {
            let frame = parse_frame(raw).expect("an encoded frame parses");
            text.extend_from_slice(frame.text());
            assert_eq!(
                frame.end() == FrameEnd::Final,
                frame.text().last() == Some(&b'\r')
            );
        }
        let mut expected = Vec::new();
        for record in &records {
            expected.extend_from_slice(record);
            expected.push(b'\r');
        }
        assert_eq!(text, expected);
    }
});
