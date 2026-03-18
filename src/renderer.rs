use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
};

use crate::component::{Component, Tracked};
use crate::frame::Frame;
use crate::node::{Node, NodeId};

/// Manages a flat list of components and renders them into a Frame.
///
/// Phase 1 uses a simple vertical stack layout. Components are rendered
/// top-to-bottom, each getting its desired height at the current width.
pub struct Renderer {
    nodes: Vec<Node>,
    width: u16,
}

impl Renderer {
    /// Create a new renderer with the given terminal width.
    pub fn new(width: u16) -> Self {
        Self {
            nodes: Vec::new(),
            width,
        }
    }

    /// Add a component to the bottom of the stack. Returns its NodeId.
    pub fn push<C: Component>(&mut self, component: C) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node::new(component));
        id
    }

    /// Access a component's tracked state for mutation.
    ///
    /// Mutation via `DerefMut` automatically marks the state dirty.
    ///
    /// # Panics
    /// Panics if the NodeId is invalid or the state type doesn't match.
    pub fn state_mut<C: Component>(&mut self, id: NodeId) -> &mut Tracked<C::State> {
        let node = &mut self.nodes[id.0];
        node.state
            .as_any_mut()
            .downcast_mut::<Tracked<C::State>>()
            .expect("state type mismatch in state_mut")
    }

    /// Freeze a component. Frozen components use their cached buffer
    /// and are not re-rendered on subsequent frames.
    pub fn freeze(&mut self, id: NodeId) {
        self.nodes[id.0].frozen = true;
    }

    /// Set the rendering width (e.g., on terminal resize).
    /// Invalidates all cached buffers since wrapping changes.
    pub fn set_width(&mut self, width: u16) {
        if self.width != width {
            self.width = width;
            // Width change invalidates all caches
            for node in &mut self.nodes {
                node.cached_buffer = None;
                node.last_height = None;
            }
        }
    }

    /// Current rendering width.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Render all components into a Frame.
    ///
    /// Performs a measure pass (desired_height for each node),
    /// allocates vertical space, then renders each node into
    /// a single Buffer.
    pub fn render(&mut self) -> Frame {
        // Measure pass: compute heights
        let mut heights: Vec<u16> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let height = if node.frozen {
                node.last_height.unwrap_or(0)
            } else {
                let state = node.state.inner_as_any();
                node.component.desired_height_erased(self.width, state)
            };
            heights.push(height);
        }

        let total_height: u16 = heights.iter().sum();

        if total_height == 0 || self.width == 0 {
            return Frame::new(Buffer::empty(Rect::new(0, 0, self.width, 0)));
        }

        // Create the frame buffer
        let area = Rect::new(0, 0, self.width, total_height);
        let mut buffer = Buffer::empty(area);

        // Render pass: each component gets its vertical slice
        let mut y_offset: u16 = 0;
        for (i, node) in self.nodes.iter_mut().enumerate() {
            let h = heights[i];
            if h == 0 {
                continue;
            }

            let node_area = Rect::new(0, y_offset, self.width, h);

            if node.frozen || !node.state.is_dirty() {
                // Frozen or clean: copy from cached buffer if available
                if let Some(ref cached) = node.cached_buffer {
                    copy_buffer(cached, &mut buffer, node_area);
                }
            } else {
                // Dirty: re-render the component
                let state = node.state.inner_as_any();
                node.component.render_erased(node_area, &mut buffer, state);

                // Cache the rendered buffer
                let mut node_buf = Buffer::empty(node_area);
                copy_buffer_region(&buffer, &mut node_buf, node_area);
                node.cached_buffer = Some(node_buf);

                // Update cached height and clear dirty
                node.last_height = Some(h);
                node.state.clear_dirty();
            }

            y_offset = y_offset.saturating_add(h);
        }

        Frame::new(buffer)
    }
}

/// Copy cells from a source buffer into a destination buffer at the given area.
fn copy_buffer(src: &Buffer, dst: &mut Buffer, area: Rect) {
    let src_area = src.area;
    for y in 0..area.height {
        for x in 0..area.width {
            let src_x = src_area.x + x;
            let src_y = src_area.y + y;
            let dst_x = area.x + x;
            let dst_y = area.y + y;

            if src_x < src_area.x + src_area.width
                && src_y < src_area.y + src_area.height
                && dst_x < dst.area.x + dst.area.width
                && dst_y < dst.area.y + dst.area.height
            {
                dst[(dst_x, dst_y)] = src[(src_x, src_y)].clone();
            }
        }
    }
}

/// Copy a region from one buffer to another buffer (both may have different areas).
fn copy_buffer_region(src: &Buffer, dst: &mut Buffer, region: Rect) {
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if x < src.area.x + src.area.width
                && y < src.area.y + src.area.height
                && x < dst.area.x + dst.area.width
                && y < dst.area.y + dst.area.height
            {
                dst[(x, y)] = src[(x, y)].clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use ratatui_core::text::Line;
    use ratatui_widgets::paragraph::Paragraph;

    /// A simple test component that displays lines of text.
    struct TextBlock;

    impl Component for TextBlock {
        type State = Vec<String>;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let text: Vec<Line> = state.iter().map(|s| Line::raw(s.as_str())).collect();
            let para = Paragraph::new(text);
            ratatui_core::widgets::Widget::render(para, area, buf);
        }

        fn desired_height(&self, _width: u16, state: &Self::State) -> u16 {
            state.len() as u16
        }

        fn initial_state(&self) -> Vec<String> {
            vec![]
        }
    }

    #[test]
    fn render_empty_renderer() {
        let mut r = Renderer::new(80);
        let frame = r.render();
        assert_eq!(frame.area().height, 0);
    }

    #[test]
    fn render_single_component() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 1);
        assert_eq!(frame.area().width, 10);

        // Check buffer content
        let buf = frame.buffer();
        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "h");
    }

    #[test]
    fn render_two_components_stacked() {
        let mut r = Renderer::new(10);
        let id1 = r.push(TextBlock);
        let id2 = r.push(TextBlock);

        r.state_mut::<TextBlock>(id1).push("top".to_string());
        r.state_mut::<TextBlock>(id2).push("bot".to_string());

        let frame = r.render();
        assert_eq!(frame.area().height, 2);

        let buf = frame.buffer();
        assert_eq!(buf[(0, 0)].symbol(), "t"); // 'top' at row 0
        assert_eq!(buf[(0, 1)].symbol(), "b"); // 'bot' at row 1
    }

    #[test]
    fn dirty_flag_cleared_after_render() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        // State should be dirty after mutation
        assert!(r.nodes[id.0].state.is_dirty());

        let _ = r.render();

        // State should be clean after render
        assert!(!r.nodes[id.0].state.is_dirty());
    }

    #[test]
    fn frozen_component_uses_cached_buffer() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);
        r.state_mut::<TextBlock>(id).push("hello".to_string());

        // First render populates cache
        let _frame1 = r.render();

        // Freeze the component
        r.freeze(id);

        // Mutate state (shouldn't affect rendering since frozen)
        // We can't actually mutate since it's frozen, but let's verify
        // the frozen render returns the same content
        let frame2 = r.render();

        assert_eq!(frame2.area().height, 1);
        assert_eq!(frame2.buffer()[(0, 0)].symbol(), "h");
    }

    #[test]
    fn component_height_changes_with_state() {
        let mut r = Renderer::new(10);
        let id = r.push(TextBlock);

        // Empty state -> height 0
        let frame1 = r.render();
        assert_eq!(frame1.area().height, 0);

        // Add a line -> height 1
        r.state_mut::<TextBlock>(id).push("line1".to_string());
        let frame2 = r.render();
        assert_eq!(frame2.area().height, 1);

        // Add another -> height 2
        r.state_mut::<TextBlock>(id).push("line2".to_string());
        let frame3 = r.render();
        assert_eq!(frame3.area().height, 2);
    }
}
