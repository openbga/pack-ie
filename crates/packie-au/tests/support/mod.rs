//! Synthetic fixtures shared by the codec and driver tests; each test binary uses a subset.
#![cfg(test)]
#![allow(dead_code)]

use packie_au::{
    AuConfig, FormatConfig, Frame, FrameEnd, FramingConfig, Message, StreamDecoder,
    TransmissionClass, WireEvent, encode_frame, parse_message,
};

/// STX/ETX framing with BCC and ETB, unit number, 4-digit rack, 8-digit sample ID.
pub(crate) fn config(class: TransmissionClass) -> AuConfig {
    AuConfig {
        framing: FramingConfig {
            start: vec![2],
            end: vec![3],
            bcc: true,
            use_etb: true,
            max_wire_len: 256,
            class,
        },
        format: FormatConfig {
            unit_no: true,
            rack_digits: 4,
            sample_id_digits: 8,
            sex: false,
            age: false,
            month: false,
            patient_info_digits: [0; 6],
            result_digits: 6,
            test_code_digits: 2,
        },
    }
}

/// The profile of [`config`] with 3-digit test codes.
pub(crate) fn config_3_digit_codes(class: TransmissionClass) -> AuConfig {
    let mut config = config(class);
    config.format.test_code_digits = 3;
    config
}

/// Hand-authored synthetic D-space result text (manual pp. 10, 14; not copied from it) with
/// the given sample ID and block identification.
pub(crate) fn result_text(sample_id: &str, block: u8) -> Vec<u8> {
    let mut text = format!("D 01000101U0001{sample_id}    E01012.34  ").into_bytes();
    text[27] = block;
    text
}

/// Like [`result_text`] for [`config_3_digit_codes`]: one item, code 001, value `012.34`, flags
/// of two spaces (item width 3 + 6 + 2).
pub(crate) fn result_text_3_digit_codes(sample_id: &str, block: u8) -> Vec<u8> {
    let mut text = format!("D 01000101U0001{sample_id}    E001012.34  ").into_bytes();
    text[27] = block;
    text
}

/// Decodes `wire` with a fresh decoder, which must yield a frame first.
pub(crate) fn decode_frame(config: &AuConfig, wire: &[u8]) -> Frame {
    let mut decoder = StreamDecoder::new(config.clone()).unwrap();
    let Some(Ok(WireEvent::Frame(frame))) = decoder.push(wire).into_iter().next() else {
        panic!("expected a frame");
    };
    frame
}

/// Encodes `text`, decodes it again and parses it.
pub(crate) fn parse_text(config: &AuConfig, text: &[u8], end: FrameEnd) -> Message {
    let wire = encode_frame(config, text, end).unwrap();
    parse_message(config, &decode_frame(config, &wire)).unwrap()
}
