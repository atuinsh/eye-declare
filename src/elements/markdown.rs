use crate::components::markdown::{Markdown, MarkdownState};
use crate::element::Element;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Element builder for a [`Markdown`] component.
///
/// ```ignore
/// els.add(MarkdownEl::new("# Hello\n\nThis is **bold**."));
/// ```
pub struct MarkdownEl {
    source: String,
}

impl MarkdownEl {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

impl Element for MarkdownEl {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        let id = renderer.append_child(parent, Markdown);
        let state = renderer.state_mut::<Markdown>(id);
        **state = MarkdownState::new(self.source);
        id
    }
}
