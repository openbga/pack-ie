//! One bridged session: `A reader → A→B buffer → B writer` and `B reader → B→A buffer →
//! A writer` (`docs/spec/bridge-channels.md` §3, §6, §8, §14).
//!
//! Each pump is its own task, so a panic in one is caught and ends only this session.
//! The first pump that stops decides why the session ended. A clean end of stream on
//! one side first delivers what that side already sent (bounded by the block timeout),
//! then closes the other side. A read or write failure closes both sides at once. A
//! drain stop (engine shutdown) stops reading, delivers the buffers within the budget,
//! shuts down sending and reads what the peers still send for up to one second.
//! The supervision loop lives in `supervise`, the optional tap in `tap`.

mod supervise;
mod tap;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use packie_core::{BridgeSide, Direction};
use packie_diagnostics::{Counters, TrafficTap};
use packie_transport::{Acceptor, ByteStream, Link};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tracing::Instrument;

use crate::buffer::FlowBuffer;
use crate::status::{ChannelShared, Role, SideCell, side_index};
use crate::stop::StopRx;
use crate::{Failure, TapSink};
use supervise::supervise;
use tap::start_taps;

pub(crate) use tap::TapSettings;

/// Bytes read from a transport per segment.
const READ_CHUNK: usize = 16 * 1024;
/// How long a drained connection's remaining input is read and discarded.
const DISCARD_AFTER_DRAIN: Duration = Duration::from_secs(1);

type Reader = ReadHalf<Box<dyn ByteStream>>;
type Writer = WriteHalf<Box<dyn ByteStream>>;

/// Everything a session needs from its channel instance.
pub(crate) struct SessionContext {
    pub(crate) shared: Arc<ChannelShared>,
    /// Endpoint counters of A and B (since the engine started).
    pub(crate) endpoint_counters: [Arc<Counters>; 2],
    pub(crate) traffic_tap: TrafficTap,
    /// The tap, when a framer is configured.
    pub(crate) tap: Option<TapSettings>,
    pub(crate) tap_sink: Option<Arc<dyn TapSink>>,
    /// The session counts as stable after this long without failing.
    pub(crate) stable_after: Duration,
    /// Budget for delivering what a closing side already sent.
    pub(crate) close_drain: Duration,
}

/// How a session ended.
#[derive(Debug)]
pub(crate) enum SessionEnd {
    /// One side ended or failed; the other was closed.
    Ended(Failure),
    /// The channel is stopping.
    Stopped,
}

enum PumpEnd {
    ReadEof(BridgeSide),
    ReadStopped(Reader),
    ReadFailed(BridgeSide, std::io::Error),
    WriteDrained,
    WriteFailed(BridgeSide, std::io::Error),
}

/// Shared by the pumps of one session.
struct Pumps {
    cx: Arc<SessionContext>,
    stable: AtomicBool,
    taps: [Option<mpsc::Sender<Vec<u8>>>; 2],
}

impl Pumps {
    fn shared(&self) -> &ChannelShared {
        &self.cx.shared
    }

    fn counters(&self, side: BridgeSide) -> &Counters {
        &self.cx.endpoint_counters[side_index(side)]
    }

    /// A segment read from `from` entered its buffer.
    fn accepted(&self, from: BridgeSide, segment: Option<Vec<u8>>, len: usize) {
        let shared = self.shared();
        match from {
            BridgeSide::A => shared.counters.bytes_in(len),
            BridgeSide::B => shared.counters.bytes_out(len),
        }
        let Some(segment) = segment else {
            return;
        };
        if let Some(cell) = shared.side(from) {
            self.cx
                .traffic_tap
                .publish(shared.id, cell.endpoint_id, Direction::Inbound, &segment);
        }
        if let Some(tap) = &self.taps[side_index(from)]
            && tap.try_send(segment).is_err()
        {
            self.tap_dropped(from);
        }
    }

    fn tap_dropped(&self, from: BridgeSide) {
        let Some(tap) = &self.shared().tap else {
            return;
        };
        let previous = tap.dropped.fetch_add(1, Ordering::Relaxed);
        // Rate limit: log the first drop and every 1000th after it.
        if previous % 1000 == 0 {
            self.shared().span.in_scope(|| {
                tracing::warn!(
                    event = "tap.dropped",
                    direction = direction_name(from),
                    "Channel {} tap fell behind and skipped observations",
                    self.shared().id
                );
            });
        }
    }

    /// Bytes were written to `to`.
    fn written(&self, to: BridgeSide, len: usize) {
        self.counters(to).bytes_out(len);
        if let Some(cell) = self.shared().side(to) {
            cell.touch();
        }
        self.mark_stable();
    }

    /// The session proved stable: the first forwarded write or enough time up.
    fn mark_stable(&self) {
        if self.stable.swap(true, Ordering::Relaxed) {
            return;
        }
        let shared = self.shared();
        for role in shared.session_stable() {
            if let Role::Side(side) = role {
                self.counters(side).clear_error();
            }
            if let Some(cell) = shared.cell(role) {
                log_recovered(shared, cell);
            }
        }
    }

    fn overflow_detected(&self, from: BridgeSide, flow: &FlowBuffer) {
        let gauge = flow.gauge();
        let shared = self.shared();
        shared.span.in_scope(|| {
            tracing::warn!(
                event = "buffer.overflow_detected",
                direction = direction_name(from),
                bytes = gauge.bytes,
                segment_count = gauge.segments,
                "Channel {} buffer {} is full; the sender is held back",
                shared.id,
                direction_name(from)
            );
        });
        shared.refresh();
    }

    fn overflow_cleared(&self, from: BridgeSide, flow: &FlowBuffer) {
        let gauge = flow.gauge();
        let shared = self.shared();
        shared.span.in_scope(|| {
            tracing::info!(
                event = "buffer.overflow_cleared",
                direction = direction_name(from),
                bytes = gauge.bytes,
                segment_count = gauge.segments,
                "Channel {} buffer {} drained",
                shared.id,
                direction_name(from)
            );
        });
        shared.refresh();
    }
}

/// `a_to_b` or `b_to_a` for the direction starting at `from`.
pub(crate) const fn direction_name(from: BridgeSide) -> &'static str {
    match from {
        BridgeSide::A => "a_to_b",
        BridgeSide::B => "b_to_a",
    }
}

/// Runs the session over `links` (A, B) until it ends or the channel stops.
pub(crate) async fn run(
    cx: Arc<SessionContext>,
    links: [Link; 2],
    listeners: &mut [Option<Box<dyn Acceptor>>; 2],
    stop: &mut StopRx,
) -> SessionEnd {
    let mut aux = JoinSet::new();
    let taps = start_taps(&cx, &mut aux);
    let pumps = Arc::new(Pumps {
        cx: Arc::clone(&cx),
        stable: AtomicBool::new(false),
        taps,
    });
    let (stop_reads, reads_stopped) = watch::channel(false);
    let mut set = JoinSet::new();
    let [link_a, link_b] = links;
    let (read_a, write_a) = tokio::io::split(link_a.stream);
    let (read_b, write_b) = tokio::io::split(link_b.stream);
    let span = tracing::Span::current();
    for (from, reader, writer) in [
        (BridgeSide::A, read_a, write_b),
        (BridgeSide::B, read_b, write_a),
    ] {
        let Some(flow) = cx.shared.flow(from).cloned() else {
            return SessionEnd::Ended(Failure::internal(from, "missing buffer"));
        };
        set.spawn(
            read_pump(
                from,
                reader,
                Arc::clone(&flow),
                reads_stopped.clone(),
                Arc::clone(&pumps),
            )
            .instrument(span.clone()),
        );
        set.spawn(
            write_pump(from.opposite(), writer, flow, Arc::clone(&pumps)).instrument(span.clone()),
        );
    }
    let end = supervise(&cx, &pumps, &mut set, listeners, stop, &stop_reads).await;
    // Both connections are closed before the channel reports what happened next.
    set.abort_all();
    aux.abort_all();
    while set.join_next().await.is_some() {}
    while aux.join_next().await.is_some() {}
    end
}

fn log_recovered(shared: &ChannelShared, cell: &SideCell) {
    let endpoint_id = cell.endpoint_id.to_string();
    let role = cell.role.as_str();
    shared.span.in_scope(|| {
        tracing::info!(
            event = "endpoint.retry_succeeded",
            endpoint_id,
            endpoint_role = role,
            endpoint_kind = cell.kind.as_str(),
            "Channel {} {role} endpoint {endpoint_id}: recovered",
            shared.id
        );
    });
}

async fn read_pump(
    from: BridgeSide,
    mut reader: Reader,
    flow: Arc<FlowBuffer>,
    mut stop_reads: watch::Receiver<bool>,
    pumps: Arc<Pumps>,
) -> PumpEnd {
    let mut chunk = vec![0_u8; READ_CHUNK];
    loop {
        let read = tokio::select! {
            biased;
            _ = stop_reads.wait_for(|stop| *stop) => {
                flow.close();
                return PumpEnd::ReadStopped(reader);
            }
            read = reader.read(&mut chunk) => read,
        };
        let n = match read {
            Ok(0) => {
                flow.close();
                return PumpEnd::ReadEof(from);
            }
            Ok(n) => n,
            Err(error) => return PumpEnd::ReadFailed(from, error),
        };
        let Some(data) = chunk.get(..n) else {
            return PumpEnd::ReadFailed(from, std::io::Error::other("read past the buffer"));
        };
        let segment = data.to_vec();
        pumps.counters(from).bytes_in(n);
        if let Some(cell) = pumps.shared().side(from) {
            cell.touch();
        }
        let copy = needs_copy(&pumps, from).then(|| segment.clone());
        let overflow = || pumps.overflow_detected(from, &flow);
        if flow.push(segment, overflow).await.is_err() {
            // Closed by a stop; what this reader held is part of the stop.
            return PumpEnd::ReadStopped(reader);
        }
        pumps.accepted(from, copy, n);
    }
}

fn needs_copy(pumps: &Pumps, from: BridgeSide) -> bool {
    pumps.taps[side_index(from)].is_some() || pumps.cx.traffic_tap.is_watched(pumps.shared().id)
}

async fn write_pump(
    to: BridgeSide,
    mut writer: Writer,
    flow: Arc<FlowBuffer>,
    pumps: Arc<Pumps>,
) -> PumpEnd {
    while let Some((segment, cleared)) = flow.pop().await {
        if cleared {
            pumps.overflow_cleared(to.opposite(), &flow);
        }
        if let Err(error) = writer.write_all(&segment).await {
            return PumpEnd::WriteFailed(to, error);
        }
        if let Err(error) = writer.flush().await {
            return PumpEnd::WriteFailed(to, error);
        }
        pumps.written(to, segment.len());
    }
    // The source side ended or the channel drains: everything was delivered.
    let _ = writer.shutdown().await;
    PumpEnd::WriteDrained
}
