//! The stop signal a registry sends to a channel instance, and the waits that
//! supervisors and sessions combine with it.

use std::time::Duration;
use tokio::sync::watch;

use tokio::time::Instant;

/// How an instance stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopMode {
    /// At once, without draining (pause, removal, replacement, restart).
    Now,
    /// Deliver what is buffered until the deadline (engine shutdown).
    Drain(Instant),
}

/// Sender side: `None` while running.
pub(crate) type StopTx = watch::Sender<Option<StopMode>>;
/// Receiver side.
pub(crate) type StopRx = watch::Receiver<Option<StopMode>>;

/// A new stop signal.
pub(crate) fn channel() -> (StopTx, StopRx) {
    watch::channel(None)
}

/// Resolves when a stop is requested (or the registry is gone). Cancel-safe.
pub(crate) async fn stopped(rx: &mut StopRx) -> StopMode {
    loop {
        if let Some(mode) = *rx.borrow_and_update() {
            return mode;
        }
        if rx.changed().await.is_err() {
            return StopMode::Now;
        }
    }
}

/// Whether a stop was requested.
pub(crate) fn is_requested(rx: &StopRx) -> bool {
    rx.borrow().is_some()
}

/// Sleeps for `delay` unless a stop is requested first; `false` when stopping.
pub(crate) async fn sleep_unless_stopped(rx: &mut StopRx, delay: Duration) -> bool {
    tokio::select! {
        biased;
        _ = stopped(rx) => false,
        () = tokio::time::sleep(delay) => true,
    }
}

/// Resolves at `deadline`; never without one (for optional `select!` branches).
pub(crate) async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
