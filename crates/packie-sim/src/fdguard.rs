//! Keeps descriptors created in this process out of spawned engines.
//!
//! The engine is a child process. A descriptor is inherited by a child spawned while the
//! descriptor exists without close-on-exec, and some platforms cannot create a socket or a
//! pseudo-terminal atomically with that flag (macOS creates, then sets it; `TTYPort::pair`
//! never sets it on the master). Scenarios run in parallel in one process, so one
//! scenario's engine could be spawned in that gap and keep another scenario's port bound
//! (the listening engine of the other scenario then fails with "address in use") or keep a
//! pseudo-terminal master open (the other engine's serial port never sees the unplug).
//!
//! Code that creates such descriptors holds [`creating`]; the code that spawns an engine
//! holds [`spawning`] around the `spawn` call only. Spawns may overlap each other, creations
//! exclude spawns and each other, and neither guard is ever held across an `.await`.

use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

static CREATION: RwLock<()> = RwLock::new(());

/// Held while a socket or pseudo-terminal is created and marked close-on-exec.
pub(crate) fn creating() -> RwLockWriteGuard<'static, ()> {
    CREATION.write().unwrap_or_else(PoisonError::into_inner)
}

/// Held around spawning a child process.
pub(crate) fn spawning() -> RwLockReadGuard<'static, ()> {
    CREATION.read().unwrap_or_else(PoisonError::into_inner)
}
