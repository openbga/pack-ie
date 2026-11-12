//! Scenario tests for the receive driver. All data is synthetic.

#![cfg(test)]

mod support;

use std::time::Duration;

use packie_au::*;
use proptest::prelude::*;
use support::{config, config_3_digit_codes, decode_frame, result_text, result_text_3_digit_codes};

fn driver(class: TransmissionClass) -> ReceiveDriver {
    ReceiveDriver::new(config(class), ReceiveOptions::default()).unwrap()
}

/// Wire bytes of one synthetic D-space result block built with `encode_frame`.
fn block(class: TransmissionClass, sample_id: &str, id: u8) -> Vec<u8> {
    let end = if id == b'E' {
        FrameEnd::Text
    } else {
        FrameEnd::Block
    };
    encode_frame(&config(class), &result_text(sample_id, id), end).unwrap()
}

fn text_frame(class: TransmissionClass, text: &[u8]) -> Vec<u8> {
    encode_frame(&config(class), text, FrameEnd::Text).unwrap()
}

fn feed(driver: &mut ReceiveDriver, wire: &[u8]) -> Vec<Action> {
    driver.handle(ReceiveEvent::Bytes(wire.to_vec())).unwrap()
}

fn persisted(driver: &mut ReceiveDriver, id: FrameId) -> Vec<Action> {
    driver.handle(ReceiveEvent::Persisted(id)).unwrap()
}

fn expire(driver: &mut ReceiveDriver, id: TimerId) -> Vec<Action> {
    driver.handle(ReceiveEvent::TimerExpired(id)).unwrap()
}

fn persist_id(actions: &[Action]) -> FrameId {
    actions
        .iter()
        .find_map(|a| match a {
            Action::PersistFrame { frame_id, .. } => Some(*frame_id),
            _ => None,
        })
        .expect("a PersistFrame action")
}

fn timer(actions: &[Action]) -> (TimerId, Duration) {
    actions
        .iter()
        .find_map(|a| match a {
            Action::StartTimer { timer_id, duration } => Some((*timer_id, *duration)),
            _ => None,
        })
        .expect("a StartTimer action")
}

fn persists(actions: &[Action]) -> usize {
    actions
        .iter()
        .filter(|a| matches!(a, Action::PersistFrame { .. }))
        .count()
}

/// Expires the timer started by `actions`, returning what it releases.
fn release(driver: &mut ReceiveDriver, actions: &[Action]) -> Vec<Action> {
    expire(driver, timer(actions).0)
}

/// Feeds a frame, confirms storage and lets the turnaround expire; returns the final actions.
fn deliver(driver: &mut ReceiveDriver, wire: &[u8]) -> Vec<Action> {
    let first = feed(driver, wire);
    assert!(persisted(driver, persist_id(&first)).is_empty());
    release(driver, &first)
}

/// Feeds a frame that must be refused: nothing is stored and a NAK follows the turnaround.
fn assert_refused(driver: &mut ReceiveDriver, wire: &[u8]) {
    let actions = feed(driver, wire);
    assert_eq!(persists(&actions), 0);
    assert_eq!(
        release(driver, &actions),
        vec![Action::Send(ControlResponse::Nak)]
    );
}

/// `wire` with its BCC corrupted.
fn corrupted(mut wire: Vec<u8>) -> Vec<u8> {
    *wire.last_mut().unwrap() ^= 1;
    wire
}

const B: TransmissionClass = TransmissionClass::B;
const A: TransmissionClass = TransmissionClass::A;

#[test]
fn fixtures_round_trip_through_the_decoder() {
    let frame = decode_frame(&config(B), &block(B, "SYN00001", b'0'));
    assert_eq!(frame.end, FrameEnd::Block);
    assert!(matches!(
        parse_message(&config(B), &frame),
        Ok(Message::ResultBlock(ResultBlock {
            block: BlockId::Intermediate(0),
            ..
        }))
    ));
}

/// Wire bytes of one result block with 3-digit test codes (`config_3_digit_codes`).
fn block_3_digit_codes(class: TransmissionClass, sample_id: &str, id: u8) -> Vec<u8> {
    let end = if id == b'E' {
        FrameEnd::Text
    } else {
        FrameEnd::Block
    };
    let text = result_text_3_digit_codes(sample_id, id);
    encode_frame(&config_3_digit_codes(class), &text, end).unwrap()
}

#[test]
fn class_b_acknowledges_every_block_with_three_digit_test_codes() {
    let mut driver =
        ReceiveDriver::new(config_3_digit_codes(B), ReceiveOptions::default()).unwrap();
    for id in *b"01" {
        let actions = deliver(&mut driver, &block_3_digit_codes(B, "SYN00001", id));
        assert!(matches!(
            actions.as_slice(),
            [
                Action::Send(ControlResponse::Ack),
                Action::StartTimer { .. }
            ]
        ));
    }
    let actions = deliver(&mut driver, &block_3_digit_codes(B, "SYN00001", b'E'));
    let [
        Action::Send(ControlResponse::Ack),
        Action::SampleComplete(sample),
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(sample.tests.len(), 3);
    assert!(sample.tests.iter().all(|t| t.code.expose() == b"001"));
    // Two-digit data under the three-digit profile is retained as unparsed, never a sample.
    let actions = deliver(&mut driver, &block(B, "SYN00002", b'E'));
    assert!(matches!(
        actions.as_slice(),
        [Action::Send(ControlResponse::Ack), Action::Unparsed { .. }]
    ));
}

#[test]
fn class_a_completes_samples_with_three_digit_test_codes() {
    let mut driver =
        ReceiveDriver::new(config_3_digit_codes(A), ReceiveOptions::default()).unwrap();
    let actions = feed(&mut driver, &block_3_digit_codes(A, "SYN00001", b'0'));
    assert!(matches!(
        actions.as_slice(),
        [Action::PersistFrame { .. }, Action::StartTimer { .. }]
    ));
    let actions = feed(&mut driver, &block_3_digit_codes(A, "SYN00001", b'E'));
    let [
        Action::StopTimer,
        Action::PersistFrame { .. },
        Action::SampleComplete(sample),
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(sample.tests.len(), 2);
}

#[test]
fn class_b_single_block_orders_persist_timer_ack_complete() {
    let mut driver = driver(B);
    let wire = block(B, "SYN00001", b'E');
    let actions = feed(&mut driver, &wire);
    let [
        Action::PersistFrame { frame_id, bytes },
        Action::StartTimer { timer_id, duration },
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(bytes.expose(), &wire);
    assert_eq!(*duration, ACK_TURNAROUND);
    // Storage confirmed first: nothing is released before the turnaround.
    assert!(persisted(&mut driver, *frame_id).is_empty());
    let actions = expire(&mut driver, *timer_id);
    let [
        Action::Send(ControlResponse::Ack),
        Action::SampleComplete(sample),
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(sample.tests.len(), 1);
    assert_eq!(sample.header.sample_id.expose(), b"SYN00001");
    assert_eq!(ControlResponse::Ack.byte(), 0x06);
    assert_eq!(ControlResponse::Nak.byte(), 0x15);
}

#[test]
fn class_b_turnaround_first_then_persisted_releases_ack() {
    let mut driver = driver(B);
    let actions = feed(&mut driver, &block(B, "SYN00001", b'E'));
    assert!(release(&mut driver, &actions).is_empty());
    let actions = persisted(&mut driver, persist_id(&actions));
    assert!(matches!(
        actions.as_slice(),
        [
            Action::Send(ControlResponse::Ack),
            Action::SampleComplete(_)
        ]
    ));
}

#[test]
fn multiblock_sample_is_acknowledged_per_block_and_completes_on_final() {
    let mut driver = driver(B);
    for id in *b"01" {
        let actions = deliver(&mut driver, &block(B, "SYN00001", id));
        let [
            Action::Send(ControlResponse::Ack),
            Action::StartTimer { duration, .. },
        ] = actions.as_slice()
        else {
            panic!("unexpected actions: {actions:?}");
        };
        // T1 starts once the ACK is written.
        assert_eq!(*duration, Duration::from_millis(9900));
    }
    let actions = deliver(&mut driver, &block(B, "SYN00001", b'E'));
    let [
        Action::Send(ControlResponse::Ack),
        Action::SampleComplete(sample),
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(sample.tests.len(), 3);
}

#[test]
fn next_block_stops_the_block_timer() {
    let mut driver = driver(B);
    deliver(&mut driver, &block(B, "SYN00001", b'0'));
    let actions = feed(&mut driver, &block(B, "SYN00001", b'1'));
    assert!(matches!(
        actions.as_slice(),
        [
            Action::StopTimer,
            Action::PersistFrame { .. },
            Action::StartTimer { .. }
        ]
    ));
}

#[test]
fn duplicate_intermediate_block_is_reacknowledged_without_second_persist() {
    let mut driver = driver(B);
    let zero = block(B, "SYN00001", b'0');
    deliver(&mut driver, &zero);
    let actions = feed(&mut driver, &zero);
    assert_eq!(persists(&actions), 0);
    assert_eq!(timer(&actions).1, ACK_TURNAROUND);
    let actions = release(&mut driver, &actions);
    assert!(matches!(
        actions.as_slice(),
        [
            Action::Send(ControlResponse::Ack),
            Action::StartTimer { .. }
        ]
    ));
    // The sample is still intact and finishes normally.
    let actions = deliver(&mut driver, &block(B, "SYN00001", b'E'));
    assert!(matches!(actions.last(), Some(Action::SampleComplete(s)) if s.tests.len() == 2));
}

#[test]
fn repeated_final_block_of_multiblock_sample_is_not_a_second_sample() {
    let mut driver = driver(B);
    deliver(&mut driver, &block(B, "SYN00001", b'0'));
    let last = block(B, "SYN00001", b'E');
    let actions = deliver(&mut driver, &last);
    assert!(matches!(actions.last(), Some(Action::SampleComplete(_))));
    let actions = feed(&mut driver, &last);
    assert_eq!(persists(&actions), 0);
    assert_eq!(
        release(&mut driver, &actions),
        vec![Action::Send(ControlResponse::Ack)]
    );
}

#[test]
fn checksum_error_is_nakked_after_the_turnaround_and_the_prefix_is_kept() {
    let mut driver = driver(B);
    deliver(&mut driver, &block(B, "SYN00001", b'0'));
    let actions = feed(&mut driver, &corrupted(block(B, "SYN00001", b'1')));
    // No partial-sample abort and nothing stored.
    assert!(matches!(
        actions.as_slice(),
        [Action::StopTimer, Action::StartTimer { .. }]
    ));
    let actions = release(&mut driver, &actions);
    assert!(matches!(
        actions.as_slice(),
        [
            Action::Send(ControlResponse::Nak),
            Action::StartTimer { .. }
        ]
    ));
    // The retransmission continues the same sample.
    deliver(&mut driver, &block(B, "SYN00001", b'1'));
    let actions = deliver(&mut driver, &block(B, "SYN00001", b'E'));
    assert!(matches!(actions.last(), Some(Action::SampleComplete(s)) if s.tests.len() == 3));
}

#[test]
fn skipped_block_aborts_the_partial_sample_and_naks() {
    let mut driver = driver(B);
    deliver(&mut driver, &block(B, "SYN00001", b'0'));
    let actions = feed(&mut driver, &block(B, "SYN00001", b'2'));
    assert!(matches!(
        actions.as_slice(),
        [
            Action::StopTimer,
            Action::Aborted(AbortReason::ProtocolError),
            Action::StartTimer { .. }
        ]
    ));
    assert_eq!(
        release(&mut driver, &actions),
        vec![Action::Send(ControlResponse::Nak)]
    );
    // The tail of the aborted sample is refused until block 0 restarts it.
    assert_refused(&mut driver, &block(B, "SYN00001", b'E'));
    deliver(&mut driver, &block(B, "SYN00001", b'0'));
}

#[test]
fn nak_limit_aborts_after_the_fifth_consecutive_failure() {
    let mut driver = driver(B);
    let bad = corrupted(block(B, "SYN00001", b'E'));
    for _ in 0..4 {
        let actions = feed(&mut driver, &bad);
        assert_eq!(
            release(&mut driver, &actions),
            vec![Action::Send(ControlResponse::Nak)]
        );
    }
    let actions = feed(&mut driver, &bad);
    assert_eq!(
        actions,
        vec![Action::Aborted(AbortReason::RetryLimitExceeded)]
    );
    assert_eq!(
        driver.handle(ReceiveEvent::Bytes(vec![2])),
        Err(DriverError::Closed)
    );
}

#[test]
fn an_accepted_frame_resets_the_failure_count() {
    let mut driver = driver(B);
    let bad = corrupted(block(B, "SYN00001", b'E'));
    for round in 0..3 {
        for _ in 0..4 {
            let actions = feed(&mut driver, &bad);
            release(&mut driver, &actions);
        }
        let actions = deliver(&mut driver, &block(B, &format!("SYN0000{round}"), b'E'));
        assert!(matches!(actions.last(), Some(Action::SampleComplete(_))));
    }
}

#[test]
fn block_timeout_aborts_the_partial_sample() {
    let mut driver = driver(B);
    let actions = deliver(&mut driver, &block(B, "SYN00001", b'0'));
    assert_eq!(timer(&actions).1, Duration::from_millis(9900));
    assert_eq!(
        release(&mut driver, &actions),
        vec![Action::Aborted(AbortReason::BlockTimeout)]
    );
    // Not terminal: the next sample is received normally.
    let actions = deliver(&mut driver, &block(B, "SYN00002", b'E'));
    assert!(matches!(actions.last(), Some(Action::SampleComplete(_))));
}

#[test]
fn new_header_interrupts_a_partial_sample() {
    let mut driver = driver(B);
    deliver(&mut driver, &block(B, "SYN00001", b'0'));
    let actions = feed(&mut driver, &block(B, "SYN00002", b'0'));
    assert!(matches!(
        actions.as_slice(),
        [
            Action::StopTimer,
            Action::Aborted(AbortReason::IncompleteSample),
            Action::PersistFrame { .. },
            Action::StartTimer { .. }
        ]
    ));
    persisted(&mut driver, persist_id(&actions));
    release(&mut driver, &actions);
    let actions = deliver(&mut driver, &block(B, "SYN00002", b'E'));
    assert!(matches!(actions.last(), Some(Action::SampleComplete(s)) if s.tests.len() == 2));
}

#[test]
fn session_message_is_a_control_event_and_interrupts_a_partial_sample() {
    let mut driver = driver(B);
    deliver(&mut driver, &block(B, "SYN00001", b'0'));
    let actions = feed(&mut driver, &text_frame(B, b"DE01"));
    assert!(matches!(
        actions.as_slice(),
        [
            Action::StopTimer,
            Action::Aborted(AbortReason::IncompleteSample),
            Action::PersistFrame { .. },
            Action::StartTimer { .. }
        ]
    ));
    persisted(&mut driver, persist_id(&actions));
    let actions = release(&mut driver, &actions);
    let [
        Action::Send(ControlResponse::Ack),
        Action::Control { message },
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert!(matches!(
        **message,
        Message::Session {
            kind: SessionKind::ResultsEnd,
            ..
        }
    ));
}

#[test]
fn query_is_a_control_event() {
    let mut driver = driver(B);
    let actions = deliver(&mut driver, &text_frame(B, b"R 01000101U0001SYN00001"));
    let [
        Action::Send(ControlResponse::Ack),
        Action::Control { message },
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert!(matches!(**message, Message::Query(_)));
}

#[test]
fn host_direction_message_is_refused() {
    let mut driver = driver(B);
    assert_refused(&mut driver, &text_frame(B, b"S 000101U0001SYN00001    E01"));
}

#[test]
fn unparsable_but_framed_text_is_retained_and_acknowledged() {
    let mut driver = driver(B);
    let wire = text_frame(B, b"ZZ01 unsupported");
    let actions = deliver(&mut driver, &wire);
    let [
        Action::Send(ControlResponse::Ack),
        Action::Unparsed { wire: kept },
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(kept.expose(), &wire);
}

#[test]
fn class_a_never_requests_ack_or_nak() {
    let mut driver = driver(A);
    // Single block: store request, then the sample at once.
    let actions = feed(&mut driver, &block(A, "SYN00001", b'E'));
    assert!(matches!(
        actions.as_slice(),
        [Action::PersistFrame { .. }, Action::SampleComplete(_)]
    ));
    // Multiblock: the block timer runs from acceptance.
    let actions = feed(&mut driver, &block(A, "SYN00002", b'0'));
    assert!(matches!(
        actions.as_slice(),
        [Action::PersistFrame { .. }, Action::StartTimer { .. }]
    ));
    let (stale_timer, _) = timer(&actions);
    // Persisted is optional and harmless.
    assert!(persisted(&mut driver, persist_id(&actions)).is_empty());
    let actions = feed(&mut driver, &block(A, "SYN00002", b'E'));
    assert!(matches!(
        actions.as_slice(),
        [
            Action::StopTimer,
            Action::PersistFrame { .. },
            Action::SampleComplete(_)
        ]
    ));
    // The earlier timer is stale now.
    assert!(expire(&mut driver, stale_timer).is_empty());
    // A corrupted frame cannot be retransmitted: the partial sample is dropped, no NAK.
    feed(&mut driver, &block(A, "SYN00003", b'0'));
    let actions = feed(&mut driver, &corrupted(block(A, "SYN00003", b'E')));
    assert_eq!(
        actions,
        vec![
            Action::StopTimer,
            Action::Aborted(AbortReason::ProtocolError)
        ]
    );
    // Block timeout works the same way.
    let actions = feed(&mut driver, &block(A, "SYN00004", b'0'));
    assert_eq!(
        release(&mut driver, &actions),
        vec![Action::Aborted(AbortReason::BlockTimeout)]
    );
}

#[test]
fn stale_timers_and_unknown_frames_are_ignored() {
    let mut driver = driver(B);
    let actions = feed(&mut driver, &block(B, "SYN00001", b'E'));
    let id = persist_id(&actions);
    let (timer_id, _) = timer(&actions);
    persisted(&mut driver, id);
    assert!(!expire(&mut driver, timer_id).is_empty());
    // Replays of the same ids do nothing.
    assert!(expire(&mut driver, timer_id).is_empty());
    assert!(persisted(&mut driver, id).is_empty());
    // A replayed turnaround id must not release a later response early.
    let actions = feed(&mut driver, &block(B, "SYN00002", b'E'));
    persisted(&mut driver, persist_id(&actions));
    assert!(expire(&mut driver, timer_id).is_empty());
}

#[test]
fn frames_arriving_during_a_response_are_processed_in_order_afterwards() {
    let mut driver = driver(B);
    let mut both = block(B, "SYN00001", b'E');
    both.extend(block(B, "SYN00002", b'E'));
    let actions = feed(&mut driver, &both);
    assert_eq!(persists(&actions), 1);
    persisted(&mut driver, persist_id(&actions));
    let actions = release(&mut driver, &actions);
    let [
        Action::Send(ControlResponse::Ack),
        Action::SampleComplete(first),
        Action::PersistFrame { .. },
        Action::StartTimer { .. },
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(first.header.sample_id.expose(), b"SYN00001");
}

#[test]
fn queue_overflow_closes_the_driver() {
    let mut driver = driver(B);
    let frame = block(B, "SYN00001", b'E');
    let flood: Vec<u8> = std::iter::repeat_n(frame, MAX_QUEUED_EVENTS + 3)
        .flatten()
        .collect();
    let actions = feed(&mut driver, &flood);
    assert!(actions.contains(&Action::Aborted(AbortReason::ProtocolError)));
    assert_eq!(
        driver.handle(ReceiveEvent::Bytes(Vec::new())),
        Err(DriverError::Closed)
    );
}

/// A driver whose frame timer (T2) is 500 ms.
fn with_frame_timeout(class: TransmissionClass) -> ReceiveDriver {
    let options = ReceiveOptions {
        frame_timeout: Duration::from_millis(500),
        ..ReceiveOptions::default()
    };
    ReceiveDriver::new(config(class), options).unwrap()
}

#[test]
fn a_frame_that_never_ends_is_discarded_after_the_frame_timeout() {
    let mut driver = with_frame_timeout(B);
    let wire = block(B, "SYN00001", b'E');
    let actions = feed(&mut driver, &wire[..wire.len() / 2]);
    assert_eq!(timer(&actions).1, Duration::from_millis(500));
    assert!(
        release(&mut driver, &actions).is_empty(),
        "no response for a frame that never ended"
    );
    // The stale half is gone: the whole frame, sent again, decodes on its own.
    let actions = deliver(&mut driver, &wire);
    assert!(matches!(actions.last(), Some(Action::SampleComplete(_))));
}

#[test]
fn the_next_block_trades_the_block_timer_for_the_frame_timer_and_back() {
    let mut driver = with_frame_timeout(B);
    let actions = deliver(&mut driver, &block(B, "SYN00001", b'0'));
    assert_eq!(timer(&actions).1, Duration::from_millis(9900), "T1 waits");
    let next = block(B, "SYN00001", b'1');
    let actions = feed(&mut driver, &next[..next.len() / 2]);
    assert_eq!(timer(&actions).1, Duration::from_millis(500), "T2 runs");
    // The block never ends: its half is dropped and the sample waits for a block again.
    let actions = release(&mut driver, &actions);
    assert_eq!(timer(&actions).1, Duration::from_millis(9900));
    assert_eq!(
        release(&mut driver, &actions),
        vec![Action::Aborted(AbortReason::BlockTimeout)]
    );
}

#[test]
fn options_and_config_are_validated() {
    let ok = |ms| {
        ReceiveDriver::new(
            config(B),
            ReceiveOptions {
                block_timeout: Duration::from_millis(ms),
                ..ReceiveOptions::default()
            },
        )
    };
    assert!(ok(100).is_ok());
    assert!(ok(9900).is_ok());
    for ms in [0, 50, 150, 10_000] {
        assert_eq!(ok(ms).err(), Some(DriverError::BlockTimeout));
    }
    for ms in [0, 50, 150, 10_000] {
        let options = ReceiveOptions {
            frame_timeout: Duration::from_millis(ms),
            ..ReceiveOptions::default()
        };
        assert_eq!(
            ReceiveDriver::new(config(B), options).err(),
            Some(DriverError::FrameTimeout)
        );
    }
    let mut bad = config(B);
    bad.framing.max_wire_len = 255;
    assert_eq!(
        ReceiveDriver::new(bad, ReceiveOptions::default()).err(),
        Some(DriverError::Configuration(AuError::Configuration))
    );
    assert_eq!(ReceiveOptions::default().max_failures, 4);
}

#[test]
fn debug_output_never_contains_message_content() {
    let mut driver = driver(B);
    let wire = block(B, "SYN00001", b'E');
    let event = ReceiveEvent::Bytes(wire.clone());
    let actions = feed(&mut driver, &wire);
    persisted(&mut driver, persist_id(&actions));
    let released = release(&mut driver, &actions);
    let text = format!("{event:?} {actions:?} {released:?} {driver:?}");
    assert!(!text.contains("SYN00001"));
    assert!(!text.contains("012.34"));
}

#[derive(Clone, Debug)]
enum Op {
    Bytes(Vec<u8>),
    Frame(usize),
    Persisted(usize),
    Timer(usize),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..600).prop_map(Op::Bytes),
        (0usize..12).prop_map(Op::Frame),
        any::<usize>().prop_map(Op::Persisted),
        any::<usize>().prop_map(Op::Timer),
    ]
}

/// The event that answers `action`: storage confirmed, turnaround expired. The block and
/// frame timers (9.9 s by default) are left running: the stream is never that slow.
fn answer(action: &Action) -> Option<ReceiveEvent> {
    match action {
        Action::PersistFrame { frame_id, .. } => Some(ReceiveEvent::Persisted(*frame_id)),
        Action::StartTimer { timer_id, duration } if *duration < Duration::from_secs(1) => {
            Some(ReceiveEvent::TimerExpired(*timer_id))
        }
        _ => None,
    }
}

/// The sample id of a completed sample.
fn completed_sample_id(action: &Action) -> Option<Vec<u8>> {
    match action {
        Action::SampleComplete(sample) => Some(sample.header.sample_id.expose().clone()),
        _ => None,
    }
}

/// Feeds `chunks`, answering every storage request and turnaround timer at once; returns
/// the sample ids completed, in order.
fn completed_samples(class: TransmissionClass, chunks: &[&[u8]]) -> Vec<Vec<u8>> {
    let mut driver = driver(class);
    let mut done = Vec::new();
    for chunk in chunks {
        let mut pending = vec![ReceiveEvent::Bytes(chunk.to_vec())];
        while let Some(event) = pending.pop() {
            let actions = driver.handle(event).unwrap();
            done.extend(actions.iter().filter_map(completed_sample_id));
            pending.extend(actions.iter().filter_map(answer));
        }
    }
    done
}

proptest! {
    #[test]
    fn random_chunking_never_loses_a_sample(
        count in 1usize..6,
        cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..12),
        class_a in any::<bool>(),
    ) {
        let class = if class_a { A } else { B };
        let ids: Vec<String> = (1..=count).map(|n| format!("SYN{n:05}")).collect();
        let stream: Vec<u8> = ids.iter().flat_map(|id| block(class, id, b'E')).collect();
        let mut points: Vec<usize> = cuts.iter().map(|cut| cut.index(stream.len() + 1)).collect();
        points.sort_unstable();
        let mut chunks = Vec::new();
        let mut start = 0;
        for point in points {
            chunks.push(&stream[start..point]);
            start = point;
        }
        chunks.push(&stream[start..]);
        let expected: Vec<Vec<u8>> = ids.iter().map(|id| id.as_bytes().to_vec()).collect();
        prop_assert_eq!(completed_samples(class, &chunks), expected);
    }

    #[test]
    fn arbitrary_events_never_panic(
        ops in prop::collection::vec(op(), 0..80),
        class_a in any::<bool>(),
    ) {
        let class = if class_a { A } else { B };
        let fixtures = [
            block(class, "SYN00001", b'0'),
            block(class, "SYN00001", b'1'),
            block(class, "SYN00001", b'E'),
            block(class, "SYN00002", b'0'),
            block(class, "SYN00002", b'E'),
            text_frame(class, b"DB01"),
            text_frame(class, b"DE01"),
            text_frame(class, b"R 01000101U0001SYN00001"),
            text_frame(class, b"ZZ01"),
            vec![6],
            vec![21],
            vec![2, 3, 3],
        ];
        let mut driver = ReceiveDriver::new(config(class), ReceiveOptions::default()).unwrap();
        let mut frames: Vec<FrameId> = Vec::new();
        let mut timers: Vec<TimerId> = Vec::new();
        for op in ops {
            let event = match op {
                Op::Bytes(bytes) => ReceiveEvent::Bytes(bytes),
                Op::Frame(n) => ReceiveEvent::Bytes(fixtures[n % fixtures.len()].clone()),
                Op::Persisted(n) if !frames.is_empty() => {
                    ReceiveEvent::Persisted(frames[n % frames.len()])
                }
                Op::Timer(n) if !timers.is_empty() => {
                    ReceiveEvent::TimerExpired(timers[n % timers.len()])
                }
                Op::Persisted(_) | Op::Timer(_) => continue,
            };
            match driver.handle(event) {
                Ok(actions) => {
                    for action in actions {
                        match action {
                            Action::PersistFrame { frame_id, .. } => frames.push(frame_id),
                            Action::StartTimer { timer_id, .. } => timers.push(timer_id),
                            Action::Send(_)
                            | Action::SampleComplete(_)
                            | Action::Control { .. }
                            | Action::Unparsed { .. }
                            | Action::Aborted(_)
                            | Action::StopTimer => {}
                        }
                    }
                }
                Err(error) => prop_assert_eq!(error, DriverError::Closed),
            }
        }
    }
}
