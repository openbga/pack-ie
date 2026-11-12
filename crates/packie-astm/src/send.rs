//! Sender-side LIS01-A2 state machine: establishment, contention, frame
//! transmission with retransmission limits, termination and receiver interrupt.

use crate::contract::{Action, DriverError, LineRole, LinkOptions, SendAbortReason, TimerId};
use crate::frame::{ACK, ENQ, EOT, NAK, encode_records_within};
use crate::timer::TimerSlot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    /// No outbound message.
    Idle,
    /// A message is queued but the line is busy with an inbound transfer.
    Pending,
    /// ENQ written; waiting for ACK, NAK or ENQ.
    AwaitReply,
    /// A frame was written; waiting for ACK, NAK or EOT.
    Transmitting,
    /// The peer was busy or silent; waiting to send ENQ again.
    BusyWait,
    /// Instrument role after contention; waiting briefly before sending ENQ again.
    ContentionWait,
    /// Computer role after contention; waiting for the instrument's ENQ.
    Yielded,
}

/// Pure sender. It never reads the transport: the owner passes received bytes
/// and timer expiries in and performs the returned actions.
#[derive(Debug)]
pub(crate) struct SendMachine {
    options: LinkOptions,
    state: State,
    frames: Vec<Vec<u8>>,
    frame_index: usize,
    retransmissions: u8,
    enq_attempts: u8,
    line_busy: bool,
    timer: TimerSlot,
}

impl SendMachine {
    pub(crate) fn new(options: LinkOptions) -> Self {
        Self {
            options,
            state: State::Idle,
            frames: Vec::new(),
            frame_index: 0,
            retransmissions: 0,
            enq_attempts: 0,
            line_busy: false,
            timer: TimerSlot::even(),
        }
    }

    /// True while a message is queued or in flight.
    pub(crate) fn is_sending(&self) -> bool {
        self.state != State::Idle
    }

    /// True when received bytes must go to the sender rather than the receiver.
    pub(crate) fn wants_bytes(&self) -> bool {
        matches!(
            self.state,
            State::AwaitReply | State::Transmitting | State::ContentionWait | State::Yielded
        )
    }

    /// Frames and starts sending one message.
    pub(crate) fn submit(
        &mut self,
        records: &[Vec<u8>],
        out: &mut Vec<Action>,
    ) -> Result<(), DriverError> {
        if self.is_sending() {
            return Err(DriverError::SendInProgress);
        }
        if records.is_empty() {
            return Err(DriverError::EmptyMessage);
        }
        self.frames = encode_records_within(records, self.options.max_frame_text)?;
        if self.line_busy {
            self.state = State::Pending;
            Ok(())
        } else {
            self.establish(out)
        }
    }

    /// An inbound transfer owns the line; a queued message waits for it.
    pub(crate) fn set_line_busy(&mut self) {
        self.line_busy = true;
        if self.state == State::BusyWait {
            // The inbound transfer's timer replaces ours.
            self.timer.forget();
            self.state = State::Pending;
        }
    }

    /// Restarts a retry wait whose timer was displaced by an inbound transfer.
    pub(crate) fn rearm_wait(&mut self, out: &mut Vec<Action>) -> Result<(), DriverError> {
        if self.state == State::BusyWait {
            self.timer.start(self.options.busy_retry_delay, out)?;
        }
        Ok(())
    }

    /// The inbound transfer ended; a queued message starts establishment.
    pub(crate) fn set_line_idle(&mut self, out: &mut Vec<Action>) -> Result<(), DriverError> {
        self.line_busy = false;
        if self.state == State::Pending {
            self.establish(out)?;
        }
        Ok(())
    }

    /// Consumes bytes addressed to the sender. Returns the bytes, if any, that
    /// the owner must pass to the receiver instead.
    pub(crate) fn on_bytes(
        &mut self,
        bytes: &[u8],
        out: &mut Vec<Action>,
    ) -> Result<Option<Vec<u8>>, DriverError> {
        for (index, byte) in bytes.iter().copied().enumerate() {
            let rest = || Some(bytes[index..].to_vec());
            let handled = match self.state {
                State::Idle | State::Pending | State::BusyWait => return Ok(rest()),
                State::AwaitReply => self.on_establish_reply(byte, out)?,
                State::Transmitting => self.on_frame_reply(byte, out)?,
                State::Yielded if byte == ENQ => {
                    self.line_busy = true;
                    self.state = State::Pending;
                    self.timer.stop(out);
                    return Ok(rest());
                }
                State::ContentionWait | State::Yielded => false,
            };
            if handled {
                let remainder = &bytes[index + 1..];
                let hand_over = self.state == State::Idle && !remainder.is_empty();
                return Ok(hand_over.then(|| remainder.to_vec()));
            }
        }
        Ok(None)
    }

    /// Handles a timer expiry; stale identities are ignored.
    pub(crate) fn on_timer(
        &mut self,
        timer_id: TimerId,
        out: &mut Vec<Action>,
    ) -> Result<(), DriverError> {
        if !self.timer.expire(timer_id) {
            return Ok(());
        }
        match self.state {
            State::AwaitReply => {
                // LIS01-A2: after a reply timeout the sender returns the line to neutral.
                out.push(Action::Write(vec![EOT]));
                self.establishment_failed(out)
            }
            State::Transmitting => {
                out.push(Action::Write(vec![EOT]));
                self.abort(SendAbortReason::ReplyTimeout, out);
                Ok(())
            }
            State::BusyWait | State::ContentionWait | State::Yielded => self.establish(out),
            State::Idle | State::Pending => Ok(()),
        }
    }

    fn on_establish_reply(&mut self, byte: u8, out: &mut Vec<Action>) -> Result<bool, DriverError> {
        match byte {
            ACK => {
                self.state = State::Transmitting;
                self.send_current_frame(out)?;
            }
            NAK => self.establishment_failed(out)?,
            ENQ => self.contention(out)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn on_frame_reply(&mut self, byte: u8, out: &mut Vec<Action>) -> Result<bool, DriverError> {
        match byte {
            ACK => self.frame_acknowledged(out)?,
            NAK => {
                self.retransmissions = self.retransmissions.saturating_add(1);
                if self.retransmissions > self.options.max_frame_retransmissions {
                    out.push(Action::Write(vec![EOT]));
                    self.abort(SendAbortReason::RetryLimitExceeded, out);
                } else {
                    self.send_current_frame(out)?;
                }
            }
            EOT => self.abort(SendAbortReason::ReceiverInterrupted, out),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn frame_acknowledged(&mut self, out: &mut Vec<Action>) -> Result<(), DriverError> {
        self.frame_index += 1;
        self.retransmissions = 0;
        if self.frame_index < self.frames.len() {
            return self.send_current_frame(out);
        }
        self.timer.stop(out);
        out.push(Action::Write(vec![EOT]));
        out.push(Action::MessageSent);
        self.finish();
        Ok(())
    }

    /// The peer sent ENQ while ours was outstanding.
    fn contention(&mut self, out: &mut Vec<Action>) -> Result<(), DriverError> {
        match self.options.role {
            LineRole::Instrument => {
                self.state = State::ContentionWait;
                self.timer.start(self.options.contention_retry_delay, out)
            }
            LineRole::Computer => {
                // Yielding is not a failed attempt.
                self.enq_attempts = self.enq_attempts.saturating_sub(1);
                self.state = State::Yielded;
                self.timer.start(self.options.contention_yield_timeout, out)
            }
        }
    }

    fn establish(&mut self, out: &mut Vec<Action>) -> Result<(), DriverError> {
        if self.enq_attempts >= self.options.max_enq_attempts {
            self.abort(SendAbortReason::EstablishmentFailed, out);
            return Ok(());
        }
        self.enq_attempts += 1;
        self.state = State::AwaitReply;
        out.push(Action::Write(vec![ENQ]));
        self.timer.start(self.options.reply_timeout, out)
    }

    fn establishment_failed(&mut self, out: &mut Vec<Action>) -> Result<(), DriverError> {
        if self.enq_attempts >= self.options.max_enq_attempts {
            self.abort(SendAbortReason::EstablishmentFailed, out);
            return Ok(());
        }
        self.state = State::BusyWait;
        self.timer.start(self.options.busy_retry_delay, out)
    }

    fn send_current_frame(&mut self, out: &mut Vec<Action>) -> Result<(), DriverError> {
        let frame = self.frames[self.frame_index].clone();
        out.push(Action::Write(frame));
        self.timer.start(self.options.reply_timeout, out)
    }

    fn abort(&mut self, reason: SendAbortReason, out: &mut Vec<Action>) {
        self.timer.stop(out);
        out.push(Action::SendAborted(reason));
        self.finish();
    }

    fn finish(&mut self) {
        self.state = State::Idle;
        self.frames.clear();
        self.frame_index = 0;
        self.retransmissions = 0;
        self.enq_attempts = 0;
    }
}
