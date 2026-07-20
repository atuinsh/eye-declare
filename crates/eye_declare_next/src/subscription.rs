//! Declarative recurring inputs: message sources that exist *because of
//! what state the app is in*.
//!
//! [`App::subscriptions`](crate::App::subscriptions) is re-derived from the
//! model after every update; the driver diffs the declared set against
//! what's running — newly declared keys start, no-longer-declared keys
//! stop. There is no start/stop bookkeeping in `update`: declaring the set
//! IS the lifecycle. (`tail()`'s philosophy applied to inputs.)
//!
//! Contrast with [`Ctx::spawn`](crate::Ctx::spawn): spawn is imperative and
//! event-shaped ("this Submit started an LLM turn"), with a [`Task`]
//! (crate::Task) handle owning the lifetime. Subscriptions are state-shaped
//! ("while a session is active, poll the server"). And per the recorded
//! design rule: presentation-time animation is *neither* — widgets declare
//! [`animated`](crate::Element::animated); animation ticks are not
//! messages.
//!
//! Keys give subscriptions identity across updates (closures and streams
//! are opaque, so the runtime can't compare them structurally). Same key +
//! same shape = keep running; an [`every`](Subscriptions::every) whose
//! interval changed restarts; a [`stream`](Subscriptions::stream) is only
//! ever compared by key — swapping the stream under an unchanged key has
//! no effect until the key disappears for one update.
//!
//! Cancellation is prompt but asynchronous: when a key disappears (or an
//! interval changes) the old task is cancelled at its next await point,
//! and one already-queued tick or stream item may still be delivered
//! afterward. Treat subscription messages like any other input: derive
//! validity from the model, not from the assumption that a cancelled
//! source falls silent instantly.

use std::time::Duration;

use futures_core::Stream;

use crate::task::MsgStream;

pub(crate) type MakeMsg<Msg> = Box<dyn Fn() -> Msg + Send>;
pub(crate) type MakeStream<Msg> = Box<dyn FnOnce() -> MsgStream<Msg> + Send>;

pub(crate) enum SubKind<Msg> {
    Every {
        interval: Duration,
        make: MakeMsg<Msg>,
    },
    Stream {
        make: MakeStream<Msg>,
    },
}

/// The declared set of recurring inputs. Build with
/// [`every`](Subscriptions::every) / [`stream`](Subscriptions::stream);
/// make entries conditional with [`when`](crate::Fluent::when).
pub struct Subscriptions<Msg> {
    pub(crate) entries: Vec<(String, SubKind<Msg>)>,
}

impl<Msg> Default for Subscriptions<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Msg> Subscriptions<Msg> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Produce a message every `interval` (first fire after one interval,
    /// not immediately — do an immediate check in `update` if you need
    /// one at declaration time).
    pub fn every(
        mut self,
        key: impl Into<String>,
        interval: Duration,
        make: impl Fn() -> Msg + Send + 'static,
    ) -> Self {
        // A zero interval would complete every sleep immediately and
        // flood the unbounded channel faster than updates drain it.
        let interval = interval.max(Duration::from_millis(1));
        self.entries.push((
            key.into(),
            SubKind::Every {
                interval,
                make: Box::new(make),
            },
        ));
        self
    }

    /// Subscribe to a stream of messages. `make` is called once, when the
    /// key first appears in the declared set.
    pub fn stream<S>(
        mut self,
        key: impl Into<String>,
        make: impl FnOnce() -> S + Send + 'static,
    ) -> Self
    where
        S: Stream<Item = Msg> + Send + 'static,
    {
        self.entries.push((
            key.into(),
            SubKind::Stream {
                make: Box::new(move || Box::pin(make()) as MsgStream<Msg>),
            },
        ));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
