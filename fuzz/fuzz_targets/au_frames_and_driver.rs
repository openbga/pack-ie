//! AU frames and the receive driver. The input starts with an instrument profile (usually a
//! valid one, sometimes arbitrary so configuration validation is exercised), then one codec
//! round, then steps: raw bytes, well-formed frames of plausible fixed-width text,
//! persistence confirmations and timer expiries replayed from what the driver issued. Every
//! frame the stream decoder yields goes through parse_message; encoded frames must decode.
#![no_main]

use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use packie_au::{
    Action, AuConfig, FormatConfig, FrameEnd, FrameId, FramingConfig, ReceiveDriver, ReceiveEvent,
    ReceiveOptions, Redacted, SampleHeader, StreamDecoder, TimerId, TransmissionClass, Tri,
    TriTest, WireEvent, encode_frame, encode_stop, encode_tri, parse_message,
};
use packie_fuzz::Reader;

const CODES: [&[u8]; 16] = [
    b"D ", b"DH", b"d ", b"dH", b"DR", b"DA", b"DQ", b"R ", b"RH", b"S ", b"SH", b"RB", b"RE",
    b"DB", b"DE", b"SE",
];

/// One to two valid delimiter bytes (1..=31 except ACK and NAK); any bytes when `wild`.
fn controls(input: &mut Reader<'_>, wild: bool) -> Option<Vec<u8>> {
    let len = if wild {
        input.range(0, 3)?
    } else {
        input.range(1, 2)?
    };
    (0..len)
        .map(|_| {
            if wild {
                input.u8()
            } else {
                input.choose(&[1, 2, 3, 4, 5, 7, 10, 13, 23, 31])
            }
        })
        .collect()
}

fn config(input: &mut Reader<'_>) -> Option<AuConfig> {
    let wild = input.u8()? % 16 == 0;
    Some(AuConfig {
        framing: FramingConfig {
            start: controls(input, wild)?,
            end: controls(input, wild)?,
            bcc: input.bool()?,
            use_etb: input.bool()?,
            max_wire_len: if wild {
                u16::from(input.u8()?) * 8
            } else {
                input.choose(&[256, 512, 1024])?
            },
            class: if input.bool()? {
                TransmissionClass::A
            } else {
                TransmissionClass::B
            },
        },
        format: FormatConfig {
            unit_no: input.bool()?,
            rack_digits: if wild {
                input.u8()?
            } else {
                input.choose(&[0, 4, 5])?
            },
            sample_id_digits: if wild {
                input.u8()?
            } else {
                input.range(1, 20)?
            },
            sex: input.bool()?,
            age: input.bool()?,
            month: input.bool()?,
            patient_info_digits: {
                let mut widths = [0_u8; 6];
                for width in &mut widths {
                    *width = if wild {
                        input.u8()?
                    } else {
                        input.range(0, 20)?
                    };
                }
                widths
            },
            result_digits: if wild {
                input.u8()?
            } else {
                input.choose(&[6, 9])?
            },
            test_code_digits: if wild {
                input.u8()?
            } else {
                input.choose(&[2, 3])?
            },
        },
    })
}

/// Fixed-width text: a message code, an optional unit number, then digit-heavy filler.
fn text(input: &mut Reader<'_>) -> Option<Vec<u8>> {
    let mut text = input.choose(&CODES)?.to_vec();
    if input.bool()? {
        text.extend_from_slice(b"01");
    }
    let len = usize::from(input.u8()?) + usize::from(input.u8()? % 2) * 256;
    for byte in input.take(len)? {
        text.push(if *byte < 160 {
            b'0' + byte % 10
        } else {
            0x20 + byte % 95
        });
    }
    Some(text)
}

fn field(input: &mut Reader<'_>, max: u8) -> Option<Redacted<Vec<u8>>> {
    let len = usize::from(input.range(0, max)?);
    Some(Redacted::new(input.take(len)?.to_vec()))
}

fn tri(input: &mut Reader<'_>) -> Option<Tri> {
    let header = SampleHeader {
        unit_no: None,
        rack: field(input, 6)?,
        cup: field(input, 3)?,
        sample_type: field(input, 2)?,
        sample_no: field(input, 5)?,
        sample_id: field(input, 22)?,
        reserved: Redacted::new(Vec::new()),
        demographics: None,
    };
    let count = input.range(0, 60)?;
    let mut tests = Vec::new();
    for _ in 0..count {
        tests.push(TriTest {
            code: field(input, 3)?,
            method: Some(input.u8()?).filter(|byte| *byte < 4),
        });
    }
    Some(Tri {
        rerun: input.bool()?,
        header,
        tests,
    })
}

#[derive(Default)]
struct Issued {
    frames: Vec<FrameId>,
    timers: Vec<TimerId>,
    pending: Vec<FrameId>,
}

fn absorb(issued: &mut Issued, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::PersistFrame { frame_id, .. } => {
                issued.frames.push(frame_id);
                issued.pending.push(frame_id);
            }
            Action::StartTimer { timer_id, .. } => issued.timers.push(timer_id),
            Action::Send(_)
            | Action::SampleComplete(_)
            | Action::Control { .. }
            | Action::Unparsed { .. }
            | Action::Aborted(_)
            | Action::StopTimer => {}
        }
    }
}

fn feed(driver: &mut ReceiveDriver, issued: &mut Issued, event: ReceiveEvent) {
    if let Ok(actions) = driver.handle(event) {
        absorb(issued, actions);
    }
}

fn decode(decoder: &mut StreamDecoder, config: &AuConfig, wire: &[u8]) {
    for event in decoder.push(wire) {
        if let Ok(WireEvent::Frame(frame)) = event {
            let _ = parse_message(config, &frame);
        }
    }
}

/// ETB marks a block only when the configured end is a sole ETX and ETB is on.
fn decoded_end(config: &AuConfig, end: FrameEnd) -> FrameEnd {
    if config.framing.use_etb && config.framing.end == [3] {
        end
    } else {
        FrameEnd::Text
    }
}

fn exercise_codec(config: &AuConfig, input: &mut Reader<'_>) -> Option<()> {
    let Ok(mut decoder) = StreamDecoder::new(config.clone()) else {
        return Some(());
    };
    let text = text(input)?;
    let end = if input.bool()? {
        FrameEnd::Text
    } else {
        FrameEnd::Block
    };
    if let Ok(wire) = encode_frame(config, &text, end) {
        for event in decoder.push(&wire) {
            if let Ok(WireEvent::Frame(frame)) = event {
                assert_eq!(frame.text.expose(), &text);
                assert_eq!(frame.end, decoded_end(config, end));
                let _ = parse_message(config, &frame);
            }
        }
        assert!(!decoder.is_pending());
    }
    if let Ok(wire) = encode_stop(config) {
        decode(&mut decoder, config, &wire);
    }
    let tri = tri(input)?;
    if let Ok(frames) = encode_tri(config, &tri) {
        for wire in frames {
            decode(&mut decoder, config, &wire);
        }
    }
    Some(())
}

fn run(data: &[u8]) -> Option<()> {
    let mut input = Reader::new(data);
    let config = config(&mut input)?;
    exercise_codec(&config, &mut input)?;
    let options = ReceiveOptions {
        block_timeout: Duration::from_millis(100 * u64::from(input.range(1, 99)?)),
        frame_timeout: Duration::from_millis(100 * u64::from(input.range(1, 99)?)),
        max_failures: input.range(1, 6)?,
    };
    let Ok(mut driver) = ReceiveDriver::new(config.clone(), options) else {
        return Some(());
    };
    let mut issued = Issued::default();
    for _ in 0..256 {
        match input.range(0, 7)? {
            0 | 1 => {
                let len = usize::from(input.range(0, 64)?);
                let bytes = input.take(len)?.to_vec();
                feed(&mut driver, &mut issued, ReceiveEvent::Bytes(bytes));
            }
            2 | 3 => {
                let text = text(&mut input)?;
                let end = if input.bool()? {
                    FrameEnd::Text
                } else {
                    FrameEnd::Block
                };
                if let Ok(wire) = encode_frame(&config, &text, end) {
                    feed(&mut driver, &mut issued, ReceiveEvent::Bytes(wire));
                }
            }
            4 => {
                let byte = input.choose(&[6, 21, 4, 3])?;
                feed(&mut driver, &mut issued, ReceiveEvent::Bytes(vec![byte]));
            }
            5 => {
                let index = usize::from(input.u8()?);
                let id = issued
                    .frames
                    .get(index % issued.frames.len().max(1))
                    .copied();
                if let Some(id) = id {
                    feed(&mut driver, &mut issued, ReceiveEvent::Persisted(id));
                }
            }
            6 => {
                let index = usize::from(input.u8()?);
                let id = issued
                    .timers
                    .get(index % issued.timers.len().max(1))
                    .copied();
                if let Some(id) = id {
                    feed(&mut driver, &mut issued, ReceiveEvent::TimerExpired(id));
                }
            }
            _ => {
                for id in std::mem::take(&mut issued.pending) {
                    feed(&mut driver, &mut issued, ReceiveEvent::Persisted(id));
                }
                if let Some(id) = issued.timers.last().copied() {
                    feed(&mut driver, &mut issued, ReceiveEvent::TimerExpired(id));
                }
            }
        }
    }
    Some(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(data);
});
