//! WAL checkpoints off the writer thread.
//!
//! A checkpoint copies committed WAL frames into the database file and syncs it. Run
//! inside a commit (SQLite's automatic checkpoint) it stalls every caller waiting on that
//! group; here a dedicated thread with its own connection runs a passive checkpoint
//! shortly after commits, while the writer keeps appending to the WAL. Commits still sync
//! the WAL (`synchronous=FULL`), so durability does not depend on this thread. The writer
//! keeps an automatic checkpoint at [`WRITER_BACKSTOP_PAGES`] in case this thread falls
//! behind. The thread sleeps while nothing is committed and ends with the writer.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use rusqlite::Connection;

/// WAL pages at which the writer checkpoints itself (SQLite's default is 1000).
pub(crate) const WRITER_BACKSTOP_PAGES: u32 = 10_000;

/// How long the thread gathers commits before it checkpoints them.
const GATHER: Duration = Duration::from_millis(100);

/// Tells the checkpoint thread that a group committed; never blocks.
pub(crate) struct Commits(SyncSender<()>);

impl Commits {
    /// A notifier with no checkpoint thread behind it.
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        Self(mpsc::sync_channel(1).0)
    }

    pub(crate) fn committed(&self) {
        // A full channel already holds a pending wake-up.
        let _ = self.0.try_send(());
    }
}

/// Starts the checkpoint thread on `conn`, a connection of its own to the database.
pub(crate) fn spawn(conn: Connection) -> std::io::Result<Commits> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("packie-store-checkpoint".into())
        .spawn(move || run(&conn, &receiver))?;
    Ok(Commits(sender))
}

fn run(conn: &Connection, commits: &Receiver<()>) {
    while commits.recv().is_ok() {
        std::thread::sleep(GATHER);
        if let Err(error) = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(())) {
            tracing::warn!(
                event = "store.checkpoint_failed",
                "WAL checkpoint failed: {error}; the writer checkpoints itself as the WAL grows"
            );
        }
    }
}
