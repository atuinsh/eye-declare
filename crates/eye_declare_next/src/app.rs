//! The application contract: Elm-shaped, with the timeline as the effect
//! boundary.

use futures_core::Stream;

use crate::element::Element;
use crate::input::Keymap;
use crate::subscription::Subscriptions;
use crate::task::{Effect, Task, spawn_effect, spawn_once_effect};
use crate::timeline::Timeline;

/// An inline application. The implementing struct IS the model: `update`
/// takes `&mut self`, `tail` takes `&self` — the borrow checker enforces
/// the discipline.
pub trait App: Sized {
    /// Messages driving the app. Everything that happens becomes one.
    type Msg;
    /// What the run loop returns after [`Ctx::exit`].
    type Output: Default;

    /// Handle a message: mutate the model, emit effects via `ctx`.
    fn update(&mut self, msg: Self::Msg, ctx: &mut Ctx<'_, Self>);

    /// Describe the live tail. Re-run every frame; borrows the model.
    fn tail(&self) -> impl Element + '_;

    /// Key bindings, rebuilt from the model each update so they can be
    /// conditional on app state.
    fn keymap(&self) -> Keymap<Self::Msg> {
        Keymap::new()
    }

    /// Declarative recurring inputs, re-derived from the model each update
    /// and diffed by the driver (see [`Subscriptions`]). Requires an async
    /// driver.
    fn subscriptions(&self) -> Subscriptions<Self::Msg> {
        Subscriptions::new()
    }
}

/// Effect context handed to [`App::update`].
///
/// Committed output is an effect: [`push`](Ctx::push) renders the block
/// immediately (so blocks may freely borrow from locals or the model) and
/// the bytes join this update's output in order. Async work is an effect
/// too: [`spawn`](Ctx::spawn) queues the stream for the driver and returns
/// a cancel-on-drop [`Task`] to hold in the model.
pub struct Ctx<'a, A: App> {
    pub(crate) timeline: &'a mut Timeline,
    pub(crate) output: &'a mut Vec<u8>,
    pub(crate) effects: &'a mut Vec<Effect<A::Msg>>,
    pub(crate) exit: Option<A::Output>,
}

impl<A: App> Ctx<'_, A> {
    /// Commit a finished block above the live tail. Irreversible, like
    /// `println!`: the block renders once at the current width and leaves
    /// the program's world.
    pub fn push(&mut self, block: impl Element) {
        let bytes = self.timeline.push(block);
        self.output.extend_from_slice(&bytes);
    }

    /// Spawn a stream of messages (the LLM-turn shape): each item feeds
    /// back into [`App::update`]. The work starts when the driver drains
    /// effects after this update returns, and stops when the stream ends
    /// or the returned [`Task`] is dropped — hold it in the model, and
    /// cancellation is `self.task = None`.
    ///
    /// Requires an async driver (the sync [`run`](crate::run) refuses apps
    /// that spawn).
    #[must_use]
    pub fn spawn(&mut self, stream: impl Stream<Item = A::Msg> + Send + 'static) -> Task {
        let (effect, task) = spawn_effect(stream);
        self.effects.push(effect);
        task
    }

    /// One-shot convenience over [`spawn`](Ctx::spawn): run a future,
    /// deliver its output as a single message.
    #[must_use]
    pub fn perform(&mut self, future: impl Future<Output = A::Msg> + Send + 'static) -> Task {
        let (effect, task) = spawn_once_effect(future);
        self.effects.push(effect);
        task
    }

    /// End the run loop; it returns this value after teardown.
    pub fn exit(&mut self, output: A::Output) {
        self.exit = Some(output);
    }
}
