use crate::contract::{Action, DriverError, Event, LinkOptions};
use crate::receive::ReceiveDriver;
use crate::send::SendMachine;

/// Pure bidirectional LIS01-A2 link driver: it receives inbound transfers
/// exactly like [`ReceiveDriver`] and sends one outbound message at a time
/// with establishment, contention handling, retransmission and termination.
///
/// At most one timer is live at any moment: every `StartTimer` replaces the
/// previous one and `StopTimer` cancels it, so the caller needs one timer slot.
#[derive(Debug)]
pub struct LinkDriver {
    receive: ReceiveDriver,
    send: SendMachine,
}

impl Default for LinkDriver {
    fn default() -> Self {
        Self::new(LinkOptions::default())
    }
}

impl LinkDriver {
    /// Constructs an idle link.
    #[must_use]
    pub fn new(options: LinkOptions) -> Self {
        Self {
            receive: ReceiveDriver::new(options.max_message_bytes)
                .with_receive_timeout(options.receive_timeout)
                .with_max_frame_text(options.max_frame_text),
            send: SendMachine::new(options),
        }
    }

    /// Reports whether an inbound transfer is in progress.
    #[must_use]
    pub const fn is_receiving(&self) -> bool {
        self.receive.is_receiving()
    }

    /// Reports whether an outbound message is queued or in flight, that is,
    /// whether a `MessageSent` or `SendAborted` action is still owed.
    #[must_use]
    pub fn is_sending(&self) -> bool {
        self.send.is_sending()
    }

    /// Starts sending one message given as unterminated records (H first, L last).
    ///
    /// Frames are numbered from 1; records longer than a frame are split over
    /// ETB frames. If an inbound transfer is running the message waits for it.
    /// Fails with [`DriverError::SendInProgress`] while a previous message is
    /// still owed a result.
    pub fn submit(&mut self, records: &[Vec<u8>]) -> Result<Vec<Action>, DriverError> {
        let mut actions = Vec::new();
        self.send.submit(records, &mut actions)?;
        Ok(actions)
    }

    /// Applies one event and returns ordered external actions.
    pub fn handle(&mut self, event: Event) -> Result<Vec<Action>, DriverError> {
        let mut actions = Vec::new();
        match event {
            Event::Bytes(bytes) => self.route_bytes(bytes, &mut actions)?,
            Event::Persisted(frame_id) => {
                self.receive_event(Event::Persisted(frame_id), &mut actions)?;
            }
            Event::TimerExpired(timer_id) => {
                self.send.on_timer(timer_id, &mut actions)?;
                self.receive_event(Event::TimerExpired(timer_id), &mut actions)?;
            }
        }
        Ok(actions)
    }

    fn route_bytes(
        &mut self,
        bytes: Vec<u8>,
        actions: &mut Vec<Action>,
    ) -> Result<(), DriverError> {
        let for_receiver = if self.send.wants_bytes() {
            self.send.on_bytes(&bytes, actions)?
        } else {
            Some(bytes)
        };
        match for_receiver {
            Some(rest) if !rest.is_empty() => self.receive_event(Event::Bytes(rest), actions),
            Some(_) | None => Ok(()),
        }
    }

    /// Runs the receiver and keeps the sender informed about line ownership.
    fn receive_event(
        &mut self,
        event: Event,
        actions: &mut Vec<Action>,
    ) -> Result<(), DriverError> {
        let was_receiving = self.receive.is_receiving();
        let produced = self.receive.handle(event)?;
        let touched_timer = produced
            .iter()
            .any(|action| matches!(action, Action::StartTimer { .. } | Action::StopTimer));
        actions.extend(produced);
        match (was_receiving, self.receive.is_receiving()) {
            (false, true) => self.send.set_line_busy(),
            (true, false) => self.send.set_line_idle(actions)?,
            // A whole transfer inside one event replaced the shared timer slot.
            (false, false) if touched_timer => self.send.rearm_wait(actions)?,
            (false, false) | (true, true) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::contract::{FrameId, SendAbortReason, TimerId};
    use crate::frame::{ACK, ENQ, EOT, FrameEnd, NAK, encode_frame, encode_records};
    use crate::testing::{chunked, transfer_bytes};

    #[derive(Clone, Debug)]
    enum Step {
        Bytes(Vec<u8>),
        Control(Vec<u8>),
        Timer(u64),
        Persisted(u64),
        Submit(Vec<Vec<u8>>),
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            prop::collection::vec(any::<u8>(), 0..80).prop_map(Step::Bytes),
            prop::collection::vec(
                prop_oneof![Just(ENQ), Just(ACK), Just(NAK), Just(EOT)],
                1..4
            )
            .prop_map(Step::Control),
            (0_u64..30).prop_map(Step::Timer),
            (0_u64..30).prop_map(Step::Persisted),
            prop::collection::vec(prop::collection::vec(any::<u8>(), 0..300), 0..4)
                .prop_map(Step::Submit),
        ]
    }

    fn feed(link: &mut LinkDriver, bytes: &[u8]) -> Vec<Action> {
        link.handle(Event::Bytes(bytes.to_vec()))
            .expect("bytes are handled")
    }

    fn writes(actions: &[Action], bytes: &[u8]) -> usize {
        actions
            .iter()
            .filter(|action| **action == Action::Write(bytes.to_vec()))
            .count()
    }

    fn receive_chunks(chunks: &[&[u8]]) -> (Vec<Vec<Vec<u8>>>, usize) {
        let mut link = LinkDriver::default();
        crate::testing::receive_chunks(chunks, |event| {
            link.handle(event).expect("event is handled")
        })
    }

    #[test]
    fn inbound_transfers_survive_a_split_at_every_byte() {
        let stream = [transfer_bytes(b"H|one"), transfer_bytes(b"H|two")].concat();
        for split in 0..=stream.len() {
            let (messages, ack_count) = receive_chunks(&[&stream[..split], &stream[split..]]);
            assert_eq!(
                messages,
                vec![vec![b"H|one".to_vec()], vec![b"H|two".to_vec()]],
                "split at {split}"
            );
            assert_eq!(ack_count, 4, "split at {split}");
        }
    }

    #[test]
    fn sender_completes_and_the_receiver_gets_an_enq_from_the_same_read() {
        let mut link = LinkDriver::default();
        let records = vec![b"L|1|N".to_vec()];
        let frames = encode_records(&records).expect("records encode");
        assert_eq!(writes(&link.submit(&records).expect("submit"), &[ENQ]), 1);
        let first = feed(&mut link, &[ACK]);
        assert_eq!(writes(&first, &frames[0]), 1);
        let actions = feed(&mut link, &[ACK, ENQ]);
        assert!(actions.contains(&Action::MessageSent));
        assert_eq!(writes(&actions, &[EOT]), 1);
        assert_eq!(writes(&actions, &[ACK]), 1);
        assert!(link.is_receiving());
        assert!(!link.is_sending());
    }

    #[test]
    fn receiver_interrupt_and_the_following_enq_arrive_in_one_read() {
        let mut link = LinkDriver::default();
        let records = vec![b"L|1|N".to_vec()];
        link.submit(&records).expect("submit");
        feed(&mut link, &[ACK]);
        let actions = feed(&mut link, &[EOT, ENQ]);
        assert!(actions.contains(&Action::SendAborted(SendAbortReason::ReceiverInterrupted)));
        assert_eq!(writes(&actions, &[ACK]), 1);
        assert!(link.is_receiving());
    }

    #[test]
    fn queued_message_waits_for_the_next_inbound_transfer_that_follows_an_eot() {
        let mut link = LinkDriver::default();
        let frame = encode_frame(1, b"L|1|N\r", FrameEnd::Final).expect("frame");
        let first = feed(&mut link, &[ENQ]);
        assert_eq!(writes(&first, &[ACK]), 1);
        // The line is busy: the message is queued, not established.
        let submitted = link.submit(&[b"L|1|N".to_vec()]).expect("submit");
        assert_eq!(writes(&submitted, &[ENQ]), 0);
        let persist = feed(&mut link, &frame);
        let id = persist
            .iter()
            .find_map(Action::persistence_id)
            .expect("frame is persisted");
        link.handle(Event::Persisted(id)).expect("persisted");
        // The first transfer ends and the peer's next ENQ is in the same read.
        let actions = feed(&mut link, &[EOT, ENQ]);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::MessageReceived(_)))
        );
        assert_eq!(writes(&actions, &[ACK]), 1);
        assert_eq!(writes(&actions, &[ENQ]), 0);
        assert!(link.is_receiving());
        // After the second transfer the queued message establishes.
        let second = link
            .handle(Event::Bytes(frame.clone()))
            .expect("second frame");
        let id = second
            .iter()
            .find_map(Action::persistence_id)
            .expect("frame is persisted");
        link.handle(Event::Persisted(id)).expect("persisted");
        let done = feed(&mut link, &[EOT]);
        assert_eq!(writes(&done, &[ENQ]), 1);
    }

    proptest! {
        #[test]
        fn random_chunking_never_loses_an_inbound_transfer(
            records in prop::collection::vec("[A-Z0-9|^]{1,40}", 1..6),
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..12),
        ) {
            let stream: Vec<u8> = records
                .iter()
                .flat_map(|record| transfer_bytes(record.as_bytes()))
                .collect();
            let chunks = chunked(&stream, &cuts);
            let (messages, ack_count) = receive_chunks(&chunks);
            let expected: Vec<Vec<Vec<u8>>> =
                records.iter().map(|record| vec![record.as_bytes().to_vec()]).collect();
            prop_assert_eq!(messages, expected);
            prop_assert_eq!(ack_count, records.len() * 2);
        }

        #[test]
        fn arbitrary_events_and_submissions_never_panic(
            steps in prop::collection::vec(step(), 0..120),
        ) {
            let mut link = LinkDriver::default();
            for step in steps {
                let _ = match step {
                    Step::Bytes(bytes) | Step::Control(bytes) => link.handle(Event::Bytes(bytes)),
                    Step::Timer(id) => link.handle(Event::TimerExpired(TimerId(id))),
                    Step::Persisted(id) => link.handle(Event::Persisted(FrameId(id))),
                    Step::Submit(records) => link.submit(&records),
                };
            }
        }
    }
}
