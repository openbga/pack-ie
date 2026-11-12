//! ASTM and AU source sessions: one loop feeds bytes and timer expiries to the protocol's
//! receive driver, commits every frame before the driver acknowledges it, and completes
//! or aborts the transfer (`docs/spec/message-channels.md` §2.2, §3, §7). The protocol
//! adapters (`FrameDriver` implementations) live in `message::astm` and `message::au`.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::time::Duration;

use packie_config::ProtocolSettings;
use packie_core::{MessageId, MessageState, ProtocolKind};
use packie_transport::Operation;
use tokio::time::Instant;

use super::{End, READ_CHUNK, Session, log_committed, log_duplicate};
use crate::message::store::{Identifiers, StoreError};
use crate::message::{astm, drain_deadline};
use crate::status::MessageCounters;
use crate::stop::{StopRx, sleep_until, stopped};

/// A receive driver behind a source session, seen as [`Step`]s.
pub(crate) trait FrameDriver {
    /// The driver's input.
    type Event;
    /// The driver's timer handle.
    type Timer: Copy;
    /// The protocol of the messages it receives.
    const PROTOCOL: ProtocolKind;
    /// Why the session ends when the driver refuses an event.
    const FAILED: &'static str;
    /// Whether the committed frames' bytes are the message (AU) rather than the records
    /// the driver hands over when it completes (ASTM).
    const FRAMES_ARE_MESSAGE: bool;

    /// Bytes read from the peer.
    fn bytes(bytes: Vec<u8>) -> Self::Event;
    /// A timer the driver started expired.
    fn expired(timer: Self::Timer) -> Self::Event;
    /// Handles `event`; `None` when the session cannot continue.
    fn steps(&mut self, event: Self::Event) -> Option<Vec<Step<Self::Event, Self::Timer>>>;
}

/// What the session does for one driver action.
pub(crate) enum Step<E, T> {
    /// Write these bytes to the peer.
    Write(Vec<u8>),
    /// Commit the frame, then feed `persisted` back so the driver acknowledges it; a
    /// failed commit feeds nothing and the sender's timer expires.
    Persist { bytes: Vec<u8>, persisted: E },
    /// The transfer is complete.
    Complete(Completion),
    /// The transfer was abandoned with this `abort_reason`.
    Abort(&'static str),
    /// Start the driver's timer.
    StartTimer(T, Duration),
    /// Stop the driver's timer.
    StopTimer,
    /// Nothing to do.
    Nothing,
}

/// How a transfer completed.
pub(crate) enum Completion {
    /// ASTM: the message's records.
    Records(Vec<Vec<u8>>),
    /// AU: the committed frames, routed unless a `verdict` keeps them.
    Frames {
        identifiers: Identifiers,
        verdict: Option<Verdict>,
    },
}

/// Why a completed unit is kept without routing, with its reason.
pub(crate) enum Verdict {
    /// Recorded `filtered` (an instrument request).
    Filtered(&'static str),
    /// Recorded `invalid`.
    Invalid(&'static str),
}

/// The transfer in progress and the driver's timer.
struct Transfer<T> {
    message: Option<MessageId>,
    /// The committed frames' bytes, when they form the message.
    frames: Vec<u8>,
    timer: Option<(T, Instant)>,
}

impl<T> Default for Transfer<T> {
    fn default() -> Self {
        Self {
            message: None,
            frames: Vec::new(),
            timer: None,
        }
    }
}

impl Session<'_> {
    /// Runs `driver` until the connection ends or the channel stops; a stop lets a
    /// transfer in progress finish within the drain budget.
    pub(super) async fn framed<D: FrameDriver>(mut self, mut driver: D, stop: &mut StopRx) -> End {
        let mut transfer = Transfer::default();
        let mut draining: Option<Instant> = None;
        let mut buf = vec![0_u8; READ_CHUNK];
        let end = loop {
            if draining.is_some() && transfer.message.is_none() {
                break End::Stopped;
            }
            // A running timer means the session waits on the peer within a transfer.
            self.waiting_for_rest(transfer.timer.is_some());
            let event = match self
                .next_event::<D>(&mut transfer, &mut draining, &mut buf, stop)
                .await
            {
                Ok(Some(event)) => event,
                Ok(None) => continue,
                Err(end) => break end,
            };
            if let Err(end) = self.feed(&mut driver, &mut transfer, event).await {
                break end;
            }
        };
        let reason = if matches!(end, End::Stopped) {
            "channel_stopped"
        } else {
            "connection_lost"
        };
        self.abort(&mut transfer, reason).await;
        end
    }

    /// The next driver event: bytes, a timer, or `None` after a drain began.
    async fn next_event<D: FrameDriver>(
        &mut self,
        transfer: &mut Transfer<D::Timer>,
        draining: &mut Option<Instant>,
        buf: &mut [u8],
        stop: &mut StopRx,
    ) -> Result<Option<D::Event>, End> {
        let timer = transfer.timer;
        tokio::select! {
            biased;
            mode = stopped(stop), if draining.is_none() => {
                // A transfer in progress may finish within the drain budget.
                *draining = Some(drain_deadline(mode).ok_or(End::Stopped)?);
                Ok(None)
            }
            () = sleep_until(*draining), if draining.is_some() => Err(End::Stopped),
            () = sleep_until(timer.map(|(_, at)| at)), if timer.is_some() => {
                transfer.timer = None;
                Ok(timer.map(|(id, _)| D::expired(id)))
            }
            read = self.read(buf) => {
                let n = read?;
                Ok(Some(D::bytes(buf.get(..n).unwrap_or_default().to_vec())))
            }
        }
    }

    /// Feeds `event` and carries out the driver's steps, including the persistence
    /// confirmations they lead to.
    async fn feed<D: FrameDriver>(
        &mut self,
        driver: &mut D,
        transfer: &mut Transfer<D::Timer>,
        event: D::Event,
    ) -> Result<(), End> {
        let mut events = VecDeque::from([event]);
        while let Some(event) = events.pop_front() {
            let Some(steps) = driver.steps(event) else {
                return Err(End::Closed {
                    operation: Operation::Read,
                    cause: D::FAILED.into(),
                });
            };
            for step in steps {
                events.extend(self.step::<D>(transfer, step).await?);
            }
        }
        Ok(())
    }

    async fn step<D: FrameDriver>(
        &mut self,
        transfer: &mut Transfer<D::Timer>,
        step: Step<D::Event, D::Timer>,
    ) -> Result<Option<D::Event>, End> {
        match step {
            Step::Write(bytes) => self.write(&bytes).await.map(|()| None),
            Step::Persist { bytes, persisted } => {
                let committed = self.persist_frame::<D>(transfer, bytes).await;
                self.store_health(&committed);
                Ok(committed.ok().map(|()| persisted))
            }
            Step::Complete(completion) => {
                self.complete(transfer, completion).await;
                Ok(None)
            }
            Step::Abort(reason) => {
                self.abort(transfer, reason).await;
                Ok(None)
            }
            Step::StartTimer(timer, duration) => {
                transfer.timer = Some((timer, Instant::now() + duration));
                Ok(None)
            }
            Step::StopTimer => {
                transfer.timer = None;
                Ok(None)
            }
            Step::Nothing => Ok(None),
        }
    }

    fn set_receiving(&self, receiving: bool) {
        if let Some(counters) = self.counters() {
            counters.receiving.store(receiving, Ordering::Relaxed);
        }
    }

    /// Commits a frame: the first one of a transfer creates the `receiving` message.
    async fn persist_frame<D: FrameDriver>(
        &mut self,
        transfer: &mut Transfer<D::Timer>,
        bytes: Vec<u8>,
    ) -> Result<(), StoreError> {
        if D::FRAMES_ARE_MESSAGE {
            transfer.frames.extend_from_slice(&bytes);
        }
        if let Some(message) = transfer.message {
            return self.cx.store.append_frame(message, bytes).await;
        }
        let receipt = self.receipt(D::PROTOCOL, Identifiers::default());
        let message = self.cx.store.begin_transfer(receipt, bytes).await?;
        transfer.message = Some(message);
        self.set_receiving(true);
        Ok(())
    }

    /// Completes the transfer; a unit without a verdict is routed.
    async fn complete<T>(&mut self, transfer: &mut Transfer<T>, completion: Completion) {
        let Some(message) = transfer.message.take() else {
            return;
        };
        self.set_receiving(false);
        let frames = std::mem::take(&mut transfer.frames);
        let (original, identifiers, verdict) = match completion {
            Completion::Records(records) => {
                let original = astm::original_bytes(&records);
                let sample = match &self.cx.channel.source.protocol {
                    ProtocolSettings::Astm(settings) => astm::sample_position(settings),
                    _ => astm::STANDARD_SAMPLE,
                };
                let identifiers = astm::identifiers(&original, sample).unwrap_or_default();
                (original, identifiers, None)
            }
            Completion::Frames {
                identifiers,
                verdict,
            } => (frames, identifiers, verdict),
        };
        let completed = self
            .cx
            .store
            .complete_transfer(
                message,
                original.clone(),
                identifiers,
                self.cx.channel.source.duplicate_detection,
            )
            .await;
        self.store_health(&completed);
        let Ok(completed) = completed else {
            return;
        };
        self.mark_stable();
        // Every frame of the transfer was acknowledged after its commit.
        log_committed(&completed, original.len(), "ACK");
        if let Some(counters) = self.counters() {
            self.cx.shared.message_received(original.len());
            if completed.state == MessageState::Duplicate {
                log_duplicate(counters, completed.message_id);
                return;
            }
        }
        let store = &self.cx.store;
        let id = completed.message_id;
        match verdict {
            Some(Verdict::Filtered(reason)) => {
                let _ = store.record_filtered(id, reason.to_owned()).await;
            }
            Some(Verdict::Invalid(reason)) => {
                let _ = store.record_invalid(id, reason.to_owned()).await;
            }
            None => {
                self.cx.route(id, &original, &[]).await;
            }
        }
    }

    /// Abandons the transfer in progress, if any, with `reason`.
    async fn abort<T>(&mut self, transfer: &mut Transfer<T>, reason: &'static str) {
        let Some(message) = transfer.message.take() else {
            return;
        };
        transfer.frames.clear();
        self.set_receiving(false);
        let _ = self.cx.store.abort_transfer(message, reason).await;
        if let Some(counters) = self.counters() {
            MessageCounters::bump(&counters.aborted);
        }
        tracing::warn!(
            event = "message.aborted",
            message_id = %message,
            error_code = reason,
            "Channel {}: transfer aborted ({reason})",
            self.cx.shared.id
        );
    }
}
