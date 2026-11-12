//! Two real drivers talking to each other over a simulated link with a virtual
//! clock: interoperability, checksum errors, simultaneous ENQ and chunked input.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::VecDeque;
use std::time::Duration;

use packie_astm::{
    Action, DriverError, Event, FrameId, LineRole, LinkDriver, LinkOptions, ReceiveAbortReason,
    SendAbortReason, TimerId,
};
use proptest::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Side {
    A,
    B,
}

impl Side {
    const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

struct Endpoint {
    link: LinkDriver,
    timer: Option<(TimerId, Duration)>,
    received: Vec<Vec<Vec<u8>>>,
    sent: usize,
    send_aborts: Vec<SendAbortReason>,
    receive_aborts: Vec<ReceiveAbortReason>,
}

impl Endpoint {
    fn new(role: LineRole) -> Self {
        Self::with_options(LinkOptions {
            role,
            ..LinkOptions::default()
        })
    }

    fn with_options(options: LinkOptions) -> Self {
        Self {
            link: LinkDriver::new(options),
            timer: None,
            received: Vec::new(),
            sent: 0,
            send_aborts: Vec::new(),
            receive_aborts: Vec::new(),
        }
    }

    /// Performs actions like a session loop and returns the bytes to put on the wire.
    fn perform(&mut self, actions: Vec<Action>, now: Duration) -> Vec<Vec<u8>> {
        let mut wire = Vec::new();
        let mut persisted: Vec<FrameId> = Vec::new();
        for action in actions {
            match action {
                Action::Write(bytes) => wire.push(bytes),
                Action::PersistFrame { frame_id, .. } => persisted.push(frame_id),
                Action::MessageReceived(records) => self.received.push(records),
                Action::ReceiveAborted(reason) => self.receive_aborts.push(reason),
                Action::MessageSent => self.sent += 1,
                Action::SendAborted(reason) => self.send_aborts.push(reason),
                Action::StartTimer { timer_id, duration } => {
                    self.timer = Some((timer_id, now + duration));
                }
                Action::StopTimer => self.timer = None,
            }
        }
        for frame_id in persisted {
            let more = self
                .link
                .handle(Event::Persisted(frame_id))
                .expect("persistence is confirmed once");
            wire.extend(self.perform(more, now));
        }
        wire
    }

    fn feed(&mut self, event: Event, now: Duration) -> Vec<Vec<u8>> {
        let actions = self.link.handle(event).expect("event is handled");
        self.perform(actions, now)
    }

    fn submit(&mut self, records: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, DriverError> {
        let actions = self.link.submit(records)?;
        Ok(self.perform(actions, Duration::ZERO))
    }
}

type Tamper<'a> = &'a mut dyn FnMut(Side, &mut Vec<u8>);

/// Both endpoints, one FIFO of in-flight writes `(destination, bytes)`, a virtual clock.
struct Sim {
    a: Endpoint,
    b: Endpoint,
    in_flight: VecDeque<(Side, Vec<u8>)>,
    now: Duration,
}

impl Sim {
    fn new(role_a: LineRole, role_b: LineRole) -> Self {
        Self::with_frame_text(
            role_a,
            role_b,
            LinkOptions::default().max_frame_text,
            LinkOptions::default().max_frame_text,
        )
    }

    /// Endpoint A sends frames of `text_a` bytes and expects `text_a`; B likewise.
    fn with_frame_text(role_a: LineRole, role_b: LineRole, text_a: usize, text_b: usize) -> Self {
        let options = |role, max_frame_text| LinkOptions {
            role,
            max_frame_text,
            ..LinkOptions::default()
        };
        Self {
            a: Endpoint::with_options(options(role_a, text_a)),
            b: Endpoint::with_options(options(role_b, text_b)),
            in_flight: VecDeque::new(),
            now: Duration::ZERO,
        }
    }

    fn endpoint(&mut self, side: Side) -> &mut Endpoint {
        match side {
            Side::A => &mut self.a,
            Side::B => &mut self.b,
        }
    }

    fn put_on_wire(&mut self, from: Side, writes: Vec<Vec<u8>>, tamper: Tamper<'_>) {
        for mut bytes in writes {
            tamper(from, &mut bytes);
            self.in_flight.push_back((from.other(), bytes));
        }
    }

    fn submit(&mut self, side: Side, records: &[Vec<u8>]) {
        let writes = self.endpoint(side).submit(records).expect("submit");
        self.put_on_wire(side, writes, &mut |_, _| {});
    }

    /// Delivers one write to `destination` in chunks of `chunk` bytes (0 = whole).
    fn deliver(&mut self, destination: Side, bytes: &[u8], chunk: usize, tamper: Tamper<'_>) {
        let size = if chunk == 0 {
            bytes.len().max(1)
        } else {
            chunk
        };
        for piece in bytes.chunks(size) {
            let now = self.now;
            let writes = self
                .endpoint(destination)
                .feed(Event::Bytes(piece.to_vec()), now);
            self.put_on_wire(destination, writes, tamper);
        }
    }

    /// Runs until nothing is in flight and no timer is armed; returns the finish time.
    fn run(&mut self, chunk: usize, tamper: Tamper<'_>) -> Duration {
        for _ in 0..20_000 {
            if let Some((destination, bytes)) = self.in_flight.pop_front() {
                self.deliver(destination, &bytes, chunk, tamper);
                continue;
            }
            let next = [(Side::A, self.a.timer), (Side::B, self.b.timer)]
                .into_iter()
                .filter_map(|(side, timer)| timer.map(|(id, due)| (due, side, id)))
                .min_by_key(|(due, _, _)| *due);
            let Some((due, side, id)) = next else {
                return self.now;
            };
            self.now = due;
            let endpoint = self.endpoint(side);
            endpoint.timer = None;
            let writes = endpoint.feed(Event::TimerExpired(id), due);
            self.put_on_wire(side, writes, tamper);
        }
        panic!("simulation did not settle");
    }
}

fn order_message(tag: &str) -> Vec<Vec<u8>> {
    vec![
        format!("H|\\^&|||{tag}^1.0").into_bytes(),
        b"P|1|SYN-PRACTICE|SYN-LAB".to_vec(),
        format!("O|1|{tag}-0001||^^^SYN-A|R").into_bytes(),
        b"L|1|N".to_vec(),
    ]
}

/// Flips one checksum digit in the `n`th frame written by side A (0-based).
fn damage_nth_frame(target: usize) -> impl FnMut(Side, &mut Vec<u8>) {
    let mut frames_seen = 0_usize;
    move |from, bytes| {
        if from != Side::A || bytes.first() != Some(&0x02) {
            return;
        }
        if frames_seen == target {
            let position = bytes.len() - 4;
            bytes[position] ^= 1;
        }
        frames_seen += 1;
    }
}

#[test]
fn sender_and_receiver_interoperate() {
    let mut sim = Sim::new(LineRole::Computer, LineRole::Instrument);
    let message = order_message("SYN-A");
    sim.submit(Side::A, &message);
    sim.run(0, &mut |_, _| {});
    assert_eq!(sim.a.sent, 1);
    assert_eq!(sim.b.received, vec![message]);
}

#[test]
fn checksum_error_is_naked_and_the_frame_is_resent() {
    let mut sim = Sim::new(LineRole::Computer, LineRole::Instrument);
    let message = order_message("SYN-A");
    sim.submit(Side::A, &message);
    // Damage the first transmission of the second frame only.
    sim.run(0, &mut damage_nth_frame(1));
    assert_eq!(sim.a.sent, 1);
    assert_eq!(sim.b.received, vec![message]);
    assert!(sim.a.send_aborts.is_empty());
}

#[test]
fn long_records_split_over_etb_frames_and_frame_numbers_wrap() {
    let mut sim = Sim::new(LineRole::Computer, LineRole::Instrument);
    let mut message = vec![b"H|\\^&|||PACKIE^1.0".to_vec()];
    for sequence in 1..=9 {
        message.push(format!("C|{sequence}|I|{}", "x".repeat(300)).into_bytes());
    }
    message.push(b"L|1|N".to_vec());
    sim.submit(Side::A, &message);
    sim.run(7, &mut |_, _| {});
    assert_eq!(sim.a.sent, 1);
    assert_eq!(sim.b.received, vec![message]);
}

/// Counts the frames (STX-led writes) endpoint A puts on the wire while the simulation runs.
fn count_frames(counter: &mut usize) -> impl FnMut(Side, &mut Vec<u8>) + '_ {
    move |from, bytes| {
        if from == Side::A && bytes.first() == Some(&0x02) {
            *counter += 1;
        }
    }
}

#[test]
fn a_1000_byte_record_travels_in_one_long_frame() {
    let mut sim = Sim::with_frame_text(LineRole::Computer, LineRole::Instrument, 64_000, 64_000);
    let message = vec![
        b"H|\\^&|||PACKIE^1.0".to_vec(),
        format!("C|1|I|{}", "x".repeat(1000)).into_bytes(),
        b"L|1|N".to_vec(),
    ];
    sim.submit(Side::A, &message);
    let mut frames = 0;
    sim.run(0, &mut count_frames(&mut frames));
    assert_eq!((sim.a.sent, frames), (1, 3));
    assert_eq!(sim.b.received, vec![message]);
}

#[test]
fn a_1000_byte_record_splits_at_300_and_reassembles_byte_identically() {
    let mut sim = Sim::with_frame_text(LineRole::Computer, LineRole::Instrument, 300, 300);
    let message = vec![
        b"H|\\^&|||PACKIE^1.0".to_vec(),
        format!("C|1|I|{}", "x".repeat(1000)).into_bytes(),
        b"L|1|N".to_vec(),
    ];
    sim.submit(Side::A, &message);
    let mut frames = 0;
    sim.run(37, &mut count_frames(&mut frames));
    assert_eq!(sim.a.sent, 1);
    // 1 header frame, ceil(1008 / 300) = 4 comment frames, 1 terminator frame.
    assert_eq!(frames, 6);
    assert_eq!(sim.b.received, vec![message]);
}

#[test]
fn a_receiver_with_the_default_limit_naks_long_frames_and_the_sender_gives_up() {
    let mut sim = Sim::with_frame_text(LineRole::Computer, LineRole::Instrument, 1000, 240);
    let message = vec![
        b"H|\\^&|||PACKIE^1.0".to_vec(),
        format!("C|1|I|{}", "x".repeat(500)).into_bytes(),
        b"L|1|N".to_vec(),
    ];
    sim.submit(Side::A, &message);
    sim.run(0, &mut |_, _| {});
    assert_eq!(sim.a.sent, 0);
    assert!(sim.b.received.is_empty());
    // The header frame fits; the 508-byte comment frame is NAKed six times, the receiver
    // gives up and the sender's last retransmission goes unanswered.
    assert_eq!(
        sim.b.receive_aborts,
        [ReceiveAbortReason::RetryLimitExceeded]
    );
    assert_eq!(sim.a.send_aborts, [SendAbortReason::ReplyTimeout]);
}

#[test]
fn simultaneous_enq_is_resolved_and_both_messages_arrive() {
    let mut sim = Sim::new(LineRole::Instrument, LineRole::Computer);
    let from_a = order_message("SYN-A");
    let from_b = order_message("SYN-B");
    // Both ENQs are on the wire before either side reads.
    sim.submit(Side::A, &from_a);
    sim.submit(Side::B, &from_b);
    let finished = sim.run(0, &mut |_, _| {});
    assert_eq!((sim.a.sent, sim.b.sent), (1, 1));
    assert_eq!(sim.b.received, vec![from_a]);
    assert_eq!(sim.a.received, vec![from_b]);
    // The instrument won after its 1 s wait.
    assert!(finished >= Duration::from_secs(1));
    assert!(sim.a.send_aborts.is_empty() && sim.b.send_aborts.is_empty());
    assert!(sim.a.receive_aborts.is_empty() && sim.b.receive_aborts.is_empty());
}

#[test]
fn submit_misuse_is_rejected_with_typed_errors() {
    let mut busy = Endpoint::new(LineRole::Computer);
    busy.submit(&order_message("SYN-A")).expect("first submit");
    assert_eq!(
        busy.submit(&order_message("SYN-A")).err(),
        Some(DriverError::SendInProgress)
    );
    let mut idle = Endpoint::new(LineRole::Computer);
    assert_eq!(idle.submit(&[]).err(), Some(DriverError::EmptyMessage));
    let with_cr = vec![b"H|\\^&\r".to_vec()];
    assert!(matches!(idle.submit(&with_cr), Err(DriverError::Encode(_))));
}

proptest! {
    #[test]
    fn random_messages_survive_random_chunking_and_damage(
        bodies in prop::collection::vec("[ -~]{0,700}", 1..6),
        chunk in 0_usize..40,
        damage_at in prop::option::of(0_usize..12),
    ) {
        let message: Vec<Vec<u8>> = bodies
            .iter()
            .enumerate()
            .map(|(index, body)| format!("C|{}|I|{body}", index + 1).into_bytes())
            .collect();
        let mut sim = Sim::new(LineRole::Computer, LineRole::Instrument);
        sim.submit(Side::A, &message);
        sim.run(chunk, &mut damage_nth_frame(damage_at.unwrap_or(usize::MAX)));
        prop_assert_eq!(sim.a.sent, 1);
        prop_assert_eq!(sim.b.received.len(), 1);
        prop_assert_eq!(&sim.b.received[0], &message);
    }
}
