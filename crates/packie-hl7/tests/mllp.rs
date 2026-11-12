use packie_hl7::mllp::{Decoder, EncodeError, Event, MalformedReason, State, encode};
use proptest::prelude::*;

fn feed(decoder: &mut Decoder, input: &[u8], events: &mut Vec<Event>) {
    decoder.feed(input, |event| events.push(event));
}

#[test]
fn preserves_payload_and_accepts_every_chunk_boundary() {
    let payload = b"MSH|^~\\&|SYNTHETIC\rPID|1\r";
    let frame = encode(payload, payload.len()).unwrap();
    for boundary in 0..=frame.len() {
        let mut decoder = Decoder::new(payload.len());
        let mut events = Vec::new();
        feed(&mut decoder, &frame[..boundary], &mut events);
        feed(&mut decoder, &frame[boundary..], &mut events);
        assert_eq!(events, vec![Event::Message(payload.to_vec())]);
        assert_eq!(decoder.finish(), None);
    }
}

#[test]
fn reports_garbage_and_multiple_frames_in_order() {
    let mut decoder = Decoder::new(8);
    let mut events = Vec::new();
    feed(&mut decoder, b"junk\x0bA\x1c\r\x0bB\x1c\r!", &mut events);
    assert_eq!(
        events,
        vec![
            Event::Garbage { bytes: 4 },
            Event::Message(b"A".to_vec()),
            Event::Message(b"B".to_vec()),
            Event::Garbage { bytes: 1 },
        ]
    );
}

#[test]
fn oversize_is_reported_once_and_recovers_at_trailer_or_new_start() {
    let mut decoder = Decoder::new(3);
    let mut events = Vec::new();
    feed(&mut decoder, b"\x0b1234more\x1c", &mut events);
    assert_eq!(decoder.buffered_len(), 0);
    assert_eq!(decoder.state(), State::DiscardingAwaitingCarriageReturn);
    feed(
        &mut decoder,
        b"\r\x0b123\x1c\r\x0b12345\x0bOK\x1c\r",
        &mut events,
    );
    assert_eq!(
        events,
        vec![
            Event::Oversize { limit: 3 },
            Event::Message(b"123".to_vec()),
            Event::Oversize { limit: 3 },
            Event::Message(b"OK".to_vec()),
        ]
    );
    assert_eq!(decoder.finish(), None);
}

#[test]
fn malformed_frames_recover_without_delivering_partial_payloads() {
    let mut decoder = Decoder::new(16);
    let mut events = Vec::new();
    feed(
        &mut decoder,
        b"\x0bold\x0bnew\x1cXtail\x0bgood\x1c\r",
        &mut events,
    );
    assert_eq!(
        events,
        vec![
            Event::Malformed {
                reason: MalformedReason::UnexpectedStartBlock,
                buffered_bytes: 3,
            },
            Event::Malformed {
                reason: MalformedReason::MissingCarriageReturn,
                buffered_bytes: 3,
            },
            Event::Garbage { bytes: 5 },
            Event::Message(b"good".to_vec()),
        ]
    );
    feed(&mut decoder, b"\x0blost\x1c\x0bkept\x1c\r", &mut events);
    assert_eq!(
        &events[4..],
        &[
            Event::Malformed {
                reason: MalformedReason::UnexpectedStartBlock,
                buffered_bytes: 4,
            },
            Event::Message(b"kept".to_vec()),
        ]
    );
}

#[test]
fn optional_end_cr_accepts_frames_with_and_without_the_trailing_cr_at_every_boundary() {
    let stream = b"\x0bA\x1c\x0bB\x1c\r\x0bC\x1c";
    for boundary in 0..=stream.len() {
        let mut decoder = Decoder::new(8).with_optional_end_cr();
        let mut events = Vec::new();
        feed(&mut decoder, &stream[..boundary], &mut events);
        feed(&mut decoder, &stream[boundary..], &mut events);
        assert_eq!(
            events,
            vec![
                Event::Message(b"A".to_vec()),
                Event::Message(b"B".to_vec()),
                Event::Message(b"C".to_vec()),
            ],
            "boundary {boundary}"
        );
        assert_eq!(decoder.finish(), None);
    }
}

#[test]
fn optional_end_cr_swallows_one_cr_and_reports_other_bytes_as_garbage() {
    let mut decoder = Decoder::new(8).with_optional_end_cr();
    let mut events = Vec::new();
    feed(&mut decoder, b"\x0bA\x1c\r\r!\x0bB\x1c", &mut events);
    assert_eq!(
        events,
        vec![
            Event::Message(b"A".to_vec()),
            Event::Garbage { bytes: 2 },
            Event::Message(b"B".to_vec()),
        ]
    );
}

#[test]
fn optional_end_cr_recovers_from_an_oversize_frame_at_its_end_byte() {
    let mut decoder = Decoder::new(3).with_optional_end_cr();
    let mut events = Vec::new();
    feed(&mut decoder, b"\x0b12345\x1c\x0bOK\x1c", &mut events);
    assert_eq!(
        events,
        vec![Event::Oversize { limit: 3 }, Event::Message(b"OK".to_vec())]
    );
}

#[test]
fn the_default_decoder_still_requires_the_cr() {
    let mut decoder = Decoder::new(8);
    let mut events = Vec::new();
    feed(&mut decoder, b"\x0bA\x1c\x0bB\x1c\r", &mut events);
    assert_eq!(
        events,
        vec![
            Event::Malformed {
                reason: MalformedReason::UnexpectedStartBlock,
                buffered_bytes: 1,
            },
            Event::Message(b"B".to_vec()),
        ]
    );
}

#[test]
fn finish_reports_each_unfinished_state_and_resets() {
    for (input, state, buffered_bytes) in [
        (&b"\x0b"[..], State::InFrame, 0),
        (&b"\x0ba"[..], State::InFrame, 1),
        (&b"\x0ba\x1c"[..], State::AwaitingCarriageReturn, 1),
        (&b"\x0bab"[..], State::Discarding, 0),
        (
            &b"\x0bab\x1c"[..],
            State::DiscardingAwaitingCarriageReturn,
            0,
        ),
    ] {
        let mut decoder = Decoder::new(1);
        decoder.feed(input, |_| {});
        assert_eq!(
            decoder.finish(),
            Some(Event::Truncated {
                state,
                buffered_bytes
            })
        );
        assert_eq!(decoder.buffered_len(), 0);
        assert_eq!(decoder.state(), State::SeekingStart);
        assert_eq!(decoder.finish(), None);
        let mut events = Vec::new();
        feed(&mut decoder, b"\x0ba\x1c\r", &mut events);
        assert_eq!(events, vec![Event::Message(b"a".to_vec())]);
    }
}

#[test]
fn zero_limit_accepts_empty_frames_only() {
    let mut decoder = Decoder::new(0);
    let mut events = Vec::new();
    feed(
        &mut decoder,
        b"\x0b\x1c\r\x0bx\x1c\r\x0b\x1c\r",
        &mut events,
    );
    assert_eq!(
        events,
        vec![
            Event::Message(vec![]),
            Event::Oversize { limit: 0 },
            Event::Message(vec![])
        ]
    );
    assert_eq!(encode(b"", 0).unwrap(), b"\x0b\x1c\r");
}

#[test]
fn encoder_rejects_oversize_and_reserved_bytes() {
    assert!(matches!(
        encode(b"abcd", 3),
        Err(EncodeError::Oversize {
            length: 4,
            limit: 3
        })
    ));
    for byte in [0x0b, 0x1c] {
        assert!(matches!(
            encode(&[b'a', byte, b'b'], 3),
            Err(EncodeError::ReservedByte { byte: found, offset: 1 }) if found == byte
        ));
    }
}

#[test]
fn giant_input_does_not_become_a_giant_buffer_or_event_queue() {
    let input = vec![b'x'; 1024 * 1024];
    let mut decoder = Decoder::new(8);
    let mut event_count = 0;
    decoder.feed(b"\x0b", |_| {});
    decoder.feed(&input, |event| {
        assert_eq!(event, Event::Oversize { limit: 8 });
        event_count += 1;
    });
    assert_eq!(event_count, 1);
    assert_eq!(decoder.buffered_len(), 0);
    decoder.feed(b"\x1c\r", |_| {});
    decoder.feed(&input, |event| {
        assert_eq!(event, Event::Garbage { bytes: input.len() });
        event_count += 1;
    });
    assert_eq!(event_count, 2);
    assert_eq!(decoder.finish(), None);
    // Even a deliberately huge configured limit does not allocate at construction.
    assert_eq!(Decoder::new(usize::MAX).buffered_len(), 0);
}

fn payload_byte() -> impl Strategy<Value = u8> {
    prop_oneof![0u8..=10, 12u8..=27, 29u8..=255]
}

proptest! {
    #[test]
    fn random_chunking_never_loses_or_duplicates_frames(
        payloads in prop::collection::vec(prop::collection::vec(payload_byte(), 0..128), 0..16),
        chunks in prop::collection::vec(1usize..64, 1..64),
    ) {
        let mut stream = Vec::new();
        for payload in &payloads {
            stream.extend(encode(payload, 128).unwrap());
        }
        let mut decoder = Decoder::new(128);
        let mut events = Vec::new();
        let mut offset = 0;
        for length in chunks.iter().cycle() {
            if offset == stream.len() { break; }
            let end = (offset + length).min(stream.len());
            feed(&mut decoder, &stream[offset..end], &mut events);
            prop_assert!(decoder.buffered_len() <= decoder.max_payload_len());
            offset = end;
        }
        let expected: Vec<_> = payloads.into_iter().map(Event::Message).collect();
        prop_assert_eq!(events, expected);
        prop_assert_eq!(decoder.finish(), None);
    }

    #[test]
    fn arbitrary_streams_stay_bounded_and_recover(
        stream in prop::collection::vec(any::<u8>(), 0..4096),
        limit in 0usize..128,
        chunk_len in 1usize..128,
    ) {
        let mut decoder = Decoder::new(limit);
        for chunk in stream.chunks(chunk_len) {
            decoder.feed(chunk, |event| {
                if let Event::Message(payload) = event {
                    assert!(payload.len() <= limit);
                    assert!(!payload.contains(&0x0b));
                    assert!(!payload.contains(&0x1c));
                }
            });
            prop_assert!(decoder.buffered_len() <= limit);
        }
        // A new start byte must recover from every possible partial/rejected state.
        let mut messages = Vec::new();
        decoder.feed(b"\x0b\x1c\r", |event| {
            if let Event::Message(payload) = event { messages.push(payload); }
        });
        prop_assert_eq!(messages, vec![Vec::<u8>::new()]);
        prop_assert_eq!(decoder.finish(), None);
        prop_assert_eq!(decoder.buffered_len(), 0);
    }
}
