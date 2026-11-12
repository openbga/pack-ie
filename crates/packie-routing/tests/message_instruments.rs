//! Instrument variants of the message channel connectors (`docs/spec/configuration.md`
//! §7): HL7 frames without the trailing CR, order responses, long ASTM frames, AU
//! three-digit test codes, and the shipped mapping presets through a running channel.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::*;
use packie_astm::{ACK, ENQ, EOT, LF, NAK, encode_records_within};
use packie_au::{AuConfig, FormatConfig, FrameEnd, FramingConfig, TransmissionClass, encode_frame};
use packie_config::profiles::catalog;
use packie_core::{AckLevel, DeliveryState, MessageState};
use packie_hl7::Message;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const SOURCE: &str = "127.0.0.1:9601";
const LIS: &str = "127.0.0.1:9701";

async fn rig(source: Value, destination: Value) -> MessageRig {
    MessageRig::start(hl7_document(hl7_channel(source, destination, json!({})))).await
}

fn oml(control: &str) -> Vec<u8> {
    format!(
        "MSH|^~\\&|LIS|HOSP|PACKIE|PACKIEFAC|20260101120000||OML^O33^OML_O33|{control}|P|2.5.1\rPID|1||PID-TEST-1\rSPM|1|SID-9\rORC|NW|PL-1\rOBR|1|PL-1||GLU^Glucose^L\r"
    )
    .into_bytes()
}

// ----- HL7 -----------------------------------------------------------------------------

#[tokio::test]
async fn a_frame_may_end_at_the_end_block_when_the_trailing_cr_is_optional() {
    let rig = rig(json!({"require_end_cr": false}), json!({})).await;
    let mut instrument = Peer::dial(&rig.net, SOURCE).await;

    let mut frame = vec![0x0b];
    frame.extend_from_slice(&oru("CTRL-A"));
    frame.push(0x1c);
    instrument.write_raw(&frame).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "CTRL-A".into()));

    // The standard form still works on the same connection, and a CR after the end block
    // is not mistaken for anything.
    instrument.send(&oru("CTRL-B")).await;
    assert_eq!(instrument.msa().await, ("AA".into(), "CTRL-B".into()));
    let messages = rig.messages_until("both committed", |m| m.len() == 2).await;
    assert_eq!(rig.body(messages[0].summary.message_id), oru("CTRL-A"));
}

#[tokio::test]
async fn a_frame_without_the_trailing_cr_is_not_accepted_by_default() {
    let rig = rig(json!({}), json!({})).await;
    let mut instrument = Peer::dial(&rig.net, SOURCE).await;
    let mut frame = vec![0x0b];
    frame.extend_from_slice(&oru("CTRL-C"));
    frame.push(0x1c);
    instrument.write_raw(&frame).await;
    assert!(instrument.frame_within(700).await.is_none());
    assert!(rig.messages().is_empty());
}

#[tokio::test]
async fn order_messages_are_answered_with_order_responses_when_configured() {
    let rig = rig(json!({"reply_type": "order_response"}), json!({})).await;
    let mut instrument = Peer::dial(&rig.net, SOURCE).await;

    instrument.send(&oml("ORD-1")).await;
    let reply = Message::parse(&instrument.frame().await).unwrap();
    assert_eq!(reply.get("MSH-9"), Some("ORL^O34^ORL_O34"));
    assert_eq!(reply.get("MSA-1"), Some("AA"));
    assert_eq!(reply.get("MSA-2"), Some("ORD-1"));

    // results are still acknowledged with an ACK
    instrument.send(&oru("CTRL-D")).await;
    let reply = Message::parse(&instrument.frame().await).unwrap();
    assert_eq!(reply.get("MSH-9"), Some("ACK^R01^ACK"));
}

async fn order_with_reply(destination: Value, reply_code: &str) -> Vec<DeliveryState> {
    let rig = rig(json!({}), destination).await;
    let (_listener, mut lis) = Peer::accept(&rig.net, LIS).await;
    let mut instrument = Peer::dial(&rig.net, SOURCE).await;
    instrument.send(&oml("ORD-2")).await;
    assert_eq!(instrument.msa().await.0, "AA");
    let sent = lis.frame().await;
    assert_eq!(sent, oml("ORD-2"));
    let response = format!(
        "MSH|^~\\&|AM|LAB|LIS|HOSP|20260101120001||ORL^O34^ORL_O34|R-1|P|2.5.1\rMSA|{reply_code}|ORD-2\r"
    );
    lis.send(response.as_bytes()).await;
    let deliveries = rig
        .deliveries_until("the delivery ends", |d| {
            d.first()
                .is_some_and(|d| !matches!(d.state, DeliveryState::Queued | DeliveryState::Sending))
        })
        .await;
    if deliveries[0].state == DeliveryState::Acknowledged {
        assert_eq!(deliveries[0].ack_level, AckLevel::Accepted);
    }
    deliveries.iter().map(|d| d.state).collect()
}

#[tokio::test]
async fn an_order_response_acknowledges_a_destination_that_lists_its_type() {
    let states = order_with_reply(json!({"ack_message_types": ["ACK", "ORL"]}), "AA").await;
    assert_eq!(states, [DeliveryState::Acknowledged]);
    let states = order_with_reply(json!({"ack_message_types": ["ORL"]}), "AR").await;
    assert_eq!(states, [DeliveryState::Rejected]);
}

#[tokio::test]
async fn an_order_response_is_not_an_acknowledgement_unless_listed() {
    // Not listed: the reply is ignored, no acknowledgement arrives in time.
    let states = order_with_reply(json!({}), "AA").await;
    assert_eq!(states, [DeliveryState::TimedOut]);
}

// ----- ASTM long frames ----------------------------------------------------------------

struct Line {
    stream: DuplexStream,
}

impl Line {
    async fn byte_within(&mut self, ms: u64) -> Option<u8> {
        let mut byte = [0_u8; 1];
        match tokio::time::timeout(Duration::from_millis(ms), self.stream.read(&mut byte)).await {
            Ok(Ok(1)) => Some(byte[0]),
            _ => None,
        }
    }
}

fn long_records() -> Vec<Vec<u8>> {
    let comment = format!("C|1|I|{}", "x".repeat(600));
    [
        "H|\\^&|CTRL-L||SIMANALYZER^1.0|||||||P|1|20260101120000",
        "P|1|PID-TEST-L",
        "O|1|SID-L||^^^GLU",
        "R|1|^^^GLU|5.4|mmol/L",
        &comment,
        "L|1|N",
    ]
    .into_iter()
    .map(|r| r.as_bytes().to_vec())
    .collect()
}

async fn astm_rig(source_settings: Value) -> MessageRig {
    let channel = json!({
        "kind": "message", "channel_id": "results", "label": "Results",
        "source": {"endpoint": "analyzer", "protocol": "astm", "settings": source_settings},
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-astm",
            "protocol": "astm", "settings": {},
        }],
        "retry": quick_retry(),
    });
    MessageRig::start(document(
        vec![listener("analyzer", 9641), client("lis-astm", 9741)],
        vec![channel],
    ))
    .await
}

#[tokio::test]
async fn a_long_frame_is_committed_and_acknowledged_when_the_limit_allows_it() {
    let rig = astm_rig(json!({"max_frame_chars": 1000})).await;
    let mut line = Line {
        stream: dial(&rig.net, "127.0.0.1:9641").await,
    };
    let records = long_records();
    line.stream.write_all(&[ENQ]).await.unwrap();
    assert_eq!(line.byte_within(3000).await, Some(ACK));
    let frames = encode_records_within(&records, 1000).unwrap();
    assert!(
        frames.iter().any(|f| f.len() > 600),
        "one frame holds the comment"
    );
    for frame in &frames {
        line.stream.write_all(frame).await.unwrap();
        assert_eq!(line.byte_within(3000).await, Some(ACK));
    }
    line.stream.write_all(&[EOT]).await.unwrap();

    let messages = rig
        .messages_until("routed", |m| {
            m.first()
                .is_some_and(|m| m.summary.state == MessageState::Routed)
        })
        .await;
    let expected: Vec<u8> = records
        .iter()
        .flat_map(|r| r.iter().copied().chain(*b"\r"))
        .collect();
    assert_eq!(rig.body(messages[0].summary.message_id), expected);
}

#[tokio::test]
async fn a_long_frame_is_refused_at_the_default_size() {
    let rig = astm_rig(json!({})).await;
    let mut line = Line {
        stream: dial(&rig.net, "127.0.0.1:9641").await,
    };
    line.stream.write_all(&[ENQ]).await.unwrap();
    assert_eq!(line.byte_within(3000).await, Some(ACK));
    let frames = encode_records_within(&long_records(), 1000).unwrap();
    let long = frames.iter().find(|f| f.len() > 600).unwrap();
    assert_eq!(long.last(), Some(&LF));
    line.stream.write_all(long).await.unwrap();
    assert_eq!(line.byte_within(3000).await, Some(NAK));
    assert!(
        rig.messages()
            .iter()
            .all(|m| m.summary.state != MessageState::Routed)
    );
}

// ----- AU --------------------------------------------------------------------------------

fn au_config(settings: &Value) -> AuConfig {
    let framing = &settings["framing"];
    let format = &settings["format"];
    let digits = |name: &str| u8::try_from(format[name].as_u64().unwrap()).unwrap();
    AuConfig {
        framing: FramingConfig {
            start: vec![2],
            end: vec![3],
            bcc: framing["bcc"].as_bool().unwrap(),
            use_etb: false,
            max_wire_len: 256,
            class: TransmissionClass::A,
        },
        format: FormatConfig {
            unit_no: format["unit_no"].as_bool().unwrap(),
            rack_digits: digits("rack_digits"),
            sample_id_digits: digits("sample_id_digits"),
            sex: false,
            age: false,
            month: false,
            patient_info_digits: [0; 6],
            result_digits: digits("result_digits"),
            test_code_digits: digits("test_code_digits"),
        },
    }
}

async fn au_rig(settings: &Value, mapping: Value, destination_settings: Value) -> MessageRig {
    let channel = json!({
        "kind": "message", "channel_id": "au", "label": "AU",
        "source": {"endpoint": "au-in", "protocol": "au", "settings": settings},
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-out",
            "protocol": "hl7_mllp", "settings": destination_settings, "mapping": mapping,
        }],
        "retry": quick_retry(),
        "delivery": {"max_attempts": 2, "send_timeout_seconds": 3, "ack_timeout_seconds": 3,
            "retry": quick_retry()},
    });
    MessageRig::start(document(
        vec![listener("au-in", 9651), client("lis-out", 9701)],
        vec![channel],
    ))
    .await
}

#[tokio::test]
async fn the_au_preset_turns_a_result_message_into_an_oru() {
    let catalog = catalog().unwrap();
    let preset = catalog.mapping("au_results_to_hl7_oru").unwrap();
    let settings = preset.sample.source_settings.clone().unwrap();
    let rig = au_rig(
        &settings,
        preset.mapping.clone(),
        preset.destination_settings.clone().unwrap(),
    )
    .await;
    let (_listener, mut lis) = Peer::accept(&rig.net, LIS).await;
    let mut instrument = dial(&rig.net, "127.0.0.1:9651").await;

    let config = au_config(&settings);
    let text = preset.sample.input.trim_matches(['\u{2}', '\u{3}']);
    let wire = encode_frame(&config, text.as_bytes(), FrameEnd::Text).unwrap();
    assert_eq!(
        wire,
        preset.sample.input.as_bytes(),
        "the sample is the wire"
    );
    instrument.write_all(&wire).await.unwrap();

    let sent = String::from_utf8(lis.frame().await).unwrap();
    let message = Message::parse(sent.as_bytes()).unwrap();
    lis.send(&ack("AA", message.get("MSH-10").unwrap())).await;
    // The engine generates the header; the rest is the sample's output.
    let body = |text: &str| text.split_once('\r').unwrap().1.to_owned();
    assert_eq!(body(&sent), body(&preset.sample.output));
    rig.deliveries_until("acknowledged", |d| {
        d.first()
            .is_some_and(|d| d.state == DeliveryState::Acknowledged)
    })
    .await;
}

#[tokio::test]
async fn three_digit_test_codes_are_read_when_the_sheet_says_so() {
    let catalog = catalog().unwrap();
    let preset = catalog.mapping("au_results_to_hl7_oru").unwrap();
    let mut settings = preset.sample.source_settings.clone().unwrap();
    settings["format"]["test_code_digits"] = json!(3);
    let rig = au_rig(&settings, preset.mapping.clone(), json!({})).await;
    let (_listener, mut lis) = Peer::accept(&rig.net, LIS).await;
    let mut instrument = dial(&rig.net, "127.0.0.1:9651").await;

    let config = au_config(&settings);
    let text = b"D 01000101U0001SYN00043    E001005.50  120123.40H ";
    let wire = encode_frame(&config, text, FrameEnd::Text).unwrap();
    instrument.write_all(&wire).await.unwrap();

    let sent = Message::parse(&lis.frame().await).unwrap();
    assert_eq!(sent.get("OBR-3"), Some("SYN00043"));
    assert_eq!(sent.get("OBX(1)-3"), Some("001"));
    assert_eq!(sent.get("OBX(2)-3"), Some("120"));
    assert_eq!(sent.get("OBX(2)-5"), Some("123.40"));
}

// ----- orders out ------------------------------------------------------------------------

/// Receives one ASTM transfer as the instrument, acknowledging everything; returns the
/// frame texts joined.
async fn receive_transfer(line: &mut Line) -> Vec<u8> {
    assert_eq!(line.byte_within(5000).await, Some(ENQ));
    line.stream.write_all(&[ACK]).await.unwrap();
    let mut text = Vec::new();
    loop {
        match line.byte_within(5000).await.expect("the line stays active") {
            EOT => return text,
            0x02 => {
                let mut frame = vec![0x02];
                while frame.last() != Some(&LF) {
                    frame.push(line.byte_within(5000).await.expect("a frame"));
                }
                text.extend_from_slice(packie_astm::parse_frame(&frame).unwrap().text());
                line.stream.write_all(&[ACK]).await.unwrap();
            }
            other => panic!("unexpected byte {other:#x}"),
        }
    }
}

async fn download_through(preset_id: &str, order: &[u8]) -> (Vec<u8>, String) {
    let catalog = catalog().unwrap();
    let preset = catalog.mapping(preset_id).unwrap();
    let channel = json!({
        "kind": "message", "channel_id": "orders", "label": "Orders",
        "source": {"endpoint": "lab-in", "protocol": "hl7_mllp", "settings": {}},
        "destinations": [{
            "destination_id": "instrument", "label": "Instrument", "endpoint": "instrument",
            "protocol": "astm", "settings": {}, "mapping": preset.mapping,
        }],
        "retry": quick_retry(),
        "delivery": {"max_attempts": 2, "send_timeout_seconds": 3, "ack_timeout_seconds": 3,
            "retry": quick_retry()},
    });
    let rig = MessageRig::start(document(
        vec![listener("lab-in", 9601), client("instrument", 9741)],
        vec![channel],
    ))
    .await;
    let (_listener, stream) = accept_on(&rig.net, "127.0.0.1:9741").await;
    let mut instrument = Line { stream };
    let mut lis = Peer::dial(&rig.net, SOURCE).await;
    lis.send(order).await;
    assert_eq!(lis.msa().await.0, "AA");
    let text = receive_transfer(&mut instrument).await;
    rig.deliveries_until("acknowledged", |d| {
        d.first()
            .is_some_and(|d| d.state == DeliveryState::Acknowledged)
    })
    .await;
    (text, preset.sample.output.clone())
}

#[tokio::test]
async fn an_hl7_order_is_downloaded_to_the_instrument_as_astm_records() {
    let catalog = catalog().unwrap();
    for id in ["hl7_orm_to_astm_download", "hl7_oml_o33_to_astm_download"] {
        let sample = catalog.mapping(id).unwrap().sample.clone();
        let (sent, expected) = download_through(id, sample.input.as_bytes()).await;
        assert_eq!(String::from_utf8(sent).unwrap(), expected, "{id}");
    }
}

#[tokio::test]
async fn an_order_the_layout_cannot_carry_fails_its_delivery_and_sends_nothing() {
    let catalog = catalog().unwrap();
    let preset = catalog.mapping("hl7_orm_to_astm_download").unwrap();
    let channel = json!({
        "kind": "message", "channel_id": "orders", "label": "Orders",
        "source": {"endpoint": "lab-in", "protocol": "hl7_mllp", "settings": {}},
        "destinations": [{
            "destination_id": "instrument", "label": "Instrument", "endpoint": "instrument",
            "protocol": "astm", "settings": {}, "mapping": preset.mapping,
        }],
        "retry": quick_retry(),
    });
    let rig = MessageRig::start(document(
        vec![listener("lab-in", 9601), client("instrument", 9741)],
        vec![channel],
    ))
    .await;
    let (_listener, stream) = accept_on(&rig.net, "127.0.0.1:9741").await;
    let mut instrument = Line { stream };
    let mut lis = Peer::dial(&rig.net, SOURCE).await;

    // 19 tests: the order is stored and acknowledged, its delivery fails without an attempt
    let mut order = String::from(
        "MSH|^~\\&|LIS|HOSP|PACKIE|LAB|20261012120000||ORM^O01|ORD-19|P|2.5.1\rPID|1||PRAC-1||DOE^JANE\r",
    );
    for n in 1..=19 {
        order.push_str(&format!(
            "ORC|NW|PL-{n}\rOBR|{n}|PL-{n}|SPEC1234|T{n}^Test{n}^L\r"
        ));
    }
    lis.send(order.as_bytes()).await;
    assert_eq!(lis.msa().await, ("AA".into(), "ORD-19".into()));
    let deliveries = rig
        .deliveries_until("the delivery fails", |d| {
            d.first().is_some_and(|d| d.state == DeliveryState::Failed)
        })
        .await;
    assert_eq!(deliveries[0].attempts, 0, "nothing was sent");
    assert_eq!(
        deliveries[0].reason_code.as_deref(),
        Some("transform_error")
    );
    assert_eq!(
        instrument.byte_within(300).await,
        None,
        "the line stayed quiet"
    );
    assert_eq!(rig.messages()[0].summary.state, MessageState::Routed);
}

// ----- specimen id where the instrument puts it ----------------------------------------

#[tokio::test]
async fn the_specimen_id_of_the_o4_layout_is_stored_searchable_and_mapped() {
    let catalog = catalog().unwrap();
    let preset = catalog
        .mapping("astm_results_to_hl7_oru_instrument_specimen_id")
        .unwrap();
    let profile = catalog
        .connector("astm_specimen_id_in_o4_component_3")
        .unwrap();
    // the source setting comes from the connector profile; the preset's sample names it too
    assert_eq!(
        preset.sample.source_settings.as_ref(),
        Some(&profile.settings)
    );
    let channel = json!({
        "kind": "message", "channel_id": "results", "label": "Results",
        "source": {"endpoint": "analyzer", "protocol": "astm", "settings": profile.settings},
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-out",
            "protocol": "hl7_mllp", "settings": preset.destination_settings,
            "mapping": preset.mapping,
        }],
        "retry": quick_retry(),
        "delivery": {"max_attempts": 2, "send_timeout_seconds": 3, "ack_timeout_seconds": 3,
            "retry": quick_retry()},
    });
    let rig = MessageRig::start(document(
        vec![listener("analyzer", 9641), client("lis-out", 9701)],
        vec![channel],
    ))
    .await;
    let (_listener, mut lis) = Peer::accept(&rig.net, LIS).await;
    let mut line = Line {
        stream: dial(&rig.net, "127.0.0.1:9641").await,
    };

    let records: Vec<Vec<u8>> = preset
        .sample
        .input
        .split('\r')
        .filter(|r| !r.is_empty())
        .map(|r| r.as_bytes().to_vec())
        .collect();
    line.stream.write_all(&[ENQ]).await.unwrap();
    assert_eq!(line.byte_within(3000).await, Some(ACK));
    for frame in packie_astm::encode_records(&records).unwrap() {
        line.stream.write_all(&frame).await.unwrap();
        assert_eq!(line.byte_within(3000).await, Some(ACK));
    }
    line.stream.write_all(&[EOT]).await.unwrap();

    // the stored message: sample id, source identity and the grid's text search
    let messages = rig
        .messages_until("routed", |m| {
            m.first()
                .is_some_and(|m| m.summary.state == MessageState::Routed)
        })
        .await;
    let record = &messages[0];
    assert_eq!(record.summary.sample_id.as_deref(), Some("SYN-0001"));
    assert_eq!(
        record.source_identity.as_deref(),
        Some("HEMA-1^1.0|20261012120000|SYN-0001")
    );
    let found = rig
        .store
        .query_messages(&packie_ipc::messages::MessageQuery {
            filter: packie_ipc::messages::MessageFilter {
                text: Some("SYN-0001".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert_eq!(found.messages.len(), 1);
    assert_eq!(found.messages[0].message_id, record.summary.message_id);

    // the mapped output is the sample's, byte for byte after the generated header
    let sent = String::from_utf8(lis.frame().await).unwrap();
    let message = Message::parse(sent.as_bytes()).unwrap();
    lis.send(&ack("AA", message.get("MSH-10").unwrap())).await;
    let body = |text: &str| text.split_once('\r').unwrap().1.to_owned();
    assert_eq!(body(&sent), body(&preset.sample.output));
}
