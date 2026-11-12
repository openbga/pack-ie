//! Beckman AU400/AU640 for message channels: the driver configuration from connector
//! settings, identifiers and abort reasons (`docs/spec/message-channels.md` §2.2, §3, §7).

use std::time::Duration;

use packie_au::{
    AbortReason, Action, AuConfig, FormatConfig, FramingConfig, ReceiveDriver, ReceiveEvent,
    ReceiveOptions, SampleResults, TimerId, TransmissionClass,
};
use packie_config::{AuClass, AuSettings};
use packie_core::ProtocolKind;

use crate::message::source::{Completion, FrameDriver, Step, Verdict};
use crate::message::store::Identifiers;

/// The driver configuration of the instrument's online parameter sheet.
pub(crate) fn config(settings: &AuSettings) -> (AuConfig, ReceiveOptions) {
    let framing = &settings.framing;
    let format = &settings.format;
    let config = AuConfig {
        framing: FramingConfig {
            start: framing.start.clone(),
            end: framing.end.clone(),
            bcc: framing.bcc,
            use_etb: framing.use_etb,
            max_wire_len: framing.max_wire_len,
            class: match framing.class {
                AuClass::A => TransmissionClass::A,
                AuClass::B => TransmissionClass::B,
            },
        },
        format: FormatConfig {
            unit_no: format.unit_no,
            rack_digits: format.rack_digits,
            sample_id_digits: format.sample_id_digits,
            sex: format.sex,
            age: format.age,
            month: format.month,
            patient_info_digits: format.patient_info_digits,
            result_digits: format.result_digits,
            test_code_digits: format.test_code_digits,
        },
    };
    let options = ReceiveOptions {
        block_timeout: Duration::from_millis(u64::from(settings.block_timeout_ms)),
        frame_timeout: Duration::from_millis(u64::from(settings.frame_timeout_ms)),
        ..ReceiveOptions::default()
    };
    (config, options)
}

/// Identifiers of a completed sample: the sample id, and the rack, cup, unit and sample
/// id with the instrument (endpoint) name as the source identity.
pub(crate) fn identifiers(results: &SampleResults, instrument: &str) -> Identifiers {
    let header = &results.header;
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
    let sample_id = Some(text(header.sample_id.expose())).filter(|v| !v.is_empty());
    let parts = [
        header
            .unit_no
            .as_ref()
            .map(|u| text(u.expose()))
            .unwrap_or_default(),
        text(header.rack.expose()),
        text(header.cup.expose()),
        sample_id.clone().unwrap_or_default(),
    ];
    let source_identity = parts
        .iter()
        .any(|p| !p.is_empty())
        .then(|| format!("{}|{instrument}", parts.join("|")));
    Identifiers {
        control_id: None,
        sample_id,
        patient_id: None,
        message_type: Some("au".to_owned()),
        source_identity,
    }
}

/// `abort_reason` of an abandoned sample.
pub(crate) const fn abort_reason(reason: AbortReason) -> &'static str {
    match reason {
        AbortReason::RetryLimitExceeded => "retry_limit_exceeded",
        AbortReason::BlockTimeout => "receive_timeout",
        AbortReason::ProtocolError => "transport_error",
        AbortReason::IncompleteSample => "incomplete_record",
    }
}

/// The source session's view of the AU receive driver: the committed frames form the
/// message; samples are routed, control and unparsed frames kept unrouted.
pub(crate) struct SourceDriver {
    driver: ReceiveDriver,
    /// The source endpoint's name, part of a sample's source identity.
    instrument: String,
}

impl SourceDriver {
    pub(crate) const fn new(driver: ReceiveDriver, instrument: String) -> Self {
        Self { driver, instrument }
    }
}

impl FrameDriver for SourceDriver {
    type Event = ReceiveEvent;
    type Timer = TimerId;
    const PROTOCOL: ProtocolKind = ProtocolKind::Au;
    const FAILED: &'static str = "AU session ended after repeated errors";
    const FRAMES_ARE_MESSAGE: bool = true;

    fn bytes(bytes: Vec<u8>) -> ReceiveEvent {
        ReceiveEvent::Bytes(bytes)
    }

    fn expired(timer: TimerId) -> ReceiveEvent {
        ReceiveEvent::TimerExpired(timer)
    }

    fn steps(&mut self, event: ReceiveEvent) -> Option<Vec<Step<ReceiveEvent, TimerId>>> {
        let actions = self.driver.handle(event).ok()?;
        let kept = |verdict| {
            Step::Complete(Completion::Frames {
                identifiers: Identifiers::default(),
                verdict: Some(verdict),
            })
        };
        Some(
            actions
                .into_iter()
                .map(|action| match action {
                    Action::Send(response) => Step::Write(vec![response.byte()]),
                    Action::PersistFrame { frame_id, bytes } => Step::Persist {
                        bytes: bytes.expose().clone(),
                        persisted: ReceiveEvent::Persisted(frame_id),
                    },
                    Action::SampleComplete(results) => Step::Complete(Completion::Frames {
                        identifiers: identifiers(&results, &self.instrument),
                        verdict: None,
                    }),
                    // Instrument requests and frames that do not parse are kept, never routed.
                    Action::Control { .. } => kept(Verdict::Filtered("control_message")),
                    Action::Unparsed { .. } => kept(Verdict::Invalid("unparseable")),
                    Action::Aborted(reason) => Step::Abort(abort_reason(reason)),
                    Action::StartTimer { timer_id, duration } => {
                        Step::StartTimer(timer_id, duration)
                    }
                    Action::StopTimer => Step::StopTimer,
                })
                .collect(),
        )
    }
}
