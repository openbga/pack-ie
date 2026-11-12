//! The pure ASTM receive driver fed arbitrary bytes, well-formed frames and whole transfers,
//! with persistence confirmations and timer expiries delivered in any order, repeatedly and
//! stale. FrameId and TimerId cannot be forged, so the harness replays the ones the driver
//! issued. Completed messages are parsed and assembled.
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_astm::{
    Action, ENQ, EOT, Event, FrameId, ReceiveDriver, TimerId, assemble_message, encode_records,
    parse_message_records,
};
use packie_fuzz::{AstmStep, astm_steps};

/// A completed transfer must parse and assemble (or fail with a typed error).
fn assemble(records: &[Vec<u8>]) {
    let mut text = Vec::new();
    for record in records {
        text.extend_from_slice(record);
        text.push(b'\r');
    }
    if let Ok(records) = parse_message_records(&text) {
        let _ = assemble_message(records);
    }
}

#[derive(Default)]
struct Session {
    frames: Vec<FrameId>,
    timers: Vec<TimerId>,
    pending: Vec<FrameId>,
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
                Action::MessageReceived(records) => assemble(&records),
                Action::Write(_)
                | Action::ReceiveAborted(_)
                | Action::MessageSent
                | Action::SendAborted(_)
                | Action::StopTimer => {}
            }
        }
    }

    fn feed(&mut self, driver: &mut ReceiveDriver, event: Event) {
        if let Ok(actions) = driver.handle(event) {
            self.absorb(actions);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut driver = ReceiveDriver::new(2048);
    let mut session = Session::default();
    for step in astm_steps(data) {
        match step {
            AstmStep::Bytes(bytes) => session.feed(&mut driver, Event::Bytes(bytes)),
            AstmStep::Control(byte) => session.feed(&mut driver, Event::Bytes(vec![byte])),
            AstmStep::Frame(frame) => session.feed(&mut driver, Event::Bytes(frame)),
            AstmStep::Transfer(records) | AstmStep::Submit(records) => {
                session.feed(&mut driver, Event::Bytes(vec![ENQ]));
                if let Ok(frames) = encode_records(&records) {
                    for frame in frames {
                        session.feed(&mut driver, Event::Bytes(frame));
                        // Acknowledge promptly so transfers get as far as completing.
                        if let Some(id) = session.pending.pop() {
                            session.feed(&mut driver, Event::Persisted(id));
                        }
                    }
                }
                session.feed(&mut driver, Event::Bytes(vec![EOT]));
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
    let _ = driver.is_receiving();
});
