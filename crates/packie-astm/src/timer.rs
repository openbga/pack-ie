//! The single timer slot of a driver: identity issue and stale-expiry check.

use std::time::Duration;

use crate::contract::{Action, DriverError, TimerId};

/// One driver's view of the shared timer slot.
///
/// Two slots with different first identities and a common step of two never
/// issue the same [`TimerId`], which lets the link driver run a receiver and a
/// sender behind one external timer.
#[derive(Clone, Debug)]
pub(crate) struct TimerSlot {
    next: u64,
    active: Option<TimerId>,
}

impl TimerSlot {
    /// The slot of the receiver: odd identities.
    pub(crate) const fn odd() -> Self {
        Self {
            next: 1,
            active: None,
        }
    }

    /// The slot of the sender: even identities.
    pub(crate) const fn even() -> Self {
        Self {
            next: 2,
            active: None,
        }
    }

    /// Arms a new timer, replacing any running one, and emits `StartTimer`.
    pub(crate) fn start(
        &mut self,
        duration: Duration,
        out: &mut Vec<Action>,
    ) -> Result<(), DriverError> {
        let timer_id = TimerId(self.next);
        self.next = self
            .next
            .checked_add(2)
            .ok_or(DriverError::TimerIdExhausted)?;
        self.active = Some(timer_id);
        out.push(Action::StartTimer { timer_id, duration });
        Ok(())
    }

    /// Emits `StopTimer` when this slot has a running timer.
    pub(crate) fn stop(&mut self, out: &mut Vec<Action>) {
        if self.active.take().is_some() {
            out.push(Action::StopTimer);
        }
    }

    /// Forgets the running timer without emitting an action; used when another
    /// driver's timer replaces it in the shared slot.
    pub(crate) const fn forget(&mut self) {
        self.active = None;
    }

    /// Reports whether `timer_id` is the running timer.
    pub(crate) fn is_active(&self, timer_id: TimerId) -> bool {
        self.active == Some(timer_id)
    }

    /// Consumes an expiry: true (and the slot is cleared) when it is the running timer.
    pub(crate) fn expire(&mut self, timer_id: TimerId) -> bool {
        let running = self.is_active(timer_id);
        if running {
            self.active = None;
        }
        running
    }
}
