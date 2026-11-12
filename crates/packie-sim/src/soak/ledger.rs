//! What the peers of one channel sent, had acknowledged and received, by sequence number.
//!
//! Message channels are compared by sets: every message the source peer had acknowledged
//! must reach the receiving peer exactly once. Bridges carry a byte stream, so a record
//! lost when a connection breaks is expected; a loss with no connection break near it, a
//! duplicate, a reordering or a damaged record is not.

use std::sync::{Mutex, MutexGuard, PoisonError};

const SENT: u8 = 1;
const ACKED: u8 = 2;
const DISCARDED: u8 = 4;
const RECV_SHIFT: u8 = 3;
const RECV_MASK: u8 = 0b11 << RECV_SHIFT;

/// A connection break explains a missing record sent up to this long before it...
const BREAK_BEFORE_MS: u32 = 2_000;
/// ...or up to this long after it (records buffered in the bridge are lost with it).
const BREAK_AFTER_MS: u32 = 90_000;

#[derive(Debug, Default)]
struct Inner {
    flags: Vec<u8>,
    sent_ms: Vec<u32>,
    acked_ms: Vec<u32>,
    recv_ms: Vec<u32>,
    breaks: Vec<u32>,
    latencies: Vec<u32>,
    corrupt: u64,
    out_of_order: u64,
    max_received: Option<u64>,
    acked: u64,
    received: u64,
    duplicates: u64,
}

/// The record of one channel; shared by its peers.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    inner: Mutex<Inner>,
}

/// Counts of a ledger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) allocated: u64,
    pub(crate) acked: u64,
    pub(crate) received: u64,
    pub(crate) duplicates: u64,
    pub(crate) corrupt: u64,
    pub(crate) out_of_order: u64,
}

/// The comparison of sent and received messages of a message channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Recon {
    pub(crate) acked: u64,
    pub(crate) received: u64,
    /// Received more than once.
    pub(crate) duplicates: Vec<u64>,
    /// Acknowledged to the source, never received, and older than the settle time.
    pub(crate) missing: Vec<u64>,
    /// Of `missing`, the ones the receiving peer deliberately discarded.
    pub(crate) discarded: Vec<u64>,
    /// Acknowledged but not yet old enough to be judged.
    pub(crate) pending: u64,
    /// Received but never acknowledged to the source (it gave up, or the run ended).
    pub(crate) unacknowledged: u64,
    pub(crate) corrupt: u64,
}

/// The comparison of a bridge's records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BridgeRecon {
    pub(crate) sent: u64,
    pub(crate) received: u64,
    pub(crate) duplicates: u64,
    pub(crate) out_of_order: u64,
    pub(crate) corrupt: u64,
    /// Missing records with a connection break near their send time.
    pub(crate) lost_at_break: u64,
    /// Missing records with no break near them.
    pub(crate) lost_unexplained: Vec<u64>,
    /// Records too recent to be judged.
    pub(crate) pending: u64,
}

fn at(slot: &mut Vec<u32>, seq: usize) -> &mut u32 {
    if slot.len() <= seq {
        slot.resize(seq + 1, 0);
    }
    &mut slot[seq]
}

fn ms32(ms: u64) -> u32 {
    u32::try_from(ms).unwrap_or(u32::MAX)
}

impl Ledger {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next sequence number.
    pub(crate) fn alloc(&self) -> u64 {
        let mut inner = self.lock();
        inner.flags.push(0);
        (inner.flags.len() - 1) as u64
    }

    /// The first attempt to send `seq`, at run time `ms`.
    pub(crate) fn sent(&self, seq: u64, ms: u64) {
        let mut inner = self.lock();
        let i = usize::try_from(seq).unwrap_or(usize::MAX);
        let Some(flags) = inner.flags.get_mut(i) else {
            return;
        };
        if *flags & SENT == 0 {
            *flags |= SENT;
            *at(&mut inner.sent_ms, i) = ms32(ms).max(1);
        }
    }

    /// The source peer received a positive acknowledgement of `seq`.
    pub(crate) fn acked(&self, seq: u64, ms: u64) {
        let mut inner = self.lock();
        let i = usize::try_from(seq).unwrap_or(usize::MAX);
        let Some(flags) = inner.flags.get_mut(i) else {
            return;
        };
        if *flags & ACKED == 0 {
            *flags |= ACKED;
            inner.acked += 1;
            *at(&mut inner.acked_ms, i) = ms32(ms).max(1);
        }
    }

    /// The receiving peer deliberately discarded `seq` without acknowledging it.
    pub(crate) fn discarded(&self, seq: u64) {
        let mut inner = self.lock();
        if let Some(flags) = inner
            .flags
            .get_mut(usize::try_from(seq).unwrap_or(usize::MAX))
        {
            *flags |= DISCARDED;
        }
    }

    /// The receiving peer got `seq`. Returns how often it has now received it.
    pub(crate) fn received(&self, seq: u64, ms: u64) -> u8 {
        let mut inner = self.lock();
        let i = usize::try_from(seq).unwrap_or(usize::MAX);
        let Some(flags) = inner.flags.get_mut(i) else {
            return 0;
        };
        let count = ((*flags & RECV_MASK) >> RECV_SHIFT)
            .saturating_add(1)
            .min(3);
        *flags = (*flags & !RECV_MASK) | (count << RECV_SHIFT);
        if count == 1 {
            inner.received += 1;
            *at(&mut inner.recv_ms, i) = ms32(ms).max(1);
            let sent = inner.sent_ms.get(i).copied().unwrap_or(0);
            if sent > 0 {
                inner.latencies.push(ms32(ms).saturating_sub(sent));
            }
        } else {
            inner.duplicates += 1;
        }
        count
    }

    /// A bridge sink got record `seq`; also checks the order. Returns `false` for a
    /// duplicate or out of order record.
    pub(crate) fn received_in_order(&self, seq: u64, ms: u64) -> bool {
        let in_order = {
            let mut inner = self.lock();
            let ok = inner.max_received.is_none_or(|max| seq > max);
            if ok {
                inner.max_received = Some(seq);
            } else {
                inner.out_of_order += 1;
            }
            ok
        };
        self.received(seq, ms);
        in_order
    }

    /// A damaged or unexpected message arrived.
    pub(crate) fn corrupt(&self) {
        self.lock().corrupt += 1;
    }

    /// A connection of this channel ended at run time `ms`.
    pub(crate) fn add_break(&self, ms: u64) {
        self.lock().breaks.push(ms32(ms));
    }

    /// Latencies in milliseconds of messages received since the last call.
    pub(crate) fn take_latencies(&self) -> Vec<u32> {
        std::mem::take(&mut self.lock().latencies)
    }

    pub(crate) fn counts(&self) -> Counts {
        let inner = self.lock();
        Counts {
            allocated: inner.flags.len() as u64,
            acked: inner.acked,
            received: inner.received,
            duplicates: inner.duplicates,
            corrupt: inner.corrupt,
            out_of_order: inner.out_of_order,
        }
    }

    /// Compares what the source peer had acknowledged with what the receiving peer got.
    /// Messages acknowledged less than `settle_ms` before `now_ms` are only counted.
    pub(crate) fn reconcile(&self, now_ms: u64, settle_ms: u64) -> Recon {
        let inner = self.lock();
        let limit = ms32(now_ms.saturating_sub(settle_ms));
        let mut recon = Recon {
            corrupt: inner.corrupt,
            ..Recon::default()
        };
        for (i, flags) in inner.flags.iter().enumerate() {
            tally(&inner, &mut recon, i, *flags, limit);
        }
        recon
    }

    /// Judges the records of a bridge. Records sent less than `settle_ms` before `now_ms`
    /// are only counted as pending.
    pub(crate) fn reconcile_bridge(&self, now_ms: u64, settle_ms: u64) -> BridgeRecon {
        let inner = self.lock();
        let limit = ms32(now_ms.saturating_sub(settle_ms));
        let mut breaks = inner.breaks.clone();
        breaks.sort_unstable();
        let mut recon = BridgeRecon {
            duplicates: inner.duplicates,
            out_of_order: inner.out_of_order,
            corrupt: inner.corrupt,
            ..BridgeRecon::default()
        };
        for (i, flags) in inner.flags.iter().enumerate() {
            tally_bridge(&inner, &breaks, &mut recon, i, *flags, limit);
        }
        recon
    }
}

/// Counts message `i` of a message channel in `recon`.
fn tally(inner: &Inner, recon: &mut Recon, i: usize, flags: u8, limit: u32) {
    let received = (flags & RECV_MASK) >> RECV_SHIFT;
    let seq = i as u64;
    if received > 1 {
        recon.duplicates.push(seq);
    }
    if received > 0 {
        recon.received += 1;
    }
    if flags & ACKED == 0 {
        recon.unacknowledged += u64::from(received > 0);
        return;
    }
    recon.acked += 1;
    if received > 0 {
        return;
    }
    if inner.acked_ms.get(i).copied().unwrap_or(0) > limit {
        recon.pending += 1;
        return;
    }
    recon.missing.push(seq);
    if flags & DISCARDED != 0 {
        recon.discarded.push(seq);
    }
}

/// Counts record `i` of a bridge in `recon`.
fn tally_bridge(
    inner: &Inner,
    breaks: &[u32],
    recon: &mut BridgeRecon,
    i: usize,
    flags: u8,
    limit: u32,
) {
    if flags & SENT == 0 {
        return;
    }
    recon.sent += 1;
    if flags & RECV_MASK != 0 {
        recon.received += 1;
        return;
    }
    let sent = inner.sent_ms.get(i).copied().unwrap_or(0);
    if sent > limit {
        recon.pending += 1;
    } else if break_near(breaks, sent) {
        recon.lost_at_break += 1;
    } else {
        recon.lost_unexplained.push(i as u64);
    }
}

/// Whether a connection break lies within the window around `sent`.
fn break_near(sorted: &[u32], sent: u32) -> bool {
    let from = sent.saturating_sub(BREAK_BEFORE_MS);
    let first = sorted.partition_point(|b| *b < from);
    sorted
        .get(first)
        .is_some_and(|b| *b <= sent.saturating_add(BREAK_AFTER_MS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(ledger: &Ledger, acked_at: u64) -> u64 {
        let seq = ledger.alloc();
        ledger.sent(seq, acked_at.saturating_sub(5));
        ledger.acked(seq, acked_at);
        seq
    }

    #[test]
    fn a_message_received_once_reconciles() {
        let l = Ledger::default();
        let seq = message(&l, 1_000);
        assert_eq!(l.received(seq, 1_050), 1);
        let r = l.reconcile(100_000, 30_000);
        assert_eq!((r.acked, r.received), (1, 1));
        assert!(r.missing.is_empty() && r.duplicates.is_empty());
        assert_eq!(l.take_latencies(), vec![55]);
    }

    #[test]
    fn missing_and_duplicate_messages_are_found() {
        let l = Ledger::default();
        let lost = message(&l, 1_000);
        let twice = message(&l, 1_100);
        l.received(twice, 1_200);
        l.received(twice, 1_300);
        let r = l.reconcile(100_000, 30_000);
        assert_eq!(r.missing, vec![lost]);
        assert_eq!(r.duplicates, vec![twice]);
        assert!(r.discarded.is_empty());
    }

    #[test]
    fn a_discarded_message_is_marked_in_the_missing_list() {
        let l = Ledger::default();
        let seq = message(&l, 1_000);
        l.discarded(seq);
        let r = l.reconcile(100_000, 30_000);
        assert_eq!((r.missing, r.discarded), (vec![seq], vec![seq]));
    }

    #[test]
    fn recent_messages_are_pending_not_missing() {
        let l = Ledger::default();
        message(&l, 90_000);
        let r = l.reconcile(100_000, 30_000);
        assert_eq!((r.pending, r.missing.len()), (1, 0));
    }

    #[test]
    fn a_received_but_unacknowledged_message_is_not_an_error() {
        let l = Ledger::default();
        let seq = l.alloc();
        l.sent(seq, 10);
        l.received(seq, 20);
        let r = l.reconcile(100_000, 1);
        assert_eq!((r.unacknowledged, r.missing.len()), (1, 0));
    }

    #[test]
    fn bridge_losses_need_a_break_near_them() {
        let l = Ledger::default();
        for seq in 0..4 {
            let n = l.alloc();
            assert_eq!(n, seq);
            l.sent(n, 10_000 + seq * 1_000);
        }
        assert!(l.received_in_order(0, 10_050));
        assert!(l.received_in_order(3, 13_050));
        l.add_break(11_500);
        let r = l.reconcile_bridge(500_000, 30_000);
        assert_eq!((r.sent, r.received, r.lost_at_break), (4, 2, 2));
        assert!(r.lost_unexplained.is_empty());
    }

    #[test]
    fn an_unexplained_bridge_loss_is_reported() {
        let l = Ledger::default();
        let a = l.alloc();
        let b = l.alloc();
        l.sent(a, 10_000);
        l.sent(b, 500_000);
        l.add_break(400_000);
        l.received_in_order(b, 500_100);
        let r = l.reconcile_bridge(900_000, 30_000);
        assert_eq!(r.lost_unexplained, vec![a]);
    }

    #[test]
    fn reordering_is_counted() {
        let l = Ledger::default();
        for _ in 0..3 {
            let n = l.alloc();
            l.sent(n, 1);
        }
        assert!(l.received_in_order(2, 5));
        assert!(!l.received_in_order(1, 6));
        assert!(!l.received_in_order(2, 7));
        let c = l.counts();
        assert_eq!((c.out_of_order, c.duplicates), (2, 1));
    }
}
