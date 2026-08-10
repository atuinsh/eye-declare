//! Spawned work and its cancellation — and durability — story.
//!
//! [`Ctx::spawn`](crate::Ctx::spawn) collects [`Effect`]s during `update`;
//! a driver (e.g. the tokio one) drains them via
//! [`Runtime::take_effects`](crate::Runtime::take_effects) and actually
//! executes the streams. The returned [`Task`] is the app's lifecycle
//! handle: **dropping it cancels the work**, so cancellation is a plain
//! assignment (`self.streaming = None`).
//!
//! [`Ctx::persist`](crate::Ctx::persist) is the opposite end of the
//! spectrum: uncancellable work the driver must wait for at teardown,
//! tracked by [`PersistTracker`].
//!
//! Everything here is executor-agnostic — cancellation and the persist
//! tracker are hand-rolled waker futures, so the core crate stays free of
//! runtime dependencies.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use futures_core::Stream;

/// A boxed message stream, the shape drivers execute.
pub type MsgStream<Msg> = Pin<Box<dyn Stream<Item = Msg> + Send>>;

/// Handle to spawned work. Dropping it cancels the work; call
/// [`detach`](Task::detach) for fire-and-forget.
pub struct Task {
    cancel: Option<Arc<Cancel>>,
}

impl Task {
    /// Let the work run to completion even after this handle is gone.
    pub fn detach(mut self) {
        self.cancel = None;
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }
}

/// Shared cancellation state between a [`Task`] handle and the driver's
/// execution of the work. Opaque outside the crate; it appears in
/// [`Effect`]'s public shape so custom drivers can hold it.
pub struct Cancel {
    cancelled: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Cancel {
    pub(crate) fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            waker: Mutex::new(None),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// A future that resolves when the task is cancelled. Drivers select
    /// this against the work's next item.
    pub fn cancelled(self: &Arc<Self>) -> Cancelled {
        Cancelled {
            cancel: Arc::clone(self),
        }
    }
}

/// See [`Cancel::cancelled`].
pub struct Cancelled {
    cancel: Arc<Cancel>,
}

impl Future for Cancelled {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(());
        }
        *self.cancel.waker.lock().unwrap() = Some(cx.waker().clone());
        // Re-check after storing the waker to close the race with a
        // concurrent cancel() that ran between the check and the store.
        if self.cancel.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Counts in-flight [`Ctx::persist`](crate::Ctx::persist) work so a driver
/// can wait for it at teardown instead of abandoning it — the whole point
/// of `persist` over a detached [`Ctx::perform`](crate::Ctx::perform).
///
/// Cloning shares the counter. [`Runtime::persists`](crate::Runtime::persists)
/// hands drivers a clone; [`wait`](PersistTracker::wait) resolves when no
/// persist work remains. One waiter at a time — a later `wait` displaces
/// an earlier one's waker (drivers are the only intended waiter).
#[derive(Clone, Default)]
pub struct PersistTracker {
    state: Arc<PersistState>,
}

#[derive(Default)]
struct PersistState {
    count: AtomicUsize,
    waker: Mutex<Option<Waker>>,
}

impl PersistTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register one unit of persist work; dropping the guard completes it.
    pub(crate) fn guard(&self) -> PersistGuard {
        self.state.count.fetch_add(1, Ordering::SeqCst);
        PersistGuard {
            state: Arc::clone(&self.state),
        }
    }

    /// Whether no persist work is in flight.
    pub fn is_idle(&self) -> bool {
        self.state.count.load(Ordering::SeqCst) == 0
    }

    /// Resolves when all persist work has completed. Requires the work's
    /// effects to actually be driven — a persist queued but never spawned
    /// keeps this pending forever.
    pub fn wait(&self) -> PersistsDone {
        PersistsDone {
            state: Arc::clone(&self.state),
        }
    }
}

/// Held across a persist future; Drop marks the work complete even if the
/// future panicked or was dropped by a dying executor.
pub(crate) struct PersistGuard {
    state: Arc<PersistState>,
}

impl Drop for PersistGuard {
    fn drop(&mut self) {
        if self.state.count.fetch_sub(1, Ordering::SeqCst) == 1
            && let Some(waker) = self.state.waker.lock().unwrap().take()
        {
            waker.wake();
        }
    }
}

/// See [`PersistTracker::wait`].
pub struct PersistsDone {
    state: Arc<PersistState>,
}

impl Future for PersistsDone {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.state.count.load(Ordering::SeqCst) == 0 {
            return Poll::Ready(());
        }
        *self.state.waker.lock().unwrap() = Some(cx.waker().clone());
        // Re-check after storing the waker to close the race with a
        // concurrent final guard drop between the check and the store.
        if self.state.count.load(Ordering::SeqCst) == 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// An effect requested during `update`, to be executed by the driver.
pub enum Effect<Msg> {
    /// Run a stream, feeding each item back into `update` as a message,
    /// until it ends or its [`Task`] is dropped.
    Spawn {
        stream: MsgStream<Msg>,
        cancel: Arc<Cancel>,
    },
}

pub(crate) fn spawn_effect<Msg>(
    stream: impl Stream<Item = Msg> + Send + 'static,
) -> (Effect<Msg>, Task) {
    let cancel = Arc::new(Cancel::new());
    (
        Effect::Spawn {
            stream: Box::pin(stream),
            cancel: Arc::clone(&cancel),
        },
        Task {
            cancel: Some(cancel),
        },
    )
}

pub(crate) fn spawn_once_effect<Msg>(
    future: impl Future<Output = Msg> + Send + 'static,
) -> (Effect<Msg>, Task) {
    spawn_effect(FutureStream {
        future: Some(Box::pin(future)),
    })
}

/// Adapts a future into a one-item stream (futures-core is traits-only,
/// so the adapter lives here).
struct FutureStream<F: Future> {
    future: Option<Pin<Box<F>>>,
}

impl<F: Future> Stream for FutureStream<F> {
    type Item = F::Output;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<F::Output>> {
        let this = self.get_mut();
        match &mut this.future {
            Some(future) => match future.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    this.future = None;
                    Poll::Ready(Some(value))
                }
                Poll::Pending => Poll::Pending,
            },
            None => Poll::Ready(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream that never yields (futures-core is traits-only).
    struct Pending;

    impl Stream for Pending {
        type Item = ();

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<()>> {
            Poll::Pending
        }
    }

    #[test]
    fn drop_cancels() {
        let (effect, task) = spawn_effect(Pending);
        let Effect::Spawn { cancel, .. } = effect;
        assert!(!cancel.is_cancelled());
        drop(task);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn detach_does_not_cancel() {
        let (effect, task) = spawn_effect(Pending);
        let Effect::Spawn { cancel, .. } = effect;
        task.detach();
        assert!(!cancel.is_cancelled());
    }

    fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
        let waker = Waker::noop();
        Pin::new(future).poll(&mut Context::from_waker(waker))
    }

    #[test]
    fn tracker_is_idle_until_guarded() {
        let tracker = PersistTracker::new();
        assert!(tracker.is_idle());
        let guard = tracker.guard();
        assert!(!tracker.is_idle());
        drop(guard);
        assert!(tracker.is_idle());
    }

    #[test]
    fn wait_resolves_when_the_last_guard_drops() {
        let tracker = PersistTracker::new();
        let a = tracker.guard();
        let b = tracker.guard();
        let mut wait = tracker.wait();
        assert_eq!(poll_once(&mut wait), Poll::Pending);
        drop(a);
        assert_eq!(poll_once(&mut wait), Poll::Pending);
        drop(b);
        assert_eq!(poll_once(&mut wait), Poll::Ready(()));
    }

    #[test]
    fn wait_on_an_idle_tracker_is_immediate() {
        let tracker = PersistTracker::new();
        assert_eq!(poll_once(&mut tracker.wait()), Poll::Ready(()));
    }

    #[test]
    fn the_final_guard_drop_wakes_the_waiter() {
        use std::sync::atomic::AtomicUsize;

        // A waker that counts wakes, so the test observes the wake itself
        // rather than only the post-wake poll result.
        static WAKES: AtomicUsize = AtomicUsize::new(0);
        fn count_waker() -> Waker {
            use std::task::{RawWaker, RawWakerVTable};
            fn wake(_: *const ()) {
                WAKES.fetch_add(1, Ordering::SeqCst);
            }
            fn clone(p: *const ()) -> RawWaker {
                RawWaker::new(p, &VTABLE)
            }
            fn drop_raw(_: *const ()) {}
            static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, drop_raw);
            // SAFETY: the vtable functions touch no data pointer.
            unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
        }

        let tracker = PersistTracker::new();
        let guard = tracker.guard();
        let mut wait = tracker.wait();
        let waker = count_waker();
        assert!(
            Pin::new(&mut wait)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let before = WAKES.load(Ordering::SeqCst);
        drop(guard);
        assert_eq!(WAKES.load(Ordering::SeqCst), before + 1);
        assert_eq!(poll_once(&mut wait), Poll::Ready(()));
    }
}
