use std::any::{Any, TypeId};
use std::time::Duration;

use eye_declare_engine::Engine;

use crate::component::{Component, EventResult, Tracked};
use crate::element::Elements;
use crate::node::NodeId;
use crate::renderer::Renderer;

/// Low-level inline rendering engine.
///
/// `InlineRenderer` manages a growing region of terminal output. Content
/// expands downward as components are added or grow taller; old content
/// scrolls into the terminal's native scrollback naturally. Each call to
/// [`render`](InlineRenderer::render) returns a `Vec<u8>` of ANSI escape
/// sequences ready to write to stdout.
///
/// For most applications, the higher-level [`Application`](crate::Application)
/// wrapper is more convenient. Use `InlineRenderer` when you need:
///
/// - A synchronous event loop
/// - Integration with an existing framework
/// - Fine-grained control over when rendering happens
///
/// # Example
///
/// ```ignore
/// use eye_declare::{InlineRenderer, Spinner};
/// use std::io::Write;
///
/// let mut renderer = InlineRenderer::new(80);
/// let id = renderer.push(Spinner::new("Working..."));
///
/// // Render loop
/// loop {
///     renderer.tick();
///     let output = renderer.render();
///     std::io::stdout().write_all(&output)?;
///     std::io::stdout().flush()?;
///     std::thread::sleep(std::time::Duration::from_millis(16));
/// }
/// ```
pub struct InlineRenderer {
    renderer: Renderer,
    /// Terminal-sync state machine (cursor tracking, row accounting,
    /// scrollback boundary). Extracted to `eye_declare_engine`; this type
    /// keeps only the component-tree concerns.
    engine: Engine,
}

impl InlineRenderer {
    /// Create a new inline renderer at the given terminal width.
    ///
    /// Queries the terminal for its height to filter scrollback writes.
    /// Falls back to `u16::MAX` (no filtering) if no terminal is attached.
    pub fn new(width: u16) -> Self {
        let terminal_height = crossterm::terminal::size()
            .map(|(_, h)| h)
            .unwrap_or(u16::MAX);
        Self::new_with_height(width, terminal_height)
    }

    /// Create a new inline renderer with an explicit terminal height.
    ///
    /// Use this in tests or environments where querying the terminal
    /// is not possible or deterministic behavior is required.
    pub fn new_with_height(width: u16, terminal_height: u16) -> Self {
        Self {
            renderer: Renderer::new(width),
            engine: Engine::new(width, terminal_height),
        }
    }

    /// The root node's ID.
    pub fn root(&self) -> NodeId {
        self.renderer.root()
    }

    /// Add a component as a child of the given parent. Returns its NodeId.
    pub fn append_child<C: Component>(&mut self, parent: NodeId, component: C) -> NodeId {
        self.renderer.append_child(parent, component)
    }

    /// Shorthand: add a component as a child of the root. Returns its NodeId.
    pub fn push<C: Component>(&mut self, component: C) -> NodeId {
        self.renderer.push(component)
    }

    /// Access a component's tracked state for mutation.
    pub fn state_mut<C: Component>(&mut self, id: NodeId) -> &mut Tracked<C::State> {
        self.renderer.state_mut::<C>(id)
    }

    /// Register a root-level context value available to all components.
    ///
    /// See [`ApplicationBuilder::with_context`](crate::ApplicationBuilder::with_context)
    /// for the higher-level API.
    pub fn set_root_context<T: Any + Send + Sync>(&mut self, value: T) {
        self.renderer.set_root_context(value);
    }

    /// Register a root-level context value from a type-erased box.
    pub(crate) fn set_root_context_raw(
        &mut self,
        type_id: TypeId,
        value: Box<dyn Any + Send + Sync>,
    ) {
        self.renderer.set_root_context_raw(type_id, value);
    }

    /// Swap the component on an existing node, preserving state.
    pub fn swap_component<C: Component>(&mut self, id: NodeId, component: C) {
        self.renderer.swap_component(id, component)
    }

    /// Freeze a component (skip future re-renders).
    pub fn freeze(&mut self, id: NodeId) {
        self.renderer.freeze(id)
    }

    /// Remove a node and all its descendants.
    pub fn remove(&mut self, id: NodeId) {
        self.renderer.remove(id)
    }

    /// List the children of a node.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.renderer.children(id)
    }

    /// Replace the children of `parent` with nodes built from `elements`.
    pub fn rebuild(&mut self, parent: NodeId, elements: Elements) {
        self.renderer.rebuild(parent, elements)
    }

    /// Find a direct child of `parent` by its key.
    pub fn find_by_key(&self, parent: NodeId, key: &str) -> Option<NodeId> {
        self.renderer.find_by_key(parent, key)
    }

    /// Register a periodic tick handler for a node.
    pub fn register_tick<C: Component>(
        &mut self,
        id: NodeId,
        interval: Duration,
        handler: impl Fn(&mut C::State) + Send + Sync + 'static,
    ) {
        self.renderer.register_tick::<C>(id, interval, handler)
    }

    /// Remove a tick registration for a node.
    pub fn unregister_tick(&mut self, id: NodeId) {
        self.renderer.unregister_tick(id)
    }

    /// Advance all registered tick handlers. Returns true if any fired.
    pub fn tick(&mut self) -> bool {
        self.renderer.tick()
    }

    /// Whether there are any active tick registrations.
    pub fn has_active(&self) -> bool {
        self.renderer.has_active()
    }

    /// Register a mount handler for a node.
    pub fn on_mount<C: Component>(
        &mut self,
        id: NodeId,
        handler: impl Fn(&mut C::State) + Send + Sync + 'static,
    ) {
        self.renderer.on_mount::<C>(id, handler)
    }

    /// Register an unmount handler for a node.
    pub fn on_unmount<C: Component>(
        &mut self,
        id: NodeId,
        handler: impl Fn(&mut C::State) + Send + Sync + 'static,
    ) {
        self.renderer.on_unmount::<C>(id, handler)
    }

    /// Set which component has focus for event routing.
    pub fn set_focus(&mut self, id: NodeId) {
        self.renderer.set_focus(id);
    }

    /// Clear focus.
    pub fn clear_focus(&mut self) {
        self.renderer.clear_focus();
    }

    /// The currently focused component, if any.
    pub fn focus(&self) -> Option<NodeId> {
        self.renderer.focus()
    }

    /// Deliver an event using capture (root → focused) then bubble (focused → root).
    pub fn handle_event(&mut self, event: &crossterm::event::Event) -> EventResult {
        self.renderer.handle_event(event)
    }

    /// Handle a terminal resize.
    ///
    /// After a width change, the terminal has already reflowed existing
    /// content, making cursor tracking invalid. This clears the visible
    /// screen (preserving scrollback), homes the cursor, and does a full
    /// re-render at the new width.
    ///
    /// Scrollback content from before the resize stays at the old
    /// wrapping. A host application with its own terminal state tracking
    /// can avoid the screen clear by diffing against its known state;
    /// this clear-and-redraw is the standalone fallback.
    ///
    /// Returns escape sequences to write to the terminal.
    pub fn resize(&mut self, new_width: u16) -> Vec<u8> {
        // Clear visible screen, home cursor, drop tracking state.
        let mut output = self.engine.reset(new_width);
        self.renderer.set_width(new_width);

        // Update terminal height (resize event gives us width, query for height)
        if let Ok((_, h)) = crossterm::terminal::size() {
            self.engine.set_terminal_height(h);
        }

        // Do a fresh render
        let render_output = self.render();
        output.extend_from_slice(&render_output);

        output
    }

    /// Render the current state and return bytes to write to the terminal.
    ///
    /// Renders the component tree to a frame, then hands it to the
    /// [`Engine`] to sync: diffing against the previous frame, claiming
    /// terminal rows for growth, streaming soon-to-scroll rows into
    /// scrollback, and positioning the hardware cursor at the focused
    /// component's hint. Returns an empty Vec if nothing changed.
    pub fn render(&mut self) -> Vec<u8> {
        let frame = self.renderer.render();
        let cursor_hint = self.renderer.cursor_hint();
        self.engine.present(frame, cursor_hint)
    }

    /// How many rows have been emitted to the terminal.
    pub fn emitted_rows(&self) -> u16 {
        self.engine.emitted_rows()
    }

    /// Update the known terminal height.
    pub fn set_terminal_height(&mut self, height: u16) {
        self.engine.set_terminal_height(height);
    }

    /// Get the last rendered height of a node.
    pub fn node_last_height(&self, id: NodeId) -> u16 {
        self.renderer.node_last_height(id)
    }

    /// Detect which children of `container` have fully scrolled into
    /// terminal scrollback and can be committed.
    ///
    /// Returns `(index, key)` pairs for each committed child, in order
    /// from the top of the container.
    pub fn detect_committed(
        &self,
        container: NodeId,
        terminal_height: u16,
    ) -> Vec<(usize, Option<String>)> {
        let scrollback_rows = self.engine.emitted_rows().saturating_sub(terminal_height);
        if scrollback_rows == 0 {
            return Vec::new();
        }

        let children = self.renderer.children(container);
        let mut accumulated: u16 = 0;
        let mut committed = Vec::new();

        for (i, &child_id) in children.iter().enumerate() {
            let child_height = self.renderer.node_last_height(child_id);
            accumulated = accumulated.saturating_add(child_height);
            if accumulated <= scrollback_rows {
                let key = self.renderer.node_key(child_id).map(|s| s.to_string());
                committed.push((i, key));
            } else {
                break;
            }
        }

        committed
    }

    /// Commit (drop) the first `count` children of `container` that
    /// have scrolled into terminal scrollback.
    ///
    /// Adjusts `prev_frame`, `emitted_rows`, and cursor tracking so
    /// subsequent diffs only cover the active region.
    pub fn commit(&mut self, container: NodeId, count: usize, committed_height: u16) {
        if count == 0 || committed_height == 0 {
            return;
        }

        // Clear focus if it's on a committed node
        if let Some(focused) = self.renderer.focus() {
            let children = self.renderer.children(container);
            let committed_ids: Vec<NodeId> = children[..count].to_vec();
            if committed_ids.contains(&focused) {
                self.renderer.clear_focus();
            }
        }

        // Remove committed children from the tree (fires unmount, cleans effects)
        let children: Vec<NodeId> = self.renderer.children(container)[..count].to_vec();
        for child_id in children {
            self.renderer.remove(child_id);
        }

        // Drop the committed rows from the engine's tracking so subsequent
        // diffs only cover the active region. v1 commits only rows already
        // in scrollback, and the cursor is never in scrollback, so the
        // cursor-repositioning bytes the engine can emit are impossible
        // here.
        let bytes = self.engine.commit_scrolled(committed_height);
        debug_assert!(
            bytes.is_empty(),
            "v1 commits scrollback-only rows; cursor cannot be above them"
        );
    }

    /// Reclaim trailing blank rows after the frame has shrunk.
    ///
    /// Call this after a final [`render`](InlineRenderer::render) when
    /// content has been removed (e.g., clearing a text input before exit).
    /// Moves the cursor to the last row of actual content, erases
    /// everything below, and adjusts internal tracking so the terminal
    /// prompt appears immediately after the content.
    ///
    /// Returns escape sequences to write to the terminal. Returns an
    /// empty Vec if there are no trailing blank rows to reclaim.
    pub fn finalize(&mut self) -> Vec<u8> {
        self.engine.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use ratatui_core::{buffer::Buffer, layout::Rect};
    use ratatui_widgets::paragraph::Paragraph;

    struct TextBlock;

    impl Component for TextBlock {
        type State = Vec<String>;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let lines: Vec<ratatui_core::text::Line> = state
                .iter()
                .map(|s| ratatui_core::text::Line::raw(s.as_str()))
                .collect();
            let para = Paragraph::new(lines);
            ratatui_core::widgets::Widget::render(para, area, buf);
        }

        fn initial_state(&self) -> Option<Vec<String>> {
            Some(vec![])
        }
    }

    struct WrappingBlock;

    impl Component for WrappingBlock {
        type State = String;

        fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
            let text = ratatui_core::text::Text::raw(state.as_str());
            ratatui_core::widgets::Widget::render(crate::wrap::wrapping_paragraph(text), area, buf);
        }

        fn desired_height(&self, width: u16, state: &Self::State) -> Option<u16> {
            let text = ratatui_core::text::Text::raw(state.as_str());
            Some(crate::wrap::wrapped_line_count(&text, width))
        }

        fn initial_state(&self) -> Option<String> {
            Some(String::new())
        }
    }

    use eye_declare_engine::test_terminal::TestTerminal;

    #[test]
    fn first_render_empty_produces_nothing() {
        let mut ir = InlineRenderer::new_with_height(10, 24);
        let _id = ir.push(TextBlock);
        let output = ir.render();
        assert!(output.is_empty());
    }

    #[test]
    fn first_render_with_content_produces_output() {
        let mut ir = InlineRenderer::new_with_height(10, 24);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("hello".to_string());

        let output = ir.render();
        assert!(!output.is_empty());

        let s = String::from_utf8_lossy(&output);
        // Should contain DEC 2026 sync sequences
        assert!(s.contains("\x1b[?2026h"));
        assert!(s.contains("\x1b[?2026l"));
        // Should contain the text
        assert!(s.contains("hello"));
    }

    #[test]
    fn no_change_produces_minimal_output() {
        let mut ir = InlineRenderer::new_with_height(10, 24);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("hello".to_string());

        let _first = ir.render();
        let second = ir.render();
        // No content changes, but cursor positioning may be emitted
        // (hide cursor if no focused component has a cursor hint)
        let s = String::from_utf8_lossy(&second);
        // Should not contain any cell content or DEC 2026 sync
        assert!(!s.contains("\x1b[?2026h"));
    }

    #[test]
    fn growing_content_emits_newlines() {
        let mut ir = InlineRenderer::new_with_height(10, 24);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("line1".to_string());

        let _first = ir.render();

        // Add a second line
        ir.state_mut::<TextBlock>(id).push("line2".to_string());
        let second = ir.render();
        let s = String::from_utf8_lossy(&second);

        // Should contain a newline for growth
        assert!(s.contains('\n'));
        // Should contain the new line text
        assert!(s.contains("line2"));
    }

    #[test]
    fn first_render_burst_populates_scrollback() {
        let mut terminal = TestTerminal::new(20, 3);
        let mut ir = InlineRenderer::new_with_height(20, 3);
        let id = ir.push(TextBlock);
        let lines: Vec<String> = (0..10).map(|i| format!("line {i:02}")).collect();
        ir.state_mut::<TextBlock>(id).extend(lines.iter().cloned());

        let output = ir.render();
        terminal.feed(&output);

        assert_eq!(terminal.scrollback_lines(), lines[..7]);
        assert_eq!(terminal.viewport_lines(), lines[7..]);
    }

    #[test]
    fn full_width_rows_do_not_create_blank_scrollback_gaps() {
        let mut terminal = TestTerminal::new(10, 3);
        let mut ir = InlineRenderer::new_with_height(10, 3);
        let id = ir.push(TextBlock);
        let lines: Vec<String> = (0..8).map(|i| format!("row-{i:06}")).collect();
        assert!(lines.iter().all(|line| line.len() == 10));
        ir.state_mut::<TextBlock>(id).extend(lines.iter().cloned());

        terminal.feed(&ir.render());

        assert_eq!(terminal.scrollback_lines(), lines[..5]);
        assert_eq!(terminal.viewport_lines(), lines[5..]);
    }

    #[test]
    fn subsequent_burst_append_populates_scrollback_once() {
        let mut terminal = TestTerminal::new(20, 5);
        let mut ir = InlineRenderer::new_with_height(20, 5);
        let id = ir.push(TextBlock);
        let initial: Vec<String> = (0..3).map(|i| format!("line {i:02}")).collect();
        ir.state_mut::<TextBlock>(id).extend(initial);
        terminal.feed(&ir.render());

        let lines: Vec<String> = (0..12).map(|i| format!("line {i:02}")).collect();
        ir.state_mut::<TextBlock>(id).clear();
        ir.state_mut::<TextBlock>(id).extend(lines.iter().cloned());
        terminal.feed(&ir.render());

        assert_eq!(terminal.scrollback_lines(), lines[..7]);
        assert_eq!(terminal.viewport_lines(), lines[7..]);
    }

    #[test]
    fn subsequent_burst_insert_above_live_tail_populates_scrollback() {
        let mut terminal = TestTerminal::new(20, 5);
        let mut ir = InlineRenderer::new_with_height(20, 5);
        let id = ir.push(TextBlock);
        let footer = ["footer 0", "footer 1", "footer 2"];

        let mut initial: Vec<String> = (0..2).map(|i| format!("line {i:02}")).collect();
        initial.extend(footer.iter().map(|s| s.to_string()));
        ir.state_mut::<TextBlock>(id).extend(initial);
        terminal.feed(&ir.render());

        let mut lines: Vec<String> = (0..12).map(|i| format!("line {i:02}")).collect();
        lines.extend(footer.iter().map(|s| s.to_string()));
        ir.state_mut::<TextBlock>(id).clear();
        ir.state_mut::<TextBlock>(id).extend(lines.iter().cloned());
        terminal.feed(&ir.render());

        assert_eq!(terminal.scrollback_lines(), lines[..10]);
        assert_eq!(terminal.viewport_lines(), lines[10..]);
    }

    #[test]
    fn burst_insert_above_live_tail_survives_footer_removal_finalize() {
        let mut terminal = TestTerminal::new(20, 5);
        let mut ir = InlineRenderer::new_with_height(20, 5);
        let id = ir.push(TextBlock);
        let footer = ["footer 0", "footer 1", "footer 2"];

        let mut initial: Vec<String> = (0..2).map(|i| format!("line {i:02}")).collect();
        initial.extend(footer.iter().map(|s| s.to_string()));
        ir.state_mut::<TextBlock>(id).extend(initial);
        terminal.feed(&ir.render());

        let transcript: Vec<String> = (0..12).map(|i| format!("line {i:02}")).collect();
        let mut live_lines = transcript.clone();
        live_lines.extend(footer.iter().map(|s| s.to_string()));
        ir.state_mut::<TextBlock>(id).clear();
        ir.state_mut::<TextBlock>(id).extend(live_lines);
        terminal.feed(&ir.render());

        ir.state_mut::<TextBlock>(id).clear();
        ir.state_mut::<TextBlock>(id)
            .extend(transcript.iter().cloned());
        terminal.feed(&ir.render());
        terminal.feed(&ir.finalize());

        let mut actual = terminal.scrollback_lines();
        actual.extend(terminal.viewport_lines());
        actual.retain(|line| !line.is_empty());

        assert_eq!(actual, transcript);
    }

    #[test]
    fn wrapped_text_burst_populates_scrollback() {
        let mut terminal = TestTerminal::new(5, 3);
        let mut ir = InlineRenderer::new_with_height(5, 3);
        let id = ir.push(WrappingBlock);
        ir.state_mut::<WrappingBlock>(id)
            .push_str("aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp");

        terminal.feed(&ir.render());

        assert_eq!(
            terminal.scrollback_lines(),
            ["aa bb", "cc dd", "ee ff", "gg hh", "ii jj"]
        );
        assert_eq!(terminal.viewport_lines(), ["kk ll", "mm nn", "oo pp"]);
    }

    #[test]
    fn finalize_reclaims_trailing_blank_rows() {
        let mut ir = InlineRenderer::new_with_height(20, 24);
        let id = ir.push(TextBlock);
        // Render 3 lines
        ir.state_mut::<TextBlock>(id)
            .extend(["a", "b", "c"].map(String::from));
        let _first = ir.render();
        assert_eq!(ir.emitted_rows(), 3);

        // Shrink to 1 line
        ir.state_mut::<TextBlock>(id).truncate(1);
        let _second = ir.render();
        // emitted_rows hasn't changed — the rows are still claimed
        assert_eq!(ir.emitted_rows(), 3);

        // Finalize should reclaim the 2 trailing blank rows
        let output = ir.finalize();
        assert!(!output.is_empty());
        assert_eq!(ir.emitted_rows(), 1);
        // Should contain erase-to-end-of-screen
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("\x1b[J"));
    }

    #[test]
    fn finalize_noop_when_no_trailing_blanks() {
        let mut ir = InlineRenderer::new_with_height(20, 24);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("hello".to_string());
        let _first = ir.render();
        assert_eq!(ir.emitted_rows(), 1);

        // No shrinkage — finalize should be a no-op
        let output = ir.finalize();
        assert!(output.is_empty());
        assert_eq!(ir.emitted_rows(), 1);
    }

    #[test]
    fn finalize_respects_scrollback_boundary() {
        // Terminal height 3, content 5 rows → 2 rows in scrollback
        let mut ir = InlineRenderer::new_with_height(20, 3);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id)
            .extend(["a", "b", "c", "d", "e"].map(String::from));
        let _first = ir.render();
        assert_eq!(ir.emitted_rows(), 5);

        // Shrink to 1 row — frame height = 1, but scrolled_past = 2
        ir.state_mut::<TextBlock>(id).truncate(1);
        let _second = ir.render();

        // Finalize should only reclaim rows the cursor can reach.
        // scrolled_past = 5 - 3 = 2, so target_row = max(1, 2) = 2.
        // Rows 0-1 are in scrollback and untouchable.
        let output = ir.finalize();
        assert!(!output.is_empty());
        assert_eq!(ir.emitted_rows(), 2);
        let s = String::from_utf8_lossy(&output);
        assert!(s.contains("\x1b[J"));
    }

    /// A component with a 1-cell border (insets on all sides).
    /// Renders a border and passes children through.
    struct BorderBox;

    impl Component for BorderBox {
        type State = ();

        fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
            // Draw simple border chars
            if area.width >= 2 && area.height >= 2 {
                // Top border
                for x in area.x..area.x + area.width {
                    buf[(x, area.y)].set_char('─');
                }
                // Bottom border
                let bot = area.y + area.height - 1;
                for x in area.x..area.x + area.width {
                    buf[(x, bot)].set_char('─');
                }
                // Side borders
                for y in area.y..area.y + area.height {
                    buf[(area.x, y)].set_char('│');
                    buf[(area.x + area.width - 1, y)].set_char('│');
                }
            }
        }

        fn content_inset(&self, _state: &()) -> crate::insets::Insets {
            crate::insets::Insets::all(1) // 1-cell border all around
        }

        fn view(&self, _state: &(), children: Elements) -> Elements {
            children
        }
    }

    crate::impl_slot_children!(BorderBox);

    #[test]
    fn finalize_after_declarative_removal() {
        // Simulate: content (5 rows) + input box (3 rows) = 8 rows total.
        // On exit, input box is removed via rebuild. Finalize should
        // reclaim exactly 3 rows, preserving all 5 content rows.
        let mut ir = InlineRenderer::new_with_height(40, 24);
        let container = ir.push(crate::component::VStack);

        // Initial build: content + input
        let mut els = crate::element::Elements::new();
        let content = TextBlock;
        els.add(content).key("content");
        let input = TextBlock;
        els.add(input).key("input");
        ir.rebuild(container, els);

        // Set content: 5 lines
        let content_id = ir.find_by_key(container, "content").unwrap();
        ir.state_mut::<TextBlock>(content_id)
            .extend(["line 1", "line 2", "line 3", "line 4", "line 5"].map(String::from));

        // Set input: 3 lines
        let input_id = ir.find_by_key(container, "input").unwrap();
        ir.state_mut::<TextBlock>(input_id)
            .extend(["┌──────┐", "│ text │", "└──────┘"].map(String::from));

        let _first = ir.render();
        assert_eq!(ir.emitted_rows(), 8);

        // Rebuild without the input component (simulates exit)
        let mut els = crate::element::Elements::new();
        let content = TextBlock;
        els.add(content).key("content");
        ir.rebuild(container, els);

        // Content is still 5 lines, input removed
        let content_id = ir.find_by_key(container, "content").unwrap();
        assert_eq!(ir.state_mut::<TextBlock>(content_id).len(), 5);

        let _second = ir.render();
        // emitted_rows stays at 8
        assert_eq!(ir.emitted_rows(), 8);

        // Finalize should reclaim exactly 3 rows (the input box)
        let output = ir.finalize();
        assert!(!output.is_empty());
        assert_eq!(
            ir.emitted_rows(),
            5,
            "finalize should leave exactly 5 rows (content), not {}",
            ir.emitted_rows()
        );
    }

    #[test]
    fn finalize_with_bordered_input_removal() {
        // Simulate Atuin pattern: content (5 rows) + bordered input (3 rows).
        // The bordered input has 1-cell insets, so it's:
        //   border-top (1) + content (1) + border-bottom (1) = 3 rows.
        // Total: 8 rows. Remove the bordered input, finalize should
        // reclaim exactly 3 rows.
        let mut ir = InlineRenderer::new_with_height(40, 24);
        let container = ir.push(crate::component::VStack);

        // Initial build: content + bordered input
        let mut els = crate::element::Elements::new();
        els.add(TextBlock).key("content");
        let mut input_children = crate::element::Elements::new();
        input_children.add(TextBlock).key("input-text");
        els.add_with_children(BorderBox, input_children)
            .key("input-box");
        ir.rebuild(container, els);

        // Set content: 5 lines
        let content_id = ir.find_by_key(container, "content").unwrap();
        ir.state_mut::<TextBlock>(content_id)
            .extend(["line 1", "line 2", "line 3", "line 4", "line 5"].map(String::from));

        // Set input text: 1 line (inside 1-cell border = 3 rows total)
        let input_box_id = ir.find_by_key(container, "input-box").unwrap();
        let input_text_id = ir.find_by_key(input_box_id, "input-text").unwrap();
        ir.state_mut::<TextBlock>(input_text_id)
            .push("Type here...".to_string());

        let _first = ir.render();
        assert_eq!(
            ir.emitted_rows(),
            8,
            "should be 5 content + 3 bordered input = 8 rows"
        );

        // Rebuild without the input box (simulates exit)
        let mut els = crate::element::Elements::new();
        els.add(TextBlock).key("content");
        ir.rebuild(container, els);

        let content_id = ir.find_by_key(container, "content").unwrap();
        assert_eq!(ir.state_mut::<TextBlock>(content_id).len(), 5);

        let _second = ir.render();
        assert_eq!(ir.emitted_rows(), 8);

        // Finalize should reclaim exactly 3 rows
        let output = ir.finalize();
        assert!(!output.is_empty());
        assert_eq!(
            ir.emitted_rows(),
            5,
            "finalize should leave exactly 5 content rows, not {}",
            ir.emitted_rows()
        );
    }

    #[test]
    fn shrink_then_grow_does_not_emit_extra_newlines() {
        // Regression test: toggling a conditional element at the bottom
        // should not emit extra newlines each cycle, which would cause
        // the content to "climb" up the viewport.
        let mut ir = InlineRenderer::new_with_height(40, 24);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id)
            .extend(["line1", "line2", "line3"].map(String::from));

        let _first = ir.render();
        assert_eq!(ir.emitted_rows(), 3);

        // Shrink: remove last line (simulates conditional disappearing)
        ir.state_mut::<TextBlock>(id).pop();
        let _second = ir.render();
        assert_eq!(
            ir.emitted_rows(),
            3,
            "emitted_rows should not decrease on shrink"
        );

        // Grow back: add the line again (simulates conditional reappearing)
        ir.state_mut::<TextBlock>(id).push("line3".to_string());
        let _third = ir.render();
        assert_eq!(
            ir.emitted_rows(),
            3,
            "emitted_rows should stay at 3 — we already have the row claimed"
        );

        // Repeat the cycle to make sure it stays stable
        ir.state_mut::<TextBlock>(id).pop();
        let _r4 = ir.render();
        ir.state_mut::<TextBlock>(id).push("line3".to_string());
        let _r5 = ir.render();
        assert_eq!(
            ir.emitted_rows(),
            3,
            "emitted_rows must remain stable across repeated shrink/grow cycles"
        );
    }

    #[test]
    fn growth_emits_cr_before_newlines() {
        // Regression test: when the frame grows on a subsequent render,
        // the growth block must emit \r before \n so the terminal column
        // resets to 0. Without this, the cursor.col tracker diverges from
        // the real terminal column, causing the first cell on the new row
        // to be written at the wrong horizontal position.
        let mut ir = InlineRenderer::new_with_height(40, 24);
        let id = ir.push(TextBlock);
        ir.state_mut::<TextBlock>(id).push("hello".to_string());

        let first = ir.render();
        // First render writes "hello" — cursor advances past col 0
        let first_str = String::from_utf8_lossy(&first);
        assert!(first_str.contains("hello"));

        // Grow: add a second line
        ir.state_mut::<TextBlock>(id).push("world".to_string());
        let second = ir.render();
        let _second_str = String::from_utf8_lossy(&second);

        // The growth output must contain \r before the \n that claims
        // the new row. Find the newline that claims the row — it appears
        // outside the DEC 2026 sync region, before the escape sequences.
        // We check that a \r precedes the \n in the raw output.
        let raw = &second;
        if let Some(newline_pos) = raw.iter().position(|&b| b == b'\n') {
            assert!(
                newline_pos > 0 && raw[newline_pos - 1] == b'\r',
                "expected \\r immediately before the growth \\n, got byte {:?}",
                raw.get(newline_pos.wrapping_sub(1))
            );
        } else {
            panic!("expected a newline in the growth output");
        }
    }
}
