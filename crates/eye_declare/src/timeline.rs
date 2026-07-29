//! The synchronous runtime core: a timeline of committed blocks plus a
//! live tail, speaking [`Element`]s on one side and escape bytes on the
//! other.
//!
//! This is both the imperative escape hatch (sync loops, embedding) and
//! the layer the Elm runtime drives: `ctx.push` lands on [`Timeline::push`],
//! each frame ends in [`Timeline::present`].

use eye_declare_engine::Engine;
use eye_declare_engine::frame::Frame;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

use crate::element::Element;

/// A terminal-backed timeline. Blocks pushed become permanent output;
/// the tail is replaced wholesale on every [`present`](Timeline::present).
pub struct Timeline {
    engine: Engine,
}

impl Timeline {
    /// Create a timeline for the given content width and terminal height.
    /// Pass `u16::MAX` as `terminal_height` when no terminal is attached.
    pub fn new(width: u16, terminal_height: u16) -> Self {
        Self {
            engine: Engine::new(width, terminal_height),
        }
    }

    pub fn width(&self) -> u16 {
        self.engine.width()
    }

    pub fn set_terminal_height(&mut self, height: u16) {
        self.engine.set_terminal_height(height);
    }

    /// Commit a finished block above the live tail. The block is rendered
    /// once at the current width, becomes permanent terminal output, and
    /// never costs anything again. Irreversible, like `println!`.
    ///
    /// Consumes the element: a block is used exactly once.
    pub fn push(&mut self, block: impl Element) -> Vec<u8> {
        let buf = render_to_buffer(&block, self.engine.width());
        self.engine.commit(&buf)
    }

    /// Replace the live tail. Diffs against the previous tail; identical
    /// tails produce (nearly) no bytes, which is what makes re-presenting
    /// every frame the intended usage rather than a cost to avoid.
    ///
    /// The tail's [`cursor`](Element::cursor) hint positions the hardware
    /// cursor; `None` hides it.
    pub fn present(&mut self, tail: &impl Element) -> Vec<u8> {
        let width = self.engine.width();
        let height = tail.height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        tail.render(area, &mut buf);
        let cursor = tail.cursor(area);
        self.engine.present(Frame::new(buf), cursor)
    }

    /// Handle a terminal width change: erase the live region (committed
    /// blocks above it are untouched — they keep the terminal's own
    /// reflow, per the committed-is-immutable semantics) and reset
    /// tracking. Follow with a [`present`](Timeline::present) to repaint
    /// the tail at the new width.
    ///
    /// Prefer [`resize_anchored`](Timeline::resize_anchored) when a
    /// cursor position report is available: without one the erase relies
    /// on pre-reflow row arithmetic, which drifts on reflowing terminals.
    pub fn resize(&mut self, new_width: u16) -> Vec<u8> {
        self.engine.reset_region(new_width)
    }

    /// [`resize`](Timeline::resize) re-anchored by a cursor position
    /// report: `cursor` is the absolute `(col, row)` (0-based) the
    /// terminal reported *after* reflowing at the new width. The erase
    /// targets the region top exactly instead of trusting stale row
    /// arithmetic.
    pub fn resize_anchored(&mut self, new_width: u16, cursor: (u16, u16)) -> Vec<u8> {
        self.engine.reset_region_anchored(new_width, cursor)
    }

    /// Hand the terminal back to the shell: park the cursor at column 0
    /// on the row after the last content row, reclaiming any rows a
    /// shrunken or empty tail vacated (call after a final `present`).
    pub fn finalize(&mut self) -> Vec<u8> {
        self.engine.finalize()
    }
}

/// Render an element to an owned buffer at exactly its measured height.
fn render_to_buffer(el: &impl Element, width: u16) -> Buffer {
    let height = el.height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    if height > 0 {
        el.render(area, &mut buf);
    }
    buf
}
