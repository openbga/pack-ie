use packie_hl7::ack::{
    Ack, AckClassification, AckError, AckMode, AckOptions, AckOutcome, AckPolicy, AckStage,
    ErrSegment, ErrorDetails, ErrorLocation, ErrorSeverity, Msa, Msh, build_ack,
    into_order_response,
};
use packie_hl7::{EncodingCharacters, Message};

fn source(version: &str, accept: &str, application: &str) -> Result<Message, packie_hl7::Error> {
    Message::parse(format!("MSH|^~\\&|DEVICE^dev.1^ISO|LAB^lab.1^ISO|APP^app.1^ISO|HOSP^hos.1^ISO|202601011200||ORU^R01|sent1|T^A|{version}|||{accept}|{application}\rPID|||synthetic\r").as_bytes())
}

const OUTCOMES: [AckOutcome; 3] = [
    AckOutcome::Accepted,
    AckOutcome::Error,
    AckOutcome::Rejected,
];

fn options() -> AckOptions<'static> {
    AckOptions {
        control_id: "reply1",
        timestamp: "202601011201",
        text: None,
        error: None,
    }
}

#[test]
fn enhanced_acknowledgement_matrix_covers_each_policy_stage_and_outcome() {
    let policies = [
        ("AL", AckPolicy::Always),
        ("NE", AckPolicy::Never),
        ("ER", AckPolicy::Error),
        ("SU", AckPolicy::Success),
    ];
    for accept in policies {
        for application in policies {
            check_enhanced_policy_pair(accept, application).unwrap();
        }
    }
}

fn check_enhanced_policy_pair(
    (accept_code, accept): (&str, AckPolicy),
    (application_code, application): (&str, AckPolicy),
) -> Result<(), Box<dyn std::error::Error>> {
    let message = source("2.5.1", accept_code, application_code)?;
    assert_eq!(
        AckMode::from_message(&message)?,
        AckMode::Enhanced {
            accept,
            application
        }
    );
    for (stage, policy) in [
        (AckStage::Commit, accept_code),
        (AckStage::Application, application_code),
    ] {
        check_enhanced_stage(&message, stage, policy)?;
    }
    Ok(())
}

fn check_enhanced_stage(
    message: &Message,
    stage: AckStage,
    policy: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for outcome in OUTCOMES {
        let expected = match policy {
            "AL" => true,
            "NE" => false,
            "ER" => outcome != AckOutcome::Accepted,
            "SU" => outcome == AckOutcome::Accepted,
            _ => return Err("unknown policy in test fixture".into()),
        };
        let reply = build_ack(message, stage, outcome, &options())?;
        assert_eq!(reply.is_some(), expected, "{policy} {stage:?} {outcome:?}");
        if let Some(reply) = reply {
            check_ack_reply(&reply, stage, outcome)?;
        }
    }
    Ok(())
}

fn check_ack_reply(
    reply: &Message,
    stage: AckStage,
    outcome: AckOutcome,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(reply.get("MSA-1"), Some(outcome.code(stage)));
    assert_eq!(reply.get("MSA-2"), Some("sent1"));
    assert!(reply.get("MSH-15").is_none_or(str::is_empty));
    assert!(reply.get("MSH-16").is_none_or(str::is_empty));
    let reparsed = Message::parse(&reply.encode())?;
    Ack::from_message(&reparsed)?.validate_correlation("sent1")?;
    Ok(())
}

#[test]
fn original_mode_only_replies_at_application_stage_for_all_outcomes() {
    for bytes in [
        b"MSH|^~\\&|D|L|A|H|202601011200||ORU^R01|sent1|T|2.3\r".as_slice(),
        b"MSH|^~\\&|D|L|A|H|202601011200||ORU^R01|sent1|T|2.3||||\r".as_slice(),
    ] {
        let message = Message::parse(bytes).unwrap();
        assert_eq!(AckMode::from_message(&message).unwrap(), AckMode::Original);
        for outcome in OUTCOMES {
            assert!(
                build_ack(&message, AckStage::Commit, outcome, &options())
                    .unwrap()
                    .is_none()
            );
            let reply = build_ack(&message, AckStage::Application, outcome, &options())
                .unwrap()
                .unwrap();
            assert_eq!(
                reply.get("MSA-1"),
                Some(outcome.code(AckStage::Application))
            );
            assert_eq!(reply.get("MSH-9.3"), None);
        }
    }
}

#[test]
fn enhanced_missing_half_means_no_ack_for_that_stage() {
    for (accept, application, omitted) in [
        ("", "AL", AckStage::Commit),
        ("AL", "", AckStage::Application),
    ] {
        let message = source("2.5.1", accept, application).unwrap();
        for outcome in OUTCOMES {
            assert!(
                build_ack(&message, omitted, outcome, &options())
                    .unwrap()
                    .is_none()
            );
        }
    }
    assert_eq!(
        AckMode::from_message(&source("2.5.1", "NE", "NE").unwrap()).unwrap(),
        AckMode::Enhanced {
            accept: AckPolicy::Never,
            application: AckPolicy::Never
        }
    );
}

#[test]
fn acknowledgement_structure_is_included_from_version_2_3_1() {
    let reply = build_ack(
        &source("2.3.1", "", "").unwrap(),
        AckStage::Application,
        AckOutcome::Accepted,
        &options(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(reply.get("MSH-9.3"), Some("ACK"));
}

#[test]
fn reply_swaps_structured_identities_and_retains_literal_escapes() {
    let mut message = source("2.5.1", "AL", "AL").unwrap();
    message
        .set("MSH-3.1", "Device | north^one&two~three\\four")
        .unwrap();
    message.set("MSH-5.1", "Receiver ^ two").unwrap();
    message.set("MSH-10", "sent|1\\id").unwrap();
    let supplied = AckOptions {
        control_id: "reply|2",
        timestamp: "202601011201",
        text: Some("text | ^ & ~ \\ \r"),
        error: None,
    };
    let reply = build_ack(&message, AckStage::Commit, AckOutcome::Accepted, &supplied)
        .unwrap()
        .unwrap();
    for (from, to) in [(3, 5), (4, 6), (5, 3), (6, 4), (11, 11), (12, 12)] {
        for component in 1..=3 {
            assert_eq!(
                reply.get(&format!("MSH-{to}.{component}")),
                message.get(&format!("MSH-{from}.{component}"))
            );
        }
    }
    assert_eq!(reply.get("MSH-7"), Some(supplied.timestamp));
    assert_eq!(reply.get("MSH-9"), Some("ACK^R01^ACK"));
    assert_eq!(reply.get("MSH-10"), Some("reply|2"));
    assert_eq!(reply.get("MSA-2"), Some("sent|1\\id"));
    assert_eq!(reply.get("MSA-3"), supplied.text);
    assert!(message.segment("PID", 1).is_some());
    assert!(reply.segment("PID", 1).is_none());
    let ack = Ack::from_message(&reply).unwrap();
    ack.validate_correlation("sent|1\\id").unwrap();
    assert_eq!(
        ack.validate_correlation("other"),
        Err(AckError::CorrelationMismatch)
    );
}

#[test]
fn custom_delimiters_and_nested_identity_subcomponents_survive_header_swap() {
    let encoding = EncodingCharacters::new(b'*', b'%', b'!', b'?', b'@').unwrap();
    let mut message = Message::new(encoding);
    for (path, value) in [
        ("MSH-3.1.1", "Device * one"),
        ("MSH-3.1.2", "nested@literal"),
        ("MSH-3.2", "oid"),
        ("MSH-5.1", "App"),
        ("MSH-7", "20260101"),
        ("MSH-9.1", "ORU"),
        ("MSH-9.2", "R01"),
        ("MSH-10", "id*1"),
        ("MSH-11", "T"),
        ("MSH-12", "2.5.1"),
    ] {
        message.set(path, value).unwrap();
    }
    let reply = build_ack(
        &message,
        AckStage::Application,
        AckOutcome::Accepted,
        &options(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(reply.encoding(), encoding);
    assert_eq!(reply.get("MSH-5.1.1"), Some("Device * one"));
    assert_eq!(reply.get("MSH-5.1.2"), Some("nested@literal"));
    assert_eq!(reply.get("MSH-5.2"), Some("oid"));
    assert_eq!(reply.get("MSA-2"), Some("id*1"));
    Ack::from_message(&reply)
        .unwrap()
        .validate_correlation("id*1")
        .unwrap();
}

fn details() -> ErrorDetails<'static> {
    ErrorDetails {
        code: "101",
        description: Some("Required | field"),
        coding_system: Some("HL70357"),
        severity: ErrorSeverity::Error,
        location: Some(ErrorLocation {
            segment: "OBX",
            segment_sequence: Some(2),
            field: Some(5),
            repetition: Some(1),
            component: Some(2),
            subcomponent: Some(3),
        }),
        diagnostic: Some("synthetic diagnostic ^ details"),
        user_message: Some("Check | input"),
    }
}

#[test]
fn modern_error_fields_and_borrowed_helpers() {
    let message = source("2.5.1", "AL", "AL").unwrap();
    let supplied = AckOptions {
        error: Some(details()),
        text: Some("invalid ^ value"),
        ..options()
    };
    let reply = build_ack(
        &message,
        AckStage::Application,
        AckOutcome::Error,
        &supplied,
    )
    .unwrap()
    .unwrap();
    let msh = Msh::new(&reply);
    for (actual, expected) in [
        (msh.sending_application(), Some("APP^app.1^ISO")),
        (msh.sending_facility(), Some("HOSP^hos.1^ISO")),
        (msh.receiving_application(), Some("DEVICE^dev.1^ISO")),
        (msh.receiving_facility(), Some("LAB^lab.1^ISO")),
        (msh.timestamp(), Some("202601011201")),
        (msh.message_type(), Some("ACK")),
        (msh.trigger_event(), Some("R01")),
        (msh.control_id(), Some("reply1")),
        (msh.processing_id(), Some("T^A")),
        (msh.version(), Some("2.5.1")),
    ] {
        assert_eq!(actual, expected);
    }
    let msa = Msa::new(&reply).unwrap();
    for (actual, expected) in [
        (msa.code(), Some("AE")),
        (msa.control_id(), Some("sent1")),
        (msa.text(), supplied.text),
    ] {
        assert_eq!(actual, expected);
    }
    let error = ErrSegment::new(&reply).unwrap();
    assert!(error.legacy_code_and_location().is_none_or(str::is_empty));
    for (actual, expected) in [
        (error.location(), Some("OBX^2^5^1^2^3")),
        (error.code(), Some("101")),
        (error.severity(), Some("E")),
        (error.diagnostic(), details().diagnostic),
        (error.user_message(), details().user_message),
        (reply.get("ERR-3.2"), details().description),
        (reply.get("ERR-3.3"), details().coding_system),
    ] {
        assert_eq!(actual, expected);
    }
}

#[test]
fn legacy_error_uses_eld_without_modern_fields() {
    for version in ["2.3", "2.4"] {
        let supplied = AckOptions {
            error: Some(details()),
            ..options()
        };
        let reply = build_ack(
            &source(version, "", "").unwrap(),
            AckStage::Application,
            AckOutcome::Rejected,
            &supplied,
        )
        .unwrap()
        .unwrap();
        assert_eq!(reply.get("ERR-1.1"), Some("OBX"));
        assert_eq!(reply.get("ERR-1.2"), Some("2"));
        assert_eq!(reply.get("ERR-1.3"), Some("5"));
        assert_eq!(reply.get("ERR-1.4.1"), Some("101"));
        assert_eq!(reply.get("ERR-1.4.2"), details().description);
        assert_eq!(reply.get("ERR-1.4.3"), details().coding_system);
        assert_eq!(reply.get("ERR-3"), None);
        assert_eq!(reply.get("ERR-4"), None);
        assert_eq!(reply.get("ERR-7"), None);
        assert!(
            ErrSegment::new(&reply)
                .unwrap()
                .legacy_code_and_location()
                .is_some()
        );
    }
}

#[test]
fn error_locations_are_optional_and_severity_codes_are_exact() {
    for (severity, code) in [
        (ErrorSeverity::Information, "I"),
        (ErrorSeverity::Warning, "W"),
        (ErrorSeverity::Error, "E"),
        (ErrorSeverity::Fatal, "F"),
    ] {
        let supplied = AckOptions {
            error: Some(ErrorDetails {
                location: None,
                severity,
                ..details()
            }),
            ..options()
        };
        let reply = build_ack(
            &source("2.5", "AL", "AL").unwrap(),
            AckStage::Commit,
            AckOutcome::Error,
            &supplied,
        )
        .unwrap()
        .unwrap();
        assert!(reply.get("ERR-2").is_none_or(str::is_empty));
        assert_eq!(reply.get("ERR-4"), Some(code));
    }
}

#[test]
fn all_ack_codes_are_classified_and_correlated() {
    for (code, expected) in [
        ("AA", AckClassification::Accepted),
        ("AE", AckClassification::Error),
        ("AR", AckClassification::Rejected),
        ("CA", AckClassification::CommitAccepted),
        ("CE", AckClassification::CommitError),
        ("CR", AckClassification::CommitRejected),
        ("XX", AckClassification::Unknown),
    ] {
        let mut message = source("2.5.1", "", "").unwrap();
        message.set("MSH-9.1", "ACK").unwrap();
        message.set("MSA-1", code).unwrap();
        message.set("MSA-2", "sent1").unwrap();
        let ack = Ack::from_message(&message).unwrap();
        assert_eq!(ack.classification, expected);
        ack.validate_correlation("sent1").unwrap();
        assert_eq!(
            ack.validate_correlation("private-control-id"),
            Err(AckError::CorrelationMismatch)
        );
    }
}

#[test]
fn malformed_acks_and_policies_produce_typed_errors_without_contents() {
    let mut message = source("2.5.1", "private-policy", "AL").unwrap();
    let error = AckMode::from_message(&message).unwrap_err();
    assert_eq!(error, AckError::InvalidPolicy);
    assert!(!error.to_string().contains("private"));
    assert!(matches!(
        build_ack(&message, AckStage::Commit, AckOutcome::Accepted, &options()),
        Err(AckError::InvalidPolicy)
    ));
    message.set("MSH-15", "AL").unwrap();
    assert_eq!(
        Ack::from_message(&message).unwrap_err(),
        AckError::NotAcknowledgement
    );
    message.set("MSH-9.1", "ACK").unwrap();
    assert_eq!(
        Ack::from_message(&message).unwrap_err(),
        AckError::MissingField("MSA")
    );
    message.set("MSA-1", "AA").unwrap();
    assert_eq!(
        Ack::from_message(&message).unwrap_err(),
        AckError::MissingField("MSA-2")
    );
    message.set("MSA-2", "private-id").unwrap();
    let error = Ack::from_message(&message)
        .unwrap()
        .validate_correlation("another-private-id")
        .unwrap_err();
    assert!(!error.to_string().contains("private"));
}

#[test]
fn non_utf8_policy_is_rejected_instead_of_inferred_as_original_mode() {
    let message =
        Message::parse(b"MSH|^~\\&|D|L|A|H|202601011200||ORU^R01|sent1|T|2.5.1|||\xff\r").unwrap();
    assert_eq!(
        AckMode::from_message(&message),
        Err(AckError::InvalidPolicy)
    );
}

#[test]
fn missing_header_or_reply_identity_is_explicit_but_suppressed_ack_needs_no_identity() {
    let mut message = source("2.5.1", "AL", "AL").unwrap();
    message.set("MSH-10", "").unwrap();
    assert!(matches!(
        build_ack(
            &message,
            AckStage::Application,
            AckOutcome::Accepted,
            &options()
        ),
        Err(AckError::MissingField("MSH-10"))
    ));
    let message = source("2.5.1", "AL", "AL").unwrap();
    for supplied in [
        AckOptions {
            control_id: "",
            ..options()
        },
        AckOptions {
            timestamp: "",
            ..options()
        },
    ] {
        assert!(matches!(
            build_ack(&message, AckStage::Commit, AckOutcome::Accepted, &supplied),
            Err(AckError::MissingReplyIdentity)
        ));
        assert!(
            build_ack(
                &source("2.5.1", "NE", "NE").unwrap(),
                AckStage::Commit,
                AckOutcome::Accepted,
                &supplied
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn unknown_error_version_fails_without_exposing_body() {
    let supplied = AckOptions {
        error: Some(details()),
        ..options()
    };
    assert!(matches!(
        build_ack(
            &source("private-version", "AL", "AL").unwrap(),
            AckStage::Application,
            AckOutcome::Error,
            &supplied
        ),
        Err(AckError::UnsupportedErrorVersion)
    ));
    assert!(
        build_ack(
            &source("private-version", "AL", "AL").unwrap(),
            AckStage::Application,
            AckOutcome::Accepted,
            &options()
        )
        .unwrap()
        .is_some()
    );
}

fn order(kind: &str, version: &str) -> Result<Message, packie_hl7::Error> {
    Message::parse(
        format!(
            "MSH|^~\\&|LIS|HOSP|AM|LAB|202601011200||{kind}|ord1|P|{version}\rPID|||synthetic\r"
        )
        .as_bytes(),
    )
}

#[test]
fn order_messages_are_answered_by_order_responses() {
    for (kind, version, expected) in [
        ("OML^O33^OML_O33", "2.5.1", "ORL^O34^ORL_O34"),
        ("OML^O35^OML_O35", "2.5.1", "ORL^O36^ORL_O36"),
        ("OML^O21^OML_O21", "2.5.1", "ORL^O22^ORL_O22"),
        ("ORM^O01", "2.5.1", "ORR^O02^ORR_O02"),
        ("ORM^O01", "2.3", "ORR^O02"),
    ] {
        let source = order(kind, version).unwrap();
        let mut ack = build_ack(
            &source,
            AckStage::Application,
            AckOutcome::Accepted,
            &options(),
        )
        .unwrap()
        .unwrap();
        assert!(into_order_response(&mut ack, &source).unwrap(), "{kind}");
        assert_eq!(ack.get("MSH-9"), Some(expected), "{kind} v{version}");
        assert_eq!(ack.get("MSA-1"), Some("AA"));
        assert_eq!(ack.get("MSA-2"), Some("ord1"));
        // The response is an acknowledgement of the order for the sender's correlation.
        let response = Ack::from_reply(&ack, &["ORL", "ORR"]).unwrap();
        assert_eq!(response.classification, AckClassification::Accepted);
        assert!(response.validate_correlation("ord1").is_ok());
        assert!(matches!(
            Ack::from_message(&ack),
            Err(AckError::NotAcknowledgement)
        ));
    }
}

#[test]
fn other_messages_keep_the_plain_acknowledgement() {
    let source = order("ORU^R01^ORU_R01", "2.5.1").unwrap();
    let mut ack = build_ack(
        &source,
        AckStage::Application,
        AckOutcome::Accepted,
        &options(),
    )
    .unwrap()
    .unwrap();
    let before = ack.encode();
    assert!(!into_order_response(&mut ack, &source).unwrap());
    assert_eq!(ack.encode(), before);
    assert_eq!(ack.get("MSH-9"), Some("ACK^R01^ACK"));
}

#[test]
fn accepted_reply_types_are_exact_and_listed() {
    let source = order("ORM^O01", "2.5.1").unwrap();
    let ack = build_ack(
        &source,
        AckStage::Application,
        AckOutcome::Rejected,
        &options(),
    )
    .unwrap()
    .unwrap();
    assert!(Ack::from_reply(&ack, &["ACK"]).is_ok());
    assert!(Ack::from_reply(&ack, &["ORR", "ACK"]).is_ok());
    assert!(matches!(
        Ack::from_reply(&ack, &["ORR"]),
        Err(AckError::NotAcknowledgement)
    ));
    assert!(matches!(
        Ack::from_reply(&ack, &[]),
        Err(AckError::NotAcknowledgement)
    ));
    // a listed type without an MSA segment is not an acknowledgement
    assert!(Ack::from_reply(&source, &["ACK", "ORM", "OML"]).is_err());
}
