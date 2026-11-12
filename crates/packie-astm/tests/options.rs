//! `LinkOptions` that come from connector settings: the receiver's wait for the next
//! frame, the frame text size and the number of ENQ attempts.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::time::Duration;

use packie_astm::{
    Action, ENQ, Event, FrameEnd, LinkDriver, LinkOptions, MAX_LONG_FRAME_TEXT, NAK,
    RECEIVE_TIMEOUT, ReceiveAbortReason, encode_frame, encode_records, encode_records_within,
    parse_frame, parse_frame_within,
};

fn timer_durations(actions: &[Action]) -> Vec<Duration> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::StartTimer { duration, .. } => Some(*duration),
            _ => None,
        })
        .collect()
}

#[test]
fn the_receiver_waits_the_configured_time_for_the_next_frame() {
    let mut default = LinkDriver::new(LinkOptions::default());
    let actions = default.handle(Event::Bytes(vec![ENQ])).unwrap();
    assert_eq!(timer_durations(&actions), [RECEIVE_TIMEOUT]);

    let mut configured = LinkDriver::new(LinkOptions {
        receive_timeout: Duration::from_secs(7),
        ..LinkOptions::default()
    });
    let actions = configured.handle(Event::Bytes(vec![ENQ])).unwrap();
    assert_eq!(timer_durations(&actions), [Duration::from_secs(7)]);
}

#[test]
fn the_sender_splits_records_at_the_configured_frame_text_size() {
    let record = vec![b'R'; 150];
    let records = [record.clone()];
    let at_64 = encode_records_within(&records, 64).unwrap();
    // 150 bytes and the record's CR: 64 + 64 + 23.
    let texts: Vec<usize> = at_64
        .iter()
        .map(|f| parse_frame(f).unwrap().text().len())
        .collect();
    assert_eq!(texts, [64, 64, 23]);
    let reassembled: Vec<u8> = at_64
        .iter()
        .flat_map(|f| parse_frame(f).unwrap().text().to_vec())
        .collect();
    assert_eq!(&reassembled[..150], &record[..]);
    assert_eq!(
        encode_records_within(&records, 240).unwrap(),
        encode_records(&records).unwrap()
    );
    // 150 bytes fit one frame at any limit from 151 up.
    assert_eq!(
        encode_records_within(&records, 1000).unwrap(),
        encode_records(&records).unwrap()
    );
}

#[test]
fn the_frame_text_limit_applies_to_the_embedded_receiver() {
    let frame = encode_frame(1, &[b'A'; 300], FrameEnd::Final).unwrap();
    let naks = |actions: &[Action]| {
        actions
            .iter()
            .filter(|a| **a == Action::Write(vec![NAK]))
            .count()
    };

    let mut default = LinkDriver::new(LinkOptions::default());
    default.handle(Event::Bytes(vec![ENQ])).unwrap();
    let actions = default.handle(Event::Bytes(frame.clone())).unwrap();
    assert_eq!(naks(&actions), 1);
    assert!(actions.iter().all(|a| a.persistence_id().is_none()));

    let mut long = LinkDriver::new(LinkOptions {
        max_frame_text: 300,
        ..LinkOptions::default()
    });
    long.handle(Event::Bytes(vec![ENQ])).unwrap();
    let actions = long.handle(Event::Bytes(frame)).unwrap();
    assert_eq!(naks(&actions), 0);
    assert!(actions.iter().any(|a| a.persistence_id().is_some()));
}

#[test]
fn a_frame_over_the_configured_limit_ends_the_inbound_transfer_after_six_naks() {
    let mut link = LinkDriver::new(LinkOptions {
        max_frame_text: 300,
        ..LinkOptions::default()
    });
    link.handle(Event::Bytes(vec![ENQ])).unwrap();
    let frame = encode_frame(1, &[b'A'; 301], FrameEnd::Final).unwrap();
    let mut last = Vec::new();
    for _ in 0..6 {
        last = link.handle(Event::Bytes(frame.clone())).unwrap();
    }
    assert!(last.contains(&Action::ReceiveAborted(
        ReceiveAbortReason::RetryLimitExceeded
    )));
    assert!(!link.is_receiving());
}

#[test]
fn long_frames_are_written_whole_and_parse_within_their_limit() {
    let records = [vec![b'R'; 1000]];
    let frames = encode_records_within(&records, MAX_LONG_FRAME_TEXT).unwrap();
    assert_eq!(frames.len(), 1);
    assert!(parse_frame(&frames[0]).is_err());
    let frame = parse_frame_within(&frames[0], MAX_LONG_FRAME_TEXT).unwrap();
    assert_eq!(frame.text().len(), 1001);
    assert_eq!(frame.end(), FrameEnd::Final);
    // Settings above the maximum are clamped, not honoured.
    assert_eq!(encode_records_within(&records, usize::MAX).unwrap(), frames);
}
