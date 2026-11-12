use super::*;
use packie_astm::{CR, encode_frame, encode_records};

const SMALL: usize = 4096;

fn run(kind: ProtocolKind, max: usize, chunks: &[&[u8]]) -> Vec<Observation> {
    let mut framer = framer_for(kind, max).expect("framer");
    let mut out = Vec::new();
    for chunk in chunks {
        framer.feed(chunk, &mut out);
    }
    out
}

fn whole(kind: ProtocolKind, stream: &[u8]) -> Vec<Observation> {
    run(kind, SMALL, &[stream])
}

fn bytewise(kind: ProtocolKind, stream: &[u8]) -> Vec<Observation> {
    let chunks: Vec<&[u8]> = stream.chunks(1).collect();
    run(kind, SMALL, &chunks)
}

/// The stream yields the same observations whole, byte by byte and split at every offset.
fn assert_chunk_invariant(kind: ProtocolKind, stream: &[u8]) -> Vec<Observation> {
    let expected = whole(kind, stream);
    assert_eq!(bytewise(kind, stream), expected, "byte by byte");
    for split in 0..=stream.len() {
        let (a, b) = stream.split_at(split);
        assert_eq!(run(kind, SMALL, &[a, b]), expected, "split at {split}");
    }
    expected
}

fn message(bytes: &[u8]) -> Observation {
    Observation::Message(bytes.to_vec())
}

// ----- HL7 -----

const HL7_A: &[u8] = b"MSH|^~\\&|A|B\rPID|1\r";
const HL7_B: &[u8] = b"MSH|^~\\&|C|D\rPID|2\r";

fn mllp(payload: &[u8]) -> Vec<u8> {
    packie_hl7::mllp::encode(payload, SMALL).unwrap()
}

#[test]
fn hl7_complete_message_counted_once() {
    let out = assert_chunk_invariant(ProtocolKind::Hl7, &mllp(HL7_A));
    assert_eq!(out, vec![message(HL7_A)]);
}

#[test]
fn hl7_two_messages_back_to_back() {
    let mut stream = mllp(HL7_A);
    stream.extend(mllp(HL7_B));
    let out = assert_chunk_invariant(ProtocolKind::Hl7, &stream);
    assert_eq!(out, vec![message(HL7_A), message(HL7_B)]);
}

#[test]
fn hl7_garbage_is_one_error_and_recovers() {
    let mut stream = b"noise".to_vec();
    stream.extend(mllp(HL7_A));
    let out = assert_chunk_invariant(ProtocolKind::Hl7, &stream);
    assert_eq!(out, vec![Observation::FramingError, message(HL7_A)]);
}

#[test]
fn hl7_malformed_trailer_is_one_error_and_recovers() {
    let mut stream = vec![0x0b];
    stream.extend_from_slice(b"MSH|x");
    stream.extend_from_slice(&[0x1c, b'X']);
    stream.extend(mllp(HL7_A));
    let out = assert_chunk_invariant(ProtocolKind::Hl7, &stream);
    assert_eq!(out, vec![Observation::FramingError, message(HL7_A)]);
}

#[test]
fn hl7_nested_start_is_one_error_and_recovers() {
    let mut stream = vec![0x0b];
    stream.extend_from_slice(b"MSH|partial");
    stream.extend(mllp(HL7_B));
    let out = assert_chunk_invariant(ProtocolKind::Hl7, &stream);
    assert_eq!(out, vec![Observation::FramingError, message(HL7_B)]);
}

#[test]
fn hl7_overlong_message_is_one_error_and_resynchronizes() {
    let mut stream = vec![0x0b];
    stream.extend(std::iter::repeat_n(b'A', 500));
    stream.extend_from_slice(&[0x1c, 0x0d]);
    stream.extend(mllp(HL7_A));
    let expected = vec![Observation::FramingError, message(HL7_A)];
    assert_eq!(run(ProtocolKind::Hl7, 64, &[&stream]), expected);
    let chunks: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(run(ProtocolKind::Hl7, 64, &chunks), expected);
}

// ----- ASTM -----

fn astm_records() -> Vec<Vec<u8>> {
    vec![
        b"H|\\^&|||Instrument|||||||P|1".to_vec(),
        b"P|1||PID1".to_vec(),
        b"O|1|SID1||^^^GLU".to_vec(),
        b"R|1|^^^GLU|5.5|mmol/L".to_vec(),
        b"L|1|N".to_vec(),
    ]
}

fn record_bytes(records: &[Vec<u8>]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|record| record.iter().copied().chain([CR]))
        .collect()
}

fn astm_stream(records: &[Vec<u8>]) -> Vec<u8> {
    let mut stream = vec![ENQ];
    for frame in encode_records(records).unwrap() {
        stream.extend(frame);
        stream.push(ACK);
    }
    stream.push(EOT);
    stream
}

#[test]
fn astm_complete_message_counted_once() {
    let records = astm_records();
    let out = assert_chunk_invariant(ProtocolKind::Astm, &astm_stream(&records));
    assert_eq!(out, vec![message(&record_bytes(&records))]);
}

#[test]
fn astm_two_messages_back_to_back() {
    let first = astm_records();
    let mut second = astm_records();
    second[1] = b"P|1||PID2".to_vec();
    let mut stream = astm_stream(&first);
    stream.extend(astm_stream(&second));
    let out = assert_chunk_invariant(ProtocolKind::Astm, &stream);
    assert_eq!(
        out,
        vec![
            message(&record_bytes(&first)),
            message(&record_bytes(&second))
        ]
    );
}

#[test]
fn astm_multi_frame_record_counted_once() {
    let mut records = astm_records();
    records[3] = [b"R|1|^^^GLU|".as_slice(), &[b'9'; 500]].concat();
    let frames = encode_records(&records).unwrap();
    let parsed = parse_frame(&frames[3]).unwrap();
    assert_eq!(parsed.end(), FrameEnd::Intermediate);
    let out = assert_chunk_invariant(ProtocolKind::Astm, &astm_stream(&records));
    assert_eq!(out, vec![message(&record_bytes(&records))]);
}

#[test]
fn astm_retransmitted_frame_is_not_counted_twice() {
    let records = astm_records();
    let frames = encode_records(&records).unwrap();
    let mut stream = vec![ENQ];
    for (index, frame) in frames.iter().enumerate() {
        stream.extend(frame);
        if index == 1 || index == frames.len() - 1 {
            // The ACK was lost, so the sender transmits the same frame again.
            stream.push(NAK);
            stream.extend(frame);
        }
        stream.push(ACK);
    }
    stream.push(EOT);
    let out = assert_chunk_invariant(ProtocolKind::Astm, &stream);
    assert_eq!(out, vec![message(&record_bytes(&records))]);
}

#[test]
fn astm_checksum_error_is_one_error_and_resend_recovers() {
    let records = astm_records();
    let frames = encode_records(&records).unwrap();
    let mut bad = frames[1].clone();
    let checksum_at = bad.len() - 3;
    bad[checksum_at] = if bad[checksum_at] == b'0' { b'1' } else { b'0' };
    let mut stream = vec![ENQ];
    stream.extend(&frames[0]);
    stream.extend(&bad);
    stream.push(NAK);
    for frame in &frames[1..] {
        stream.extend(frame);
    }
    stream.push(EOT);
    let out = assert_chunk_invariant(ProtocolKind::Astm, &stream);
    assert_eq!(
        out,
        vec![Observation::FramingError, message(&record_bytes(&records))]
    );
}

#[test]
fn astm_stray_bytes_count_one_error_per_run() {
    let records = astm_records();
    let mut stream = b"garbage".to_vec();
    stream.extend(astm_stream(&records));
    stream.extend_from_slice(b"more noise");
    stream.extend(astm_stream(&records));
    let out = assert_chunk_invariant(ProtocolKind::Astm, &stream);
    let expected_message = message(&record_bytes(&records));
    assert_eq!(
        out,
        vec![
            Observation::FramingError,
            expected_message.clone(),
            Observation::FramingError,
            expected_message
        ]
    );
}

#[test]
fn astm_abandoned_frame_is_one_error_and_partial_message_is_dropped() {
    let records = astm_records();
    let frames = encode_records(&records).unwrap();
    let mut stream = vec![ENQ];
    stream.extend(&frames[0]);
    stream.extend_from_slice(&[STX, b'2', b'P']);
    stream.push(EOT);
    stream.extend(astm_stream(&records));
    let out = assert_chunk_invariant(ProtocolKind::Astm, &stream);
    assert_eq!(
        out,
        vec![Observation::FramingError, message(&record_bytes(&records))]
    );
}

#[test]
fn astm_eot_discards_partial_message() {
    let records = astm_records();
    let frames = encode_records(&records).unwrap();
    let mut stream = vec![ENQ];
    stream.extend(&frames[0]);
    stream.extend(&frames[1]);
    stream.push(EOT);
    let out = assert_chunk_invariant(ProtocolKind::Astm, &stream);
    assert!(out.is_empty());
}

#[test]
fn astm_overlong_message_is_one_error_and_resynchronizes() {
    let mut long = astm_records();
    for number in 0..20 {
        long.insert(
            3,
            format!("R|{number}|^^^GLU|{}", "9".repeat(40)).into_bytes(),
        );
    }
    let ok = astm_records();
    let mut stream = astm_stream(&long);
    stream.extend(astm_stream(&ok));
    let expected = vec![Observation::FramingError, message(&record_bytes(&ok))];
    assert_eq!(run(ProtocolKind::Astm, 256, &[&stream]), expected);
    let chunks: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(run(ProtocolKind::Astm, 256, &chunks), expected);
}

#[test]
fn astm_overlong_frame_without_end_is_one_error_and_resynchronizes() {
    let ok = astm_records();
    let mut stream = vec![ENQ, STX, b'1'];
    stream.extend(std::iter::repeat_n(b'A', 2000));
    stream.extend(astm_stream(&ok));
    let out = assert_chunk_invariant(ProtocolKind::Astm, &stream);
    assert_eq!(
        out,
        vec![Observation::FramingError, message(&record_bytes(&ok))]
    );
}

#[test]
fn astm_frame_text_is_not_a_message_without_terminator_record() {
    let frame = encode_frame(1, b"H|\\^&\r", FrameEnd::Final).unwrap();
    let mut stream = vec![ENQ];
    stream.extend(frame);
    stream.push(EOT);
    assert!(assert_chunk_invariant(ProtocolKind::Astm, &stream).is_empty());
}

// ----- AU -----

fn au(text: &[u8], end: packie_au::FrameEnd) -> Vec<u8> {
    packie_au::encode_frame(&au_framing_config(), text, end).unwrap()
}

const AU_A: &[u8] = b"R 0001000000123456789";
const AU_B: &[u8] = b"D 0002000000987654321";

#[test]
fn au_complete_message_counted_once() {
    let out = assert_chunk_invariant(ProtocolKind::Au, &au(AU_A, packie_au::FrameEnd::Text));
    assert_eq!(out, vec![message(AU_A)]);
}

#[test]
fn au_two_messages_and_control_bytes() {
    let mut stream = au(AU_A, packie_au::FrameEnd::Text);
    stream.push(ACK);
    stream.extend(au(AU_B, packie_au::FrameEnd::Text));
    stream.push(NAK);
    let out = assert_chunk_invariant(ProtocolKind::Au, &stream);
    assert_eq!(out, vec![message(AU_A), message(AU_B)]);
}

#[test]
fn au_blocks_are_joined_into_one_message() {
    let mut stream = au(b"first-", packie_au::FrameEnd::Block);
    stream.extend(au(b"second", packie_au::FrameEnd::Text));
    let out = assert_chunk_invariant(ProtocolKind::Au, &stream);
    assert_eq!(out, vec![message(b"first-second")]);
}

#[test]
fn au_bad_block_check_is_one_error_and_recovers() {
    let mut bad = au(AU_A, packie_au::FrameEnd::Text);
    let last = bad.len() - 1;
    bad[last] ^= 0x7f;
    let mut stream = bad;
    stream.extend(au(AU_B, packie_au::FrameEnd::Text));
    let out = assert_chunk_invariant(ProtocolKind::Au, &stream);
    assert_eq!(out, vec![Observation::FramingError, message(AU_B)]);
}

#[test]
fn au_stray_bytes_count_one_error_per_run() {
    let mut stream = b"xyz".to_vec();
    stream.extend(au(AU_A, packie_au::FrameEnd::Text));
    let out = assert_chunk_invariant(ProtocolKind::Au, &stream);
    assert_eq!(out, vec![Observation::FramingError, message(AU_A)]);
}

#[test]
fn au_overlong_frame_is_one_error_and_resynchronizes() {
    let mut stream = vec![STX];
    stream.extend(std::iter::repeat_n(b'A', 1500));
    stream.extend(au(AU_A, packie_au::FrameEnd::Text));
    let out = assert_chunk_invariant(ProtocolKind::Au, &stream);
    assert_eq!(out, vec![Observation::FramingError, message(AU_A)]);
}

#[test]
fn au_message_beyond_the_limit_is_one_error() {
    let mut stream = au(AU_A, packie_au::FrameEnd::Text);
    stream.extend(au(b"short", packie_au::FrameEnd::Text));
    let out = run(ProtocolKind::Au, 8, &[&stream]);
    assert_eq!(out, vec![Observation::FramingError, message(b"short")]);
}

// ----- Selection and robustness -----

#[test]
fn kinds_without_framing_have_no_framer() {
    assert!(framer_for(ProtocolKind::Raw, DEFAULT_MAX_MESSAGE_BYTES).is_none());
    assert!(framer_for(ProtocolKind::Delimited, DEFAULT_MAX_MESSAGE_BYTES).is_none());
    for kind in [ProtocolKind::Astm, ProtocolKind::Au, ProtocolKind::Hl7] {
        assert!(framer_for(kind, DEFAULT_MAX_MESSAGE_BYTES).is_some());
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    let mut state = 0x2545_f491_u32;
    let mut stream = Vec::new();
    for _ in 0..20_000 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        stream.push(state.to_le_bytes()[0]);
    }
    for kind in [ProtocolKind::Astm, ProtocolKind::Au, ProtocolKind::Hl7] {
        assert_eq!(whole(kind, &stream), bytewise(kind, &stream));
    }
}
