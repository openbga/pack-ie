use crate::contract::{
    Action, DEFAULT_MAX_FRAMES_PER_TRANSFER, DEFAULT_MAX_MESSAGE_BYTES, DriverError, Event,
    FrameId, RECEIVE_TIMEOUT, ReceiveAbortReason, WIRE_FRAMING_ALLOWANCE_BYTES,
};
use crate::frame::{
    ACK, CR, ENQ, EOT, FRAME_OVERHEAD_BYTES, Frame, FrameEnd, LF, MAX_TEXT_BYTES, NAK, STX,
    clamp_text_limit, parse_frame_within,
};
use crate::timer::TimerSlot;
use std::time::Duration;

const MAX_FAILED_ATTEMPTS: u8 = 6;

/// A validated frame waiting for its durable-storage confirmation, with the
/// transfer totals that become current once it is confirmed.
#[derive(Clone, Debug)]
struct PendingFrame {
    id: FrameId,
    frame: Frame,
    accepted_text_bytes: usize,
    accepted_frames: u32,
    accepted_wire_bytes: usize,
}

/// State of the transfer in progress; a fresh default starts every transfer.
#[derive(Debug)]
struct Transfer {
    expected_number: u8,
    failed_attempts: u8,
    current_record: Vec<u8>,
    record_continues: bool,
    records: Vec<Vec<u8>>,
    accepted_text_bytes: usize,
    accepted_frames: u32,
    accepted_wire_bytes: usize,
    pending: Option<PendingFrame>,
    last_accepted: Option<Frame>,
}

impl Default for Transfer {
    fn default() -> Self {
        Self {
            expected_number: 1,
            failed_attempts: 0,
            current_record: Vec::new(),
            record_continues: false,
            records: Vec::new(),
            accepted_text_bytes: 0,
            accepted_frames: 0,
            accepted_wire_bytes: 0,
            pending: None,
            last_accepted: None,
        }
    }
}

/// Pure receive-side low-level driver with durable frame acknowledgement gating.
#[derive(Debug)]
pub struct ReceiveDriver {
    receiving: bool,
    transfer: Transfer,
    wire_buffer: Vec<u8>,
    max_message_bytes: usize,
    max_frame_text: usize,
    max_frames: u32,
    max_wire_bytes: usize,
    next_frame_id: u64,
    timer: TimerSlot,
    receive_timeout: Duration,
}

impl Default for ReceiveDriver {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MESSAGE_BYTES)
    }
}

impl ReceiveDriver {
    /// Constructs an idle receiver with an explicit whole-transfer text limit. Frames may
    /// hold 240 text bytes (see [`Self::with_max_frame_text`]); the transfer's exact frame
    /// bytes may exceed the text limit by the framing allowance.
    #[must_use]
    pub fn new(max_message_bytes: usize) -> Self {
        Self {
            receiving: false,
            transfer: Transfer::default(),
            wire_buffer: Vec::new(),
            max_message_bytes,
            max_frame_text: MAX_TEXT_BYTES,
            max_frames: DEFAULT_MAX_FRAMES_PER_TRANSFER,
            max_wire_bytes: max_message_bytes.saturating_add(WIRE_FRAMING_ALLOWANCE_BYTES),
            next_frame_id: 1,
            timer: TimerSlot::odd(),
            receive_timeout: RECEIVE_TIMEOUT,
        }
    }

    /// The same receiver waiting `timeout` (instead of [`RECEIVE_TIMEOUT`]) for the next
    /// frame or EOT before it aborts the transfer.
    #[must_use]
    pub const fn with_receive_timeout(mut self, timeout: Duration) -> Self {
        self.receive_timeout = timeout;
        self
    }

    /// The same receiver accepting up to `max_text` text bytes per frame (default 240,
    /// clamped to 1 through [`crate::MAX_LONG_FRAME_TEXT`]). Frames over the limit are NAKed.
    ///
    /// The bounds on buffered input and on a transfer's exact frame bytes always leave
    /// room for one frame of this size, so a legal frame is never refused for its size
    /// alone.
    #[must_use]
    pub const fn with_max_frame_text(mut self, max_text: usize) -> Self {
        self.max_frame_text = clamp_text_limit(max_text);
        let one_frame = self.wire_frame_limit();
        if self.max_wire_bytes < one_frame {
            self.max_wire_bytes = one_frame;
        }
        self
    }

    /// Reports whether a transfer is in progress (ENQ accepted, EOT or abort not yet seen).
    #[must_use]
    pub const fn is_receiving(&self) -> bool {
        self.receiving
    }

    /// Applies one event and returns ordered external actions.
    pub fn handle(&mut self, event: Event) -> Result<Vec<Action>, DriverError> {
        let mut actions = Vec::new();
        match event {
            Event::Bytes(bytes) => {
                if self.buffer_bytes(&bytes, &mut actions) {
                    self.process_wire(&mut actions)?;
                }
            }
            Event::Persisted(frame_id) => {
                self.handle_persisted(frame_id, &mut actions)?;
                self.process_wire(&mut actions)?;
            }
            Event::TimerExpired(timer_id) => {
                if self.receiving && self.timer.is_active(timer_id) {
                    self.abort(ReceiveAbortReason::ReceiveTimeout, &mut actions);
                }
            }
        }
        Ok(actions)
    }

    /// Most wire bytes one frame can occupy: its text plus the framing bytes.
    const fn wire_frame_limit(&self) -> usize {
        self.max_frame_text + FRAME_OVERHEAD_BYTES
    }

    fn buffer_bytes(&mut self, bytes: &[u8], actions: &mut Vec<Action>) -> bool {
        let buffered_limit = self
            .max_message_bytes
            .saturating_mul(2)
            .max(self.wire_frame_limit());
        let within_limit = self
            .wire_buffer
            .len()
            .checked_add(bytes.len())
            .is_some_and(|size| size <= buffered_limit);
        if !within_limit {
            self.abort(ReceiveAbortReason::InputBufferExceeded, actions);
            return false;
        }
        self.wire_buffer.extend_from_slice(bytes);
        true
    }

    fn process_wire(&mut self, actions: &mut Vec<Action>) -> Result<(), DriverError> {
        loop {
            if self.transfer.pending.is_some() || self.wire_buffer.is_empty() {
                return Ok(());
            }
            if !self.receiving {
                self.process_idle_bytes(actions)?;
                continue;
            }
            if self.wire_buffer[0] == EOT {
                // Whatever follows the EOT (usually the peer's next ENQ) is the start of
                // the next exchange; ending the transfer must not discard it.
                let following = self.wire_buffer.split_off(1);
                self.finish_transfer(actions);
                self.wire_buffer = following;
                continue;
            }
            if self.wire_buffer[0] != STX {
                self.wire_buffer.clear();
                return self.reject_attempt(actions);
            }
            let Some(frame_length) = complete_frame_length(&self.wire_buffer) else {
                return self.reject_oversize_incomplete_frame(actions);
            };
            let raw: Vec<u8> = self.wire_buffer.drain(..frame_length).collect();
            self.process_frame(&raw, actions)?;
        }
    }

    fn reject_oversize_incomplete_frame(
        &mut self,
        actions: &mut Vec<Action>,
    ) -> Result<(), DriverError> {
        if self.wire_buffer.len() <= self.wire_frame_limit() {
            return Ok(());
        }
        self.wire_buffer.clear();
        self.reject_attempt(actions)
    }

    /// Idle line: everything up to and including the first ENQ is discarded and
    /// the ENQ starts a transfer.
    fn process_idle_bytes(&mut self, actions: &mut Vec<Action>) -> Result<(), DriverError> {
        let Some(enq_index) = self.wire_buffer.iter().position(|byte| *byte == ENQ) else {
            self.wire_buffer.clear();
            return Ok(());
        };
        self.wire_buffer.drain(..=enq_index);
        self.receiving = true;
        self.transfer = Transfer::default();
        actions.push(Action::Write(vec![ACK]));
        self.restart_timer(actions)
    }

    fn process_frame(&mut self, raw: &[u8], actions: &mut Vec<Action>) -> Result<(), DriverError> {
        let Ok(frame) = parse_frame_within(raw, self.max_frame_text) else {
            return self.reject_attempt(actions);
        };
        let transfer = &mut self.transfer;

        if transfer.last_accepted.as_ref() == Some(&frame) {
            transfer.failed_attempts = 0;
            actions.push(Action::Write(vec![ACK]));
            return self.restart_timer(actions);
        }
        if frame.number() != transfer.expected_number {
            return self.reject_attempt(actions);
        }
        if !transfer.record_continues && !transfer.current_record.is_empty() {
            self.abort(ReceiveAbortReason::IncompleteRecord, actions);
            return Ok(());
        }
        if frame.text().is_empty() {
            self.abort(ReceiveAbortReason::EmptyFrame, actions);
            return Ok(());
        }
        let new_frame_count = transfer
            .accepted_frames
            .checked_add(1)
            .filter(|count| *count <= self.max_frames);
        let new_wire_bytes = transfer
            .accepted_wire_bytes
            .checked_add(frame.raw().len())
            .filter(|bytes| *bytes <= self.max_wire_bytes);
        let new_text_bytes = transfer
            .accepted_text_bytes
            .checked_add(frame.text().len())
            .filter(|bytes| *bytes <= self.max_message_bytes);
        let (Some(accepted_frames), Some(accepted_wire_bytes), Some(accepted_text_bytes)) =
            (new_frame_count, new_wire_bytes, new_text_bytes)
        else {
            let reason = if new_frame_count.is_none() {
                ReceiveAbortReason::FrameLimitExceeded
            } else if new_wire_bytes.is_none() {
                ReceiveAbortReason::WireBytesExceeded
            } else {
                ReceiveAbortReason::MessageTooLarge
            };
            self.abort(reason, actions);
            return Ok(());
        };

        let id = self.take_frame_id()?;
        actions.push(Action::PersistFrame {
            frame_id: id,
            bytes: frame.raw().to_vec(),
        });
        self.transfer.pending = Some(PendingFrame {
            id,
            frame,
            accepted_text_bytes,
            accepted_frames,
            accepted_wire_bytes,
        });
        self.restart_timer(actions)
    }

    fn handle_persisted(
        &mut self,
        frame_id: FrameId,
        actions: &mut Vec<Action>,
    ) -> Result<(), DriverError> {
        let transfer = &mut self.transfer;
        let Some(pending) = transfer.pending.take_if(|pending| pending.id == frame_id) else {
            return Err(DriverError::UnknownFrameId);
        };

        transfer.accepted_text_bytes = pending.accepted_text_bytes;
        transfer.accepted_frames = pending.accepted_frames;
        transfer.accepted_wire_bytes = pending.accepted_wire_bytes;
        transfer
            .current_record
            .extend_from_slice(pending.frame.text());
        transfer.record_continues = pending.frame.end() == FrameEnd::Intermediate;
        transfer.extract_complete_records();
        transfer.expected_number = (pending.frame.number() + 1) % 8;
        transfer.failed_attempts = 0;
        while self.wire_buffer.starts_with(pending.frame.raw()) {
            self.wire_buffer.drain(..pending.frame.raw().len());
        }
        transfer.last_accepted = Some(pending.frame);
        actions.push(Action::Write(vec![ACK]));
        self.restart_timer(actions)
    }

    fn finish_transfer(&mut self, actions: &mut Vec<Action>) {
        let transfer = &mut self.transfer;
        if transfer.record_continues || !transfer.current_record.is_empty() {
            self.abort(ReceiveAbortReason::IncompleteRecord, actions);
            return;
        }
        if transfer.records.is_empty() {
            self.abort(ReceiveAbortReason::EmptyTransfer, actions);
            return;
        }

        let records = std::mem::take(&mut transfer.records);
        self.reset_transfer();
        actions.push(Action::StopTimer);
        actions.push(Action::MessageReceived(records));
    }

    fn reject_attempt(&mut self, actions: &mut Vec<Action>) -> Result<(), DriverError> {
        self.transfer.failed_attempts = self.transfer.failed_attempts.saturating_add(1);
        actions.push(Action::Write(vec![NAK]));
        if self.transfer.failed_attempts >= MAX_FAILED_ATTEMPTS {
            self.abort(ReceiveAbortReason::RetryLimitExceeded, actions);
            return Ok(());
        }
        self.restart_timer(actions)
    }

    fn abort(&mut self, reason: ReceiveAbortReason, actions: &mut Vec<Action>) {
        self.reset_transfer();
        actions.push(Action::StopTimer);
        actions.push(Action::ReceiveAborted(reason));
    }

    fn reset_transfer(&mut self) {
        self.receiving = false;
        self.transfer = Transfer::default();
        self.wire_buffer.clear();
        self.timer.forget();
    }

    fn restart_timer(&mut self, actions: &mut Vec<Action>) -> Result<(), DriverError> {
        self.timer.start(self.receive_timeout, actions)
    }

    fn take_frame_id(&mut self) -> Result<FrameId, DriverError> {
        let frame_id = FrameId(self.next_frame_id);
        self.next_frame_id = self
            .next_frame_id
            .checked_add(1)
            .ok_or(DriverError::FrameIdExhausted)?;
        Ok(frame_id)
    }
}

impl Transfer {
    /// Moves every CR-terminated record out of the record text buffer.
    fn extract_complete_records(&mut self) {
        while let Some(end) = self.current_record.iter().position(|byte| *byte == CR) {
            let mut remainder = self.current_record.split_off(end + 1);
            let _ = self.current_record.pop();
            self.records.push(std::mem::take(&mut self.current_record));
            self.current_record.append(&mut remainder);
        }
    }
}

fn complete_frame_length(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(2)
        .position(|pair| pair == [CR, LF])
        .map(|index| index + 2)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::contract::TimerId;
    use crate::frame::{encode_frame, encode_records};
    use crate::testing::{chunked, receive_chunks, transfer_bytes};

    fn begin(driver: &mut ReceiveDriver) {
        driver
            .handle(Event::Bytes(vec![ENQ]))
            .expect("ENQ is handled");
    }

    fn final_frame(number: u8, text: &[u8]) -> Vec<u8> {
        encode_frame(number, text, FrameEnd::Final).expect("frame encodes")
    }

    fn feed(driver: &mut ReceiveDriver, bytes: Vec<u8>) -> Vec<Action> {
        driver
            .handle(Event::Bytes(bytes))
            .expect("bytes are handled")
    }

    fn persistence_id(actions: &[Action]) -> FrameId {
        actions
            .iter()
            .find_map(Action::persistence_id)
            .expect("persistence action exists")
    }

    /// Confirms the frame persisted by `actions` and returns the resulting actions.
    fn confirm(driver: &mut ReceiveDriver, actions: &[Action]) -> Vec<Action> {
        driver
            .handle(Event::Persisted(persistence_id(actions)))
            .expect("persistence is handled")
    }

    fn feed_and_confirm(driver: &mut ReceiveDriver, bytes: Vec<u8>) -> Vec<Action> {
        let actions = feed(driver, bytes);
        confirm(driver, &actions)
    }

    fn acks(actions: &[Action]) -> usize {
        actions
            .iter()
            .filter(|action| **action == Action::Write(vec![ACK]))
            .count()
    }

    fn persists(actions: &[Action]) -> bool {
        actions
            .iter()
            .any(|action| matches!(action, Action::PersistFrame { .. }))
    }

    /// The transfer aborted with `reason` before persisting or acknowledging anything.
    fn assert_aborted_unacknowledged(actions: &[Action], reason: ReceiveAbortReason) {
        assert!(actions.contains(&Action::ReceiveAborted(reason)));
        assert!(!persists(actions));
        assert_eq!(acks(actions), 0);
    }

    fn persist_first_frame(driver: &mut ReceiveDriver, first: Vec<u8>) {
        begin(driver);
        feed_and_confirm(driver, first);
    }

    fn assert_next_frame_aborts(driver: &mut ReceiveDriver, reason: ReceiveAbortReason) {
        let actions = feed(driver, final_frame(2, b"L|1|N\r"));
        assert!(actions.contains(&Action::ReceiveAborted(reason)));
        assert!(!persists(&actions));
    }

    fn receive_persist_then_eot(text: &[u8]) -> Vec<Action> {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let frame = encode_frame(1, text, FrameEnd::Intermediate).expect("frame encodes");
        feed_and_confirm(&mut driver, frame);
        feed(&mut driver, vec![EOT])
    }

    fn assert_incomplete_without_message(actions: &[Action]) {
        assert!(actions.contains(&Action::ReceiveAborted(
            ReceiveAbortReason::IncompleteRecord
        )));
        assert!(
            actions
                .iter()
                .all(|action| !matches!(action, Action::MessageReceived(_)))
        );
    }

    #[test]
    fn ack_waits_for_matching_persistence() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let received = feed(&mut driver, final_frame(1, b"H|\\^&\r"));
        assert_eq!(acks(&received), 0);
        assert_eq!(acks(&confirm(&mut driver, &received)), 1);
    }

    #[test]
    fn wrong_persistence_id_cannot_release_ack() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let received = feed(&mut driver, final_frame(1, b"H|\\^&\r"));
        let id = persistence_id(&received);
        let wrong = FrameId(id.0 + 1);
        assert_eq!(
            driver.handle(Event::Persisted(wrong)),
            Err(DriverError::UnknownFrameId)
        );
        assert_eq!(acks(&confirm(&mut driver, &received)), 1);
    }

    #[test]
    fn duplicate_after_lost_ack_is_not_persisted_twice() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let frame = final_frame(1, b"H|\\^&\r");
        feed_and_confirm(&mut driver, frame.clone());

        let duplicate = feed(&mut driver, frame);
        assert_eq!(acks(&duplicate), 1);
        assert!(!persists(&duplicate));
    }

    #[test]
    fn buffered_duplicate_is_coalesced_before_sender_advances() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let frame = final_frame(1, b"H|\\^&\r");
        let first = feed(&mut driver, frame.clone());
        assert!(feed(&mut driver, frame).is_empty());

        let after_persist = confirm(&mut driver, &first);
        assert_eq!(acks(&after_persist), 1);
        assert!(!persists(&after_persist));

        let second = feed(&mut driver, final_frame(2, b"L|1|N\r"));
        assert_eq!(acks(&second), 0);
        assert_eq!(acks(&confirm(&mut driver, &second)), 1);
    }

    #[test]
    fn eot_with_incomplete_record_aborts_without_message() {
        assert_incomplete_without_message(&receive_persist_then_eot(b"H|\\^&"));
    }

    #[test]
    fn etb_with_record_terminator_cannot_complete_on_eot() {
        assert_incomplete_without_message(&receive_persist_then_eot(b"H|\\^&\r"));
    }

    #[test]
    fn timeout_while_persistence_is_pending_aborts_without_frame_ack() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let received = feed(&mut driver, final_frame(1, b"H|\\^&\r"));
        let timer_id = received
            .iter()
            .find_map(|action| {
                if let Action::StartTimer { timer_id, .. } = action {
                    Some(*timer_id)
                } else {
                    None
                }
            })
            .expect("persistence timer exists");
        let timed_out = driver
            .handle(Event::TimerExpired(timer_id))
            .expect("timeout is handled");
        assert_aborted_unacknowledged(&timed_out, ReceiveAbortReason::ReceiveTimeout);
    }

    #[test]
    fn message_limit_aborts_before_persistence_or_ack() {
        let mut driver = ReceiveDriver::new(3);
        begin(&mut driver);
        let actions = feed(&mut driver, final_frame(1, b"H|X\r"));
        assert_aborted_unacknowledged(&actions, ReceiveAbortReason::MessageTooLarge);
    }

    #[test]
    fn empty_frame_aborts_before_persistence_or_ack() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let actions = feed(&mut driver, final_frame(1, b""));
        assert_aborted_unacknowledged(&actions, ReceiveAbortReason::EmptyFrame);
    }

    #[test]
    fn frame_count_limit_aborts_before_excess_frame_persistence() {
        let mut driver = ReceiveDriver {
            max_frames: 1,
            ..ReceiveDriver::default()
        };
        persist_first_frame(&mut driver, final_frame(1, b"H|x\r"));
        assert_next_frame_aborts(&mut driver, ReceiveAbortReason::FrameLimitExceeded);
    }

    #[test]
    fn wire_byte_limit_aborts_before_excess_frame_persistence() {
        let first = final_frame(1, b"H|x\r");
        let mut driver = ReceiveDriver {
            max_wire_bytes: first.len(),
            ..ReceiveDriver::default()
        };
        persist_first_frame(&mut driver, first);
        assert_next_frame_aborts(&mut driver, ReceiveAbortReason::WireBytesExceeded);
    }

    #[test]
    fn buffered_wire_input_is_bounded_while_persistence_is_pending() {
        let mut driver = ReceiveDriver::new(3);
        begin(&mut driver);
        assert!(persists(&feed(&mut driver, final_frame(1, b"H\r"))));

        let overflow = feed(
            &mut driver,
            vec![b'A'; MAX_TEXT_BYTES + FRAME_OVERHEAD_BYTES + 1],
        );
        assert!(overflow.contains(&Action::ReceiveAborted(
            ReceiveAbortReason::InputBufferExceeded
        )));
        assert_eq!(acks(&overflow), 0);
    }

    #[test]
    fn sixth_bad_frame_aborts_after_nak() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let mut bad = final_frame(1, b"H|\\^&\r");
        let checksum_index = bad.len() - 4;
        bad[checksum_index] = if bad[checksum_index] == b'0' {
            b'1'
        } else {
            b'0'
        };

        for _ in 0..5 {
            let actions = feed(&mut driver, bad.clone());
            assert!(actions.contains(&Action::Write(vec![NAK])));
            assert!(
                actions
                    .iter()
                    .all(|action| !matches!(action, Action::ReceiveAborted(_)))
            );
        }
        let sixth = feed(&mut driver, bad);
        assert!(sixth.contains(&Action::Write(vec![NAK])));
        assert!(sixth.contains(&Action::ReceiveAborted(
            ReceiveAbortReason::RetryLimitExceeded
        )));
    }

    #[test]
    fn noise_before_enq_is_discarded_in_one_pass() {
        let mut driver = ReceiveDriver::default();
        let actions = feed(&mut driver, vec![0x00, 0x41, ENQ, 0x7f]);
        assert_eq!(acks(&actions), 1);
        assert!(driver.is_receiving());
        let more_noise = feed(&mut ReceiveDriver::default(), vec![0x41; 8]);
        assert!(more_noise.is_empty());
    }

    #[test]
    fn complete_transfer_returns_original_records() {
        let records = vec![b"H|\\^&|||SYNTH-ANALYZER^1.0".to_vec(), b"L|1|N".to_vec()];
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        for frame in encode_records(&records).expect("records encode") {
            feed_and_confirm(&mut driver, frame);
        }
        assert!(feed(&mut driver, vec![EOT]).contains(&Action::MessageReceived(records)));
    }

    #[test]
    fn packed_records_in_one_frame_are_emitted_individually() {
        let records = vec![b"H|\\^&|||SYNTHETIC".to_vec(), b"L|1|N".to_vec()];
        let packed: Vec<u8> = records
            .iter()
            .flat_map(|record| [record.as_slice(), &[CR]].concat())
            .collect();
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        feed_and_confirm(&mut driver, final_frame(1, &packed));
        assert!(feed(&mut driver, vec![EOT]).contains(&Action::MessageReceived(records)));
    }

    fn drive_chunks(chunks: &[&[u8]]) -> (Vec<Vec<Vec<u8>>>, usize) {
        let mut driver = ReceiveDriver::default();
        receive_chunks(chunks, |event| {
            driver.handle(event).expect("event is handled")
        })
    }

    #[test]
    fn enq_in_the_same_read_as_the_previous_eot_is_acknowledged() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        feed_and_confirm(&mut driver, final_frame(1, b"L|1|N\r"));
        let actions = feed(&mut driver, vec![EOT, ENQ]);
        assert!(actions.contains(&Action::MessageReceived(vec![b"L|1|N".to_vec()])));
        assert_eq!(acks(&actions), 1);
        assert!(driver.is_receiving());
        // The second transfer completes normally.
        feed_and_confirm(&mut driver, final_frame(1, b"L|1|N\r"));
        assert!(
            feed(&mut driver, vec![EOT])
                .iter()
                .any(|action| matches!(action, Action::MessageReceived(_)))
        );
    }

    #[test]
    fn eot_with_an_incomplete_record_still_keeps_the_following_enq() {
        let mut driver = ReceiveDriver::default();
        begin(&mut driver);
        let frame = encode_frame(1, b"H|partial", FrameEnd::Intermediate).expect("frame encodes");
        feed_and_confirm(&mut driver, frame);
        let actions = feed(&mut driver, vec![EOT, ENQ]);
        assert_incomplete_without_message(&actions);
        assert_eq!(acks(&actions), 1);
        assert!(driver.is_receiving());
    }

    #[test]
    fn two_transfers_survive_a_split_at_every_byte() {
        let stream = [transfer_bytes(b"H|one"), transfer_bytes(b"H|two")].concat();
        for split in 0..=stream.len() {
            let (messages, ack_count) = drive_chunks(&[&stream[..split], &stream[split..]]);
            assert_eq!(
                messages,
                vec![vec![b"H|one".to_vec()], vec![b"H|two".to_vec()]],
                "split at {split}"
            );
            // Two ENQs and two frames.
            assert_eq!(ack_count, 4, "split at {split}");
        }
    }

    fn long_driver(max_text: usize) -> ReceiveDriver {
        ReceiveDriver::default().with_max_frame_text(max_text)
    }

    fn naks(actions: &[Action]) -> usize {
        actions
            .iter()
            .filter(|action| **action == Action::Write(vec![NAK]))
            .count()
    }

    #[test]
    fn frame_over_the_default_text_limit_is_naked_until_configured() {
        let frame = final_frame(1, &[b'A'; 241]);

        let mut default = ReceiveDriver::default();
        begin(&mut default);
        let actions = feed(&mut default, frame.clone());
        assert_eq!(naks(&actions), 1);
        assert!(!persists(&actions));

        let mut configured = long_driver(241);
        begin(&mut configured);
        let actions = feed(&mut configured, frame);
        assert!(persists(&actions));
        assert_eq!(naks(&actions), 0);
    }

    #[test]
    fn frame_over_the_configured_limit_follows_the_six_nak_policy() {
        let frame = final_frame(1, &[b'A'; 301]);
        let mut driver = long_driver(300);
        begin(&mut driver);
        for _ in 0..5 {
            let actions = feed(&mut driver, frame.clone());
            assert_eq!(naks(&actions), 1);
            assert!(!persists(&actions));
            assert!(driver.is_receiving());
        }
        let sixth = feed(&mut driver, frame);
        assert_eq!(naks(&sixth), 1);
        assert!(sixth.contains(&Action::ReceiveAborted(
            ReceiveAbortReason::RetryLimitExceeded
        )));
    }

    #[test]
    fn long_frame_is_persisted_before_its_ack_and_arrives_byte_identical() {
        let record = vec![b'R'; 1000];
        let mut driver = long_driver(64_000);
        begin(&mut driver);
        let received = feed(
            &mut driver,
            final_frame(1, &[record.as_slice(), &[CR]].concat()),
        );
        assert!(persists(&received));
        assert_eq!(acks(&received), 0);
        assert_eq!(acks(&confirm(&mut driver, &received)), 1);
        assert!(feed(&mut driver, vec![EOT]).contains(&Action::MessageReceived(vec![record])));
    }

    #[test]
    fn long_frames_split_with_etb_reassemble_byte_identically() {
        let records = vec![vec![b'H'; 1000], b"L|1|N".to_vec()];
        let mut driver = long_driver(300);
        begin(&mut driver);
        let frames = crate::frame::encode_records_within(&records, 300).expect("records encode");
        assert!(frames.len() > 2);
        for frame in frames {
            let received = feed(&mut driver, frame);
            assert!(persists(&received));
            assert_eq!(acks(&received), 0);
            assert_eq!(acks(&confirm(&mut driver, &received)), 1);
        }
        assert!(feed(&mut driver, vec![EOT]).contains(&Action::MessageReceived(records)));
    }

    #[test]
    fn a_maximum_size_frame_is_never_refused_for_its_size() {
        let text = [vec![b'A'; 63_999], vec![CR]].concat();
        let frame = final_frame(1, &text);
        // In one read and in small chunks, with a message limit far below the frame size
        // (which then aborts as too large) and with a large one (which accepts it).
        for chunk_size in [frame.len(), 4_096, 64] {
            let mut driver = ReceiveDriver::new(4 * 1024 * 1024).with_max_frame_text(64_000);
            begin(&mut driver);
            let mut persisted = false;
            for chunk in frame.chunks(chunk_size) {
                let actions = feed(&mut driver, chunk.to_vec());
                assert!(!actions.contains(&Action::ReceiveAborted(
                    ReceiveAbortReason::InputBufferExceeded
                )));
                persisted |= persists(&actions);
            }
            assert!(persisted, "chunk size {chunk_size}");
        }

        let mut small = ReceiveDriver::new(3).with_max_frame_text(64_000);
        begin(&mut small);
        let actions = feed(&mut small, frame);
        assert_aborted_unacknowledged(&actions, ReceiveAbortReason::MessageTooLarge);
    }

    #[test]
    fn frame_limits_leave_room_for_one_long_frame() {
        let driver = ReceiveDriver::new(10).with_max_frame_text(64_000);
        assert!(driver.max_wire_bytes >= 64_000 + FRAME_OVERHEAD_BYTES);
        assert_eq!(driver.wire_frame_limit(), 64_000 + FRAME_OVERHEAD_BYTES);
        let default = ReceiveDriver::default();
        assert_eq!(default.wire_frame_limit(), 247);
        assert_eq!(
            default.max_wire_bytes,
            crate::contract::DEFAULT_MAX_WIRE_BYTES_PER_TRANSFER
        );
        // Out-of-range settings are clamped.
        assert_eq!(long_driver(0).max_frame_text, 1);
        assert_eq!(long_driver(usize::MAX).max_frame_text, 64_000);
    }

    #[test]
    fn unterminated_frame_is_cut_off_at_the_configured_frame_size() {
        let mut stream = vec![STX];
        stream.extend(vec![b'A'; 8_000]);
        // Default: 247 bytes without a terminator are NAKed and dropped.
        let mut default = ReceiveDriver::default();
        begin(&mut default);
        assert_eq!(naks(&feed(&mut default, stream.clone())), 1);

        // Long frames: the same bytes are still a frame in progress...
        let mut long = long_driver(64_000);
        begin(&mut long);
        assert!(feed(&mut long, stream).is_empty());
        // ...until it outgrows the largest legal frame.
        let mut aborted = false;
        for _ in 0..8 {
            let actions = feed(&mut long, vec![b'A'; 8_000]);
            aborted |= naks(&actions) == 1;
        }
        assert!(aborted);
    }

    #[test]
    fn buffered_wire_input_stays_bounded_with_long_frames() {
        let mut driver = ReceiveDriver::new(3).with_max_frame_text(64_000);
        begin(&mut driver);
        assert!(persists(&feed(&mut driver, final_frame(1, b"H\r"))));

        // While persistence is pending nothing is consumed: garbage piles up to the
        // bound and then ends the transfer.
        let limit = 64_000 + FRAME_OVERHEAD_BYTES;
        assert!(feed(&mut driver, vec![b'A'; limit]).is_empty());
        let overflow = feed(&mut driver, vec![b'A']);
        assert!(overflow.contains(&Action::ReceiveAborted(
            ReceiveAbortReason::InputBufferExceeded
        )));
        assert_eq!(acks(&overflow), 0);
    }

    proptest! {
        #[test]
        fn random_chunking_never_loses_a_transfer(
            records in prop::collection::vec("[A-Z0-9|^]{1,40}", 1..6),
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..12),
        ) {
            let stream: Vec<u8> = records
                .iter()
                .flat_map(|record| transfer_bytes(record.as_bytes()))
                .collect();
            let chunks = chunked(&stream, &cuts);
            let (messages, ack_count) = drive_chunks(&chunks);
            let expected: Vec<Vec<Vec<u8>>> =
                records.iter().map(|record| vec![record.as_bytes().to_vec()]).collect();
            prop_assert_eq!(messages, expected);
            prop_assert_eq!(ack_count, records.len() * 2);
        }

        #[test]
        fn arbitrary_byte_chunks_and_timer_events_never_panic(
            chunks in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..80), 0..40),
        ) {
            let mut driver = ReceiveDriver::new(4096);
            for chunk in chunks {
                let _ = driver.handle(Event::Bytes(chunk));
                let _ = driver.handle(Event::TimerExpired(TimerId(0)));
                let _ = driver.handle(Event::Persisted(FrameId(0)));
            }
        }
    }
}
