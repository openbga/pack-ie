//! Send-side and link conformance scenarios.
//!
//! Each `tests/corpus/send/*.scenario` file is a script that drives a
//! `LinkDriver` the way a session loop would, and states every action the
//! driver must produce. Directives, one per line (`#` starts a comment):
//!
//! | Directive | Meaning |
//! | --- | --- |
//! | `option role computer\|instrument` | Contention role (default computer). |
//! | `option max_enq_attempts N` / `max_frame_retransmissions N` / `max_frame_text N` | Limits. |
//! | `option busy_retry_delay SECONDS` | Wait after a NAK or silence before ENQ is repeated. |
//! | `record TEXT` | Add one record (`<CR>` stands for a carriage return) to the message to send. |
//! | `submit` | Submit the collected records. |
//! | `peer ACK\|NAK\|ENQ\|EOT` | The peer sends one control byte. |
//! | `peer frame N final\|block TEXT` | The peer sends a frame. |
//! | `persist` | Confirm the last `PersistFrame` as durable. |
//! | `tick` | The armed timer fires. |
//! | `tick stale` | A timer that was already replaced fires. |
//! | `expect write ENQ\|ACK\|NAK\|EOT` | Next action writes a control byte. |
//! | `expect frame N final\|block TEXT` | Next action writes this frame. |
//! | `expect timer SECONDS` | Next action starts a timer. |
//! | `expect stop_timer`, `expect persist`, `expect sent` | Next action is that action. |
//! | `expect send_aborted REASON`, `expect receive_aborted REASON` | Abort with this reason. |
//! | `expect received R1 ; R2` | A complete inbound message with these records. |
//!
//! After every `submit`, `peer`, `persist` or `tick` step the actions it
//! produced must be consumed exactly by the following `expect` lines.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use packie_astm::{
    ACK, Action, ENQ, EOT, Event, FrameEnd, FrameId, LineRole, LinkDriver, LinkOptions, NAK,
    ReceiveAbortReason, SendAbortReason, TimerId, encode_frame,
};

struct Runner {
    options: LinkOptions,
    driver: Option<LinkDriver>,
    records: Vec<Vec<u8>>,
    pending: VecDeque<Action>,
    timer: Option<TimerId>,
    retired_timers: Vec<TimerId>,
    frame_id: Option<FrameId>,
}

impl Runner {
    fn new() -> Self {
        Self {
            options: LinkOptions::default(),
            driver: None,
            records: Vec::new(),
            pending: VecDeque::new(),
            timer: None,
            retired_timers: Vec::new(),
            frame_id: None,
        }
    }

    fn driver(&mut self) -> &mut LinkDriver {
        let options = self.options.clone();
        self.driver.get_or_insert_with(|| LinkDriver::new(options))
    }

    /// Replaces the single timer slot, remembering the replaced timer as stale.
    fn arm(&mut self, timer: Option<TimerId>) {
        let old = std::mem::replace(&mut self.timer, timer);
        self.retired_timers.extend(old);
    }

    fn absorb(&mut self, actions: Vec<Action>, line: &str) {
        assert!(
            self.pending.is_empty(),
            "{line}: unexpected actions left from the previous step: {:?}",
            self.pending
        );
        for action in &actions {
            match action {
                Action::StartTimer { timer_id, .. } => self.arm(Some(*timer_id)),
                Action::StopTimer => self.arm(None),
                Action::PersistFrame { frame_id, .. } => self.frame_id = Some(*frame_id),
                Action::Write(_)
                | Action::MessageReceived(_)
                | Action::ReceiveAborted(_)
                | Action::MessageSent
                | Action::SendAborted(_) => {}
            }
        }
        self.pending = actions.into();
    }

    fn step(&mut self, line: &str, words: &[&str]) {
        let event = match words {
            ["option", rest @ ..] => return self.option(line, rest),
            ["record", ..] => {
                let text = line
                    .trim_start()
                    .strip_prefix("record")
                    .unwrap_or("")
                    .trim_start();
                self.records.push(text.replace("<CR>", "\r").into_bytes());
                return;
            }
            ["submit"] => {
                let records = std::mem::take(&mut self.records);
                let actions = self
                    .driver()
                    .submit(&records)
                    .unwrap_or_else(|error| panic!("{line}: submit failed: {error}"));
                return self.absorb(actions, line);
            }
            ["peer", "frame", number, end, ..] => {
                let text = line
                    .splitn(5, ' ')
                    .nth(4)
                    .unwrap_or("")
                    .replace("<CR>", "\r");
                let number = number.parse().expect("frame number");
                let frame =
                    encode_frame(number, text.as_bytes(), end_of(end)).expect("peer frame encodes");
                Event::Bytes(frame)
            }
            ["peer", name] => Event::Bytes(vec![control(name)]),
            ["persist"] => {
                Event::Persisted(self.frame_id.take().expect("a frame awaits persistence"))
            }
            ["tick"] => Event::TimerExpired(self.timer.expect("a timer is armed")),
            ["tick", "stale"] => {
                Event::TimerExpired(*self.retired_timers.last().expect("a replaced timer exists"))
            }
            ["expect", rest @ ..] => return self.expect(line, rest),
            _ => panic!("unknown directive: {line}"),
        };
        let actions = self
            .driver()
            .handle(event)
            .unwrap_or_else(|error| panic!("{line}: driver error: {error}"));
        self.absorb(actions, line);
    }

    fn option(&mut self, line: &str, words: &[&str]) {
        assert!(self.driver.is_none(), "{line}: options come first");
        match words {
            ["role", "computer"] => self.options.role = LineRole::Computer,
            ["role", "instrument"] => self.options.role = LineRole::Instrument,
            ["max_enq_attempts", n] => self.options.max_enq_attempts = n.parse().expect("number"),
            ["max_frame_retransmissions", n] => {
                self.options.max_frame_retransmissions = n.parse().expect("number");
            }
            ["max_frame_text", n] => self.options.max_frame_text = n.parse().expect("number"),
            ["busy_retry_delay", seconds] => {
                self.options.busy_retry_delay =
                    Duration::from_secs(seconds.parse().expect("seconds"));
            }
            _ => panic!("unknown option: {line}"),
        }
    }

    fn expect(&mut self, line: &str, words: &[&str]) {
        let action = self
            .pending
            .pop_front()
            .unwrap_or_else(|| panic!("{line}: no action left to check"));
        let matches = match (words, &action) {
            (["write", name], Action::Write(bytes)) => bytes.as_slice() == [control(name)],
            (["frame", number, end, ..], Action::Write(bytes)) => {
                let text = line.splitn(5, ' ').nth(4).unwrap_or("");
                let expected = encode_frame(
                    number.parse().expect("frame number"),
                    text.replace("<CR>", "\r").as_bytes(),
                    end_of(end),
                )
                .expect("expected frame encodes");
                *bytes == expected
            }
            (["timer", seconds], Action::StartTimer { duration, .. }) => {
                *duration == Duration::from_secs(seconds.parse().expect("seconds"))
            }
            (["stop_timer"], Action::StopTimer)
            | (["persist"], Action::PersistFrame { .. })
            | (["sent"], Action::MessageSent) => true,
            (["send_aborted", reason], Action::SendAborted(actual)) => {
                send_reason(reason) == *actual
            }
            (["receive_aborted", reason], Action::ReceiveAborted(actual)) => {
                receive_reason(reason) == *actual
            }
            (["received", ..], Action::MessageReceived(records)) => {
                let text = line
                    .trim_start()
                    .strip_prefix("expect received")
                    .unwrap_or("");
                let expected: Vec<Vec<u8>> = text
                    .split(" ; ")
                    .map(|record| record.trim().replace("<CR>", "\r").into_bytes())
                    .collect();
                *records == expected
            }
            _ => false,
        };
        assert!(matches, "{line}: driver produced {action:?}");
    }

    fn finish(&self, name: &str) {
        assert!(
            self.pending.is_empty(),
            "{name}: actions never checked: {:?}",
            self.pending
        );
    }
}

fn control(name: &str) -> u8 {
    match name {
        "ENQ" => ENQ,
        "ACK" => ACK,
        "NAK" => NAK,
        "EOT" => EOT,
        _ => panic!("unknown control byte {name}"),
    }
}

fn end_of(name: &str) -> FrameEnd {
    match name {
        "final" => FrameEnd::Final,
        "block" => FrameEnd::Intermediate,
        _ => panic!("unknown frame end {name}"),
    }
}

fn send_reason(name: &str) -> SendAbortReason {
    match name {
        "establishment_failed" => SendAbortReason::EstablishmentFailed,
        "retry_limit" => SendAbortReason::RetryLimitExceeded,
        "reply_timeout" => SendAbortReason::ReplyTimeout,
        "receiver_interrupted" => SendAbortReason::ReceiverInterrupted,
        _ => panic!("unknown send abort reason {name}"),
    }
}

fn receive_reason(name: &str) -> ReceiveAbortReason {
    match name {
        "receive_timeout" => ReceiveAbortReason::ReceiveTimeout,
        "retry_limit" => ReceiveAbortReason::RetryLimitExceeded,
        _ => panic!("unknown receive abort reason {name}"),
    }
}

fn run_scenario(path: &Path) {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let script = fs::read_to_string(path).expect("scenario is readable");
    let mut runner = Runner::new();
    for raw in script.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        runner.step(line, &words);
    }
    runner.finish(&name);
}

#[test]
fn every_send_scenario_passes() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/send");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("scenario directory exists")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "scenario"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 12,
        "expected the full scenario set, found {}",
        paths.len()
    );
    let mut failures = Vec::new();
    for path in &paths {
        if std::panic::catch_unwind(|| run_scenario(path)).is_err() {
            failures.push(path.display().to_string());
        }
    }
    assert!(failures.is_empty(), "failing scenarios: {failures:?}");
}
