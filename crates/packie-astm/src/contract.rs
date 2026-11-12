//! Types shared by the receive driver, the send machine and the link driver.

use std::time::Duration;

use thiserror::Error;

use crate::frame::FrameError;

/// Default maximum application text accepted in one transfer (one mebibyte).
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Maximum accepted frames in one transfer, independent of application-text size.
pub const DEFAULT_MAX_FRAMES_PER_TRANSFER: u32 = 8_192;
/// Framing bytes a transfer may add to its text limit.
pub(crate) const WIRE_FRAMING_ALLOWANCE_BYTES: usize = 50 * 1024;
/// Maximum exact frame bytes accepted: the text limit plus 50 KiB of framing.
pub const DEFAULT_MAX_WIRE_BYTES_PER_TRANSFER: usize =
    DEFAULT_MAX_MESSAGE_BYTES + WIRE_FRAMING_ALLOWANCE_BYTES;
/// Receiver inter-frame and persistence timeout requested from the caller.
pub const RECEIVE_TIMEOUT: Duration = Duration::from_secs(30);

/// Opaque identity for one frame persistence request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FrameId(pub(crate) u64);

/// Opaque identity used to ignore expiry events from replaced timers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TimerId(pub(crate) u64);

/// Why a receive transfer ended without exposing a partial message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiveAbortReason {
    /// Six consecutive malformed or out-of-sequence attempts were rejected.
    RetryLimitExceeded,
    /// The current receiver timer expired.
    ReceiveTimeout,
    /// EOT arrived while record text remained unterminated.
    IncompleteRecord,
    /// EOT arrived before any complete record was accepted.
    EmptyTransfer,
    /// Accepted frame text would exceed the configured message limit.
    MessageTooLarge,
    /// Buffered unprocessed wire input exceeded the bounded receive capacity.
    InputBufferExceeded,
    /// A frame contained no application text.
    EmptyFrame,
    /// Accepted frame count would exceed the transfer bound.
    FrameLimitExceeded,
    /// Accepted exact frame bytes would exceed the transfer bound.
    WireBytesExceeded,
}

/// Why an outbound message was abandoned. The message is never partially
/// delivered from the caller's point of view: retry it from the start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendAbortReason {
    /// The peer never accepted the establishment ENQ (NAK, silence or contention
    /// without resolution) within the configured number of attempts.
    EstablishmentFailed,
    /// One frame was rejected more than the configured number of retransmissions.
    RetryLimitExceeded,
    /// The peer did not reply to a frame within the reply timeout.
    ReplyTimeout,
    /// The peer answered a frame with EOT, asking the sender to stop.
    ReceiverInterrupted,
}

/// Work that the I/O and persistence owner must perform, in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Write these exact bytes to the transport (ENQ, ACK, NAK, EOT or a frame).
    Write(Vec<u8>),
    /// Durably store the exact validated frame bytes under this opaque identity.
    PersistFrame {
        /// Identity to return only after the bytes are durably committed.
        frame_id: FrameId,
        /// Exact bytes from STX through CR/LF.
        bytes: Vec<u8>,
    },
    /// Expose a complete inbound transfer as unterminated E1394 record byte strings.
    MessageReceived(Vec<Vec<u8>>),
    /// Discard the partial inbound transfer and surface an operational failure.
    ReceiveAborted(ReceiveAbortReason),
    /// The outbound message was acknowledged frame by frame and EOT was written.
    MessageSent,
    /// The outbound message was abandoned; the line is neutral again.
    SendAborted(SendAbortReason),
    /// Start or replace the single link timer with this identity and duration.
    StartTimer {
        /// Identity returned in the expiry event.
        timer_id: TimerId,
        /// Duration the external scheduler should use.
        duration: Duration,
    },
    /// Stop the active link timer, if any.
    StopTimer,
}

impl Action {
    /// Returns the identity to confirm when this action requests persistence.
    #[must_use]
    pub const fn persistence_id(&self) -> Option<FrameId> {
        match self {
            Self::PersistFrame { frame_id, .. } => Some(*frame_id),
            Self::Write(_)
            | Self::MessageReceived(_)
            | Self::ReceiveAborted(_)
            | Self::MessageSent
            | Self::SendAborted(_)
            | Self::StartTimer { .. }
            | Self::StopTimer => None,
        }
    }
}

/// Inputs from the byte transport, persistence owner, and external scheduler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// One or more bytes arrived; chunk boundaries carry no meaning.
    Bytes(Vec<u8>),
    /// The exact frame associated with this identity is durably committed.
    Persisted(FrameId),
    /// A previously requested timer expired.
    TimerExpired(TimerId),
}

/// Misuse or exhaustion of a driver.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DriverError {
    /// No outstanding persistence request has this identity.
    #[error("ASTM persistence confirmation does not match the pending frame")]
    UnknownFrameId,
    /// The driver's monotonic frame identity space was exhausted.
    #[error("ASTM frame identity space is exhausted")]
    FrameIdExhausted,
    /// The driver's monotonic timer identity space was exhausted.
    #[error("ASTM timer identity space is exhausted")]
    TimerIdExhausted,
    /// A message is already being sent; wait for `MessageSent` or `SendAborted`.
    #[error("ASTM driver is already sending a message")]
    SendInProgress,
    /// The message to send contains no records.
    #[error("ASTM message to send has no records")]
    EmptyMessage,
    /// The message to send cannot be framed.
    #[error(transparent)]
    Encode(#[from] FrameError),
}

/// Which side of a contention the driver plays (LIS01-A2: the instrument has
/// priority to transmit when both systems send ENQ at the same time).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LineRole {
    /// Instrument side: keeps the line, waits [`LinkOptions::contention_retry_delay`]
    /// and sends ENQ again.
    Instrument,
    /// Computer side: yields, prepares to receive and waits up to
    /// [`LinkOptions::contention_yield_timeout`] for the instrument's ENQ.
    #[default]
    Computer,
}

/// Link-level settings. Defaults follow LIS01-A2.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkOptions {
    /// Contention role of this end.
    pub role: LineRole,
    /// Whole-transfer text limit for inbound messages.
    pub max_message_bytes: usize,
    /// How long to wait for ACK/NAK/EOT after ENQ or after a frame (15 s).
    pub reply_timeout: Duration,
    /// Wait after a NAK or a silent peer before sending ENQ again (10 s).
    pub busy_retry_delay: Duration,
    /// Instrument role: wait after contention before sending ENQ again (1 s).
    pub contention_retry_delay: Duration,
    /// Computer role: how long to wait for the instrument's ENQ after yielding (20 s).
    pub contention_yield_timeout: Duration,
    /// Maximum ENQ transmissions for one message before giving up (3).
    pub max_enq_attempts: u8,
    /// Maximum retransmissions of one frame after the first transmission (6).
    pub max_frame_retransmissions: u8,
    /// Receiver: how long to wait for the next frame or EOT before the transfer aborts
    /// ([`RECEIVE_TIMEOUT`]).
    pub receive_timeout: Duration,
    /// Most text bytes in one frame, 1 through [`crate::MAX_LONG_FRAME_TEXT`] (240). The
    /// sender splits longer records over ETB frames; the receiver NAKs frames over it.
    /// The peer must be configured for the same frame size.
    pub max_frame_text: usize,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self {
            role: LineRole::default(),
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            reply_timeout: Duration::from_secs(15),
            busy_retry_delay: Duration::from_secs(10),
            contention_retry_delay: Duration::from_secs(1),
            contention_yield_timeout: Duration::from_secs(20),
            max_enq_attempts: 3,
            max_frame_retransmissions: 6,
            receive_timeout: RECEIVE_TIMEOUT,
            max_frame_text: crate::frame::MAX_TEXT_BYTES,
        }
    }
}
