use packie_hl7::{EncodingCharacters, Error, Message, Path, decode, escape};
use proptest::prelude::*;

const HEADER: &[u8] = b"MSH|^~\\&|SENDER|LAB|RECEIVER|DEST|20260101120000||ORU^R01|id|T|2.5.1\r";

fn message_with(segment: &[u8]) -> Result<Message, Error> {
    let mut bytes = HEADER.to_vec();
    bytes.extend_from_slice(segment);
    Message::parse(&bytes)
}

#[test]
fn synthetic_corpus_is_lossless_across_message_types_and_versions() {
    for version in ["2.3", "2.4", "2.5.1"] {
        for kind in ["ORU", "ORM", "OML", "ORL", "ADT", "ACK"] {
            check_synthetic_fixture(version, kind).unwrap();
        }
    }
}

fn check_synthetic_fixture(version: &str, kind: &str) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let file = root.join(format!(
        "{}_{}.hl7",
        kind.to_lowercase(),
        version.replace('.', "_")
    ));
    let bytes = std::fs::read(file)?;
    let message = Message::parse(&bytes)?;
    assert_eq!(message.encode(), bytes);
    assert_eq!(message.original_bytes(), bytes);
    for (path, expected) in [
        ("MSH-9.1", kind),
        ("MSH-12", version),
        ("MSH-1", "|"),
        ("MSH-2", "^~\\&"),
        ("ZLX-1[0].3.2", "sub"),
        ("ZLX-6", ""),
    ] {
        assert_eq!(message.get(path), Some(expected), "{path}");
    }
    if kind != "ACK" {
        assert_eq!(message.get("PID-3[1].1"), Some("P002"));
        assert_eq!(message.get("PID-14"), Some(""));
    }
    if kind == "ORU" {
        assert_eq!(message.get("OBX-5[0]"), Some("A|B^C&D~E\\F\r\nG\\ZLOCAL\\"));
        assert_eq!(message.get("OBX(2)-5.1"), Some("12.30"));
    }
    Ok(())
}

#[test]
fn malformed_corpus_has_typed_failures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (file, error) in [
        ("bad_msh.hl7", Error::MissingMsh),
        ("missing_msh2.hl7", Error::InvalidEncodingCharacters),
        ("truncated_header.hl7", Error::MissingHeaderField(9)),
        ("truncated_escape.hl7", Error::TruncatedEscape),
        ("wrong_separators.hl7", Error::InvalidEncodingCharacters),
        ("wrong_segment_separator.hl7", Error::InvalidSegment(2)),
        ("bad_hex_escape.hl7", Error::InvalidHexEscape),
    ] {
        assert_eq!(
            Message::parse(&std::fs::read(root.join(file)).unwrap()),
            Err(error),
            "{file}"
        );
    }
    assert!(matches!(
        Message::parse(&std::fs::read(root.join("control_character.hl7")).unwrap()),
        Err(Error::ControlCharacter(_))
    ));
}

#[test]
fn getters_and_setters_preserve_siblings_and_escape_literal_values() {
    let mut message =
        message_with(b"PID|||first^^^AUTH~second^name||Family^Given|\rOBX|1||||old\r").unwrap();
    let original = message.original_bytes().to_vec();
    message.set("PID-3[1].4.2", "Lab | East^&~\\\r\n").unwrap();
    message.set("OBX(2)-5.1", "new").unwrap();
    message.set("ZLX-3[2].2", "created").unwrap();
    assert_eq!(message.get("PID-3[0].4"), Some("AUTH"));
    assert_eq!(message.get("PID-3[1].1"), Some("second"));
    assert_eq!(message.get("PID-3[1].4.2"), Some("Lab | East^&~\\\r\n"));
    assert_eq!(message.get("PID-3[1].4.1"), Some(""));
    assert_eq!(message.get("OBX(2)-5"), Some("new"));
    assert_eq!(message.get("OBX-5"), Some("old"));
    assert_eq!(message.get("ZLX-3"), Some("~~^created"));
    let parsed = Message::parse(&message.encode()).unwrap();
    assert_eq!(parsed.get("PID-3[1].4.2"), message.get("PID-3[1].4.2"));
    assert_eq!(parsed.get("PID-5"), Some("Family^Given"));
    assert_eq!(message.original_bytes(), original);
    message.set("PID-3[1]", "literal^not-a-component").unwrap();
    assert_eq!(message.get("PID-3[1]"), Some("literal^not-a-component"));
    assert_eq!(message.get("PID-3[1].2"), None);
    message.set("PID-3", "one~literal").unwrap();
    assert_eq!(message.get("PID-3[0]"), Some("one~literal"));
    assert_eq!(message.get("PID-3[1]"), None);
}

#[test]
fn custom_delimiters_and_unknown_escapes_survive_edits() {
    let bytes = b"MSH*$%!?*S*L*R*L*20260101**ORU$R01*id*T*2.4\rPID***a!F!b!S!c!T!d!R!e!E!f!X4142!g!Zlocal*literal$inside!%next$component?sub*\r";
    let mut message = Message::parse(bytes).unwrap();
    assert_eq!(
        message.get("PID-3[0]"),
        Some("a*b$c?d%e!fABg!Zlocal*literal$inside!")
    );
    assert_eq!(message.get("PID-3[1].2.2"), Some("sub"));
    assert_eq!(message.encode(), bytes);
    message.set("PID-4", "*$$?!%\n").unwrap();
    let encoded = message.encode();
    assert!(
        encoded
            .windows(b"!Zlocal*literal$inside!".len())
            .any(|window| window == b"!Zlocal*literal$inside!")
    );
    let parsed = Message::parse(&encoded).unwrap();
    assert_eq!(parsed.get("PID-4"), Some("*$$?!%\n"));
    assert_eq!(parsed.get("PID-3[0]"), message.get("PID-3[0]"));
}

#[test]
fn newline_tolerance_is_lossless_until_an_edit() {
    let canonical = [HEADER, b"PID|||id||name|||\rZLX|\r"].concat();
    for terminator in ["\r", "\n", "\r\n"] {
        let bytes = String::from_utf8(canonical.clone())
            .unwrap()
            .replace('\r', terminator)
            .into_bytes();
        let mut message = Message::parse(&bytes).unwrap();
        assert_eq!(message.encode(), bytes);
        message.set("PID-3", "changed").unwrap();
        assert!(!message.encode().contains(&b'\n'));
        assert_eq!(message.get("PID-8"), Some(""));
    }
    let mut no_final_cr = canonical;
    no_final_cr.pop();
    assert_eq!(Message::parse(&no_final_cr).unwrap().encode(), no_final_cr);
}

#[test]
fn binary_values_remain_accessible_and_are_not_lossily_transcoded() {
    let message = message_with(b"PID|||\xff|\\XFE\\|\\XC3A9\\|\r").unwrap();
    assert_eq!(message.get("PID-3"), None);
    assert_eq!(message.get("PID-4"), None);
    assert_eq!(message.get("PID-5"), Some("é"));
    let leaf = &message
        .segment("PID", 1)
        .unwrap()
        .field(3)
        .unwrap()
        .repetitions()[0]
        .components()[0]
        .subcomponents()[0];
    assert_eq!(leaf.encoded(), b"\xff");
    assert_eq!(message.encode(), message.original_bytes());
}

#[test]
fn paths_reject_invalid_and_unbounded_indices() {
    for path in [
        "",
        "PID",
        "pid-1",
        "PID-0",
        "PID(0)-1",
        "PID-3[-1]",
        "PID-3[]",
        "PID-3.0",
        "PID-3.1.0",
        "PID-3.1.1.1",
        "PID-3x",
        "PID-3[1]junk",
        "PID(2)junk-3",
        "MSH(2)-9",
    ] {
        assert!(Path::parse(path).is_err(), "{path}");
    }
    for path in [
        "PID-65536",
        "PID-3[65536]",
        "PID(65536)-1",
        "PID-99999999999999999999999999999999999",
    ] {
        assert_eq!(Path::parse(path), Err(Error::PathIndexTooLarge));
    }
    let path = Path::parse("OBX(2)-5[1].4.2").unwrap();
    assert_eq!(
        (
            path.segment(),
            path.occurrence(),
            path.field(),
            path.repetition(),
            path.component(),
            path.subcomponent()
        ),
        ("OBX", 2, 5, Some(1), Some(4), Some(2))
    );
    let mut message = message_with(b"PID|1\r").unwrap();
    assert_eq!(message.get("PID(0)-1"), None);
    for path in ["MSH-1", "MSH-2", "MSH-2.1"] {
        assert_eq!(
            message.set(path, "x"),
            Err(Error::ImmutableEncodingCharacters)
        );
    }
    assert_eq!(message.segment("MSH", 0), None);
    assert_eq!(message.segment("MSH", 1).unwrap().field(0), None);
}

#[test]
fn bad_headers_and_segments_are_rejected_without_panics() {
    for bytes in [
        b"".as_slice(),
        b"MSH",
        b"MSH|",
        b"MSH|^~\\&",
        b"MSH|^~\\&|",
        b"MSH\r^~\\&|",
    ] {
        assert!(Message::parse(bytes).is_err());
    }
    for segment in [
        b"P\r".as_slice(),
        b"pid|1\r",
        b"PID^1\r",
        b"MSH|^~\\&|\r",
        b"\r",
    ] {
        let mut bytes = HEADER.to_vec();
        bytes.extend_from_slice(segment);
        assert!(Message::parse(&bytes).is_err());
    }
    for value in [b"\\X\\".as_slice(), b"\\X0\\", b"\\XGG\\", b"\\unfinished"] {
        assert!(decode(value, EncodingCharacters::default()).is_err());
    }
}

proptest! {
    #[test]
    fn escape_round_trip(value in any::<String>()) {
        for encoding in [EncodingCharacters::default(), EncodingCharacters::new(b'*',b'$',b'%',b'!',b'?').unwrap()] {
            let escaped = escape(&value, encoding);
            prop_assert_eq!(decode(&escaped, encoding).unwrap(), value.as_bytes());
            prop_assert!(!escaped.iter().any(u8::is_ascii_control));
        }
    }

    #[test]
    fn parse_encode_identity_and_literal_edit_round_trip(
        values in prop::collection::vec(any::<String>(), 0..24),
        terminator in prop_oneof![Just("\r"), Just("\n"), Just("\r\n")],
    ) {
        let mut bytes = HEADER[..HEADER.len()-1].to_vec();
        bytes.extend_from_slice(terminator.as_bytes());
        bytes.extend_from_slice(b"ZLX|");
        for (index, value) in values.iter().enumerate() {
            if index != 0 { bytes.push(b'|'); }
            bytes.extend(escape(value, EncodingCharacters::default()));
        }
        bytes.extend_from_slice(terminator.as_bytes());
        let mut message = Message::parse(&bytes).unwrap();
        prop_assert_eq!(message.encode(), bytes);
        for (index, value) in values.iter().enumerate() {
            prop_assert_eq!(message.get(&format!("ZLX-{}", index+1)), Some(value.as_str()));
            message.set(&format!("PID-3[{index}].4.2"), value).unwrap();
        }
        let reparsed = Message::parse(&message.encode()).unwrap();
        for (index, value) in values.iter().enumerate() {
            prop_assert_eq!(reparsed.get(&format!("PID-3[{index}].4.2")), Some(value.as_str()));
        }
    }

    #[test]
    fn arbitrary_input_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(message) = Message::parse(&bytes) { prop_assert_eq!(message.encode(), bytes); }
    }

    #[test]
    fn arbitrary_body_never_panics(body in prop::collection::vec(any::<u8>(), 0..4096)) {
        let bytes = [HEADER, body.as_slice()].concat();
        if let Ok(message) = Message::parse(&bytes) { prop_assert_eq!(message.encode(), bytes); }
    }
}
