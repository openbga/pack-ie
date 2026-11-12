//! The scripted peers of the soak: instruments that send at a steady rate, and the systems
//! that receive. They behave like real devices: a connection is kept open, a message is
//! sent again when it was not acknowledged, and a peer that was cut off reconnects.
//!
//! | Module | Peers |
//! | --- | --- |
//! | [`bridge`] | The instrument and the sink of a bridge. |
//! | [`hl7`] | The HL7 instrument and the HL7 system that acknowledges. |
//! | [`astm`] | The ASTM instrument. |
//! | [`folder`] | The writer and the reader of the folder channel. |

pub(crate) mod astm;
pub(crate) mod bridge;
pub(crate) mod folder;
pub(crate) mod hl7;

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::chan::{ChannelRt, Env, Progress};
use super::topology::Kind;
use super::util::{Rng, Stop};

/// How far behind schedule a peer may fall before it stops catching up.
const MAX_CATCH_UP: Duration = Duration::from_secs(2);

/// Paces a peer at a mean interval with +-20% jitter.
#[derive(Debug)]
pub(crate) struct Pacer {
    next: Instant,
    mean_ms: u64,
    /// The least time between two messages, however far behind the pacer is.
    min_gap: Duration,
    last: Option<Instant>,
}

impl Pacer {
    /// The first message is due after a random part of one interval, so channels do not
    /// all send at the same instant.
    pub(crate) fn new(mean_ms: u64, rng: &mut Rng) -> Self {
        Self {
            next: Instant::now() + Duration::from_millis(rng.below(mean_ms.max(1))),
            mean_ms,
            min_gap: Duration::ZERO,
            last: None,
        }
    }

    /// Never sends two messages closer together than `gap`: an instrument that is behind
    /// does not send back to back.
    pub(crate) fn with_min_gap(mut self, gap: Duration) -> Self {
        self.min_gap = gap;
        self
    }

    /// Waits until the next message is due; `false` when the run is ending.
    pub(crate) async fn wait(&mut self, stop: &Stop, rng: &mut Rng) -> bool {
        let now = Instant::now();
        let earliest = self.last.map_or(now, |last| last + self.min_gap);
        let due = self.next.max(earliest);
        if due > now {
            if !stop.sleep(due - now).await {
                return false;
            }
        } else if now - self.next > MAX_CATCH_UP {
            self.next = now;
        }
        self.last = Some(Instant::now());
        self.next += Duration::from_millis(self.mean_ms * rng.between(80, 120) / 100);
        !stop.is_set()
    }

    /// The message that was due has been delivered (or given up): the minimum gap counts
    /// from now, so an instrument that was held up does not send the next message at once.
    pub(crate) fn finished(&mut self) {
        self.last = Some(Instant::now());
    }
}

/// Opens a connection to `port`, retrying until it works; `None` when the run is ending.
pub(crate) async fn connect(env: &Env, ch: &ChannelRt, port: u16) -> Option<TcpStream> {
    let mut delay = Duration::from_millis(250);
    while !env.stop.is_set() {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)).await {
            let _ = stream.set_nodelay(true);
            Progress::add(&ch.prog.connects, 1);
            return Some(stream);
        }
        if !env.stop.sleep(delay).await {
            return None;
        }
        delay = (delay * 2).min(Duration::from_secs(2));
    }
    None
}

/// Drops `stream`: with a reset when asked to, otherwise with an orderly close.
pub(crate) async fn drop_link(mut stream: TcpStream, reset: bool) {
    if reset {
        let _ = stream.set_zero_linger();
    } else {
        let _ = tokio::io::AsyncWriteExt::shutdown(&mut stream).await;
    }
}

fn bind(port: u16) -> std::io::Result<TcpListener> {
    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(SocketAddr::from(([127, 0, 0, 1], port)))?;
    socket.listen(1024)
}

/// Listens again on `port` after the peer was down.
async fn rebind(env: &Env, port: u16) -> Option<TcpListener> {
    for _ in 0..100 {
        if let Ok(listener) = bind(port) {
            return Some(listener);
        }
        if !env.halt.sleep(Duration::from_millis(200)).await {
            return None;
        }
    }
    None
}

/// Serves the receiving side of a channel: accepts connections and hands each to
/// `handler`. While [`super::chan::Control::down`] is set the port is closed and every
/// connection is dropped, like a system that is switched off.
pub(crate) async fn serve<F, Fut>(
    env: Arc<Env>,
    ch: Arc<ChannelRt>,
    listener: TcpListener,
    handler: F,
) where
    F: Fn(Arc<Env>, Arc<ChannelRt>, TcpStream) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut listener = Some(listener);
    let mut connections = JoinSet::new();
    while !env.halt.is_set() {
        if ch.ctl.is_down() {
            drop(listener.take());
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            while ch.ctl.is_down() && env.halt.sleep(Duration::from_millis(200)).await {}
            listener = rebind(&env, ch.plan.out_port).await;
            continue;
        }
        let Some(open) = listener.as_ref() else {
            return;
        };
        tokio::select! {
            accepted = open.accept() => {
                if let Ok((stream, _)) = accepted {
                    let _ = stream.set_nodelay(true);
                    Progress::add(&ch.prog.accepts, 1);
                    connections.spawn(handler(Arc::clone(&env), Arc::clone(&ch), stream));
                }
            }
            () = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
        while connections.try_join_next().is_some() {}
    }
}

/// Waits while the receiving side of `ch` is stalled; `false` when the run is ending.
pub(crate) async fn while_stalled(env: &Env, ch: &ChannelRt) -> bool {
    while ch.ctl.stalled(env.now_ms()) {
        if !env.halt.sleep(Duration::from_millis(100)).await {
            return false;
        }
    }
    !env.halt.is_set()
}

/// Starts every peer of `channels` on `set`; `listeners` holds the receiving ports.
pub(crate) fn spawn_all(
    env: &Arc<Env>,
    channels: &[Arc<ChannelRt>],
    mut listeners: std::collections::BTreeMap<String, TcpListener>,
    set: &mut JoinSet<()>,
) {
    for ch in channels {
        let listener = listeners.remove(&ch.plan.id);
        let (env, ch) = (Arc::clone(env), Arc::clone(ch));
        match (ch.plan.kind, listener) {
            (Kind::Bridge | Kind::TapBridge, Some(listener)) => {
                set.spawn(bridge::instrument(Arc::clone(&env), Arc::clone(&ch)));
                set.spawn(serve(env, ch, listener, bridge::sink));
            }
            (Kind::Hl7, Some(listener)) => {
                set.spawn(hl7::source(Arc::clone(&env), Arc::clone(&ch)));
                set.spawn(serve(env, ch, listener, hl7::destination));
            }
            (Kind::Astm, Some(listener)) => {
                set.spawn(astm::source(Arc::clone(&env), Arc::clone(&ch)));
                set.spawn(serve(env, ch, listener, hl7::destination));
            }
            (Kind::Folder, _) => {
                set.spawn(folder::writer(Arc::clone(&env), Arc::clone(&ch)));
                set.spawn(folder::reader(env, ch));
            }
            (_, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_pacer_keeps_its_rate_and_does_not_burst() {
        let stop = Stop::new();
        let mut rng = Rng::new(1);
        let mut pacer = Pacer::new(20, &mut rng);
        let start = Instant::now();
        for _ in 0..20 {
            assert!(pacer.wait(&stop, &mut rng).await);
        }
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(300), "{elapsed:?}");
        assert!(elapsed <= Duration::from_millis(900), "{elapsed:?}");
    }

    #[tokio::test]
    async fn a_pacer_that_is_behind_still_keeps_its_minimum_gap() {
        let stop = Stop::new();
        let mut rng = Rng::new(3);
        let mut pacer = Pacer::new(1, &mut rng).with_min_gap(Duration::from_millis(30));
        assert!(pacer.wait(&stop, &mut rng).await);
        let first = Instant::now();
        for _ in 0..4 {
            assert!(pacer.wait(&stop, &mut rng).await);
        }
        assert!(
            first.elapsed() >= Duration::from_millis(110),
            "{:?}",
            first.elapsed()
        );
    }

    #[tokio::test]
    async fn the_gap_counts_from_the_end_of_a_slow_delivery() {
        let stop = Stop::new();
        let mut rng = Rng::new(4);
        let mut pacer = Pacer::new(1, &mut rng).with_min_gap(Duration::from_millis(60));
        assert!(pacer.wait(&stop, &mut rng).await);
        // The delivery takes longer than the gap; the next one still waits the full gap.
        tokio::time::sleep(Duration::from_millis(100)).await;
        pacer.finished();
        let after = Instant::now();
        assert!(pacer.wait(&stop, &mut rng).await);
        assert!(
            after.elapsed() >= Duration::from_millis(55),
            "{:?}",
            after.elapsed()
        );
    }

    #[tokio::test]
    async fn the_pacer_stops_when_the_run_ends() {
        let stop = Stop::new();
        let mut rng = Rng::new(2);
        let mut pacer = Pacer::new(60_000, &mut rng);
        stop.set();
        assert!(!pacer.wait(&stop, &mut rng).await);
    }
}
