//! HL7 v2 over MLLP on a source connection: commit, acknowledge, pipeline, application
//! acknowledgement (`docs/spec/message-channels.md` §2.2, §3).

use std::time::Duration;

use packie_config::{AckMode, Hl7Settings};
use packie_core::{MessageId, MessageState, ProtocolKind, Timestamp};
use packie_hl7::ack::AckOutcome;
use packie_hl7::mllp;
use packie_transport::Operation;
use tokio::time::Instant;

use super::{End, READ_CHUNK, Session, log_committed, log_duplicate};
use crate::message::hl7::{self, SourceAckMode};
use crate::message::pipeline::Routed;
use crate::status::MessageCounters;
use crate::stop::{StopRx, sleep_until, stopped};

impl Session<'_> {
    pub(super) async fn hl7(mut self, settings: &Hl7Settings, stop: &mut StopRx) -> End {
        let max = usize::try_from(settings.max_message_bytes).unwrap_or(usize::MAX);
        let frame_timeout = Duration::from_millis(u64::from(settings.frame_timeout_ms));
        let mut decoder = mllp::Decoder::new(max);
        if !settings.require_end_cr {
            decoder = decoder.with_optional_end_cr();
        }
        let mut buf = vec![0_u8; READ_CHUNK];
        let mut frame_deadline: Option<Instant> = None;
        loop {
            let read = tokio::select! {
                biased;
                _ = stopped(stop) => return End::Stopped,
                () = sleep_until(frame_deadline), if frame_deadline.is_some() => Err(End::Closed {
                    operation: Operation::Read,
                    cause: "a frame did not end within frame_timeout_ms".into(),
                }),
                read = self.read(&mut buf) => read,
            };
            let n = match read {
                Ok(n) => n,
                Err(end) => return self.ended(end, frame_deadline.is_some()),
            };
            let mut events = Vec::new();
            decoder.feed(buf.get(..n).unwrap_or_default(), |event| events.push(event));
            if let Err(end) = self.hl7_events(settings, events, max).await {
                return end;
            }
            // The timeout of a frame left open runs only while the engine waits for its
            // bytes: it starts once the complete frames of this read are committed and
            // acknowledged, and starts again with every read that leaves a frame open.
            frame_deadline = match decoder.state() {
                mllp::State::SeekingStart | mllp::State::AfterEndBlock => None,
                _ => Some(Instant::now() + frame_timeout),
            };
            self.waiting_for_rest(frame_deadline.is_some());
        }
    }

    /// The session ends with `end`. When a frame was open (`in_frame`), the partial frame
    /// is discarded and logged: it was never acknowledged, so the sender sends it again.
    fn ended(&self, end: End, in_frame: bool) -> End {
        let End::Closed { cause, .. } = &end else {
            return end;
        };
        if !in_frame {
            return end;
        }
        tracing::warn!(
            event = "source.frame_abandoned",
            error_summary = cause.as_str(),
            "Channel {}: the connection ended inside a frame ({cause}); the partial frame was discarded",
            self.cx.shared.id
        );
        end
    }

    async fn hl7_events(
        &mut self,
        settings: &Hl7Settings,
        events: Vec<mllp::Event>,
        max: usize,
    ) -> Result<(), End> {
        for event in events {
            self.hl7_event(settings, event, max).await?;
        }
        Ok(())
    }

    async fn hl7_event(
        &mut self,
        settings: &Hl7Settings,
        event: mllp::Event,
        max: usize,
    ) -> Result<(), End> {
        match event {
            mllp::Event::Message(payload) => self.hl7_message(settings, payload, max).await,
            mllp::Event::Garbage { .. }
            | mllp::Event::Malformed { .. }
            | mllp::Event::Oversize { .. }
            | mllp::Event::AllocationFailed
            | mllp::Event::Truncated { .. } => {
                tracing::warn!(
                    event = "source.framing_error",
                    "Channel {}: bytes outside a valid MLLP frame were discarded",
                    self.cx.shared.id
                );
                Ok(())
            }
        }
    }

    /// One frame: commit, acknowledge, pipeline, application acknowledgement.
    async fn hl7_message(
        &mut self,
        settings: &Hl7Settings,
        payload: Vec<u8>,
        max: usize,
    ) -> Result<(), End> {
        let mode = match settings.ack_mode {
            AckMode::Enhanced => SourceAckMode::Enhanced {
                send_application_ack: settings.send_application_ack,
            },
            AckMode::Original | AckMode::None => SourceAckMode::Original,
        };
        let parsed = hl7::verdict(&payload);
        let identifiers = parsed.as_ref().map(hl7::identifiers).unwrap_or_default();
        let receipt = self.receipt(ProtocolKind::Hl7, identifiers);
        let committed = self
            .cx
            .store
            .commit_received(receipt, payload.clone())
            .await;
        self.store_health(&committed);
        let outcome = match (&committed, &parsed) {
            (Err(_), _) => AckOutcome::Error,
            (Ok(c), _) if c.state == MessageState::Duplicate => AckOutcome::Accepted,
            (Ok(_), Err(_)) => AckOutcome::Rejected,
            (Ok(_), Ok(_)) => AckOutcome::Accepted,
        };
        let reply = hl7::first_reply(
            mode,
            parsed.as_ref().ok(),
            &payload,
            outcome,
            settings.reply_type,
        );
        if let Some(reply) = &reply {
            self.send_hl7(&reply.bytes, max).await?;
        }
        let Ok(committed) = committed else {
            return Ok(());
        };
        self.mark_stable();
        let code = reply.as_ref().map_or("", |r| r.code);
        log_committed(&committed, payload.len(), code);
        if let Some(reply) = reply {
            let _ = self
                .cx
                .store
                .record_response(
                    committed.message_id,
                    reply.code.to_owned(),
                    Timestamp::now(),
                )
                .await;
        }
        let invalid = parsed.as_ref().err().copied();
        self.after_commit(
            committed.message_id,
            committed.state,
            payload.len(),
            invalid,
        )
        .await;
        let Ok(message) = parsed else {
            return Ok(());
        };
        if committed.state != MessageState::Received {
            return Ok(());
        }
        let routed = self.cx.route(committed.message_id, &payload, &[]).await;
        let outcome = match routed {
            Some(Routed::To(_) | Routed::Filtered(_)) => AckOutcome::Accepted,
            Some(Routed::Invalid(_)) => AckOutcome::Rejected,
            None => AckOutcome::Error,
        };
        if let Some(reply) = hl7::application_reply(mode, &message, outcome, settings.reply_type) {
            self.send_hl7(&reply.bytes, max).await?;
        }
        Ok(())
    }

    /// Counts the commit; an HL7 message that failed the syntax check (`invalid`, with the
    /// verdict's reason) is recorded `invalid`.
    async fn after_commit(
        &mut self,
        message: MessageId,
        state: MessageState,
        bytes: usize,
        invalid: Option<&'static str>,
    ) {
        let Some(counters) = self.counters() else {
            return;
        };
        // Every committed message counts as received, duplicates included.
        self.cx.shared.message_received(bytes);
        if state == MessageState::Duplicate {
            log_duplicate(counters, message);
            return;
        }
        if let Some(reason) = invalid {
            let _ = self
                .cx
                .store
                .record_invalid(message, reason.to_owned())
                .await;
            MessageCounters::bump(&counters.invalid);
            tracing::warn!(event = "message.invalid", message_id = %message, error_code = reason, "Invalid HL7 message");
        }
    }

    async fn send_hl7(&mut self, reply: &[u8], max: usize) -> Result<(), End> {
        let Ok(frame) = mllp::encode(reply, max.max(reply.len())) else {
            return Ok(());
        };
        self.write(&frame).await
    }
}
