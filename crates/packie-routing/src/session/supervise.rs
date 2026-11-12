//! The supervision loop of one bridge session: how pump ends, stops, drains, the close
//! deadline and newcomers at a bound listener decide the session's end.

use std::pin::pin;
use std::sync::atomic::Ordering;

use packie_core::BridgeSide;
use packie_transport::{Acceptor, Incoming, Operation, describe_io};
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::Instant as TokioInstant;

use super::{DISCARD_AFTER_DRAIN, PumpEnd, Pumps, READ_CHUNK, Reader, SessionContext, SessionEnd};
use crate::Failure;
use crate::listen::accept;
use crate::status::{ChannelShared, side_index};
use crate::stop::{StopMode, StopRx, sleep_until, stopped};

/// What the session loop does after a pump stopped.
enum Next {
    Continue,
    End(SessionEnd),
    Finish,
}

/// Bookkeeping of one session's supervision loop.
struct Watch<'a> {
    cx: &'a SessionContext,
    first: Option<Failure>,
    close_deadline: Option<TokioInstant>,
    drain: Option<TokioInstant>,
    drained_writers: u8,
    read_halves: Vec<Reader>,
}

impl Watch<'_> {
    fn ended(&mut self, failure: Failure) -> Next {
        let side = failure.side;
        Next::End(ended(self.first.take().or(Some(failure)), side))
    }

    fn on_pump(&mut self, joined: Result<PumpEnd, tokio::task::JoinError>) -> Next {
        let end = match joined {
            Ok(end) => end,
            Err(error) => {
                return self.ended(Failure::internal(
                    BridgeSide::A,
                    format!("bridge pump stopped: {error}"),
                ));
            }
        };
        let draining = self.drain.is_some();
        match end {
            PumpEnd::ReadEof(side) if !draining && self.first.is_none() => {
                self.first = Some(Failure::new(
                    side,
                    Operation::Read,
                    "peer closed the connection",
                ));
                self.close_deadline = Some(TokioInstant::now() + self.cx.close_drain);
                Next::Continue
            }
            PumpEnd::ReadEof(_) => Next::Continue,
            PumpEnd::ReadStopped(reader) => {
                self.read_halves.push(reader);
                Next::Continue
            }
            // A peer that drops during the drain is part of the stop.
            PumpEnd::ReadFailed(..) if draining => Next::Continue,
            PumpEnd::WriteFailed(..) if draining => Next::End(SessionEnd::Stopped),
            PumpEnd::ReadFailed(side, error) => {
                self.ended(Failure::new(side, Operation::Read, describe_io(&error)))
            }
            PumpEnd::WriteFailed(side, error) => {
                let failure = Failure::new(side, Operation::Write, describe_io(&error));
                log_write_failed(self.cx, &failure);
                self.ended(failure)
            }
            PumpEnd::WriteDrained => {
                self.drained_writers += 1;
                if draining && self.drained_writers == 2 {
                    Next::Finish
                } else if !draining && self.first.is_some() {
                    Next::End(ended(self.first.take(), BridgeSide::A))
                } else {
                    Next::Continue
                }
            }
        }
    }
}

pub(super) async fn supervise(
    cx: &SessionContext,
    pumps: &Pumps,
    set: &mut JoinSet<PumpEnd>,
    listeners: &mut [Option<Box<dyn Acceptor>>; 2],
    stop: &mut StopRx,
    stop_reads: &watch::Sender<bool>,
) -> SessionEnd {
    let mut watch = Watch {
        cx,
        first: None,
        close_deadline: None,
        drain: None,
        drained_writers: 0,
        read_halves: Vec::new(),
    };
    let mut stable_timer = pin!(tokio::time::sleep(cx.stable_after));
    let mut refuser = pin!(refuse_newcomers(&cx.shared, listeners));
    loop {
        let next = tokio::select! {
            biased;
            mode = stopped(stop), if watch.drain.is_none() => match mode {
                StopMode::Now => Next::End(SessionEnd::Stopped),
                StopMode::Drain(deadline) => {
                    let _ = stop_reads.send(true);
                    watch.drain = Some(deadline);
                    Next::Continue
                }
            },
            () = sleep_until(watch.drain), if watch.drain.is_some() => Next::End(SessionEnd::Stopped),
            () = sleep_until(watch.close_deadline), if watch.close_deadline.is_some() => {
                Next::End(ended(watch.first.take(), BridgeSide::A))
            }
            () = &mut stable_timer, if !pumps.stable.load(Ordering::Relaxed) => {
                pumps.mark_stable();
                Next::Continue
            }
            () = &mut refuser => Next::Continue,
            joined = set.join_next() => match joined {
                Some(joined) => watch.on_pump(joined),
                None if watch.drain.is_some() => Next::Finish,
                None => Next::End(ended(watch.first.take(), BridgeSide::A)),
            },
        };
        match next {
            Next::Continue => {}
            Next::End(end) => return end,
            Next::Finish => return finish_drain(std::mem::take(&mut watch.read_halves)).await,
        }
    }
}

fn ended(first: Option<Failure>, fallback: BridgeSide) -> SessionEnd {
    SessionEnd::Ended(first.unwrap_or_else(|| Failure::internal(fallback, "session ended")))
}

/// After a completed drain: closing with unread input would reset the connection and
/// could discard the tail of what was just delivered, so read and discard what the
/// peers still send, for up to one second.
async fn finish_drain(read_halves: Vec<Reader>) -> SessionEnd {
    let mut readers = JoinSet::new();
    for reader in read_halves {
        readers.spawn(discard(reader));
    }
    let all = async { while readers.join_next().await.is_some() {} };
    let _ = tokio::time::timeout(DISCARD_AFTER_DRAIN, all).await;
    SessionEnd::Stopped
}

async fn discard(mut reader: Reader) {
    let mut sink = vec![0_u8; READ_CHUNK];
    while matches!(reader.read(&mut sink).await, Ok(n) if n > 0) {}
}

fn log_write_failed(cx: &SessionContext, failure: &Failure) {
    let shared = &cx.shared;
    let Some(cell) = shared.side(failure.side) else {
        return;
    };
    let endpoint_id = cell.endpoint_id.to_string();
    let role = failure.side.as_str();
    let summary = failure.summary(shared.id, cell);
    shared.span.in_scope(|| {
        tracing::warn!(
            event = "endpoint.write_failed",
            endpoint_id,
            endpoint_role = role,
            endpoint_kind = cell.kind.as_str(),
            error_summary = summary,
            "Channel {} {role} endpoint {endpoint_id}: write failed",
            shared.id
        );
    });
}

/// Refuses every peer that reaches a bound listener while the session is active. A
/// listener whose socket fails is dropped and rebound by the next attempt. Never ends.
async fn refuse_newcomers(shared: &ChannelShared, listeners: &mut [Option<Box<dyn Acceptor>>; 2]) {
    loop {
        let [a, b] = &mut *listeners;
        let (side, result) = match (a.as_mut(), b.as_mut()) {
            (Some(a), Some(b)) => tokio::select! {
                r = accept(a, shared.side(BridgeSide::A)) => (BridgeSide::A, r),
                r = accept(b, shared.side(BridgeSide::B)) => (BridgeSide::B, r),
            },
            (Some(a), None) => (BridgeSide::A, accept(a, shared.side(BridgeSide::A)).await),
            (None, Some(b)) => (BridgeSide::B, accept(b, shared.side(BridgeSide::B)).await),
            (None, None) => std::future::pending().await,
        };
        match result {
            Ok(incoming) => refuse(shared, side, incoming),
            Err(_) => listeners[side_index(side)] = None,
        }
    }
}

/// Resets a peer that reached a listener while the session is active and logs
/// `listener.peer_rejected` (transports that enforce `max_connections` themselves never
/// hand one over).
fn refuse(shared: &ChannelShared, side: BridgeSide, incoming: Incoming) {
    let peer = incoming.link.peer.clone().unwrap_or_default();
    incoming.link.stream.reset();
    let Some(cell) = shared.side(side) else {
        return;
    };
    cell.count_refusal(peer.clone(), "session_active");
    let endpoint_id = cell.endpoint_id.to_string();
    let role = side.as_str();
    shared.span.in_scope(|| {
        tracing::warn!(
            event = "listener.peer_rejected",
            endpoint_id,
            endpoint_role = role,
            endpoint_kind = cell.kind.as_str(),
            peer,
            error_code = "session_active",
            "Channel {} {role} endpoint {endpoint_id}: refused peer {peer} (session_active)",
            shared.id
        );
    });
}
