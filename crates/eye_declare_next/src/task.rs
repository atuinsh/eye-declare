//! Spawned work and its cancellation story.
//!
//! [`Ctx::spawn`](crate::Ctx::spawn) collects [`Effect`]s during `update`;
//! a driver (e.g. the tokio one) drains them via
//! [`Runtime::take_effects`](crate::Runtime::take_effects) and actually
//! executes the streams. The returned [`Task`] is the app's lifecycle
//! handle: **dropping it cancels the work**, so cancellation is a plain
//! assignment (`self.streaming = None`).
//!
//! Everything here is executor-agnostic — cancellation is a hand-rolled
//! waker future, so the core crate stays free of runtime dependencies.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use futures_core::Stream;

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
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            waker: Mutex::new(None),
        }
    }

    fn cancel(&self) {
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

/// An effect requested during `update`, to be executed by the driver.
pub enum Effect<Msg> {
    /// Run a stream, feeding each item back into `update` as a message,
    /// until it ends or its [`Task`] is dropped.
    Spawn {
        stream: Pin<Box<dyn Stream<Item = Msg> + Send>>,
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
}
