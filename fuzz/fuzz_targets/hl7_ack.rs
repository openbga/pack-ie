//! build_ack on parsed messages. The first eight input bytes choose MSH-15, MSH-16, the
//! version, the stage, the outcome and the options; the rest is the source message, so any
//! HL7 fixture preceded by eight bytes is a seed.
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_hl7::Message;
use packie_hl7::ack::{
    Ack, AckMode, AckOptions, AckOutcome, AckStage, ErrorDetails, ErrorLocation, ErrorSeverity,
    build_ack,
};

const POLICIES: [&str; 8] = ["AL", "NE", "ER", "SU", "", "XX", "al", "AL~NE"];
const VERSIONS: [&str; 8] = ["2.3", "2.3.1", "2.4", "2.5", "2.5.1", "2.8.2", "9", "2."];
const TEXTS: [&str; 6] = ["", "ok", "a|b^c~d\\e&f", "line\rbreak", "\\X0D\\", "é\u{0}"];
const NUMBERS: [usize; 5] = [0, 1, 2, 65_535, usize::MAX];

fn pick<T: Copy>(table: &[T], index: u8) -> T {
    table[usize::from(index) % table.len()]
}

fuzz_target!(|data: &[u8]| {
    let Some((control, message)) = data.split_first_chunk::<8>() else {
        return;
    };
    let Ok(mut source) = Message::parse(message) else {
        return;
    };
    // Unusable edits (a field that cannot be written) are skipped.
    if control[0] & 0x80 != 0 {
        let _ = source.set("MSH-15", pick(&POLICIES, control[0]));
    }
    if control[1] & 0x80 != 0 {
        let _ = source.set("MSH-16", pick(&POLICIES, control[1]));
    }
    if control[2] & 0x80 != 0 {
        let _ = source.set("MSH-12", pick(&VERSIONS, control[2]));
    }
    let _ = AckMode::from_message(&source);

    let stage = if control[3] & 1 == 0 {
        AckStage::Application
    } else {
        AckStage::Commit
    };
    let outcome = match control[4] % 3 {
        0 => AckOutcome::Accepted,
        1 => AckOutcome::Error,
        _ => AckOutcome::Rejected,
    };
    let text = pick(&TEXTS, control[5]);
    let error = (control[5] & 0x80 != 0).then(|| ErrorDetails {
        code: pick(&TEXTS, control[6]),
        description: (control[6] & 0x40 != 0).then(|| pick(&TEXTS, control[6] >> 3)),
        coding_system: (control[6] & 0x20 != 0).then(|| pick(&TEXTS, control[6] >> 2)),
        severity: match control[6] % 4 {
            0 => ErrorSeverity::Information,
            1 => ErrorSeverity::Warning,
            2 => ErrorSeverity::Error,
            _ => ErrorSeverity::Fatal,
        },
        location: (control[7] & 0x80 != 0).then(|| ErrorLocation {
            segment: pick(&TEXTS, control[7]),
            segment_sequence: Some(pick(&NUMBERS, control[7])),
            field: Some(pick(&NUMBERS, control[7] >> 1)),
            repetition: Some(pick(&NUMBERS, control[7] >> 2)),
            component: (control[7] & 0x40 != 0).then(|| pick(&NUMBERS, control[7] >> 3)),
            subcomponent: None,
        }),
        diagnostic: (control[7] & 0x20 != 0).then(|| pick(&TEXTS, control[7])),
        user_message: (control[7] & 0x10 != 0).then(|| pick(&TEXTS, control[7] >> 1)),
    });
    let options = AckOptions {
        control_id: if control[3] & 2 == 0 { "ACK-1" } else { "" },
        timestamp: if control[3] & 4 == 0 {
            "20261012120000"
        } else {
            ""
        },
        text: (control[3] & 8 != 0).then_some(text),
        error,
    };
    let Ok(Some(reply)) = build_ack(&source, stage, outcome, &options) else {
        return;
    };
    // The acknowledgement we built must parse and read back as an acknowledgement of the source.
    let reparsed = Message::parse(&reply.encode()).expect("a built ACK parses");
    if let Ok(ack) = Ack::from_message(&reparsed) {
        assert_eq!(Some(ack.control_id), source.get("MSH-10"));
    }
});
