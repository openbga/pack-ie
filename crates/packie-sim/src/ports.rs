//! Loopback port reservation that cannot collide.
//!
//! A port is handed out only after two checks, and stays reserved until its lease is
//! dropped:
//!
//! 1. **A lock.** Every port of [`RANGE`] has a lock file in a shared directory
//!    ([`lock_dir`]). A lease holds an exclusive advisory lock (`flock`) on its file, so
//!    no other scenario, test thread or `packie-sim` process gets the same port while the
//!    lease lives. The operating system drops the lock when the process dies, so nothing
//!    goes stale.
//! 2. **A bound socket.** The lease also binds the port. That proves nothing else owns it.
//!    A `tcp_server` actor that listens at once takes the socket over
//!    ([`PortLease::take_socket`]); for ports the engine binds, and for servers that
//!    listen later, the socket is released ([`PortLease::release_socket`]) just before
//!    it is needed and the lock keeps the port reserved. The socket is never kept while a
//!    peer may connect: on macOS a bound socket that does not listen makes connects hang
//!    instead of being refused.
//!
//! [`RANGE`] lies below every operating system's ephemeral range (Linux starts at 32768,
//! macOS and Windows at 49152). Outgoing connections of actors and engine therefore never
//! take a reserved port, which is what made the earlier probe-and-release scheme fail:
//! a freed port was often handed to somebody's outgoing connection before the engine bound
//! it. If a host moves its ephemeral range down into [`RANGE`], lower it back.

use std::fs::{File, OpenOptions, TryLockError};
use std::net::IpAddr;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::net::TcpSocket;

use crate::actors::tcp::bound_socket;
use crate::error::SimError;

/// The ports scenarios may use.
pub const RANGE: RangeInclusive<u16> = 20_000..=29_999;

/// Where the lock files live: `PACKIE_SIM_PORT_DIR`, else `packie-sim-ports` in the temporary
/// directory. Every `packie-sim` process of a user must see the same directory.
#[must_use]
pub fn lock_dir() -> PathBuf {
    std::env::var_os("PACKIE_SIM_PORT_DIR").map_or_else(
        || std::env::temp_dir().join("packie-sim-ports"),
        PathBuf::from,
    )
}

/// A reserved port. Dropping the lease releases the lock and the socket.
#[derive(Debug)]
pub struct PortLease {
    port: u16,
    socket: Option<TcpSocket>,
    /// Holds the lock; closing the file releases it.
    _lock: File,
}

/// Where the next search starts, so that concurrent searches begin at different ports and
/// a just-released port is not the first one reused.
fn next_start() -> u32 {
    static CURSOR: OnceLock<AtomicU32> = OnceLock::new();
    let span = u32::from(*RANGE.end() - *RANGE.start()) + 1;
    let cursor = CURSOR.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        AtomicU32::new(std::process::id().wrapping_mul(2_654_435_761) ^ nanos)
    });
    cursor.fetch_add(1, Ordering::Relaxed) % span
}

impl PortLease {
    /// Reserves a free port of [`RANGE`] on `bind`, with `recv_buffer` set on its socket.
    pub fn reserve(bind: IpAddr, recv_buffer: Option<u32>) -> Result<Self, SimError> {
        let dir = lock_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            SimError::io(
                format!("create the port lock directory {}", dir.display()),
                e,
            )
        })?;
        let low = u32::from(*RANGE.start());
        let span = u32::from(*RANGE.end()) - low + 1;
        let start = next_start();
        for offset in 0..span {
            let Ok(port) = u16::try_from(low + (start + offset) % span) else {
                continue;
            };
            let Some(lock) = try_lock(&dir, port) else {
                continue;
            };
            // Something outside this tool may own the port: skip it. The lock is released
            // when `lock` goes out of scope.
            if let Ok(socket) = bound_socket(bind, port, recv_buffer) {
                return Ok(Self {
                    port,
                    socket: Some(socket),
                    _lock: lock,
                });
            }
        }
        Err(SimError::other(format!(
            "no free port in {}-{} on {bind}",
            RANGE.start(),
            RANGE.end()
        )))
    }

    /// Reserves a port on 127.0.0.1.
    pub fn reserve_loopback() -> Result<Self, SimError> {
        Self::reserve(IpAddr::from([127, 0, 0, 1]), None)
    }

    /// The reserved port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The bound, not yet listening socket, for the actor that will listen on it.
    pub fn take_socket(&mut self) -> Option<TcpSocket> {
        self.socket.take()
    }

    /// Closes the bound socket so another process can bind the port. The port stays
    /// reserved against other leases until the lease is dropped.
    pub fn release_socket(&mut self) {
        self.socket = None;
    }
}

/// Locks `<dir>/<port>.lock`; `None` when another lease holds it or the file cannot be used.
fn try_lock(dir: &std::path::Path, port: u16) -> Option<File> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join(format!("{port}.lock")))
        .ok()?;
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(TryLockError::WouldBlock | TryLockError::Error(_)) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn leases_are_distinct_inside_the_range_and_released_on_drop() {
        let mut first = PortLease::reserve_loopback().unwrap();
        let second = PortLease::reserve_loopback().unwrap();
        assert_ne!(first.port(), second.port());
        assert!(RANGE.contains(&first.port()) && RANGE.contains(&second.port()));
        // A second socket cannot take a leased port, even after the socket is released,
        // because the lock stays: another lease never picks it.
        let port = first.port();
        first.release_socket();
        let others: Vec<PortLease> = (0..50)
            .map(|_| PortLease::reserve_loopback().unwrap())
            .collect();
        assert!(others.iter().all(|l| l.port() != port));
        drop(first);
        // Freed again: bindable by anyone.
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    /// Holds 20 leases (two descriptors each; keep well under a 1024 descriptor limit) for a while, then records their ports in `seen`.
    fn lease_and_record(seen: &Mutex<HashSet<u16>>) {
        let leases: Vec<PortLease> = (0..20)
            .map(|_| PortLease::reserve_loopback().unwrap())
            .collect();
        // Every lease of every thread is alive at the same time.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let fresh = leases.iter().all(|l| seen.lock().unwrap().insert(l.port()));
        assert!(fresh, "a port was leased twice");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    #[test]
    fn concurrent_threads_never_share_a_port() {
        let seen = Arc::new(Mutex::new(HashSet::new()));
        let threads: Vec<_> = (0..16)
            .map(|_| {
                let seen = Arc::clone(&seen);
                std::thread::spawn(move || lease_and_record(&seen))
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(seen.lock().unwrap().len(), 16 * 20);
    }
}
