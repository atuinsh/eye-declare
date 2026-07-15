//! The application contract: Elm-shaped, with the timeline as the effect
//! boundary.

use crate::element::Element;
use crate::input::Keymap;
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
    fn update(&mut self, msg: Self::Msg, ctx: &mut Ctx<'_, Self::Output>);

    /// Describe the live tail. Re-run every frame; borrows the model.
    fn tail(&self) -> impl Element + '_;

    /// Key bindings, rebuilt from the model each update so they can be
    /// conditional on app state.
    fn keymap(&self) -> Keymap<Self::Msg> {
        Keymap::new()
    }
}

/// Effect context handed to [`App::update`].
///
/// Committed output is an effect: [`push`](Ctx::push) renders the block
/// immediately (so blocks may freely borrow from locals or the model) and
/// the bytes join this update's output in order.
pub struct Ctx<'a, Out> {
    pub(crate) timeline: &'a mut Timeline,
    pub(crate) output: &'a mut Vec<u8>,
    pub(crate) exit: Option<Out>,
}

impl<Out> Ctx<'_, Out> {
    /// Commit a finished block above the live tail. Irreversible, like
    /// `println!`: the block renders once at the current width and leaves
    /// the program's world.
    pub fn push(&mut self, block: impl Element) {
        let bytes = self.timeline.push(block);
        self.output.extend_from_slice(&bytes);
    }

    /// End the run loop; it returns this value after teardown.
    pub fn exit(&mut self, output: Out) {
        self.exit = Some(output);
    }
}
