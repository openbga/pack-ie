//! The runtime state of one channel's peers: what chaos asks them to do, what they have
//! done, and what they recorded.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use super::ledger::Ledger;
use super::topology::Plan;
use super::util::Stop;

/// What the run gives every peer.
#[derive(Debug)]
pub(crate) struct Env {
    pub(crate) started: Instant,
    /// Ends the load: instruments and writers stop sending, chaos stops.
    pub(crate) stop: Stop,
    /// Ends everything else: the receiving peers and the monitor. It is set after the
    /// load stopped and the engine drained.
    pub(crate) halt: Stop,
    pub(crate) seed: u64,
    /// Mean time between messages of a message channel.
    pub(crate) message_interval_ms: u64,
    /// Bytes per second a bridge carries from the instrument to the sink.
    pub(crate) bridge_bytes_per_sec: u64,
    /// Size of the HL7 messages of message channels.
    pub(crate) message_size: usize,
    /// Size of the records of plain bridges.
    pub(crate) bridge_record_size: usize,
    /// Size of the HL7 messages of tapped bridges.
    pub(crate) tap_message_size: usize,
    /// Peers also note every byte of the ASTM exchange (`PACKIE_SOAK_TRACE=1`).
    pub(crate) trace: bool,
    /// Where peers write what they noticed (a wait that timed out, an unexpected byte).
    pub(crate) journal: std::sync::Arc<super::rig::Journal>,
}

impl Env {
    /// Milliseconds since the run started.
    pub(crate) fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Writes a line about something a peer noticed to the chaos journal.
    pub(crate) fn note(&self, event: &str, detail: serde_json::Value) {
        let mut line = serde_json::json!({ "t_ms": self.now_ms(), "event": event });
        if let (Some(map), Some(extra)) = (line.as_object_mut(), detail.as_object()) {
            map.extend(extra.clone());
        }
        self.journal.chaos(&line);
    }

    /// Records an anomaly a peer noticed (it fails the run).
    pub(crate) fn fail(&self, category: &'static str, owner: super::rig::Owner, summary: String) {
        let at = self.now_ms();
        self.journal.note(
            at,
            super::rig::Anomaly {
                at_ms: at,
                unix_ms: super::util::unix_ms(),
                category,
                owner,
                summary,
                count: 1,
                fatal: true,
            },
        );
    }

    /// Size of what the instrument of a bridge of `kind` writes at a time.
    pub(crate) fn record_size(&self, kind: super::topology::Kind) -> usize {
        if kind == super::topology::Kind::TapBridge {
            self.tap_message_size
        } else {
            self.bridge_record_size
        }
    }
}

/// What chaos asks the peers of a channel to do. Peers read these flags between actions.
#[derive(Debug, Default)]
pub(crate) struct Control {
    /// The instrument drops its connection in the middle of its next message, this many times.
    pub(crate) source_disconnects: AtomicU32,
    /// The receiving side drops its connection instead of acknowledging, this many times.
    pub(crate) sink_disconnects: AtomicU32,
    /// Drop connections with a reset instead of an orderly close.
    pub(crate) reset: AtomicBool,
    /// The receiving side reads nothing until this run time (milliseconds); 0 for no stall.
    pub(crate) stall_until_ms: AtomicU64,
    /// The receiving side does not listen.
    pub(crate) down: AtomicBool,
    /// The ASTM instrument ends its next transfer with EOT and the next ENQ in one write,
    /// this many times.
    pub(crate) back_to_back: AtomicU32,
    /// The folder writer writes its next files in two steps, this many times.
    pub(crate) slow_writes: AtomicU32,
}

impl Control {
    /// Takes one pending request from `counter`.
    pub(crate) fn take(counter: &AtomicU32) -> bool {
        counter
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }

    pub(crate) fn stalled(&self, now_ms: u64) -> bool {
        now_ms < self.stall_until_ms.load(Ordering::SeqCst)
    }

    pub(crate) fn is_down(&self) -> bool {
        self.down.load(Ordering::SeqCst)
    }

    pub(crate) fn wants_reset(&self) -> bool {
        self.reset.load(Ordering::SeqCst)
    }
}

/// What the peers of a channel have done.
#[derive(Debug, Default)]
pub(crate) struct Progress {
    /// Bridge: bytes the instrument wrote.
    pub(crate) bytes_sent: AtomicU64,
    /// Bridge: bytes the sink read.
    pub(crate) bytes_received: AtomicU64,
    /// Bridge: complete records or messages the sink read.
    pub(crate) records_received: AtomicU64,
    /// Bridge: bytes the instrument read back from the sink.
    pub(crate) bytes_back: AtomicU64,
    /// Connections the source peer opened.
    pub(crate) connects: AtomicU64,
    /// Connections the receiving peer accepted.
    pub(crate) accepts: AtomicU64,
    /// Mid-message disconnects the source peer performed.
    pub(crate) source_disconnects_done: AtomicU64,
    /// Disconnects the receiving peer performed.
    pub(crate) sink_disconnects_done: AtomicU64,
    /// ASTM: transfers that ended with EOT and the next ENQ in one write.
    pub(crate) chained_done: AtomicU64,
    /// Of those, the ones whose ENQ was acknowledged.
    pub(crate) chained_ok: AtomicU64,
    /// Negative acknowledgements the source peer received.
    pub(crate) rejected: AtomicU64,
    /// Messages the receiving peer could not attribute to a sent message.
    pub(crate) unattributed: AtomicU64,
    /// Run time of the last acknowledgement the source peer got.
    pub(crate) last_ack_ms: AtomicU64,
    /// Run time of the last message the receiving peer got.
    pub(crate) last_recv_ms: AtomicU64,
}

impl Progress {
    pub(crate) fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    pub(crate) fn get(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}

/// One channel's peers.
#[derive(Debug)]
pub(crate) struct ChannelRt {
    pub(crate) plan: Plan,
    pub(crate) ledger: Ledger,
    pub(crate) ctl: Control,
    pub(crate) prog: Progress,
}

impl ChannelRt {
    pub(crate) fn new(plan: Plan) -> Self {
        Self {
            plan,
            ledger: Ledger::default(),
            ctl: Control::default(),
            prog: Progress::default(),
        }
    }

    /// A number that grows whenever data reaches the receiving peer: used to tell that
    /// traffic flows again after an episode.
    pub(crate) fn delivered(&self) -> u64 {
        if self.plan.kind.is_bridge() {
            Progress::get(&self.prog.records_received)
        } else {
            self.ledger.counts().received
        }
    }

    /// A number that grows whenever the source peer gets through: messages acknowledged,
    /// or for a bridge the bytes written.
    pub(crate) fn accepted(&self) -> u64 {
        if self.plan.kind.is_bridge() {
            Progress::get(&self.prog.bytes_sent)
        } else {
            self.ledger.counts().acked
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_taken_one_at_a_time() {
        let counter = AtomicU32::new(2);
        assert!(Control::take(&counter));
        assert!(Control::take(&counter));
        assert!(!Control::take(&counter));
    }

    #[test]
    fn a_stall_ends_at_its_time() {
        let ctl = Control::default();
        assert!(!ctl.stalled(0));
        ctl.stall_until_ms.store(500, Ordering::SeqCst);
        assert!(ctl.stalled(499));
        assert!(!ctl.stalled(500));
    }
}
