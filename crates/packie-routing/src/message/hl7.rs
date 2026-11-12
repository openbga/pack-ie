//! HL7 v2 over MLLP for message channels: identifiers, the syntax verdict, the source's
//! acknowledgements and the destination's acknowledgement handling
//! (`docs/spec/message-channels.md` §2.2, §3, §7, §8).

use packie_config::ReplyType;
use packie_core::{AckLevel, Timestamp};
use packie_hl7::Message;
use packie_hl7::ack::{
    Ack, AckClassification, AckMode, AckOptions, AckOutcome, AckStage, Msh, build_ack,
    into_order_response,
};

use crate::message::store::Identifiers;

/// How the source acknowledges (`settings.ack_mode` of an `hl7_mllp` source).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceAckMode {
    /// One `ACK` after the commit and the syntax check.
    Original,
    /// Commit and application acknowledgements as the sender asks (MSH-15/16).
    Enhanced { send_application_ack: bool },
}

/// The parsed message, or why it cannot be accepted (a condition, never a value).
pub(crate) fn verdict(bytes: &[u8]) -> Result<Message, &'static str> {
    let message = Message::parse(bytes).map_err(|_| "unparseable")?;
    let msh = Msh::new(&message);
    let present = |value: Option<&str>| value.is_some_and(|v| !v.is_empty());
    if !present(msh.message_type()) {
        return Err("msh_9_missing");
    }
    if !present(msh.control_id()) {
        return Err("msh_10_missing");
    }
    if !present(msh.processing_id()) {
        return Err("msh_11_missing");
    }
    if !present(msh.version()) {
        return Err("msh_12_missing");
    }
    Ok(message)
}

/// Identifiers of `message` (`message-channels.md` §3, §7).
pub(crate) fn identifiers(message: &Message) -> Identifiers {
    let text = |path: &str| {
        message
            .get(path)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let msh = Msh::new(message);
    let identity_parts = [
        msh.sending_application(),
        msh.sending_facility(),
        msh.control_id(),
    ];
    let source_identity = identity_parts
        .iter()
        .any(|p| p.is_some_and(|v| !v.is_empty()))
        .then(|| identity_parts.map(|p| p.unwrap_or("")).join("|"));
    Identifiers {
        control_id: text("MSH-10"),
        // The specimen when the message names one (SPM-2: placer then filler assigned id,
        // entity identifier of each), else the order's filler or placer number.
        sample_id: text("SPM-2.1.1")
            .or_else(|| text("SPM-2.2.1"))
            .or_else(|| text("OBR-3"))
            .or_else(|| text("OBR-2")),
        patient_id: text("PID-3.1"),
        message_type: text("MSH-9"),
        source_identity,
    }
}

/// The source's reply for one stage, or `None` when the stage asks for none.
pub(crate) struct Reply {
    pub(crate) code: &'static str,
    pub(crate) bytes: Vec<u8>,
}

/// The first acknowledgement after the commit: an `ACK` (original) or the commit
/// acknowledgement (enhanced). `parsed` is `None` for an unparseable message.
pub(crate) fn first_reply(
    mode: SourceAckMode,
    parsed: Option<&Message>,
    original: &[u8],
    outcome: AckOutcome,
    reply_type: ReplyType,
) -> Option<Reply> {
    let Some(message) = parsed else {
        // Unparseable: a minimal negative ACK echoing what can be found.
        let stage = stage_for(mode, None);
        return Some(minimal_reply(original, outcome.code(stage)));
    };
    let stage = stage_for(mode, Some(message));
    let source = match (mode, stage) {
        (SourceAckMode::Original, _) | (_, AckStage::Application) => without_enhanced(message),
        (SourceAckMode::Enhanced { .. }, AckStage::Commit) => message.clone(),
    };
    reply(&source, stage, outcome, reply_type)
}

/// The application acknowledgement of enhanced mode, when the sender asks for it and
/// the source sends them.
pub(crate) fn application_reply(
    mode: SourceAckMode,
    message: &Message,
    outcome: AckOutcome,
    reply_type: ReplyType,
) -> Option<Reply> {
    let SourceAckMode::Enhanced {
        send_application_ack: true,
    } = mode
    else {
        return None;
    };
    if !matches!(AckMode::from_message(message), Ok(AckMode::Enhanced { .. })) {
        return None;
    }
    reply(message, AckStage::Application, outcome, reply_type)
}

/// Enhanced mode with MSH-15 and MSH-16 empty behaves as original.
fn stage_for(mode: SourceAckMode, message: Option<&Message>) -> AckStage {
    let enhanced = matches!(mode, SourceAckMode::Enhanced { .. })
        && message
            .is_some_and(|m| matches!(AckMode::from_message(m), Ok(AckMode::Enhanced { .. })));
    if enhanced {
        AckStage::Commit
    } else {
        AckStage::Application
    }
}

fn without_enhanced(message: &Message) -> Message {
    let mut copy = message.clone();
    // Only the policy fields change, and only in this private copy used to build the ACK.
    let _ = copy.set("MSH-15", "");
    let _ = copy.set("MSH-16", "");
    copy
}

/// An order message answered at the application stage gets its order response when the
/// connector asks for them (`reply_type`); the commit acknowledgement is always an `ACK`.
fn reply(
    source: &Message,
    stage: AckStage,
    outcome: AckOutcome,
    reply_type: ReplyType,
) -> Option<Reply> {
    let control_id = packie_core::MessageId::generate().to_string();
    let timestamp = hl7_timestamp(Timestamp::now());
    let options = AckOptions {
        control_id: &control_id,
        timestamp: &timestamp,
        text: None,
        error: None,
    };
    match build_ack(source, stage, outcome, &options) {
        Ok(Some(mut ack)) => {
            if reply_type == ReplyType::OrderResponse
                && stage == AckStage::Application
                && into_order_response(&mut ack, source).is_err()
            {
                return Some(minimal_reply(source.original_bytes(), outcome.code(stage)));
            }
            Some(Reply {
                code: outcome.code(stage),
                bytes: ack.encode(),
            })
        }
        Ok(None) => None,
        Err(_) => Some(minimal_reply(source.original_bytes(), outcome.code(stage))),
    }
}

/// `MSH` and `MSA` only, for a message whose header cannot be used. MSA-2 echoes MSH-10
/// when it can be found.
fn minimal_reply(original: &[u8], code: &'static str) -> Reply {
    let control = control_id_of(original).unwrap_or_default();
    let id = packie_core::MessageId::generate();
    let timestamp = hl7_timestamp(Timestamp::now());
    let text = format!("MSH|^~\\&|||||{timestamp}||ACK|{id}|P|2.5.1\rMSA|{code}|{control}\r");
    Reply {
        code,
        bytes: text.into_bytes(),
    }
}

/// MSH-10 found by splitting the first segment on `|`, without full parsing.
fn control_id_of(original: &[u8]) -> Option<String> {
    let first = original.split(|b| *b == b'\r' || *b == b'\n').next()?;
    if !first.starts_with(b"MSH") {
        return None;
    }
    let field = first.split(|b| *b == b'|').nth(9)?;
    let text = std::str::from_utf8(field).ok()?;
    // Never echo something that could break the reply's structure.
    text.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        .then(|| text.to_owned())
}

/// `YYYYMMDDHHMMSS` in UTC.
pub(crate) fn hl7_timestamp(at: Timestamp) -> String {
    at.as_jiff().strftime("%Y%m%d%H%M%S").to_string()
}

/// A destination's view of one reply frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReplyVerdict {
    /// Not an acknowledgement of the message in flight (`delivery.ack_unmatched`).
    Unmatched,
    /// A lower level was proven; keep waiting.
    Progress { level: AckLevel, code: String },
    /// The required level or a terminal answer.
    Final {
        accepted: bool,
        level: AckLevel,
        code: String,
        /// The acknowledgement's own control id (its MSH-10).
        ack_control_id: Option<String>,
    },
}

/// Classifies `reply` for the message with control id `sent_control_id`, given what the
/// delivery requires.
pub(crate) fn classify_reply(
    reply: &[u8],
    sent_control_id: &str,
    required: AckLevel,
    message_types: &[&str],
) -> ReplyVerdict {
    let Ok(message) = Message::parse(reply) else {
        return ReplyVerdict::Unmatched;
    };
    let Ok(ack) = Ack::from_reply(&message, message_types) else {
        return ReplyVerdict::Unmatched;
    };
    if ack.validate_correlation(sent_control_id).is_err() {
        return ReplyVerdict::Unmatched;
    }
    let code = message.get("MSA-1").unwrap_or("").to_owned();
    let ack_control_id = message
        .get("MSH-10")
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    match ack.classification {
        AckClassification::Accepted => ReplyVerdict::Final {
            accepted: true,
            level: AckLevel::Accepted,
            code,
            ack_control_id,
        },
        AckClassification::CommitAccepted if required <= AckLevel::Received => {
            ReplyVerdict::Final {
                accepted: true,
                level: AckLevel::Received,
                code,
                ack_control_id,
            }
        }
        AckClassification::CommitAccepted => ReplyVerdict::Progress {
            level: AckLevel::Received,
            code,
        },
        AckClassification::Error
        | AckClassification::Rejected
        | AckClassification::CommitError
        | AckClassification::CommitRejected => ReplyVerdict::Final {
            accepted: false,
            level: AckLevel::None,
            code,
            ack_control_id,
        },
        AckClassification::Unknown => ReplyVerdict::Unmatched,
    }
}

/// MSH-10 of an outgoing message, for correlation.
pub(crate) fn outgoing_control_id(body: &[u8]) -> Option<String> {
    let message = Message::parse(body).ok()?;
    Msh::new(&message)
        .control_id()
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORU: &[u8] = b"MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|CTRL-1|P|2.5.1\rPID|1||PID-TEST-1||TEST^PATIENT\rOBR|1|PLACER|SID-1\r";
    const ENHANCED: &[u8] = b"MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|CTRL-2|P|2.5.1|||AL|AL\rPID|1||PID-TEST-2\r";

    fn msa(reply: &Reply) -> (String, String) {
        let message = Message::parse(&reply.bytes).unwrap();
        (
            message.get("MSA-1").unwrap().to_owned(),
            message.get("MSA-2").unwrap_or("").to_owned(),
        )
    }

    #[test]
    fn identifiers_follow_the_spec_paths() {
        let message = verdict(ORU).unwrap();
        let ids = identifiers(&message);
        assert_eq!(ids.control_id.as_deref(), Some("CTRL-1"));
        assert_eq!(ids.sample_id.as_deref(), Some("SID-1"));
        assert_eq!(ids.patient_id.as_deref(), Some("PID-TEST-1"));
        assert_eq!(ids.message_type.as_deref(), Some("ORU^R01"));
        assert_eq!(ids.source_identity.as_deref(), Some("SIMLAB|SIMFAC|CTRL-1"));
    }

    #[test]
    fn the_specimen_id_comes_from_spm_when_the_message_has_one() {
        let ids = |text: &str| identifiers(&verdict(text.as_bytes()).unwrap()).sample_id;
        let msh = "MSH|^~\\&|LIS|HOSP|PACKIE|LAB|20260101||OML^O33^OML_O33|ORD-1|P|2.5.1\r";
        // plain, composite (entity identifier & namespace), filler only, empty
        assert_eq!(
            ids(&format!("{msh}SPM|1|SPEC1234||SER\rOBR|1|PL-1\r")).as_deref(),
            Some("SPEC1234")
        );
        assert_eq!(
            ids(&format!("{msh}SPM|1|SPEC9&LIS\rOBR|1|PL-1|FIL-1\r")).as_deref(),
            Some("SPEC9")
        );
        assert_eq!(
            ids(&format!("{msh}SPM|1|^FILLER7\rOBR|1|PL-1\r")).as_deref(),
            Some("FILLER7")
        );
        // an empty SPM-2 falls back to the order numbers, filler (OBR-3) before placer
        assert_eq!(
            ids(&format!("{msh}SPM|1||\rOBR|1|PL-1|FIL-1\r")).as_deref(),
            Some("FIL-1")
        );
        assert_eq!(
            ids(&format!("{msh}SPM|1\rOBR|1|PL-1\r")).as_deref(),
            Some("PL-1")
        );
        assert_eq!(ids(&format!("{msh}SPM|1\r")), None);
        // messages without SPM keep the order numbers
        assert_eq!(
            ids(&format!("{msh}OBR|1|PL-1|FIL-1\r")).as_deref(),
            Some("FIL-1")
        );
        assert_eq!(ids(&format!("{msh}OBR|1|PL-1\r")).as_deref(), Some("PL-1"));
    }

    #[test]
    fn the_catalog_orders_and_results_get_their_specimen_as_sample_id() {
        let catalog = packie_config::profiles::catalog().unwrap();
        let sample = |id: &str| catalog.mapping(id).unwrap().sample.input.clone();
        for (id, expected) in [
            ("hl7_oml_o33_to_astm_download", "SPEC1234"),
            ("hl7_orm_to_astm_download", "SPEC1234"),
            ("hl7_orm_to_astm_download_roche_cobas", "SPEC1234"),
        ] {
            let ids = identifiers(&verdict(sample(id).as_bytes()).unwrap());
            assert_eq!(ids.sample_id.as_deref(), Some(expected), "{id}");
            assert_eq!(ids.patient_id.as_deref(), Some("PRAC-1"), "{id}");
        }
        // an ORU keeps OBR-3
        assert_eq!(
            identifiers(&verdict(ORU).unwrap()).sample_id.as_deref(),
            Some("SID-1")
        );
    }

    #[test]
    fn verdict_names_conditions() {
        assert_eq!(verdict(b"PID|1\r").unwrap_err(), "unparseable");
        let no_id = b"MSH|^~\\&|A|B|C|D|20260101||ORU^R01||P|2.5.1\r";
        assert!(
            verdict(no_id).is_err(),
            "a header without MSH-10 is rejected"
        );
    }

    #[test]
    fn original_mode_answers_once_with_the_application_code() {
        let message = verdict(ORU).unwrap();
        let reply = first_reply(
            SourceAckMode::Original,
            Some(&message),
            ORU,
            AckOutcome::Accepted,
            ReplyType::Ack,
        )
        .unwrap();
        assert_eq!(reply.code, "AA");
        assert_eq!(msa(&reply), ("AA".into(), "CTRL-1".into()));
        assert!(
            application_reply(
                SourceAckMode::Original,
                &message,
                AckOutcome::Accepted,
                ReplyType::Ack
            )
            .is_none()
        );
        // An original-mode source answers AA even when the sender asked for enhanced.
        let enhanced = verdict(ENHANCED).unwrap();
        let reply = first_reply(
            SourceAckMode::Original,
            Some(&enhanced),
            ENHANCED,
            AckOutcome::Accepted,
            ReplyType::Ack,
        )
        .unwrap();
        assert_eq!(reply.code, "AA");
    }

    #[test]
    fn enhanced_mode_commits_then_applies() {
        let mode = SourceAckMode::Enhanced {
            send_application_ack: true,
        };
        let message = verdict(ENHANCED).unwrap();
        let commit = first_reply(
            mode,
            Some(&message),
            ENHANCED,
            AckOutcome::Accepted,
            ReplyType::Ack,
        )
        .unwrap();
        assert_eq!(msa(&commit), ("CA".into(), "CTRL-2".into()));
        let app = application_reply(mode, &message, AckOutcome::Accepted, ReplyType::Ack).unwrap();
        assert_eq!(app.code, "AA");
        // Enhanced without MSH-15/16 behaves as original.
        let plain = verdict(ORU).unwrap();
        assert_eq!(
            first_reply(mode, Some(&plain), ORU, AckOutcome::Error, ReplyType::Ack)
                .unwrap()
                .code,
            "AE"
        );
        assert!(application_reply(mode, &plain, AckOutcome::Accepted, ReplyType::Ack).is_none());
    }

    #[test]
    fn unparseable_messages_get_a_minimal_negative_ack() {
        let bad = b"MSH|^~\\&|A|B|C|D|20260101||ORU^R01|CTRL-9|P\rgarbage";
        let reply = first_reply(
            SourceAckMode::Original,
            None,
            bad,
            AckOutcome::Rejected,
            ReplyType::Ack,
        )
        .unwrap();
        assert_eq!(msa(&reply), ("AR".into(), "CTRL-9".into()));
        let worse = b"\x00\x01";
        let reply = first_reply(
            SourceAckMode::Original,
            None,
            worse,
            AckOutcome::Rejected,
            ReplyType::Ack,
        )
        .unwrap();
        assert_eq!(msa(&reply), ("AR".into(), String::new()));
    }

    #[test]
    fn destination_classifies_and_correlates_replies() {
        let ack = |code: &str, id: &str| {
            format!("MSH|^~\\&|LIS|F|PACKIE|F|20260101||ACK|A1|P|2.5.1\rMSA|{code}|{id}\r")
                .into_bytes()
        };
        assert_eq!(
            classify_reply(&ack("AA", "CTRL-1"), "CTRL-1", AckLevel::Accepted, &["ACK"]),
            ReplyVerdict::Final {
                accepted: true,
                level: AckLevel::Accepted,
                code: "AA".into(),
                ack_control_id: Some("A1".into()),
            }
        );
        assert_eq!(
            classify_reply(&ack("AA", "OTHER"), "CTRL-1", AckLevel::Accepted, &["ACK"]),
            ReplyVerdict::Unmatched
        );
        assert_eq!(
            classify_reply(&ack("CA", "CTRL-1"), "CTRL-1", AckLevel::Accepted, &["ACK"]),
            ReplyVerdict::Progress {
                level: AckLevel::Received,
                code: "CA".into()
            }
        );
        assert!(matches!(
            classify_reply(&ack("CA", "CTRL-1"), "CTRL-1", AckLevel::Received, &["ACK"]),
            ReplyVerdict::Final { accepted: true, .. }
        ));
        assert!(matches!(
            classify_reply(&ack("AR", "CTRL-1"), "CTRL-1", AckLevel::Accepted, &["ACK"]),
            ReplyVerdict::Final {
                accepted: false,
                ..
            }
        ));
        assert_eq!(outgoing_control_id(ORU).as_deref(), Some("CTRL-1"));
    }

    const ORDER: &[u8] =
        b"MSH|^~\\&|LIS|HOSP|PACKIE|LAB|20260101||OML^O33^OML_O33|ORD-1|P|2.5.1\rPID|||PID-TEST-1\rSPM|1|SID-1\r";

    #[test]
    fn order_messages_get_order_responses_only_when_configured() {
        let message = verdict(ORDER).unwrap();
        let plain = first_reply(
            SourceAckMode::Original,
            Some(&message),
            ORDER,
            AckOutcome::Accepted,
            ReplyType::Ack,
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&plain.bytes).contains("|ACK^O33^ACK|"));
        let response = first_reply(
            SourceAckMode::Original,
            Some(&message),
            ORDER,
            AckOutcome::Accepted,
            ReplyType::OrderResponse,
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&response.bytes).contains("|ORL^O34^ORL_O34|"));
        assert_eq!(msa(&response), ("AA".into(), "ORD-1".into()));
        // results keep their ACK
        let result = verdict(ORU).unwrap();
        let reply = first_reply(
            SourceAckMode::Original,
            Some(&result),
            ORU,
            AckOutcome::Accepted,
            ReplyType::OrderResponse,
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&reply.bytes).contains("|ACK^R01^ACK|"));
    }

    #[test]
    fn destination_accepts_only_the_listed_reply_types() {
        let orl = b"MSH|^~\\&|AM|LAB|LIS|F|20260101||ORL^O34^ORL_O34|R1|P|2.5.1\rMSA|AA|CTRL-1\r";
        let verdict = |types: &[&str]| classify_reply(orl, "CTRL-1", AckLevel::Accepted, types);
        assert_eq!(verdict(&["ACK"]), ReplyVerdict::Unmatched);
        assert!(matches!(
            verdict(&["ACK", "ORL"]),
            ReplyVerdict::Final { accepted: true, .. }
        ));
    }
}
