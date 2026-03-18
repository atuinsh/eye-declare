use crate::components::spinner::{Spinner, SpinnerState};
use crate::element::Element;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Element builder for a [`Spinner`] component.
///
/// ```ignore
/// els.add(SpinnerEl::new("Thinking..."));
/// els.add(SpinnerEl::new("Done").done("Completed!"));
/// ```
pub struct SpinnerEl {
    label: String,
    done: bool,
    done_label: Option<String>,
}

impl SpinnerEl {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            done: false,
            done_label: None,
        }
    }

    /// Mark the spinner as already done, with a completion label.
    pub fn done(mut self, label: impl Into<String>) -> Self {
        self.done = true;
        self.done_label = Some(label.into());
        self
    }
}

impl Element for SpinnerEl {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        let id = renderer.append_child(parent, Spinner);
        let state = renderer.state_mut::<Spinner>(id);
        **state = SpinnerState::new(self.label);
        if self.done {
            state.complete(self.done_label);
        }
        id
    }
}
