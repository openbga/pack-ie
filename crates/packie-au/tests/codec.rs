#![cfg(test)]

mod support;

use packie_au::*;
use proptest::prelude::*;
use support::{config_3_digit_codes, decode_frame, parse_text, result_text};

fn config() -> AuConfig {
    support::config(TransmissionClass::B)
}

fn result(config: &AuConfig, block: u8, end: FrameEnd) -> ResultBlock {
    let Message::ResultBlock(result) = parse_text(config, &result_text("SYN00001", block), end)
    else {
        panic!("result");
    };
    result
}

#[test]
fn hand_calculated_xor_and_chunking() {
    // Manual p5: XOR excludes STX and includes text + ETX.
    let wire = [2, b'D', b'B', b'0', b'1', 3, 4];
    let mut decoder = StreamDecoder::new(config()).unwrap();
    for byte in &wire[..wire.len() - 1] {
        assert!(decoder.push(&[*byte]).is_empty());
    }
    let events = decoder.push(&wire[wire.len() - 1..]);
    let WireEvent::Frame(frame) = events[0].as_ref().unwrap() else {
        panic!("frame");
    };
    assert_eq!(frame.text.expose(), b"DB01");
    assert!(matches!(
        parse_message(&config(), frame).unwrap(),
        Message::Session {
            kind: SessionKind::ResultsStart,
            ..
        }
    ));
    assert!(!decoder.is_pending());
}

#[test]
fn exact_patient_value_flags_and_redaction() {
    let result = result(&config(), b'E', FrameEnd::Text);
    assert_eq!(result.header.sample_id.expose(), b"SYN00001");
    assert_eq!(result.tests[0].value.expose(), b"012.34");
    assert_eq!(result.tests[0].flags.expose(), b"  ");
    assert!(!format!("{result:?}").contains("SYN00001"));
    assert!(!format!("{result:?}").contains("012.34"));
}

#[test]
fn assembly_sequence_retry_conflicts_and_completion() {
    let cfg = config();
    let mut assembler = ResultAssembler::new();
    let zero = result(&cfg, b'0', FrameEnd::Block);
    assert_eq!(
        assembler.push(zero.clone()).unwrap(),
        AssemblyOutcome::Pending
    );
    assert_eq!(
        assembler.push(zero.clone()).unwrap(),
        AssemblyOutcome::Duplicate
    );
    assert_eq!(assembler.finish(), Err(AuError::IncompleteSample));
    let mut conflict = zero;
    conflict.tests[0].value = Redacted::new(b"999999".to_vec());
    assert_eq!(assembler.push(conflict), Err(AuError::BlockConflict));
    assert_eq!(
        assembler.push(result(&cfg, b'2', FrameEnd::Block)),
        Err(AuError::BlockSequence)
    );
    let mut changed_header = result(&cfg, b'1', FrameEnd::Block);
    changed_header.header.sample_id = Redacted::new(b"SYN00002".to_vec());
    assert_eq!(assembler.push(changed_header), Err(AuError::HeaderConflict));
    assert_eq!(
        assembler.push(result(&cfg, b'1', FrameEnd::Block)).unwrap(),
        AssemblyOutcome::Pending
    );
    let AssemblyOutcome::Complete(sample) =
        assembler.push(result(&cfg, b'E', FrameEnd::Text)).unwrap()
    else {
        panic!("complete");
    };
    assert_eq!(sample.tests.len(), 3);
    assert_eq!(assembler.finish(), Ok(()));
    // No final-frame deduplication: session context belongs to the caller.
    assert!(matches!(
        assembler.push(result(&cfg, b'E', FrameEnd::Text)).unwrap(),
        AssemblyOutcome::Complete(_)
    ));
}

#[test]
fn numeric_and_final_terminators_must_match() {
    let cfg = config();
    let wire = encode_frame(
        &cfg,
        b"D 01000101U0001SYN00001    001012.34  ",
        FrameEnd::Text,
    )
    .unwrap();
    let frame = decode_frame(&cfg, &wire);
    assert_eq!(parse_message(&cfg, &frame), Err(AuError::BlockTerminator));
}

#[test]
fn query_normal_and_rerun_session_and_nonpatient_layouts() {
    let cfg = config();
    for code in [b"RB", b"RE", b"DB", b"DE"] {
        let text = [code.as_slice(), b"01"].concat();
        assert!(matches!(
            parse_text(&cfg, &text, FrameEnd::Text),
            Message::Session { .. }
        ));
    }
    for code in [b"R ", b"RH"] {
        let text = [code.as_slice(), b"01000101U0001SYN00001"].concat();
        assert!(matches!(
            parse_text(&cfg, &text, FrameEnd::Text),
            Message::Query(_)
        ));
    }
    for (text, kind) in [
        (
            b"DR01       R001            E01999999ab".as_slice(),
            ResultKind::ReagentBlank,
        ),
        (
            b"DA01       A001CAL00001  01E01999999ab",
            ResultKind::Calibration,
        ),
        (
            b"DQ01       Q001QC000001  01E01999999ab",
            ResultKind::QualityControl,
        ),
    ] {
        let Message::ResultBlock(block) = parse_text(&cfg, text, FrameEnd::Text) else {
            panic!("result");
        };
        assert_eq!(block.kind, kind);
        assert_eq!(block.tests[0].value.expose(), b"999999");
        assert_eq!(block.tests[0].flags.expose(), b"ab");
    }
}

#[test]
fn stat_rerun_and_nine_digit_values_are_opaque() {
    let mut cfg = config();
    cfg.format.result_digits = 9;
    // Manual pp11–12: stat rack is spaces; rerun has original sample number.
    for (text, kind) in [
        (
            b"d 01    22UP001SYN00001    E01000100200  ".as_slice(),
            ResultKind::Stat,
        ),
        (
            b"dH01    22UP002SYN00001P001E01000100200  ",
            ResultKind::StatRerun,
        ),
        (
            b"DH01000101U0002SYN000010001E01000100200  ",
            ResultKind::Rerun,
        ),
    ] {
        let Message::ResultBlock(block) = parse_text(&cfg, text, FrameEnd::Text) else {
            panic!("result");
        };
        assert_eq!(block.kind, kind);
        assert_eq!(block.tests[0].value.expose(), b"000100200");
    }
}

#[test]
fn tri_has_no_unit_no_and_splits_whole_items() {
    let cfg = config();
    let block = result(&cfg, b'E', FrameEnd::Text);
    let mut header = block.header;
    header.demographics = Some(Demographics {
        sex: Redacted::new(vec![]),
        age: Redacted::new(vec![]),
        month: Redacted::new(vec![]),
        information: std::array::from_fn(|_| Redacted::new(vec![])),
    });
    let tri = Tri {
        rerun: false,
        header,
        tests: (0..250)
            .map(|_| TriTest {
                code: Redacted::new(b"01".to_vec()),
                method: None,
            })
            .collect(),
    };
    let wires = encode_tri(&cfg, &tri).unwrap();
    assert!(wires.len() > 1);
    let mut tests = 0;
    for (index, wire) in wires.iter().enumerate() {
        assert!(wire.len() <= 256);
        let frame = decode_frame(&cfg, wire);
        assert!(frame.text.expose().starts_with(b"S 000101U0001"));
        let Message::TriBlock(block) = parse_message(&cfg, &frame).unwrap() else {
            panic!("tri");
        };
        assert!(block.header.unit_no.is_none());
        assert_eq!(
            block.block,
            if index + 1 == wires.len() {
                BlockId::Final
            } else {
                BlockId::Intermediate(u8::try_from(index).unwrap())
            }
        );
        tests += block.tests.len();
    }
    assert_eq!(tests, 250);
    let empty = Tri {
        tests: Vec::new(),
        ..tri
    };
    assert_eq!(encode_tri(&cfg, &empty), Err(AuError::Length));
    let stop = encode_stop(&cfg).unwrap();
    assert_eq!(&stop[1..3], b"SE");
    assert_eq!(
        parse_message(&cfg, &decode_frame(&cfg, &stop)).unwrap(),
        Message::Session {
            kind: SessionKind::TriStop,
            unit_no: None
        }
    );
}

#[test]
fn custom_controls_without_bcc_or_etb() {
    let mut cfg = config();
    cfg.framing.start = vec![1, 2];
    cfg.framing.end = vec![13, 10];
    cfg.framing.bcc = false;
    let wire = encode_frame(&cfg, b"RB01", FrameEnd::Text).unwrap();
    assert_eq!(wire, b"\x01\x02RB01\r\n");
    let mut decoder = StreamDecoder::new(cfg.clone()).unwrap();
    assert!(decoder.push(&wire[..1]).is_empty());
    assert_eq!(decoder.push(&wire[1..]).len(), 1);
    let numeric = result(&cfg, b'0', FrameEnd::Text);
    assert_eq!(numeric.block, BlockId::Intermediate(0));
}

#[test]
fn invalid_profiles_are_rejected() {
    let cfg = config();
    let mut bad = cfg.clone();
    bad.format.sample_id_digits = 0;
    assert_eq!(bad.validate(), Err(AuError::Configuration));
    bad = cfg.clone();
    bad.framing.start = vec![2, 2, 2];
    assert_eq!(bad.validate(), Err(AuError::Configuration));
    bad = cfg;
    bad.framing.max_wire_len = 255;
    assert_eq!(bad.validate(), Err(AuError::Configuration));
}

#[test]
fn invalid_bcc_length_field_and_incomplete_wire_fail() {
    let cfg = config();
    let mut wire = encode_frame(&cfg, b"RB01", FrameEnd::Text).unwrap();
    *wire.last_mut().unwrap() ^= 1;
    assert_eq!(
        StreamDecoder::new(cfg.clone()).unwrap().push(&wire),
        vec![Err(AuError::Checksum)]
    );
    let mut decoder = StreamDecoder::new(cfg).unwrap();
    decoder.push(b"\x02D");
    assert_eq!(decoder.finish(), Err(AuError::Framing));
    assert!(!decoder.is_pending());
}

proptest! {
    #[test]
    fn decoder_and_parser_are_bounded_and_do_not_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..8192),
        chunk_size in 1usize..128,
    ) {
        let cfg = config();
        let mut decoder = StreamDecoder::new(cfg.clone()).unwrap();
        for chunk in bytes.chunks(chunk_size) {
            for event in decoder.push(chunk) {
                if let Ok(WireEvent::Frame(frame)) = event {
                    let _parsed = parse_message(&cfg, &frame);
                }
            }
            prop_assert!(decoder.buffered_len() <= 256);
        }
        let _end = decoder.finish();
    }

    #[test]
    fn fixed_width_parser_does_not_panic(bytes in prop::collection::vec(32u8..=126, 0..1200)) {
        let frame = Frame {
            wire: Redacted::new(vec![]),
            text: Redacted::new(bytes),
            end: FrameEnd::Text,
        };
        let _parsed = parse_message(&config(), &frame);
    }
}

#[test]
fn rack_profiles_and_enabled_demographics_have_exact_widths() {
    let mut cfg = config();
    cfg.format.rack_digits = 0;
    let Message::Query(query) = parse_text(&cfg, b"R 01U0001SYN00001", FrameEnd::Text) else {
        panic!("query");
    };
    assert!(query.header.rack.expose().is_empty());
    assert!(query.header.cup.expose().is_empty());
    cfg.format.rack_digits = 5;
    cfg.format.unit_no = false;
    let Message::Query(query) = parse_text(&cfg, b"RH0000101U0001SYN00001", FrameEnd::Text) else {
        panic!("query");
    };
    assert_eq!(query.header.rack.expose(), b"00001");
    assert!(query.header.unit_no.is_none());
    cfg = config();
    cfg.format.sex = true;
    cfg.format.age = true;
    cfg.format.month = true;
    cfg.format.patient_info_digits = [2, 0, 0, 0, 0, 0];
    let Message::ResultBlock(block) = parse_text(
        &cfg,
        b"D 01000101U0001SYN00001    EM03211AB01012.34  ",
        FrameEnd::Text,
    ) else {
        panic!("result");
    };
    let demographic = block.header.demographics.unwrap();
    assert_eq!(demographic.sex.expose(), b"M");
    assert_eq!(demographic.age.expose(), b"032");
    assert_eq!(demographic.month.expose(), b"11");
    assert_eq!(demographic.information[0].expose(), b"AB");
}

#[test]
fn malformed_fields_partial_items_and_unsupported_nonpatient_profile_fail() {
    let cfg = config();
    for text in [
        b"D 01000101U0001SYN00001    Z01012.34  ".as_slice(),
        b"D 01000101U0001SYN00001    E00012.34  ",
        b"D 01000101U0001SYN00001    E01012.34 ",
        b"D AA000101U0001SYN00001    E01012.34  ",
    ] {
        let frame = Frame {
            wire: Redacted::new(vec![]),
            text: Redacted::new(text.to_vec()),
            end: FrameEnd::Text,
        };
        assert!(parse_message(&cfg, &frame).is_err());
    }
    let mut unsupported = cfg;
    unsupported.format.rack_digits = 0;
    let frame = Frame {
        wire: Redacted::new(vec![]),
        text: Redacted::new(b"DQ01 Q001QC000001  01E01999999ab".to_vec()),
        end: FrameEnd::Text,
    };
    assert_eq!(
        parse_message(&unsupported, &frame),
        Err(AuError::Configuration)
    );
}

#[test]
fn rerun_tri_preserves_original_sample_and_explicit_method() {
    let cfg = config();
    let Message::ResultBlock(block) = parse_text(
        &cfg,
        b"DH01000101U0002SYN000010001E01012.34  ",
        FrameEnd::Text,
    ) else {
        panic!("result");
    };
    let tri = Tri {
        rerun: true,
        header: block.header,
        tests: vec![TriTest {
            code: Redacted::new(b"01".to_vec()),
            method: Some(b'2'),
        }],
    };
    let wires = encode_tri(&cfg, &tri).unwrap();
    let frame = decode_frame(&cfg, &wires[0]);
    assert_eq!(frame.text.expose(), b"SH000101U0002SYN000010001E012");
    let Message::TriBlock(block) = parse_message(&cfg, &frame).unwrap() else {
        panic!("tri");
    };
    assert_eq!(block.tests[0].method, Some(b'2'));
    assert_eq!(block.header.reserved.expose(), b"0001");
    let mut invalid = tri;
    invalid.tests[0].method = None;
    assert_eq!(encode_tri(&cfg, &invalid), Err(AuError::Field));
}

#[test]
fn decoder_limits_resynchronizes_and_preserves_control_events() {
    let mut cfg = config();
    cfg.framing.start = vec![1, 2];
    let wire = encode_frame(&cfg, b"RB01", FrameEnd::Text).unwrap();
    let mut decoder = StreamDecoder::new(cfg).unwrap();
    assert!(decoder.push(&[1]).is_empty());
    // The second 01 begins a fresh two-byte start after the incomplete prefix.
    let mut overlapping = vec![1];
    overlapping.extend_from_slice(&wire[1..]);
    let events = decoder.push(&overlapping);
    assert!(events[0].is_err());
    assert!(matches!(events[1], Ok(WireEvent::Frame(_))));
    assert_eq!(
        decoder.push(&[6, 21]),
        vec![Ok(WireEvent::Ack), Ok(WireEvent::Nak)]
    );
    let mut decoder = StreamDecoder::new(config()).unwrap();
    let oversized = [vec![2], vec![b'X'; 256]].concat();
    assert!(
        decoder
            .push(&oversized)
            .contains(&Err(AuError::FrameTooLong))
    );
    assert!(!decoder.is_pending());
    for maximum in [256, 512, 1024] {
        let mut cfg = config();
        cfg.framing.max_wire_len = maximum;
        let text = vec![b'X'; usize::from(maximum) - 3];
        assert_eq!(
            encode_frame(&cfg, &text, FrameEnd::Text).unwrap().len(),
            usize::from(maximum)
        );
        let too_long = vec![b'X'; usize::from(maximum) - 2];
        assert_eq!(
            encode_frame(&cfg, &too_long, FrameEnd::Text),
            Err(AuError::FrameTooLong)
        );
    }
}

#[test]
fn matching_control_prefixes_and_etb_start_are_unambiguous_inside_frames() {
    for (start, end) in [
        (vec![2, 1], vec![2, 3]),
        (vec![23], vec![3]),
        (vec![2], vec![2]),
    ] {
        let mut cfg = config();
        cfg.framing.start = start;
        cfg.framing.end = end;
        assert!(matches!(
            parse_text(&cfg, b"RB01", FrameEnd::Text),
            Message::Session { .. }
        ));
        if cfg.framing.end == [3] {
            assert_eq!(
                result(&cfg, b'0', FrameEnd::Block).block,
                BlockId::Intermediate(0)
            );
        }
    }
}

fn config_3() -> AuConfig {
    config_3_digit_codes(TransmissionClass::B)
}

fn text_frame(text: &[u8]) -> Frame {
    Frame {
        wire: Redacted::new(vec![]),
        text: Redacted::new(text.to_vec()),
        end: FrameEnd::Text,
    }
}

fn codes(tests: &[ResultTest]) -> Vec<&[u8]> {
    tests.iter().map(|t| t.code.expose().as_slice()).collect()
}

#[test]
fn invalid_test_code_widths_are_rejected() {
    for digits in [0, 1, 4, 255] {
        let mut cfg = config();
        cfg.format.test_code_digits = digits;
        assert_eq!(cfg.validate(), Err(AuError::Configuration));
        assert!(matches!(
            StreamDecoder::new(cfg.clone()),
            Err(AuError::Configuration)
        ));
        assert_eq!(
            parse_message(&cfg, &text_frame(b"RB01")),
            Err(AuError::Configuration)
        );
    }
    assert_eq!(config_3().validate(), Ok(()));
}

#[test]
fn three_digit_normal_result_items_are_code_value_and_two_flags() {
    // Layout of the text after the header `D 01 0001 01 U 0001 SYN00001 ____ E`: every item is
    // 3 code bytes + 6 value bytes + 2 flag bytes, so item 1 is `001|012.34|  ` and item 2 is
    // `102|123456|AB`.
    let cfg = config_3();
    let Message::ResultBlock(block) = parse_text(
        &cfg,
        b"D 01000101U0001SYN00001    E001012.34  102123456AB",
        FrameEnd::Text,
    ) else {
        panic!("result");
    };
    assert_eq!(block.kind, ResultKind::Normal);
    assert_eq!(block.block, BlockId::Final);
    assert_eq!(codes(&block.tests), [b"001".as_slice(), b"102"]);
    assert_eq!(block.tests[0].value.expose(), b"012.34");
    assert_eq!(block.tests[0].flags.expose(), b"  ");
    assert_eq!(block.tests[1].value.expose(), b"123456");
    assert_eq!(block.tests[1].flags.expose(), b"AB");
    assert!(!format!("{block:?}").contains("102"));
    // Nine-digit values: item width 3 + 9 + 2 = 14.
    let mut nine = cfg;
    nine.format.result_digits = 9;
    let Message::ResultBlock(block) = parse_text(
        &nine,
        b"D 01000101U0001SYN00001    E999000100200  ",
        FrameEnd::Text,
    ) else {
        panic!("result");
    };
    assert_eq!(block.tests[0].code.expose(), b"999");
    assert_eq!(block.tests[0].value.expose(), b"000100200");
}

#[test]
fn three_digit_quality_control_and_reagent_blank_items() {
    // Same headers as the two-digit nonpatient fixtures; only the item grows to `201|999999|ab`.
    let cfg = config_3();
    for (text, kind) in [
        (
            b"DQ01       Q001QC000001  01E201999999ab".as_slice(),
            ResultKind::QualityControl,
        ),
        (
            b"DR01       R001            E010999999ab",
            ResultKind::ReagentBlank,
        ),
    ] {
        let Message::ResultBlock(block) = parse_text(&cfg, text, FrameEnd::Text) else {
            panic!("result");
        };
        assert_eq!(block.kind, kind);
        assert_eq!(block.tests.len(), 1);
        assert_eq!(block.tests[0].code.expose().len(), 3);
        assert_eq!(block.tests[0].value.expose(), b"999999");
    }
}

#[test]
fn three_digit_requisition_lists_parse() {
    // Normal `S `: the header ends with block `E`, then bare 3-digit codes `001` `002` `123`.
    // Rerun `SH`: each code is followed by one method byte, `0012` `1231`.
    let cfg = config_3();
    let Message::TriBlock(normal) =
        parse_text(&cfg, b"S 000101U0001SYN00001    E001002123", FrameEnd::Text)
    else {
        panic!("tri");
    };
    assert!(!normal.rerun);
    let listed: Vec<_> = normal
        .tests
        .iter()
        .map(|t| t.code.expose().clone())
        .collect();
    assert_eq!(listed, [b"001".to_vec(), b"002".to_vec(), b"123".to_vec()]);
    assert!(normal.tests.iter().all(|t| t.method.is_none()));
    let Message::TriBlock(rerun) =
        parse_text(&cfg, b"SH000101U0002SYN000010001E00121231", FrameEnd::Text)
    else {
        panic!("tri");
    };
    assert!(rerun.rerun);
    assert_eq!(rerun.tests[0].code.expose(), b"001");
    assert_eq!(rerun.tests[0].method, Some(b'2'));
    assert_eq!(rerun.tests[1].code.expose(), b"123");
    assert_eq!(rerun.tests[1].method, Some(b'1'));
    // A method byte outside 0-2 is still refused.
    assert_eq!(
        parse_message(&cfg, &text_frame(b"SH000101U0002SYN000010001E0013")),
        Err(AuError::Field)
    );
}

/// A synthetic requisition header: normal requisitions carry empty demographics, reruns the
/// original sample number.
fn requisition_header(rerun: bool) -> SampleHeader {
    let Message::ResultBlock(sample) = parse_text(
        &config_3(),
        &support::result_text_3_digit_codes("SYN00001", b'E'),
        FrameEnd::Text,
    ) else {
        panic!("result");
    };
    let mut header = sample.header;
    if rerun {
        header.reserved = Redacted::new(b"0001".to_vec());
        header.demographics = None;
    } else {
        header.demographics = Some(Demographics {
            sex: Redacted::new(vec![]),
            age: Redacted::new(vec![]),
            month: Redacted::new(vec![]),
            information: std::array::from_fn(|_| Redacted::new(vec![])),
        });
    }
    header
}

#[test]
fn three_digit_requisitions_round_trip_across_blocks() {
    let cfg = config_3();
    for (rerun, item_width) in [(false, 3), (true, 4)] {
        let tests: Vec<_> = (0..300u16)
            .map(|i| TriTest {
                code: Redacted::new(format!("{:03}", 1 + i * 3).into_bytes()),
                method: rerun.then_some(b'0' + u8::try_from(i % 3).unwrap()),
            })
            .collect();
        let tri = Tri {
            rerun,
            header: requisition_header(rerun),
            tests: tests.clone(),
        };
        let wires = encode_tri(&cfg, &tri).unwrap();
        // 256-byte frames: (256 - 3 control bytes - 26 byte prefix) / item width per block.
        let per_block = (256 - 3 - 26) / item_width;
        assert_eq!(wires.len(), tests.len().div_ceil(per_block));
        assert!(wires.len() > 3);
        let mut decoded = Vec::new();
        for (index, wire) in wires.iter().enumerate() {
            assert!(wire.len() <= 256);
            let Message::TriBlock(block) = parse_message(&cfg, &decode_frame(&cfg, wire)).unwrap()
            else {
                panic!("tri");
            };
            let last = index + 1 == wires.len();
            assert_eq!(block.block == BlockId::Final, last);
            assert!(last || block.tests.len() == per_block);
            decoded.extend(block.tests);
        }
        assert_eq!(decoded.len(), tests.len());
        for (got, want) in decoded.iter().zip(&tests) {
            assert_eq!(got.code.expose(), want.code.expose());
            assert_eq!(got.method, want.method);
        }
    }
}

#[test]
fn three_digit_requisition_needing_more_than_eleven_blocks_is_refused() {
    let tri = Tri {
        rerun: false,
        header: requisition_header(false),
        tests: (0..1000)
            .map(|_| TriTest {
                code: Redacted::new(b"001".to_vec()),
                method: None,
            })
            .collect(),
    };
    assert_eq!(encode_tri(&config_3(), &tri), Err(AuError::TooManyBlocks));
}

#[test]
fn test_code_width_mismatch_and_bad_codes_are_rejected() {
    let cfg2 = config();
    let cfg3 = config_3();
    // Two-digit data under the three-digit profile: items are truncated.
    for text in [
        b"D 01000101U0001SYN00001    E01012.34  ".as_slice(),
        b"D 01000101U0001SYN00001    E01012.34  02123456AB",
        b"S 000101U0001SYN00001    E01",
        b"SH000101U0002SYN000010001E012",
    ] {
        assert_eq!(
            parse_message(&cfg3, &text_frame(text)),
            Err(AuError::Length),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
    // Three-digit data under the two-digit profile.
    assert_eq!(
        parse_message(
            &cfg2,
            &text_frame(b"D 01000101U0001SYN00001    E001012.34  ")
        ),
        Err(AuError::Length)
    );
    // All-zero and non-digit codes.
    for text in [
        b"D 01000101U0001SYN00001    E000012.34  ".as_slice(),
        b"D 01000101U0001SYN00001    E0A1012.34  ",
        b"D 01000101U0001SYN00001    E 01012.34  ",
        b"D 01000101U0001SYN00001    E-01012.34  ",
        b"S 000101U0001SYN00001    E000",
        b"S 000101U0001SYN00001    E00x",
        b"SH000101U0002SYN000010001E0000",
    ] {
        assert_eq!(
            parse_message(&cfg3, &text_frame(text)),
            Err(AuError::Field),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
    // 001 is a valid code at three digits although it starts with zeros.
    assert!(
        parse_message(
            &cfg3,
            &text_frame(b"D 01000101U0001SYN00001    E001012.34  ")
        )
        .is_ok()
    );
}

#[test]
fn encoder_enforces_the_configured_code_width() {
    let cfg2 = config();
    let cfg3 = config_3();
    let Message::ResultBlock(sample) = parse_text(
        &cfg3,
        &support::result_text_3_digit_codes("SYN00001", b'E'),
        FrameEnd::Text,
    ) else {
        panic!("result");
    };
    let mut header = sample.header;
    header.demographics = Some(Demographics {
        sex: Redacted::new(vec![]),
        age: Redacted::new(vec![]),
        month: Redacted::new(vec![]),
        information: std::array::from_fn(|_| Redacted::new(vec![])),
    });
    let tri = |code: &[u8]| Tri {
        rerun: false,
        header: header.clone(),
        tests: vec![TriTest {
            code: Redacted::new(code.to_vec()),
            method: None,
        }],
    };
    assert_eq!(encode_tri(&cfg3, &tri(b"01")), Err(AuError::Length));
    assert_eq!(encode_tri(&cfg3, &tri(b"0001")), Err(AuError::Length));
    assert_eq!(encode_tri(&cfg3, &tri(b"000")), Err(AuError::Field));
    assert_eq!(encode_tri(&cfg3, &tri(b"0 1")), Err(AuError::Field));
    assert_eq!(encode_tri(&cfg3, &tri(b"010")).map(|w| w.len()), Ok(1));
    assert_eq!(encode_tri(&cfg2, &tri(b"010")), Err(AuError::Length));
    assert_eq!(encode_tri(&cfg2, &tri(b"00")), Err(AuError::Field));
}

#[test]
fn assembler_joins_three_digit_blocks() {
    let cfg = config_3();
    let block = |id: u8, end: FrameEnd| {
        let Message::ResultBlock(block) = parse_text(
            &cfg,
            &support::result_text_3_digit_codes("SYN00001", id),
            end,
        ) else {
            panic!("result");
        };
        block
    };
    let mut assembler = ResultAssembler::new();
    assert_eq!(
        assembler.push(block(b'0', FrameEnd::Block)).unwrap(),
        AssemblyOutcome::Pending
    );
    let AssemblyOutcome::Complete(sample) = assembler.push(block(b'E', FrameEnd::Text)).unwrap()
    else {
        panic!("complete");
    };
    assert_eq!(sample.tests.len(), 2);
    assert_eq!(sample.tests[1].code.expose(), b"001");
}

proptest! {
    #[test]
    fn three_digit_decoder_and_parser_do_not_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..4096),
        chunk_size in 1usize..128,
    ) {
        let cfg = config_3();
        let mut decoder = StreamDecoder::new(cfg.clone()).unwrap();
        for chunk in bytes.chunks(chunk_size) {
            for event in decoder.push(chunk) {
                if let Ok(WireEvent::Frame(frame)) = event {
                    let _parsed = parse_message(&cfg, &frame);
                }
            }
            prop_assert!(decoder.buffered_len() <= 256);
        }
        let _end = decoder.finish();
    }

    #[test]
    fn three_digit_fixed_width_parser_does_not_panic(
        bytes in prop::collection::vec(32u8..=126, 0..1200),
        tail in prop::collection::vec(prop::sample::select(b"0123456789 AB.-".to_vec()), 0..80),
        prefix in prop::sample::select(vec![
            b"D 01000101U0001SYN00001    E".to_vec(),
            b"DQ01       Q001QC000001  01E".to_vec(),
            b"S 000101U0001SYN00001    E".to_vec(),
            b"SH000101U0002SYN000010001E".to_vec(),
        ]),
    ) {
        let cfg = config_3();
        let _parsed = parse_message(&cfg, &text_frame(&bytes));
        // Valid headers with arbitrary item bytes exercise the width arithmetic.
        let parsed = parse_message(&cfg, &text_frame(&[prefix, tail].concat()));
        if let Ok(Message::ResultBlock(block)) = parsed {
            prop_assert!(block.tests.iter().all(|t| t.code.expose().len() == 3));
        }
    }
}
