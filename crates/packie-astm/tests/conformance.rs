#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
use std::collections::{HashSet, VecDeque};

use packie_astm::{
    ACK, Action, ENQ, EOT, Event, FrameId, LinkDriver, Message, MessageError, NAK, PatientNode,
    ReceiveAbortReason, ReceiveDriver, assemble_message, encode_records, parse_message_records,
};
use proptest::prelude::*;

#[derive(Debug, Default)]
struct SessionOutcome {
    acknowledgements: usize,
    negative_acknowledgements: usize,
    persistence_requests: usize,
    persisted: HashSet<FrameId>,
    completed_records: Option<Vec<Vec<u8>>>,
    aborts: Vec<ReceiveAbortReason>,
    ack_without_durable_frame: bool,
}

fn run_session(raw: &[u8]) -> Result<SessionOutcome, packie_astm::DriverError> {
    let mut driver = ReceiveDriver::default();
    let mut outcome = SessionOutcome::default();
    let mut establishment_ack_seen = false;

    for byte in raw {
        let mut events = VecDeque::from([Event::Bytes(vec![*byte])]);
        while let Some(event) = events.pop_front() {
            let actions = driver.handle(event)?;
            for action in actions {
                apply_action(
                    action,
                    &mut outcome,
                    &mut events,
                    &mut establishment_ack_seen,
                );
            }
        }
    }
    Ok(outcome)
}

fn apply_action(
    action: Action,
    outcome: &mut SessionOutcome,
    events: &mut VecDeque<Event>,
    establishment_ack_seen: &mut bool,
) {
    match action {
        Action::Write(bytes) if bytes == [ACK] => record_ack(outcome, establishment_ack_seen),
        Action::Write(bytes) if bytes == [NAK] => outcome.negative_acknowledgements += 1,
        Action::Write(_)
        | Action::MessageSent
        | Action::SendAborted(_)
        | Action::StartTimer { .. }
        | Action::StopTimer => {}
        Action::PersistFrame { frame_id, .. } => {
            outcome.persistence_requests += 1;
            outcome.persisted.insert(frame_id);
            events.push_back(Event::Persisted(frame_id));
        }
        Action::MessageReceived(records) => outcome.completed_records = Some(records),
        Action::ReceiveAborted(reason) => outcome.aborts.push(reason),
    }
}

fn record_ack(outcome: &mut SessionOutcome, establishment_ack_seen: &mut bool) {
    outcome.acknowledgements += 1;
    if !*establishment_ack_seen {
        *establishment_ack_seen = true;
    } else if outcome.persisted.is_empty() {
        outcome.ack_without_durable_frame = true;
    }
}

fn assembled(outcome: &SessionOutcome) -> Result<Message, MessageError> {
    let records = outcome
        .completed_records
        .as_ref()
        .ok_or(MessageError::Empty)?;
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(record);
        bytes.push(b'\r');
    }
    assemble_message(parse_message_records(&bytes)?)
}

fn frame_count(records: &[Vec<u8>]) -> Result<usize, packie_astm::FrameError> {
    Ok(encode_records(records)?.len())
}

fn decode_hex_fixture(hex: &str) -> Vec<u8> {
    let digits = hex.trim();
    assert!(digits.len().is_multiple_of(2));
    (0..digits.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&digits[index..index + 2], 16).expect("hex digits"))
        .collect()
}

#[test]
fn valid_single_result_matches_expected_outcome() {
    let expected = include_str!("corpus/valid_single_result.expected").trim();
    assert_eq!(expected, "complete patients=1 orders=1 results=1");
    let outcome = run_session(include_bytes!("corpus/valid_single_result.raw"))
        .expect("single-result session runs");
    let message = assembled(&outcome).expect("single-result hierarchy is valid");
    assert_eq!(message.patients().len(), 1);
    assert_eq!(message.patients()[0].orders().len(), 1);
    assert_eq!(message.patients()[0].orders()[0].results().len(), 1);
    let result = message.patients()[0].orders()[0].results()[0]
        .record()
        .as_result()
        .expect("result accessor exists");
    assert_eq!(result.value_text(), Some("<0.0100"));
    let header = message
        .header()
        .as_header()
        .expect("header accessor exists");
    assert_eq!(header.sender_name(), Some("SYNTH-ANALYZER"));
    assert_eq!(header.sender_version(), Some("1.0"));
    assert_eq!(header.processing_id(), Some("P"));
    assert_eq!(header.version(), Some("LIS2-A2"));
    assert_eq!(header.timestamp_text(), Some("20261012120000"));
    assert!(!outcome.ack_without_durable_frame);
}

#[test]
fn packed_records_in_one_frame_complete_as_two_records() {
    assert_eq!(
        include_str!("corpus/packed_records.expected").trim(),
        "complete records=2 frames=1"
    );
    let raw = decode_hex_fixture(include_str!("corpus/packed_records.hex"));
    let outcome = run_session(&raw).expect("packed-record session runs");
    assert_eq!(outcome.persistence_requests, 1);
    assert_eq!(
        outcome.completed_records,
        Some(vec![b"H|\\^&|||SYNTHETIC".to_vec(), b"L|1|N".to_vec()])
    );
}

#[test]
fn multiple_patients_orders_and_results_match_expected_counts() {
    let expected = include_str!("corpus/multiple_patients_orders_results.expected").trim();
    assert_eq!(expected, "complete patients=2 orders=3 results=4");
    let outcome = run_session(include_bytes!(
        "corpus/multiple_patients_orders_results.raw"
    ))
    .expect("multiple-result session runs");
    let message = assembled(&outcome).expect("multi-patient hierarchy is valid");
    let order_count: usize = message
        .patients()
        .iter()
        .map(|patient| patient.orders().len())
        .sum();
    let result_count: usize = message
        .patients()
        .iter()
        .flat_map(PatientNode::orders)
        .map(|order| order.results().len())
        .sum();
    assert_eq!(
        (message.patients().len(), order_count, result_count),
        (2, 3, 4)
    );
}

#[test]
fn spanning_custom_delimiter_and_escape_cases_decode() {
    assert_eq!(
        include_str!("corpus/record_spanning_etb.expected").trim(),
        "complete long_comment_bytes=500"
    );
    let spanning = run_session(include_bytes!("corpus/record_spanning_etb.raw"))
        .expect("spanning session runs");
    let spanning_message = assembled(&spanning).expect("spanning record hierarchy is valid");
    let comment = spanning_message.patients()[0].orders()[0].results()[0].comments()[0]
        .as_comment()
        .expect("comment accessor exists");
    assert_eq!(comment.text().map(str::len), Some(500));

    assert_eq!(
        include_str!("corpus/custom_delimiters.expected").trim(),
        "complete field=! repeat=@ component=$ escape=%"
    );
    let custom = run_session(include_bytes!("corpus/custom_delimiters.raw"))
        .expect("custom-delimiter session runs");
    let custom_message = assembled(&custom).expect("custom-delimiter hierarchy is valid");
    let header = custom_message
        .header()
        .as_header()
        .expect("header accessor exists");
    assert_eq!(header.sender_name(), Some("SYNTH-ANALYZER"));

    assert_eq!(
        include_str!("corpus/escape_sequences.expected").trim(),
        "complete decoded=TEXT|FIELD^COMP\\REPEAT&!"
    );
    let escapes =
        run_session(include_bytes!("corpus/escape_sequences.raw")).expect("escape session runs");
    let escape_message = assembled(&escapes).expect("escape hierarchy is valid");
    let result = escape_message.patients()[0].orders()[0].results()[0]
        .record()
        .as_result()
        .expect("result accessor exists");
    assert_eq!(result.value_text(), Some("TEXT|FIELD^COMP\\REPEAT&!"));
}

#[test]
fn comments_attach_to_their_immediate_parent() {
    assert_eq!(
        include_str!("corpus/comments.expected").trim(),
        "complete patient_comments=1 order_comments=1 result_comments=1"
    );
    let outcome = run_session(include_bytes!("corpus/comments.raw")).expect("comment session runs");
    let message = assembled(&outcome).expect("comment hierarchy is valid");
    let patient = &message.patients()[0];
    let order = &patient.orders()[0];
    assert_eq!(patient.comments().len(), 1);
    assert_eq!(order.comments().len(), 1);
    assert_eq!(order.results()[0].comments().len(), 1);
}

#[test]
fn checksum_failure_then_retransmission_completes_without_early_ack() {
    assert_eq!(
        include_str!("corpus/bad_checksum_then_retransmission.expected").trim(),
        "complete nak=1 no_early_ack=true"
    );
    let outcome = run_session(include_bytes!(
        "corpus/bad_checksum_then_retransmission.raw"
    ))
    .expect("checksum retransmission session runs");
    assert_eq!(outcome.negative_acknowledgements, 1);
    assert!(assembled(&outcome).is_ok());
    assert!(!outcome.ack_without_durable_frame);
}

#[test]
fn duplicate_after_lost_ack_is_not_persisted_twice() {
    assert_eq!(
        include_str!("corpus/duplicate_after_lost_ack.expected").trim(),
        "complete duplicate_acks=1 duplicate_persists=0"
    );
    let outcome = run_session(include_bytes!("corpus/duplicate_after_lost_ack.raw"))
        .expect("duplicate session runs");
    let records = outcome
        .completed_records
        .as_ref()
        .expect("duplicate case completes");
    assert_eq!(
        outcome.persistence_requests,
        frame_count(records).expect("completed records re-encode")
    );
    assert_eq!(outcome.acknowledgements, outcome.persistence_requests + 2);
    assert!(assembled(&outcome).is_ok());
}

#[test]
fn malformed_frames_are_naked_without_persistence() {
    let cases: &[(&[u8], &str)] = &[
        (
            include_bytes!("corpus/out_of_sequence_frame.raw"),
            include_str!("corpus/out_of_sequence_frame.expected").trim(),
        ),
        (
            include_bytes!("corpus/oversize_frame.raw"),
            include_str!("corpus/oversize_frame.expected").trim(),
        ),
        (
            include_bytes!("corpus/disallowed_byte.raw"),
            include_str!("corpus/disallowed_byte.expected").trim(),
        ),
        (
            include_bytes!("corpus/lowercase_checksum.raw"),
            include_str!("corpus/lowercase_checksum.expected").trim(),
        ),
        (
            include_bytes!("corpus/bad_frame_number.raw"),
            include_str!("corpus/bad_frame_number.expected").trim(),
        ),
        (
            include_bytes!("corpus/missing_end_marker.raw"),
            include_str!("corpus/missing_end_marker.expected").trim(),
        ),
        (
            include_bytes!("corpus/stray_byte_after_enq.raw"),
            include_str!("corpus/stray_byte_after_enq.expected").trim(),
        ),
    ];
    for (raw, expected) in cases {
        assert_eq!(*expected, "incomplete nak=1 persists=0");
        let outcome = run_session(raw).expect("malformed session runs");
        assert_eq!(outcome.negative_acknowledgements, 1);
        assert_eq!(outcome.persistence_requests, 0);
        assert!(outcome.completed_records.is_none());
    }
}

#[test]
fn noise_before_enq_is_ignored() {
    assert_eq!(
        include_str!("corpus/noise_before_enq.expected").trim(),
        "complete patients=1 orders=1 results=1"
    );
    let outcome =
        run_session(include_bytes!("corpus/noise_before_enq.raw")).expect("noisy session runs");
    assert_eq!(outcome.negative_acknowledgements, 0);
    let message = assembled(&outcome).expect("hierarchy is valid after noise");
    assert_eq!(message.patients()[0].orders()[0].results().len(), 1);
}

/// The frames the send side writes are byte-identical to the receive corpus.
#[test]
fn sent_frames_match_the_receive_corpus_bytes() {
    let raw: &[u8] = include_bytes!("corpus/valid_single_result.raw");
    let wire = &raw[1..raw.len() - 1];
    let frames: Vec<&[u8]> = wire.split_inclusive(|byte| *byte == b'\n').collect();
    let records: Vec<Vec<u8>> = wire
        .split(|byte| *byte == b'\n')
        .filter(|frame| !frame.is_empty())
        .map(|frame| frame[2..frame.len() - 4].to_vec())
        .map(|text| text[..text.len() - 1].to_vec())
        .collect();
    let mut link = LinkDriver::default();
    assert_eq!(
        link.submit(&records).expect("submit")[0],
        Action::Write(vec![ENQ])
    );
    let mut written = Vec::new();
    let mut next = vec![ACK];
    loop {
        let actions = link.handle(Event::Bytes(next.clone())).expect("reply");
        let frame = actions.into_iter().find_map(|action| match action {
            Action::Write(bytes) if bytes != [EOT] => Some(bytes),
            _ => None,
        });
        match frame {
            Some(frame) => written.push(frame),
            None => break,
        }
        next = vec![ACK];
    }
    assert_eq!(written.len(), frames.len());
    for (sent, expected) in written.iter().zip(frames) {
        assert_eq!(sent.as_slice(), expected);
    }
}

#[test]
fn incomplete_and_empty_transfers_never_emit_messages() {
    assert_eq!(
        include_str!("corpus/eot_mid_record.expected").trim(),
        "abort=incomplete_record messages=0"
    );
    let incomplete =
        run_session(include_bytes!("corpus/eot_mid_record.raw")).expect("incomplete session runs");
    assert_eq!(
        incomplete.aborts,
        vec![ReceiveAbortReason::IncompleteRecord]
    );
    assert!(incomplete.completed_records.is_none());

    assert_eq!(
        include_str!("corpus/empty_transfer.expected").trim(),
        "abort=empty_transfer messages=0"
    );
    let empty =
        run_session(include_bytes!("corpus/empty_transfer.raw")).expect("empty session runs");
    assert_eq!(empty.aborts, vec![ReceiveAbortReason::EmptyTransfer]);
    assert!(empty.completed_records.is_none());
}

#[test]
fn missing_l_record_fails_structural_assembly() {
    assert_eq!(
        include_str!("corpus/missing_terminator_record.expected").trim(),
        "structural_error=missing_terminator"
    );
    let outcome = run_session(include_bytes!("corpus/missing_terminator_record.raw"))
        .expect("missing-terminator session runs");
    assert_eq!(assembled(&outcome), Err(MessageError::MissingTerminator));
}

proptest! {
    #[test]
    fn encode_driver_parse_round_trip_preserves_result_text(
        value in "[A-Z0-9.<>+-]{1,80}",
    ) {
        let records = vec![
            b"H|\\^&|||SYNTH-ANALYZER^1.0||||||P|LIS2-A2|20261012120000".to_vec(),
            b"P|1|SYN-PRACTICE-0001|SYN-LAB-0001".to_vec(),
            b"O|1|SYN-0001|SYN-INST-0001|^^^SYN-TEST|R||||||N||||URINE".to_vec(),
            format!("R|1|^^^SYN-TEST|{value}|ng/mL||||F").into_bytes(),
            b"L|1|N".to_vec(),
        ];
        let frames = encode_records(&records).expect("generated records encode");
        let mut session = vec![packie_astm::ENQ];
        for frame in frames {
            session.extend_from_slice(&frame);
        }
        session.push(EOT);
        let outcome = run_session(&session).expect("generated session runs");
        prop_assert_eq!(outcome.completed_records.as_ref(), Some(&records));
        let message = assembled(&outcome).expect("generated message assembles");
        let result = message.patients()[0].orders()[0].results()[0]
            .record()
            .as_result()
            .expect("result accessor exists");
        prop_assert_eq!(result.value_text(), Some(value.as_str()));
        prop_assert!(!outcome.ack_without_durable_frame);
    }
}
