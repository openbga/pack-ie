//! Sending on a destination connection, per protocol: HL7 over MLLP with its
//! acknowledgements, the ASTM link driver, and AU frames with ACK/NAK
//! (`docs/spec/message-channels.md` §8).

use std::collections::VecDeque;
use std::time::Duration;

use packie_astm::{Action, Event};
use packie_au::{TransmissionClass, WireEvent};
use packie_config::AckMode;
use packie_core::{AckLevel, AttemptResult};
use packie_hl7::mllp;
use packie_transport::describe_io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

use super::outcome::{Outcome, Progress, handle_replies};
use super::{Conn, READ_CHUNK, Speaker};
use crate::message::astm;
use crate::message::hl7::outgoing_control_id;
use crate::message::transform::au_frames;
use crate::stop::sleep_until;

/// How often an AU frame is sent again after a NAK before the attempt fails.
const AU_FRAME_RETRIES: u32 = 3;
impl Conn {
    async fn write_all(&mut self, bytes: &[u8], limit: Duration) -> Result<(), Outcome> {
        let write = async {
            self.writer.write_all(bytes).await?;
            self.writer.flush().await
        };
        match tokio::time::timeout(limit, write).await {
            Ok(Ok(())) => {
                self.tap.sent(bytes);
                Ok(())
            }
            Ok(Err(error)) => Err(
                Outcome::new(AttemptResult::WriteFailed, AckLevel::None, false)
                    .cause(describe_io(&error)),
            ),
            Err(_) => Err(
                Outcome::new(AttemptResult::SendTimeout, AckLevel::None, false)
                    .cause("the message was not written within send_timeout_seconds"),
            ),
        }
    }

    // ----- HL7 ------------------------------------------------------------------------

    pub(super) async fn send_hl7(
        &mut self,
        body: &[u8],
        send_timeout: Duration,
        ack_timeout: Duration,
        required: AckLevel,
        progress: &Progress<'_>,
    ) -> Outcome {
        let Speaker::Hl7 {
            ack_mode,
            max,
            ack_types,
            ..
        } = &self.speaker
        else {
            return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true);
        };
        let (ack_mode, max, ack_types) = (*ack_mode, *max, ack_types.clone());
        let Ok(frame) = mllp::encode(body, max) else {
            return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true)
                .cause("the message exceeds max_message_bytes");
        };
        if let Err(outcome) = self.write_all(&frame, send_timeout).await {
            return outcome;
        }
        self.sent = true;
        if ack_mode == AckMode::None {
            return Outcome::new(AttemptResult::Acknowledged, AckLevel::HandedOff, true);
        }
        let control = outgoing_control_id(body).unwrap_or_default();
        self.await_hl7_ack(
            &control,
            required,
            &ack_types,
            Instant::now() + ack_timeout,
            progress,
        )
        .await
    }

    async fn await_hl7_ack(
        &mut self,
        control: &str,
        required: AckLevel,
        ack_types: &[String],
        deadline: Instant,
        progress: &Progress<'_>,
    ) -> Outcome {
        let types: Vec<&str> = ack_types.iter().map(String::as_str).collect();
        let mut level = AckLevel::None;
        loop {
            let replies = match self.read_replies(deadline, level).await {
                Ok(replies) => replies,
                Err(outcome) => return outcome,
            };
            if let Some(outcome) =
                handle_replies(&replies, control, required, &types, &mut level, progress).await
            {
                return outcome;
            }
        }
    }

    /// The next complete reply frames, or how the wait ended.
    async fn read_replies(
        &mut self,
        deadline: Instant,
        level: AckLevel,
    ) -> Result<Vec<Vec<u8>>, Outcome> {
        let mut buf = vec![0_u8; READ_CHUNK];
        let read = match tokio::time::timeout_at(deadline, self.reader.read(&mut buf)).await {
            Err(_) => {
                return Err(Outcome::new(AttemptResult::AckTimeout, level, true)
                    .cause("no acknowledgement within ack_timeout_seconds"));
            }
            Ok(Ok(0)) => {
                return Err(
                    Outcome::new(AttemptResult::ConnectionLostAfterSend, level, false)
                        .cause("the connection ended before the acknowledgement"),
                );
            }
            Ok(Err(error)) => {
                return Err(
                    Outcome::new(AttemptResult::ConnectionLostAfterSend, level, false)
                        .cause(describe_io(&error)),
                );
            }
            Ok(Ok(n)) => n,
        };
        self.tap.received(buf.get(..read).unwrap_or_default());
        Ok(match &mut self.speaker {
            Speaker::Hl7 { decoder, .. } => frames(decoder, buf.get(..read).unwrap_or_default()),
            Speaker::Astm { .. } | Speaker::Au { .. } => Vec::new(),
        })
    }

    // ----- ASTM -----------------------------------------------------------------------

    pub(super) async fn send_astm(&mut self, body: &[u8], send_timeout: Duration) -> Outcome {
        let records = astm::records_of(body);
        let deadline = Instant::now() + send_timeout;
        let submitted = match &mut self.speaker {
            Speaker::Astm { driver, .. } => driver.submit(&records),
            Speaker::Hl7 { .. } | Speaker::Au { .. } => {
                return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true);
            }
        };
        let Ok(actions) = submitted else {
            return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true)
                .cause("the ASTM line is busy");
        };
        let mut queue: VecDeque<Action> = actions.into();
        loop {
            if let Some(outcome) = self.astm_actions(&mut queue, send_timeout).await {
                return outcome;
            }
            let event = match self.next_astm_event(deadline).await {
                Ok(Some(event)) => event,
                Ok(None) => continue,
                Err(outcome) => return outcome,
            };
            let handled = match &mut self.speaker {
                Speaker::Astm { driver, .. } => driver.handle(event),
                Speaker::Hl7 { .. } | Speaker::Au { .. } => continue,
            };
            match handled {
                Ok(actions) => queue.extend(actions),
                Err(_) => {
                    return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, false)
                        .cause("ASTM link error");
                }
            }
        }
    }

    /// The next driver event (bytes or timer), or how the transfer ended.
    async fn next_astm_event(&mut self, deadline: Instant) -> Result<Option<Event>, Outcome> {
        let timer = match &self.speaker {
            Speaker::Astm { timer, .. } => *timer,
            Speaker::Hl7 { .. } | Speaker::Au { .. } => None,
        };
        let mut buf = vec![0_u8; READ_CHUNK];
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => Err(
                Outcome::new(AttemptResult::SendTimeout, AckLevel::None, false)
                    .cause("the transfer did not finish within send_timeout_seconds"),
            ),
            () = sleep_until(timer.map(|(_, at)| at)), if timer.is_some() => {
                if let Speaker::Astm { timer, .. } = &mut self.speaker {
                    *timer = None;
                }
                Ok(timer.map(|(id, _)| Event::TimerExpired(id)))
            }
            read = self.reader.read(&mut buf) => match read {
                Ok(n) if n > 0 => {
                    let bytes = buf.get(..n).unwrap_or_default().to_vec();
                    self.tap.received(&bytes);
                    Ok(Some(Event::Bytes(bytes)))
                }
                Ok(_) => Err(Outcome::new(AttemptResult::WriteFailed, AckLevel::None, false)
                    .cause("peer closed the connection")),
                Err(error) => Err(Outcome::new(AttemptResult::WriteFailed, AckLevel::None, false)
                    .cause(describe_io(&error))),
            },
        }
    }

    /// Carries out queued driver actions; `Some` when the transfer ended.
    async fn astm_actions(
        &mut self,
        queue: &mut VecDeque<Action>,
        limit: Duration,
    ) -> Option<Outcome> {
        while let Some(action) = queue.pop_front() {
            let ended = self.astm_action(action, limit).await;
            if ended.is_some() {
                return ended;
            }
        }
        None
    }

    /// Carries out one driver action; `Some` when the transfer ended.
    async fn astm_action(&mut self, action: Action, limit: Duration) -> Option<Outcome> {
        match action {
            Action::Write(bytes) => self.write_all(&bytes, limit).await.err().map(|mut o| {
                // A frame never fully written was not received: safe to send again.
                o.sent_completely = false;
                o
            }),
            Action::MessageSent => {
                self.sent = true;
                Some(Outcome::new(
                    AttemptResult::Acknowledged,
                    AckLevel::Received,
                    true,
                ))
            }
            Action::SendAborted(reason) => Some(
                Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true)
                    .cause(astm::send_abort_cause(reason)),
            ),
            Action::StartTimer { timer_id, duration } => {
                if let Speaker::Astm { timer, .. } = &mut self.speaker {
                    *timer = Some((timer_id, Instant::now() + duration));
                }
                None
            }
            Action::StopTimer => {
                if let Speaker::Astm { timer, .. } = &mut self.speaker {
                    *timer = None;
                }
                None
            }
            // An inbound transfer on a destination line is not stored here; it is never
            // acknowledged, so the sender keeps it.
            Action::PersistFrame { .. }
            | Action::MessageReceived(_)
            | Action::ReceiveAborted(_) => None,
        }
    }
}

impl Conn {
    // ----- AU -------------------------------------------------------------------------

    /// Sends the frames of an AU message in order. Class B waits for each frame's ACK and
    /// sends a frame again after a NAK, up to [`AU_FRAME_RETRIES`] times; every frame
    /// acknowledged proves `received`. Class A has no acknowledgement: a completely
    /// written message proves `handed_off`.
    pub(super) async fn send_au(
        &mut self,
        body: &[u8],
        send_timeout: Duration,
        ack_timeout: Duration,
    ) -> Outcome {
        let Speaker::Au { config, .. } = &self.speaker else {
            return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true);
        };
        let config = config.clone();
        let class_b = config.framing.class == TransmissionClass::B;
        let Some(frames) = au_frames(&config, body) else {
            return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true)
                .cause("the stored AU message does not decode with the destination profile");
        };
        for (text, end) in frames {
            let Ok(wire) = packie_au::encode_frame(&config, &text, end) else {
                return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true)
                    .cause("an AU frame does not fit the destination profile");
            };
            if let Err(outcome) = self
                .send_au_frame(&wire, class_b, send_timeout, ack_timeout)
                .await
            {
                return outcome;
            }
        }
        self.sent = true;
        let level = if class_b {
            AckLevel::Received
        } else {
            AckLevel::HandedOff
        };
        Outcome::new(AttemptResult::Acknowledged, level, true)
    }

    async fn send_au_frame(
        &mut self,
        wire: &[u8],
        class_b: bool,
        send_timeout: Duration,
        ack_timeout: Duration,
    ) -> Result<(), Outcome> {
        let mut naks = 0_u32;
        loop {
            self.write_all(wire, send_timeout).await.map_err(|mut o| {
                o.sent_completely = false;
                o
            })?;
            if !class_b || self.au_reply(Instant::now() + ack_timeout).await? {
                return Ok(());
            }
            naks += 1;
            if naks > AU_FRAME_RETRIES {
                let mut outcome = Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true)
                    .cause("the instrument refused an AU frame after every retry");
                // A refused frame was not received: the message may be sent again.
                outcome.sent_completely = false;
                return Err(outcome);
            }
        }
    }

    /// `true` for ACK, `false` for NAK; frames the instrument sends meanwhile are ignored.
    async fn au_reply(&mut self, deadline: Instant) -> Result<bool, Outcome> {
        let mut buf = vec![0_u8; READ_CHUNK];
        loop {
            let read = tokio::time::timeout_at(deadline, self.reader.read(&mut buf)).await;
            let n = match read {
                Err(_) => {
                    return Err(
                        Outcome::new(AttemptResult::AckTimeout, AckLevel::None, true)
                            .cause("no ACK or NAK for an AU frame within ack_timeout_seconds"),
                    );
                }
                Ok(Ok(0)) => {
                    return Err(Outcome::new(
                        AttemptResult::ConnectionLostAfterSend,
                        AckLevel::None,
                        false,
                    )
                    .cause("the connection ended before the ACK"));
                }
                Ok(Err(error)) => {
                    return Err(Outcome::new(
                        AttemptResult::ConnectionLostAfterSend,
                        AckLevel::None,
                        false,
                    )
                    .cause(describe_io(&error)));
                }
                Ok(Ok(n)) => n,
            };
            let bytes = buf.get(..n).unwrap_or_default();
            self.tap.received(bytes);
            let Speaker::Au {
                decoder: Some(decoder),
                ..
            } = &mut self.speaker
            else {
                continue;
            };
            if let Some(acknowledged) = decoder.push(bytes).into_iter().find_map(ack_or_nak) {
                return Ok(acknowledged);
            }
        }
    }
}

/// `true` for an ACK, `false` for a NAK; other events are not replies.
fn ack_or_nak(event: Result<WireEvent, packie_au::AuError>) -> Option<bool> {
    match event {
        Ok(WireEvent::Ack) => Some(true),
        Ok(WireEvent::Nak) => Some(false),
        Ok(WireEvent::Frame(_)) | Err(_) => None,
    }
}

/// Complete MLLP frames in `bytes`.
fn frames(decoder: &mut mllp::Decoder, bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    decoder.feed(bytes, |event| frames.extend(message_of(event)));
    frames
}

fn message_of(event: mllp::Event) -> Option<Vec<u8>> {
    match event {
        mllp::Event::Message(bytes) => Some(bytes),
        _ => None,
    }
}
