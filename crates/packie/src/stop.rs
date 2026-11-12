//! Stop requests for `packie run`: SIGTERM and Ctrl-C (SIGINT) on Unix, Ctrl-C and Ctrl-Break on
//! Windows.
//!
//! A service manager or test runner may send SIGTERM while the engine is still starting.
//! Before the handlers exist the default action would kill the process mid-start, so on Unix
//! [`hold`] blocks both signals as the first thing `main` does, and [`install`] registers the
//! handlers and only then unblocks them: a signal that arrived meanwhile stays pending and
//! is handled, and the engine stops cleanly once started.

use std::future::Future;
use std::pin::Pin;

/// A stop request: completes when the process is asked to stop.
pub(crate) type StopRequest = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Blocks SIGTERM and SIGINT for this thread and every thread started after it (Unix).
pub(crate) fn hold() {
    // Failing to hold them only reopens the startup window; nothing to report.
    #[cfg(unix)]
    let _ = mask(nix::sys::signal::SigmaskHow::SIG_BLOCK);
}

/// Test seam for `sigterm_during_startup_exits_cleanly` (`tests/cli.rs`): with
/// `PACKIE_TEST_HELD_MARKER` set, creates that file once the signals are held, so the test can
/// tell a signal that arrived before `main` from one the engine must handle; with
/// `PACKIE_TEST_HELD_PAUSE_MS` set too, then waits that long (at most 5 s) before going on, so
/// a signal can be delivered between holding and installing the handlers. Without them,
/// nothing happens.
pub(crate) fn held_for_test() {
    let Some(marker) = std::env::var_os("PACKIE_TEST_HELD_MARKER") else {
        return;
    };
    let _ = std::fs::write(marker, b"");
    let pause = std::env::var("PACKIE_TEST_HELD_PAUSE_MS")
        .ok()
        .and_then(|ms| ms.parse::<u64>().ok());
    if let Some(ms) = pause {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(5000)));
    }
}

/// Lets SIGTERM and SIGINT through again with their default actions, for commands that do
/// not install handlers.
pub(crate) fn release() {
    #[cfg(unix)]
    let _ = mask(nix::sys::signal::SigmaskHow::SIG_UNBLOCK);
}

#[cfg(unix)]
fn mask(how: nix::sys::signal::SigmaskHow) -> nix::Result<()> {
    use nix::sys::signal::{SigSet, Signal, pthread_sigmask};
    let mut set = SigSet::empty();
    set.add(Signal::SIGTERM);
    set.add(Signal::SIGINT);
    pthread_sigmask(how, Some(&set), None)
}

/// Registers the stop handlers, then releases the held signals; a stop that arrived during
/// startup completes the returned request at once.
pub(crate) fn install() -> std::io::Result<StopRequest> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        release();
        Ok(Box::pin(async move {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = interrupt.recv() => {}
            }
        }))
    }
    #[cfg(not(unix))]
    {
        // Ctrl-Break is how a process manager (or the simulator) stops a console process it
        // started in its own process group, as SIGTERM does on Unix.
        let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
        let mut ctrl_break = tokio::signal::windows::ctrl_break()?;
        Ok(Box::pin(async move {
            tokio::select! {
                _ = ctrl_c.recv() => {}
                _ = ctrl_break.recv() => {}
            }
        }))
    }
}
