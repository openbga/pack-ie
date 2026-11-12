//! The traffic tap: live, bounded, opt-in copies of channel traffic for one local viewer
//! per channel (`docs/spec/operator-console.md` §7).
//!
//! Nothing is copied unless a viewer is subscribed to the channel, so with no viewer the
//! hot path pays one map lookup. A second viewer of the same channel is refused
//! ([`TapBusy`]). Segments wait for the viewer in one ring per direction of at most
//! [`RING_SEGMENTS`] segments or [`RING_BYTES`] bytes, oldest dropped and reported as
//! missed. Segments are never persisted or logged.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use packie_core::{ChannelId, ConnectorId, Direction, EndpointId, Timestamp};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use ts_rs::TS;

/// Most segments waiting per direction.
const RING_SEGMENTS: usize = 1000;
/// Most bytes waiting per direction (1 MiB).
const RING_BYTES: usize = 1024 * 1024;

/// What a segment is in the conversation. A message channel's source side carries
/// `received` and `response` (the engine's acknowledgements to the source), its destination
/// side `sent` and `reply` (what the destination answered). Bridge segments are `received`
/// (inbound) or `sent` (outbound).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TapRole {
    /// Came in from a source (or a bridge side).
    Received,
    /// The engine's answer to a source.
    Response,
    /// Went out to a destination (or a bridge side).
    Sent,
    /// A destination's answer.
    Reply,
}

impl TapRole {
    /// Relative to the engine.
    #[must_use]
    pub const fn direction(self) -> Direction {
        match self {
            Self::Received | Self::Reply => Direction::Inbound,
            Self::Response | Self::Sent => Direction::Outbound,
        }
    }
}

/// One captured segment of traffic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TapSegment {
    /// The channel.
    pub channel_id: ChannelId,
    /// The endpoint the bytes came from or went to.
    pub endpoint_id: EndpointId,
    /// Relative to the engine.
    pub direction: Direction,
    /// Its place in the conversation.
    pub role: TapRole,
    /// The destination the segment belongs to (`sent`, `reply`); null on the source side
    /// and on bridges.
    pub destination_id: Option<ConnectorId>,
    /// When the segment was captured.
    pub at: Timestamp,
    /// The bytes.
    pub bytes: Vec<u8>,
}

/// The channel already has a viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Transcript in use by another client.")]
pub struct TapBusy;

/// Why [`TapViewer::recv`] returned no segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TapRecvError {
    /// This many segments were dropped from a full ring since the last call.
    #[error("{0} segments were dropped")]
    Lagged(u64),
    /// The channel's tap was removed.
    #[error("the tap was removed")]
    Closed,
}

/// Segments waiting for a channel's viewer, in order of capture.
#[derive(Debug, Default)]
struct Ring {
    segments: VecDeque<(u64, TapSegment)>,
    bytes: usize,
}

impl Ring {
    /// Adds `segment`; returns how many old segments were dropped to make room.
    fn push(&mut self, seq: u64, segment: TapSegment) -> u64 {
        self.bytes += segment.bytes.len();
        self.segments.push_back((seq, segment));
        let mut dropped = 0;
        while self.segments.len() > RING_SEGMENTS
            || (self.bytes > RING_BYTES && self.segments.len() > 1)
        {
            if let Some((_, old)) = self.segments.pop_front() {
                self.bytes -= old.bytes.len();
                dropped += 1;
            }
        }
        dropped
    }

    fn front_seq(&self) -> Option<u64> {
        self.segments.front().map(|(seq, _)| *seq)
    }

    fn pop(&mut self) -> Option<TapSegment> {
        let (_, segment) = self.segments.pop_front()?;
        self.bytes -= segment.bytes.len();
        Some(segment)
    }
}

/// One channel's viewer slot.
#[derive(Debug)]
struct Watch {
    viewer: u64,
    next_seq: u64,
    inbound: Ring,
    outbound: Ring,
    missed: u64,
    notify: Arc<Notify>,
}

impl Watch {
    /// The oldest waiting segment of either direction.
    fn pop(&mut self) -> Option<TapSegment> {
        match (self.inbound.front_seq(), self.outbound.front_seq()) {
            (Some(a), Some(b)) if b < a => self.outbound.pop(),
            (Some(_), _) => self.inbound.pop(),
            (None, _) => self.outbound.pop(),
        }
    }
}

#[derive(Debug, Default)]
struct Shared {
    watches: HashMap<ChannelId, Watch>,
    next_viewer: u64,
}

/// Per-channel tap registry. Cheap to clone; clones share state.
#[derive(Debug, Clone, Default)]
pub struct TrafficTap {
    shared: Arc<Mutex<Shared>>,
}

impl TrafficTap {
    /// An empty tap.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts copying `channel`'s traffic to the returned viewer until it is dropped.
    ///
    /// # Errors
    /// [`TapBusy`] while another viewer watches the channel.
    pub fn subscribe(&self, channel: ChannelId) -> Result<TapViewer, TapBusy> {
        let mut shared = self.lock();
        if shared.watches.contains_key(&channel) {
            return Err(TapBusy);
        }
        shared.next_viewer += 1;
        let viewer = shared.next_viewer;
        let notify = Arc::new(Notify::new());
        shared.watches.insert(
            channel,
            Watch {
                viewer,
                next_seq: 0,
                inbound: Ring::default(),
                outbound: Ring::default(),
                missed: 0,
                notify: Arc::clone(&notify),
            },
        );
        Ok(TapViewer {
            tap: self.clone(),
            channel,
            viewer,
            notify,
        })
    }

    /// Whether anyone is watching `channel`.
    #[must_use]
    pub fn is_watched(&self, channel: ChannelId) -> bool {
        self.lock().watches.contains_key(&channel)
    }

    /// Records a bridge's `bytes` if `channel` is watched; otherwise does nothing.
    pub fn publish(
        &self,
        channel: ChannelId,
        endpoint: EndpointId,
        direction: Direction,
        bytes: &[u8],
    ) {
        let role = match direction {
            Direction::Inbound => TapRole::Received,
            Direction::Outbound => TapRole::Sent,
        };
        self.publish_message(channel, endpoint, role, None, bytes);
    }

    /// Records a message channel's `bytes` in `role`, for `destination` on the destination
    /// side, if `channel` is watched; otherwise does nothing.
    pub fn publish_message(
        &self,
        channel: ChannelId,
        endpoint: EndpointId,
        role: TapRole,
        destination: Option<ConnectorId>,
        bytes: &[u8],
    ) {
        let mut shared = self.lock();
        let Some(watch) = shared.watches.get_mut(&channel) else {
            return;
        };
        let segment = TapSegment {
            channel_id: channel,
            endpoint_id: endpoint,
            direction: role.direction(),
            role,
            destination_id: destination,
            at: Timestamp::now(),
            bytes: bytes.to_vec(),
        };
        let seq = watch.next_seq;
        watch.next_seq += 1;
        let ring = match segment.direction {
            Direction::Inbound => &mut watch.inbound,
            Direction::Outbound => &mut watch.outbound,
        };
        watch.missed += ring.push(seq, segment);
        watch.notify.notify_one();
    }

    /// Drops `channel`'s tap (for example when the channel is removed); its viewer ends.
    pub fn remove(&self, channel: ChannelId) {
        if let Some(watch) = self.lock().watches.remove(&channel) {
            watch.notify.notify_one();
        }
    }
}

/// The one viewer of a channel's traffic. Dropping it stops the copying.
#[derive(Debug)]
pub struct TapViewer {
    tap: TrafficTap,
    channel: ChannelId,
    viewer: u64,
    notify: Arc<Notify>,
}

impl TapViewer {
    /// The next segment, oldest first across both directions.
    ///
    /// # Errors
    /// [`TapRecvError::Lagged`] once after segments were dropped from a full ring (the
    /// following calls continue with what is left); [`TapRecvError::Closed`] when the tap
    /// was removed.
    pub async fn recv(&mut self) -> Result<TapSegment, TapRecvError> {
        loop {
            if let Some(result) = self.try_recv() {
                return result;
            }
            self.notify.notified().await;
        }
    }

    /// What [`recv`](Self::recv) would return now, or `None` when nothing waits.
    pub fn try_recv(&mut self) -> Option<Result<TapSegment, TapRecvError>> {
        let mut shared = self.tap.lock();
        let Some(watch) = shared
            .watches
            .get_mut(&self.channel)
            .filter(|w| w.viewer == self.viewer)
        else {
            return Some(Err(TapRecvError::Closed));
        };
        if watch.missed > 0 {
            return Some(Err(TapRecvError::Lagged(std::mem::take(&mut watch.missed))));
        }
        watch.pop().map(Ok)
    }
}

impl Drop for TapViewer {
    fn drop(&mut self) {
        let mut shared = self.tap.lock();
        if shared
            .watches
            .get(&self.channel)
            .is_some_and(|w| w.viewer == self.viewer)
        {
            shared.watches.remove(&self.channel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> (ChannelId, EndpointId) {
        (ChannelId::generate(), EndpointId::generate())
    }

    #[test]
    fn copies_only_while_watched() {
        let tap = TrafficTap::new();
        let (channel, endpoint) = ids();
        tap.publish(channel, endpoint, Direction::Inbound, b"ignored");
        assert!(!tap.is_watched(channel));

        let mut viewer = tap.subscribe(channel).unwrap();
        assert!(tap.is_watched(channel));
        assert!(viewer.try_recv().is_none());
        tap.publish(channel, endpoint, Direction::Inbound, b"\x02hello\x03");
        let segment = viewer.try_recv().unwrap().unwrap();
        assert_eq!(segment.bytes, b"\x02hello\x03");
        assert_eq!(segment.direction, Direction::Inbound);
        assert_eq!(segment.role, TapRole::Received);
        assert_eq!(segment.destination_id, None);

        let lis = ConnectorId::new("lis").unwrap();
        tap.publish_message(channel, endpoint, TapRole::Reply, Some(lis), b"\x06");
        let reply = viewer.try_recv().unwrap().unwrap();
        assert_eq!(
            (reply.role, reply.direction),
            (TapRole::Reply, Direction::Inbound)
        );
        assert_eq!(reply.destination_id, Some(lis));

        drop(viewer);
        tap.publish(channel, endpoint, Direction::Outbound, b"x");
        assert!(!tap.is_watched(channel));
    }

    #[test]
    fn one_viewer_per_channel() {
        let tap = TrafficTap::new();
        let (channel, _) = ids();
        let first = tap.subscribe(channel).unwrap();
        assert_eq!(tap.subscribe(channel).unwrap_err(), TapBusy);
        assert!(tap.subscribe(ChannelId::generate()).is_ok());
        drop(first);
        assert!(tap.subscribe(channel).is_ok());
    }

    #[test]
    fn rings_keep_the_newest_and_report_what_was_dropped() {
        let tap = TrafficTap::new();
        let (channel, endpoint) = ids();
        let mut viewer = tap.subscribe(channel).unwrap();
        // Segment cap: 1000 per direction.
        for i in 0..RING_SEGMENTS + 5 {
            tap.publish(
                channel,
                endpoint,
                Direction::Inbound,
                &[u8::try_from(i % 256).unwrap()],
            );
        }
        tap.publish(channel, endpoint, Direction::Outbound, b"out");
        assert_eq!(viewer.try_recv(), Some(Err(TapRecvError::Lagged(5))));
        assert_eq!(viewer.try_recv().unwrap().unwrap().bytes, [5]);
        let mut rest = 1;
        while let Some(Ok(_)) = viewer.try_recv() {
            rest += 1;
        }
        assert_eq!(
            rest,
            RING_SEGMENTS + 1,
            "999 inbound left plus the outbound one"
        );

        // Byte cap: 1 MiB per direction.
        let chunk = vec![0_u8; 256 * 1024];
        for _ in 0..6 {
            tap.publish(channel, endpoint, Direction::Outbound, &chunk);
        }
        assert_eq!(viewer.try_recv(), Some(Err(TapRecvError::Lagged(2))));
        let mut kept = 0;
        while let Some(Ok(_)) = viewer.try_recv() {
            kept += 1;
        }
        assert_eq!(kept, 4);
    }

    #[test]
    fn removing_the_tap_ends_the_viewer() {
        let tap = TrafficTap::new();
        let (channel, _) = ids();
        let mut viewer = tap.subscribe(channel).unwrap();
        tap.remove(channel);
        assert_eq!(viewer.try_recv(), Some(Err(TapRecvError::Closed)));
        // A new viewer may watch again; the old one does not take its slot on drop.
        let _second = tap.subscribe(channel).unwrap();
        drop(viewer);
        assert!(tap.is_watched(channel));
    }
}
