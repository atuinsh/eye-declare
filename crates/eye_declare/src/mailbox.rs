//! Exactly-once handoff to an async consumer that might never run.
//!
//! The problem this solves: `update` wants to hand a value to spawned
//! work, but [`Task`](crate::Task)s are cancel-on-drop — a keystroke that
//! respawns the work cancels the previous task, and any value moved into
//! it is silently lost. The classic symptom is a mode switch the UI shows
//! but the backend never received, because the task carrying it was
//! cancelled before it ran.

use std::sync::{Arc, Mutex, PoisonError};

/// A shared slot holding at most one value. [`post`](Mailbox::post) from
/// `update`; [`take`](Mailbox::take) from whichever spawned task actually
/// runs. A cancelled task that never ran leaves the value in place for
/// the next taker, so the handoff survives any amount of task churn.
///
/// Posting replaces an undelivered value — the slot carries latest-wins
/// state (a mode, a config, a target), not a queue.
///
/// Clones share the slot. Locking is poison-proof: a panic in another
/// holder never turns `post`/`take` into a second panic.
pub struct Mailbox<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T> Mailbox<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a value in the slot, returning the undelivered value it
    /// displaced, if any.
    pub fn post(&self, value: T) -> Option<T> {
        self.lock().replace(value)
    }

    /// Claim the value, leaving the slot empty. Returns `None` if there
    /// is nothing to deliver (already taken, or never posted).
    pub fn take(&self) -> Option<T> {
        self.lock().take()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_none()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<T>> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// Manual impls: derives would bound `T`, and sharing/emptiness need no
// `T: Clone`/`T: Default`.
impl<T> Clone for Mailbox<T> {
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
        }
    }
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self {
            slot: Arc::new(Mutex::new(None)),
        }
    }
}

impl<T> std::fmt::Debug for Mailbox<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailbox")
            .field("occupied", &!self.is_empty())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_claims_the_posted_value() {
        let mailbox = Mailbox::new();
        assert_eq!(mailbox.post(1), None);
        assert_eq!(mailbox.take(), Some(1));
        assert_eq!(mailbox.take(), None);
    }

    #[test]
    fn posting_displaces_an_undelivered_value() {
        let mailbox = Mailbox::new();
        mailbox.post("old");
        assert_eq!(mailbox.post("new"), Some("old"));
        assert_eq!(mailbox.take(), Some("new"));
    }

    #[test]
    fn a_consumer_that_never_runs_leaves_the_value_for_the_next() {
        let mailbox = Mailbox::new();
        mailbox.post(7);
        // The first consumer clone is dropped without taking — the
        // cancelled-task case.
        drop(mailbox.clone());
        assert_eq!(mailbox.clone().take(), Some(7));
    }

    #[test]
    fn clones_share_the_slot() {
        let a: Mailbox<u8> = Mailbox::new();
        let b = a.clone();
        a.post(9);
        assert!(!b.is_empty());
        assert_eq!(b.take(), Some(9));
        assert!(a.is_empty());
    }
}
