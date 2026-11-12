//! Receive-side session state machine: bytes in, actions out.
//!
//! [`ReceiveDriver`] composes [`StreamDecoder`], [`parse_message`] and
//! [`ResultAssembler`] and ports the protocol logic of an AU receive session. It performs no
//! I/O, owns no clock and never blocks: the caller feeds [`ReceiveEvent`]s and carries out the
//! returned [`Action`]s (write a byte, store a frame, arm a timer, hand a message onwards).
//!
//! # Ordering guarantees
//!
//! * Actions are returned in the order they occurred; a single `handle` call may return the
//!   consequences of several queued frames, in arrival order.
//! * Class B: a frame that is accepted yields [`Action::PersistFrame`] and a turnaround
//!   [`Action::StartTimer`] together. `Send(Ack)` is returned only after **both** the matching
//!   [`ReceiveEvent::Persisted`] and the expiry of the turnaround timer
//!   ([`ACK_TURNAROUND`], 500 ms, manual pp. 20-22) have been seen, in either order. The
//!   message event of that frame ([`Action::SampleComplete`], [`Action::Control`] or
//!   [`Action::Unparsed`]) follows `Send(Ack)` immediately, so the caller never acts on a
//!   frame that is not durably stored and acknowledged. A rejected frame yields only a
//!   turnaround timer and then `Send(Nak)`; it is not persisted and produces no message
//!   event. A repeated frame that is re-acknowledged yields no `PersistFrame`.
//! * Class B: decoded frames that arrive while a response is outstanding are queued (at most
//!   [`MAX_QUEUED_EVENTS`]) and processed in order once the response has been released.
//!   Overflowing the queue aborts with [`AbortReason::ProtocolError`] and closes the driver.
//! * Class A: no ACK or NAK is ever requested and nothing is gated. An accepted frame yields
//!   `PersistFrame` followed at once by its message event. `Persisted` events are ignored.
//! * At most one timer is active. `StartTimer` replaces any active timer; `StopTimer` cancels
//!   it. A [`ReceiveEvent::TimerExpired`] whose id is not the active timer is ignored, as is a
//!   `Persisted` whose id is not awaited.
//! * The block timer (T1/T7) is armed when a partial sample exists: after the ACK or NAK is
//!   written (class B) or after the frame is accepted (class A). It bounds the gap before the
//!   next frame starts; its expiry yields [`Action::Aborted`] with
//!   [`AbortReason::BlockTimeout`].
//! * The frame timer (T2, [`ReceiveOptions::frame_timeout`]) starts when a frame's first byte
//!   arrives, replacing the block timer (the next block has started), and stops when the
//!   frame ends. Its expiry discards the incomplete frame's bytes; no response is sent, and
//!   when a partial sample exists the block timer is armed again, so a sample whose rest
//!   never comes is still aborted. While a class B response is outstanding the turnaround
//!   timer keeps the slot.
//! * [`Action::Aborted`] means "discard the unit of frames stored so far". It is emitted when a
//!   partial sample is dropped, always before the actions of the frame that caused it.
//!   [`AbortReason::RetryLimitExceeded`] and an overflowed queue are terminal: later calls fail
//!   with [`DriverError::Closed`] and the caller should close the connection.
//! * After an abort the sample identity is remembered; later non-initial blocks of the same
//!   sample are refused with NAK until block 0 starts it again.
//! * Failures are counted per rejected frame (assembler conflicts, checksum or framing errors,
//!   stray ACK/NAK bytes, host-direction messages) and reset by any newly accepted frame. More
//!   than [`ReceiveOptions::max_failures`] consecutive failures end the session.
//!
//! Not covered: restart and deduplication across connections (for example refusing the tail of
//! a multiblock sample after a reconnect) need durable state and belong to the caller, as do
//! timing out a connection that never sends, and discarding the stored unit when the transport
//! drops.

use std::collections::VecDeque;
use std::time::Duration;

use thiserror::Error;

use crate::frame::{ACK, NAK};
use crate::{
    AssemblyOutcome, AuConfig, AuError, BlockId, Frame, Message, Redacted, ResultAssembler,
    ResultBlock, ResultKind, SampleHeader, SampleResults, StreamDecoder, TransmissionClass,
    WireEvent, parse_message,
};

/// Earliest time the analyzer accepts a response after its frame (class B, manual pp. 20-22).
pub const ACK_TURNAROUND: Duration = Duration::from_millis(500);

/// Most decoded events held back while a class B response is outstanding.
pub const MAX_QUEUED_EVENTS: usize = 16;

/// Identifies a frame handed out for storage; echoed back in [`ReceiveEvent::Persisted`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameId(u64);

impl FrameId {
    /// The numeric value, for correlation and logging.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Identifies an armed timer; echoed back in [`ReceiveEvent::TimerExpired`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(u32);

impl TimerId {
    /// The numeric value, for correlation and logging.
    pub fn value(self) -> u32 {
        self.0
    }
}

/// Session settings that are not part of the instrument's wire profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiveOptions {
    /// Inter-block timer T1/T7: 100 ms to 9900 ms in 100 ms steps (pp. 20-22, 26).
    pub block_timeout: Duration,
    /// Frame timer T2: how long one frame may take from its first byte; 100 ms to 9900 ms
    /// in 100 ms steps.
    pub frame_timeout: Duration,
    /// Consecutive rejected frames tolerated; one more ends the session.
    pub max_failures: u8,
}

impl Default for ReceiveOptions {
    fn default() -> Self {
        Self {
            block_timeout: Duration::from_millis(9900),
            frame_timeout: Duration::from_millis(9900),
            max_failures: 4,
        }
    }
}

/// Failures of the driver itself, as opposed to protocol failures reported as actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum DriverError {
    /// The wire configuration is invalid.
    #[error("invalid AU configuration: {0}")]
    Configuration(AuError),
    /// The block timeout is outside 100..=9900 ms or not a multiple of 100 ms.
    #[error("AU block timeout must be 100 ms to 9900 ms in 100 ms steps")]
    BlockTimeout,
    /// The frame timeout is outside 100..=9900 ms or not a multiple of 100 ms.
    #[error("AU frame timeout must be 100 ms to 9900 ms in 100 ms steps")]
    FrameTimeout,
    /// The session ended with a terminal abort; create a new driver for a new connection.
    #[error("AU receive session is closed")]
    Closed,
}

/// Input to the driver.
#[derive(Clone, PartialEq, Eq)]
pub enum ReceiveEvent {
    /// Bytes read from the transport, in any chunking.
    Bytes(Vec<u8>),
    /// The caller has durably stored the frame requested by [`Action::PersistFrame`].
    Persisted(FrameId),
    /// The timer armed by [`Action::StartTimer`] has expired.
    TimerExpired(TimerId),
}

impl std::fmt::Debug for ReceiveEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
            Self::Persisted(id) => f.debug_tuple("Persisted").field(id).finish(),
            Self::TimerExpired(id) => f.debug_tuple("TimerExpired").field(id).finish(),
        }
    }
}

/// A one-byte host response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlResponse {
    /// ACK, byte 0x06.
    Ack,
    /// NAK, byte 0x15.
    Nak,
}

impl ControlResponse {
    /// The byte to write to the transport.
    pub fn byte(self) -> u8 {
        match self {
            Self::Ack => ACK,
            Self::Nak => NAK,
        }
    }
}

/// Why a partial sample or the whole session was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortReason {
    /// Too many consecutive rejected frames (or queued frames); the session is closed.
    RetryLimitExceeded,
    /// The next block or frame did not arrive within the block timeout.
    BlockTimeout,
    /// A block or frame conflicted with the sample in progress, or was unusable.
    ProtocolError,
    /// A session or query message, or a different sample, interrupted a partial sample.
    IncompleteSample,
}

/// Output of the driver; the caller carries these out in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Write this response byte to the transport.
    Send(ControlResponse),
    /// Store this frame, then report [`ReceiveEvent::Persisted`]. For class B the ACK for the
    /// frame waits for that report; for class A the report may be omitted.
    PersistFrame {
        /// Correlates the report.
        frame_id: FrameId,
        /// The exact wire bytes of the frame.
        bytes: Redacted<Vec<u8>>,
    },
    /// A sample finished: all of its blocks are stored and acknowledged.
    SampleComplete(Box<SampleResults>),
    /// A session or query message (`RB`, `RE`, `DB`, `DE`, `SE`, `R `, `RH`).
    Control {
        /// The parsed message.
        message: Box<Message>,
    },
    /// Well-framed text that does not parse; it was stored and acknowledged all the same.
    Unparsed {
        /// The exact wire bytes of the frame.
        wire: Redacted<Vec<u8>>,
    },
    /// Discard the unit of frames stored so far; see the module docs.
    Aborted(AbortReason),
    /// Arm the single timer, replacing any armed timer.
    StartTimer {
        /// Echo this in [`ReceiveEvent::TimerExpired`].
        timer_id: TimerId,
        /// Time until expiry.
        duration: Duration,
    },
    /// Cancel the armed timer.
    StopTimer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimerKind {
    Turnaround,
    Block,
    Frame,
}

#[derive(Clone, Copy, Debug)]
struct ActiveTimer {
    id: TimerId,
    kind: TimerKind,
}

/// A response held back until its frame is stored and the turnaround has elapsed.
#[derive(Debug)]
struct PendingResponse {
    byte: ControlResponse,
    persist: Option<FrameId>,
    turnaround_elapsed: bool,
    deferred: Vec<Action>,
}

/// Identity of the sample in progress: classification plus repeated header.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    kind: ResultKind,
    header: SampleHeader,
}

/// Pure receive state machine; see the module docs for the contract.
#[derive(Debug)]
pub struct ReceiveDriver {
    config: AuConfig,
    options: ReceiveOptions,
    decoder: StreamDecoder,
    assembler: ResultAssembler,
    queue: VecDeque<Result<WireEvent, AuError>>,
    response: Option<PendingResponse>,
    timer: Option<ActiveTimer>,
    next_timer: u32,
    next_frame: u64,
    unit: Option<Identity>,
    blocked: Option<Identity>,
    last_accepted: Option<Redacted<Vec<u8>>>,
    completed_multiblock: bool,
    failures: u8,
    closed: bool,
}

/// A timer setting the instrument's parameter sheet allows: 100 ms to 9900 ms in 100 ms
/// steps.
fn valid_timer(timeout: Duration) -> bool {
    let ms = timeout.as_millis();
    (100..=9900).contains(&ms)
        && ms.is_multiple_of(100)
        && timeout.subsec_nanos().is_multiple_of(1_000_000)
}

impl ReceiveDriver {
    /// Validates the configuration and options and creates an idle driver.
    pub fn new(config: AuConfig, options: ReceiveOptions) -> Result<Self, DriverError> {
        if !valid_timer(options.block_timeout) {
            return Err(DriverError::BlockTimeout);
        }
        if !valid_timer(options.frame_timeout) {
            return Err(DriverError::FrameTimeout);
        }
        let decoder = StreamDecoder::new(config.clone()).map_err(DriverError::Configuration)?;
        Ok(Self {
            config,
            options,
            decoder,
            assembler: ResultAssembler::new(),
            queue: VecDeque::new(),
            response: None,
            timer: None,
            next_timer: 0,
            next_frame: 0,
            unit: None,
            blocked: None,
            last_accepted: None,
            completed_multiblock: false,
            failures: 0,
            closed: false,
        })
    }

    /// Feeds one event and returns the resulting actions in order.
    ///
    /// Never panics on any input. Fails only with [`DriverError::Closed`] after a terminal abort.
    pub fn handle(&mut self, event: ReceiveEvent) -> Result<Vec<Action>, DriverError> {
        if self.closed {
            return Err(DriverError::Closed);
        }
        let mut out = Vec::new();
        match event {
            ReceiveEvent::Bytes(bytes) => self.on_bytes(&bytes, &mut out),
            ReceiveEvent::Persisted(id) => self.on_persisted(id, &mut out),
            ReceiveEvent::TimerExpired(id) => self.on_timer(id, &mut out),
        }
        Ok(out)
    }

    fn class_b(&self) -> bool {
        self.config.framing.class == TransmissionClass::B
    }

    fn on_bytes(&mut self, bytes: &[u8], out: &mut Vec<Action>) {
        let was_pending = self.decoder.is_pending();
        let events = self.decoder.push(bytes);
        let ended = !events.is_empty();
        self.queue.extend(events);
        self.drain(out);
        if self.closed {
            return;
        }
        // A frame started in these bytes: after a frame or error ended in them, or from idle.
        let started = self.decoder.is_pending() && (ended || !was_pending);
        if started && !self.timer.is_some_and(|t| t.kind == TimerKind::Turnaround) {
            self.start_timer(TimerKind::Frame, out);
        }
        if self.queue.len() > MAX_QUEUED_EVENTS {
            self.terminate(AbortReason::ProtocolError, out);
        }
    }

    fn on_persisted(&mut self, id: FrameId, out: &mut Vec<Action>) {
        if let Some(response) = &mut self.response
            && response.persist == Some(id)
        {
            response.persist = None;
            self.try_release(out);
        }
    }

    fn on_timer(&mut self, id: TimerId, out: &mut Vec<Action>) {
        let Some(active) = self.timer.filter(|t| t.id == id) else {
            return;
        };
        self.timer = None;
        match active.kind {
            TimerKind::Turnaround => {
                if let Some(response) = &mut self.response {
                    response.turnaround_elapsed = true;
                }
                self.try_release(out);
            }
            TimerKind::Block => self.abort_unit(Some(AbortReason::BlockTimeout), out),
            TimerKind::Frame => {
                // The frame never ended: its bytes are dropped and the sender, which gets no
                // response, sends it again.
                self.decoder.reset();
                self.arm_block_timer(out);
            }
        }
    }

    /// Releases the held response once its frame is stored and the turnaround has elapsed.
    fn try_release(&mut self, out: &mut Vec<Action>) {
        let Some(response) = self
            .response
            .take_if(|r| r.persist.is_none() && r.turnaround_elapsed)
        else {
            return;
        };
        out.push(Action::Send(response.byte));
        out.extend(response.deferred);
        self.arm_block_timer(out);
        self.drain(out);
    }

    fn drain(&mut self, out: &mut Vec<Action>) {
        while self.response.is_none() && !self.closed {
            let Some(event) = self.queue.pop_front() else {
                break;
            };
            self.disarm(out);
            match event {
                Ok(WireEvent::Frame(frame)) => self.on_frame(frame, out),
                Ok(WireEvent::Ack | WireEvent::Nak) => self.reject(out),
                Err(_) => self.on_wire_error(out),
            }
        }
    }

    fn on_frame(&mut self, frame: Frame, out: &mut Vec<Action>) {
        // A repeat of the frame just accepted means the analyzer missed the response.
        if (self.assembler.is_pending() || self.completed_multiblock)
            && self.last_accepted.as_ref() == Some(&frame.wire)
        {
            self.respond_bare(ControlResponse::Ack, out);
            return;
        }
        match parse_message(&self.config, &frame) {
            Ok(Message::ResultBlock(block)) => self.on_result(frame, block, out),
            Ok(message @ (Message::Query(_) | Message::Session { .. })) => {
                self.on_control(frame, message, out);
            }
            Ok(Message::TriBlock(_)) => self.reject(out),
            Err(_) => self.on_unparsed(frame, out),
        }
    }

    fn on_result(&mut self, frame: Frame, block: ResultBlock, out: &mut Vec<Action>) {
        let identity = Identity {
            kind: block.kind,
            header: block.header.clone(),
        };
        if block.block != BlockId::Intermediate(0) && self.blocked.as_ref() == Some(&identity) {
            self.reject(out);
            return;
        }
        if self.unit.as_ref().is_some_and(|unit| *unit != identity) {
            self.abort_unit(Some(AbortReason::IncompleteSample), out);
        }
        let was_pending = self.assembler.is_pending();
        let outcome = self.assembler.push(block);
        if matches!(outcome, Ok(AssemblyOutcome::Duplicate)) {
            self.respond_bare(ControlResponse::Ack, out);
            return;
        }
        self.unit = Some(identity);
        let mut deferred = Vec::new();
        match outcome {
            Ok(AssemblyOutcome::Pending) => self.blocked = None,
            Ok(AssemblyOutcome::Complete(sample)) => {
                self.unit = None;
                self.completed_multiblock = was_pending;
                self.blocked = None;
                deferred.push(Action::SampleComplete(sample));
            }
            Ok(AssemblyOutcome::Duplicate) | Err(_) => {
                self.reject(out);
                return;
            }
        }
        self.accept(frame, deferred, out);
    }

    fn on_control(&mut self, frame: Frame, message: Message, out: &mut Vec<Action>) {
        self.abort_unit(Some(AbortReason::IncompleteSample), out);
        self.accept(
            frame,
            vec![Action::Control {
                message: Box::new(message),
            }],
            out,
        );
    }

    fn on_unparsed(&mut self, frame: Frame, out: &mut Vec<Action>) {
        // Retained and acknowledged: the caller keeps the bytes, nothing is lost.
        self.abort_unit(Some(AbortReason::ProtocolError), out);
        let wire = frame.wire.clone();
        self.accept(frame, vec![Action::Unparsed { wire }], out);
    }

    /// A decoder error: class B asks for a retransmission and keeps the acknowledged prefix;
    /// class A cannot, so the partial sample is dropped.
    fn on_wire_error(&mut self, out: &mut Vec<Action>) {
        self.refuse(!self.class_b(), out);
    }

    fn reject(&mut self, out: &mut Vec<Action>) {
        self.refuse(true, out);
    }

    /// Counts one failure: past the limit the session ends, otherwise NAK, dropping the partial
    /// sample first when `abort` is set.
    fn refuse(&mut self, abort: bool, out: &mut Vec<Action>) {
        self.failures = self.failures.saturating_add(1);
        if self.failures > self.options.max_failures {
            self.terminate(AbortReason::RetryLimitExceeded, out);
        } else {
            if abort {
                self.abort_unit(Some(AbortReason::ProtocolError), out);
            }
            self.respond_bare(ControlResponse::Nak, out);
        }
    }

    /// Stores the frame, remembers it as acknowledged and queues the response.
    fn accept(&mut self, frame: Frame, deferred: Vec<Action>, out: &mut Vec<Action>) {
        let frame_id = FrameId(self.next_frame);
        self.next_frame = self.next_frame.wrapping_add(1);
        out.push(Action::PersistFrame {
            frame_id,
            bytes: frame.wire.clone(),
        });
        self.last_accepted = Some(frame.wire);
        self.failures = 0;
        self.respond(ControlResponse::Ack, Some(frame_id), deferred, out);
    }

    /// A response with nothing to store and nothing deferred.
    fn respond_bare(&mut self, byte: ControlResponse, out: &mut Vec<Action>) {
        self.respond(byte, None, Vec::new(), out);
    }

    /// Class B holds the response for the turnaround (and storage); class A has none and
    /// releases the deferred events at once.
    fn respond(
        &mut self,
        byte: ControlResponse,
        persist: Option<FrameId>,
        deferred: Vec<Action>,
        out: &mut Vec<Action>,
    ) {
        if self.class_b() {
            self.start_timer(TimerKind::Turnaround, out);
            self.response = Some(PendingResponse {
                byte,
                persist,
                turnaround_elapsed: false,
                deferred,
            });
        } else {
            out.extend(deferred);
            self.arm_block_timer(out);
        }
    }

    /// Drops the partial sample, remembering its identity so a stray tail is refused.
    fn abort_unit(&mut self, reason: Option<AbortReason>, out: &mut Vec<Action>) {
        let in_progress = self.unit.is_some() || self.assembler.is_pending();
        if let Some(unit) = self.unit.take() {
            self.blocked = Some(unit);
        }
        self.assembler.reset();
        self.last_accepted = None;
        self.completed_multiblock = false;
        if in_progress && let Some(reason) = reason {
            out.push(Action::Aborted(reason));
        }
    }

    fn terminate(&mut self, reason: AbortReason, out: &mut Vec<Action>) {
        self.disarm(out);
        self.abort_unit(None, out);
        self.queue.clear();
        self.response = None;
        self.closed = true;
        out.push(Action::Aborted(reason));
    }

    fn arm_block_timer(&mut self, out: &mut Vec<Action>) {
        if self.assembler.is_pending() {
            self.start_timer(TimerKind::Block, out);
        }
    }

    fn start_timer(&mut self, kind: TimerKind, out: &mut Vec<Action>) {
        let id = TimerId(self.next_timer);
        self.next_timer = self.next_timer.wrapping_add(1);
        self.timer = Some(ActiveTimer { id, kind });
        out.push(Action::StartTimer {
            timer_id: id,
            duration: match kind {
                TimerKind::Turnaround => ACK_TURNAROUND,
                TimerKind::Block => self.options.block_timeout,
                TimerKind::Frame => self.options.frame_timeout,
            },
        });
    }

    fn disarm(&mut self, out: &mut Vec<Action>) {
        if self.timer.take().is_some() {
            out.push(Action::StopTimer);
        }
    }
}
