//! The ASTM link driver (receive and send state machines together) fed the same steps as the
//! receive driver plus message submissions, with both line roles and small retry limits so
//! contention, busy-line, retransmission and give-up paths are reached.
#![no_main]

use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use packie_astm::{
    ACK, Action, ENQ, EOT, Event, FrameId, LineRole, LinkDriver, LinkOptions, NAK, STX, TimerId,
    encode_records,
};
use packie_fuzz::{AstmStep, astm_steps};

#[derive(Default)]
struct Session {
    frames: Vec<FrameId>,
    timers: Vec<TimerId>,
    pending: Vec<FrameId>,
    /// A cooperative peer answers the driver's ENQ and frames; `nak_every` makes it refuse some.
    peer: Option<u8>,
    replies: Vec<u8>,
    writes: u32,
}

impl Session {
    fn absorb(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::PersistFrame { frame_id, .. } => {
                    self.frames.push(frame_id);
                    self.pending.push(frame_id);
                }
                Action::StartTimer { timer_id, .. } => self.timers.push(timer_id),
                Action::Write(bytes) => self.peer_answers(&bytes),
                Action::MessageReceived(_)
                | Action::ReceiveAborted(_)
                | Action::MessageSent
                | Action::SendAborted(_)
                | Action::StopTimer => {}
            }
        }
    }

    /// The cooperative peer's answer to something the driver wrote.
    fn peer_answers(&mut self, written: &[u8]) {
        self.writes += 1;
        let Some(nak_every) = self.peer else {
            return;
        };
        if written == [ENQ] || written.first() == Some(&STX) {
            let refuse = nak_every != 0 && self.writes.is_multiple_of(u32::from(nak_every));
            self.replies.push(if refuse { NAK } else { ACK });
        }
    }

    fn feed(&mut self, driver: &mut LinkDriver, event: Event) {
        if let Ok(actions) = driver.handle(event) {
            self.absorb(actions);
        }
        self.reply(driver);
    }

    fn reply(&mut self, driver: &mut LinkDriver) {
        for _ in 0..4096 {
            let Some(byte) = self.replies.pop() else {
                return;
            };
            if let Ok(actions) = driver.handle(Event::Bytes(vec![byte])) {
                self.absorb(actions);
            }
        }
        self.replies.clear();
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&control, data)) = data.split_first() else {
        return;
    };
    let options = LinkOptions {
        role: if control & 1 == 0 {
            LineRole::Computer
        } else {
            LineRole::Instrument
        },
        max_message_bytes: 2048,
        reply_timeout: Duration::from_secs(1 + u64::from(control >> 4)),
        max_enq_attempts: 1 + (control >> 1) % 3,
        max_frame_retransmissions: 1 + (control >> 3) % 3,
        ..LinkOptions::default()
    };
    let mut driver = LinkDriver::new(options);
    let mut session = Session {
        peer: (control & 0x80 != 0).then_some((control >> 5) & 3),
        ..Session::default()
    };
    for step in astm_steps(data) {
        match step {
            AstmStep::Bytes(bytes) => session.feed(&mut driver, Event::Bytes(bytes)),
            AstmStep::Control(byte) => session.feed(&mut driver, Event::Bytes(vec![byte])),
            AstmStep::Frame(frame) => session.feed(&mut driver, Event::Bytes(frame)),
            AstmStep::Transfer(records) => {
                session.feed(&mut driver, Event::Bytes(vec![ENQ]));
                if let Ok(frames) = encode_records(&records) {
                    for frame in frames {
                        session.feed(&mut driver, Event::Bytes(frame));
                        if let Some(id) = session.pending.pop() {
                            session.feed(&mut driver, Event::Persisted(id));
                        }
                    }
                }
                session.feed(&mut driver, Event::Bytes(vec![EOT]));
            }
            AstmStep::Submit(records) => {
                if let Ok(actions) = driver.submit(&records) {
                    session.absorb(actions);
                }
                session.reply(&mut driver);
            }
            AstmStep::Persisted(index) => {
                if let Some(id) = session.frames.get(index % session.frames.len().max(1)) {
                    session.feed(&mut driver, Event::Persisted(*id));
                }
            }
            AstmStep::Timer(index) => {
                if let Some(id) = session.timers.get(index % session.timers.len().max(1)) {
                    session.feed(&mut driver, Event::TimerExpired(*id));
                }
            }
            AstmStep::Drain => {
                for id in std::mem::take(&mut session.pending) {
                    session.feed(&mut driver, Event::Persisted(id));
                }
            }
        }
    }
    let _ = (driver.is_receiving(), driver.is_sending());
});
