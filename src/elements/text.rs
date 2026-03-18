use ratatui_core::style::Style;

use crate::components::text::TextBlock;
use crate::element::Element;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Element builder for a [`TextBlock`] component.
///
/// ```ignore
/// let mut els = Elements::new();
/// els.add(TextBlockEl::new()
///     .line("Hello", Style::default())
///     .unstyled("World"));
/// ```
pub struct TextBlockEl {
    lines: Vec<(String, Style)>,
}

impl TextBlockEl {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Add a styled line.
    pub fn line(mut self, text: impl Into<String>, style: Style) -> Self {
        self.lines.push((text.into(), style));
        self
    }

    /// Add an unstyled line (default style).
    pub fn unstyled(mut self, text: impl Into<String>) -> Self {
        self.lines.push((text.into(), Style::default()));
        self
    }
}

impl Default for TextBlockEl {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for TextBlockEl {
    fn build(self: Box<Self>, renderer: &mut Renderer, parent: NodeId) -> NodeId {
        let id = renderer.append_child(parent, TextBlock);
        let state = renderer.state_mut::<TextBlock>(id);
        for (text, style) in self.lines {
            state.push(text, style);
        }
        id
    }
}
