//! mllp::Decoder fed the input in chunks whose sizes come from the input itself, with a
//! small payload limit so the oversize paths are reached. The same bytes fed one byte at a
//! time must produce the same events (chunking must not change the framing).
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_hl7::mllp::{self, Decoder, Event};

fn decoder(limit: usize, optional_cr: bool) -> Decoder {
    let decoder = Decoder::new(limit);
    if optional_cr {
        decoder.with_optional_end_cr()
    } else {
        decoder
    }
}

fn run(
    data: &[u8],
    limit: usize,
    optional_cr: bool,
    chunks: impl Iterator<Item = usize>,
) -> Vec<Event> {
    let mut decoder = decoder(limit, optional_cr);
    let mut events = Vec::new();
    let mut rest = data;
    for size in chunks {
        let (chunk, tail) = rest.split_at(size.min(rest.len()));
        decoder.feed(chunk, |event| events.push(event));
        rest = tail;
        assert!(decoder.buffered_len() <= limit);
    }
    decoder.feed(rest, |event| events.push(event));
    events.extend(decoder.finish());
    events
}

/// Adjacent garbage runs are split at chunk boundaries; merge them to compare.
fn normalize(events: Vec<Event>) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    for event in events {
        match (out.last_mut(), event) {
            (Some(Event::Garbage { bytes }), Event::Garbage { bytes: more }) => *bytes += more,
            (_, event) => out.push(event),
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let Some((&control, data)) = data.split_first() else {
        return;
    };
    let limit = usize::from(control & 0x3f) * 4 + 1;
    let optional_cr = control & 0x80 != 0;
    let sizes = data.iter().map(|byte| usize::from(*byte & 0x1f));
    let chunked = normalize(run(data, limit, optional_cr, sizes));
    let bytewise = normalize(run(
        data,
        limit,
        optional_cr,
        std::iter::repeat_n(1, data.len()),
    ));
    assert_eq!(chunked, bytewise);

    for event in &chunked {
        if let Event::Message(payload) = event {
            assert!(payload.len() <= limit);
            if let Ok(frame) = mllp::encode(payload, limit) {
                let mut decoder = decoder(limit, optional_cr);
                let mut again = Vec::new();
                decoder.feed(&frame, |event| again.push(event));
                assert_eq!(again, [Event::Message(payload.clone())]);
            }
        }
    }
});
