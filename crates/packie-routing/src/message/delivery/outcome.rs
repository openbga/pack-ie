//! What a delivery attempt proved, and the acknowledgements that settle it
//! (`docs/spec/message-channels.md` §5, §8, §11).

use packie_core::{AckLevel, AttemptResult, ConnectorId, DeliveryId, Timestamp};

use super::log_unmatched;
use crate::message::MessageContext;
use crate::message::hl7::{ReplyVerdict, classify_reply};

/// What an attempt proved.
pub(super) struct Outcome {
    pub(super) result: AttemptResult,
    pub(super) level: AckLevel,
    pub(super) code: Option<String>,
    /// The acknowledgement's own control id.
    pub(super) ack_control_id: Option<String>,
    pub(super) cause: Option<String>,
    pub(super) sent_completely: bool,
    /// The connection is still usable.
    pub(super) keep: bool,
}

impl Outcome {
    pub(super) fn new(result: AttemptResult, level: AckLevel, keep: bool) -> Self {
        Self {
            result,
            level,
            code: None,
            ack_control_id: None,
            cause: None,
            sent_completely: !matches!(
                result,
                AttemptResult::WriteFailed | AttemptResult::SendTimeout
            ),
            keep,
        }
    }

    pub(super) fn cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }
}

/// Records lower acknowledgement levels as they arrive.
pub(super) struct Progress<'a> {
    pub(super) cx: &'a MessageContext,
    pub(super) delivery: DeliveryId,
    pub(super) destination: ConnectorId,
}

impl Progress<'_> {
    async fn record(&self, level: AckLevel, code: String) {
        let _ = self
            .cx
            .store
            .record_ack_progress(self.delivery, level, Some(code), Timestamp::now())
            .await;
    }
}

/// Applies replies to the attempt in flight; `Some` when one ended it.
pub(super) async fn handle_replies(
    replies: &[Vec<u8>],
    control: &str,
    required: AckLevel,
    message_types: &[&str],
    level: &mut AckLevel,
    progress: &Progress<'_>,
) -> Option<Outcome> {
    for reply in replies {
        match classify_reply(reply, control, required, message_types) {
            ReplyVerdict::Unmatched => log_unmatched(progress.cx, progress.destination),
            ReplyVerdict::Progress {
                level: reached,
                code,
            } => {
                *level = reached;
                progress.record(reached, code).await;
            }
            ReplyVerdict::Final {
                accepted,
                level: reached,
                code,
                ack_control_id,
            } => {
                let mut outcome = final_outcome(accepted, reached, *level, code);
                outcome.ack_control_id = ack_control_id;
                return Some(outcome);
            }
        }
    }
    None
}

/// The outcome of a final reply: accepted at `reached`, or rejected at what was proven.
fn final_outcome(accepted: bool, reached: AckLevel, proven: AckLevel, code: String) -> Outcome {
    let mut outcome = if accepted {
        Outcome::new(AttemptResult::Acknowledged, reached, true)
    } else {
        Outcome::new(AttemptResult::Rejected, proven, true)
            .cause(format!("the destination answered {code}"))
    };
    outcome.code = Some(code);
    outcome
}

/// An attempt cut short by a stop: `outcome_unknown` when every byte was written,
/// otherwise back to the queue uncounted (`message-channels.md` §11, §14).
pub(super) fn stopped_outcome(sent: bool) -> Outcome {
    let mut outcome = Outcome::new(AttemptResult::EngineStopped, AckLevel::None, false)
        .cause("the channel stopped during the attempt");
    outcome.sent_completely = sent;
    outcome
}
