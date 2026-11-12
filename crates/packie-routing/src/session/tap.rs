//! The optional tap of a bridge session: a protocol framer per direction observes copies
//! of the forwarded bytes, counts messages and hands them to the tap sink
//! (`docs/spec/bridge-channels.md` §9). It never holds up the bridge: when its queue is
//! full, observations are dropped and counted.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use packie_config::{Tap, TapFramer};
use packie_core::{BridgeSide, ProtocolKind};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::SessionContext;
use crate::framer::{Framer, Observation, framer_for};
use crate::status::{ChannelShared, side_index};
use crate::{TapObservation, TapSink};

/// Copies queued for a tap framer before observations are dropped.
const TAP_QUEUE: usize = 256;

/// A configured tap (`tap` of a bridge channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TapSettings {
    pub(crate) protocol: ProtocolKind,
    pub(crate) max_message_bytes: usize,
    /// Observed messages go to the tap sink.
    pub(crate) record: bool,
}

impl TapSettings {
    /// The settings of `tap`, or `None` without a framer.
    pub(crate) fn of(tap: &Tap) -> Option<Self> {
        let protocol = match tap.framer {
            TapFramer::None => return None,
            TapFramer::Astm => ProtocolKind::Astm,
            TapFramer::Hl7Mllp => ProtocolKind::Hl7,
            TapFramer::Au => ProtocolKind::Au,
        };
        Some(Self {
            protocol,
            max_message_bytes: usize::try_from(tap.max_message_bytes).unwrap_or(usize::MAX),
            record: tap.record_messages,
        })
    }
}

pub(super) fn start_taps(
    cx: &SessionContext,
    aux: &mut JoinSet<()>,
) -> [Option<mpsc::Sender<Vec<u8>>>; 2] {
    let mut taps = [None, None];
    let Some(tap) = cx.tap else {
        return taps;
    };
    let sink = cx.tap_sink.clone().filter(|_| tap.record);
    for from in [BridgeSide::A, BridgeSide::B] {
        let Some(framer) = framer_for(tap.protocol, tap.max_message_bytes) else {
            continue;
        };
        let (tx, rx) = mpsc::channel(TAP_QUEUE);
        taps[side_index(from)] = Some(tx);
        let task = TapTask {
            shared: Arc::clone(&cx.shared),
            sink: sink.clone(),
            from,
            protocol: tap.protocol,
        };
        aux.spawn(run_tap(task, framer, rx));
    }
    taps
}

/// One direction's tap: where its observations go.
struct TapTask {
    shared: Arc<ChannelShared>,
    sink: Option<Arc<dyn TapSink>>,
    from: BridgeSide,
    protocol: ProtocolKind,
}

async fn run_tap(task: TapTask, mut framer: Box<dyn Framer>, mut rx: mpsc::Receiver<Vec<u8>>) {
    let mut observations = Vec::new();
    while let Some(segment) = rx.recv().await {
        framer.feed(&segment, &mut observations);
        for observation in observations.drain(..) {
            observe(&task, observation);
        }
    }
}

fn observe(task: &TapTask, observation: Observation) {
    let (shared, from) = (&task.shared, task.from);
    let Some(tap) = &shared.tap else {
        return;
    };
    match observation {
        Observation::Message(bytes) => {
            if let Some(count) = tap.messages.get(side_index(from)) {
                count.fetch_add(1, Ordering::Relaxed);
            }
            match from {
                BridgeSide::A => shared.counters.message_in(),
                BridgeSide::B => shared.counters.message_out(),
            }
            let (Some(sink), Some(cell)) = (&task.sink, shared.side(from)) else {
                return;
            };
            sink.observed(TapObservation {
                channel_id: shared.id,
                channel_label: shared.definition().label().to_owned(),
                from,
                endpoint_id: cell.endpoint_id,
                protocol: task.protocol,
                config_version: shared.config_version,
                message: bytes,
            });
        }
        Observation::FramingError => {
            tap.framing_errors.fetch_add(1, Ordering::Relaxed);
        }
    }
}
