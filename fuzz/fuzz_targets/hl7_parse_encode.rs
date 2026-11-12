//! Message::parse on arbitrary bytes. Where parsing succeeds: encode is the identity, the
//! encoding parses back to an equal message, and re-writing every field with its own decoded
//! value (which forces the writer, not the verbatim original) preserves every value.
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_hl7::Message;

fn field_paths(message: &Message) -> Vec<(String, String)> {
    let mut paths = Vec::new();
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for segment in message.segments() {
        let name = segment.name();
        let occurrence = match seen.iter_mut().find(|(seen_name, _)| *seen_name == name) {
            Some((_, count)) => {
                *count += 1;
                *count
            }
            None => {
                seen.push((name, 1));
                1
            }
        };
        for (index, field) in segment.fields().iter().enumerate() {
            let number = index + 1;
            if name == "MSH" && number <= 2 {
                continue;
            }
            if let Some(value) = field.value() {
                paths.push((format!("{name}({occurrence})-{number}"), value.to_owned()));
            }
        }
    }
    paths
}

fuzz_target!(|data: &[u8]| {
    let Ok(message) = Message::parse(data) else {
        return;
    };
    let encoded = message.encode();
    assert_eq!(encoded, data, "an unedited message encodes to its input");
    assert_eq!(Message::parse(&encoded).as_ref(), Ok(&message));

    let paths = field_paths(&message);
    let mut edited = message.clone();
    for (path, value) in &paths {
        edited
            .set(path, value)
            .unwrap_or_else(|error| panic!("set {path}: {error}"));
    }
    let rewritten = edited.encode();
    let reparsed = Message::parse(&rewritten)
        .unwrap_or_else(|error| panic!("rewritten message does not parse: {error}"));
    for (path, value) in &paths {
        assert_eq!(reparsed.get(path), Some(value.as_str()), "{path}");
    }
    assert_eq!(reparsed.encode(), rewritten);
});
