//! Resize behavior against a *reflowing* terminal (Ghostty, kitty,
//! iTerm2, WezTerm…). The engine's committed rows must survive width
//! changes: they are immutable and can never be repainted, so erasing
//! one is permanent data loss for the user.
//!
//! Each test mirrors the runtime's real call order on `Event::Resize`:
//! the terminal reflows first (that's what a resize *is* from the
//! app's point of view), then `set_terminal_height` + `reset_region` +
//! a fresh `present` at the new width.

use eye_declare_engine::Engine;
use eye_declare_engine::frame::Frame;
use eye_declare_engine::test_terminal::TestTerminal;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

fn buffer_with_lines(width: u16, lines: &[String]) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, lines.len() as u16));
    for (y, line) in lines.iter().enumerate() {
        buf.set_stringn(
            0,
            y as u16,
            line,
            width as usize,
            ratatui_core::style::Style::default(),
        );
    }
    buf
}

/// An Atuin-AI-shaped live tail: a full-width bordered input box plus a
/// status line. The border rows span the entire width, so they re-wrap
/// on narrow — the geometry that breaks stale row arithmetic.
fn tail_lines(w: usize) -> Vec<String> {
    vec![
        format!("┌{}┐", "─".repeat(w - 2)),
        format!("│ Type a message...{}│", " ".repeat(w - 20)),
        format!("└{}┘", "─".repeat(w - 2)),
        " Model: balanced".to_string(),
    ]
}

const COMMITTED: [&str; 4] = [" You", "  Hello!", " Atuin AI", "  Hey there!"];

struct Sim {
    term: TestTerminal,
    engine: Engine,
    height: usize,
    /// Cursor hint for presents: `Some` = visible cursor in the input
    /// (wrap-model resize path), `None` = hidden cursor parked on the
    /// region top (report-is-the-answer resize path, what Atuin AI's
    /// custom-drawn cursor uses).
    hint: Option<(u16, u16)>,
}

impl Sim {
    /// Paint the committed conversation and the live tail at `width`.
    fn start(width: usize, height: usize) -> Self {
        Self::start_with_hint(width, height, Some((3, 1)))
    }

    fn start_with_hint(width: usize, height: usize, hint: Option<(u16, u16)>) -> Self {
        let mut sim = Sim {
            term: TestTerminal::new(width, height),
            engine: Engine::new(width as u16, height as u16),
            height,
            hint,
        };
        let committed: Vec<String> = COMMITTED.iter().map(|s| s.to_string()).collect();
        let bytes = sim
            .engine
            .commit(&buffer_with_lines(width as u16, &committed));
        sim.term.feed(&bytes);
        sim.present_tail(width);
        sim
    }

    fn present_tail(&mut self, width: usize) {
        let tail = buffer_with_lines(width as u16, &tail_lines(width));
        let bytes = self.engine.present(Frame::new(tail), self.hint);
        self.term.feed(&bytes);
    }

    /// One user resize: the terminal reflows, then the app reacts the
    /// way `Runtime::resize_anchored` does — querying the (post-reflow)
    /// cursor position and re-anchoring the erase on it.
    fn resize(&mut self, width: usize) {
        self.term.resize_reflow(width, self.height);
        self.engine.set_terminal_height(self.height as u16);
        let (row, col) = self.term.cursor();
        let bytes = self
            .engine
            .reset_region_anchored(width as u16, (col as u16, row as u16));
        self.term.feed(&bytes);
        self.present_tail(width);
    }

    /// Everything on the terminal, scrollback then viewport.
    fn all_lines(&self) -> Vec<String> {
        let mut lines = self.term.scrollback_lines();
        lines.extend(self.term.viewport_lines());
        lines
    }

    fn assert_committed_intact(&self, context: &str) {
        let all = self.all_lines();
        for line in COMMITTED {
            assert_eq!(
                all.iter().filter(|l| l.as_str() == line).count(),
                1,
                "{context}: committed line {line:?} must appear exactly once, screen:\n{}",
                self.all_lines().join("\n")
            );
        }
    }

    /// The terminal document (scrollback then viewport, trailing blanks
    /// dropped) must be exactly: committed conversation, then the tail
    /// at the current width — no stale fragments, no duplicates.
    /// Committed rows may sit in scrollback: reflowing terminals pin the
    /// content end and scroll them out on narrow, and the erase+repaint
    /// leaves hard lines the terminal won't pull back on widen.
    fn assert_document_exact(&self, width: usize) {
        let mut expected: Vec<String> = COMMITTED.iter().map(|s| s.to_string()).collect();
        expected.extend(tail_lines(width).iter().map(|l| l.trim_end().to_string()));
        let mut actual = self.all_lines();
        while actual.last().is_some_and(|l| l.is_empty()) {
            actual.pop();
        }
        assert_eq!(actual, expected, "document after resize to width {width}");
    }
}

#[test]
fn narrow_keeps_committed_rows() {
    let mut sim = Sim::start(40, 12);
    sim.resize(34);
    sim.assert_committed_intact("after narrow 40->34");
    sim.assert_document_exact(34);
}

#[test]
fn widen_keeps_committed_rows() {
    let mut sim = Sim::start(40, 12);
    sim.resize(46);
    sim.assert_committed_intact("after widen 40->46");
    sim.assert_document_exact(46);
}

#[test]
fn widen_after_narrow_keeps_committed_rows() {
    let mut sim = Sim::start(40, 12);
    sim.resize(34);
    sim.assert_committed_intact("after narrow 40->34");
    sim.resize(40);
    sim.assert_committed_intact("after widen back 34->40");
    sim.assert_document_exact(40);
}

/// Consecutive drag steps on a content-end-pinning terminal can push a
/// border fragment of the old region into scrollback *before* the erase
/// runs — those rows are physically unreachable, so a drag may orphan
/// fragments in history. The screen itself must stay clean: committed
/// rows intact, and the viewport showing exactly one tail.
fn assert_viewport_clean(sim: &Sim, width: usize) {
    let mut vp = sim.term.viewport_lines();
    while vp.last().is_some_and(|l| l.is_empty()) {
        vp.pop();
    }
    let tail: Vec<String> = tail_lines(width)
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect();
    assert!(
        vp.len() >= tail.len() && vp[vp.len() - tail.len()..] == tail[..],
        "viewport must end with exactly the tail at width {width}, got:\n{}",
        vp.join("\n")
    );
    let box_tops = vp.iter().filter(|l| l.starts_with('┌')).count();
    assert_eq!(
        box_tops,
        1,
        "exactly one box top on screen, got:\n{}",
        vp.join("\n")
    );
}

#[test]
fn drag_narrow_keeps_committed_rows() {
    let mut sim = Sim::start(40, 12);
    for w in [38, 36, 34, 32, 30] {
        sim.resize(w);
        sim.assert_committed_intact(&format!("during drag at width {w}"));
    }
    assert_viewport_clean(&sim, 30);
}

#[test]
fn drag_narrow_then_widen_keeps_committed_rows() {
    let mut sim = Sim::start(40, 12);
    for w in [36, 32, 30, 34, 38, 40] {
        sim.resize(w);
        sim.assert_committed_intact(&format!("during drag at width {w}"));
    }
    assert_viewport_clean(&sim, 40);
}

/// A session whose screen is already full (shell history above the app):
/// commits and presents scroll the terminal, and on narrow the scroll can
/// cancel the growth signal. Committed rows must never be lost — at worst
/// they scroll into scrollback; a stale fragment above the tail is
/// tolerated in this edge, destroyed history is not.
#[test]
fn full_screen_drag_never_loses_committed_rows() {
    let width = 40;
    let height = 12;
    let mut sim = Sim {
        term: TestTerminal::new(width, height),
        engine: Engine::new(width as u16, height as u16),
        height,
        hint: Some((3, 1)),
    };
    // Fill the screen with shell history so the app starts at the bottom.
    for i in 0..height - 1 {
        sim.term.feed(format!("hist {i}\r\n").as_bytes());
    }
    let committed: Vec<String> = COMMITTED.iter().map(|s| s.to_string()).collect();
    let bytes = sim
        .engine
        .commit(&buffer_with_lines(width as u16, &committed));
    sim.term.feed(&bytes);
    sim.present_tail(width);
    sim.assert_committed_intact("before resizing");

    for w in [36, 32, 30, 34, 38, 40] {
        sim.resize(w);
        sim.assert_committed_intact(&format!("during full-screen drag at width {w}"));
    }
}

/// A terminal that does NOT reflow on resize (xterm-style truncation),
/// with a hidden cursor parked on the region top: the position report
/// is the region top on any terminal, so the erase stays exact.
#[test]
fn non_reflowing_terminal_stays_exact_when_parked() {
    let mut sim = Sim::start_with_hint(40, 12, None);
    for w in [34_usize, 30, 36, 40] {
        sim.term.resize(w, 12); // truncating resize, no reflow
        sim.engine.set_terminal_height(12);
        let (row, col) = sim.term.cursor();
        let bytes = sim
            .engine
            .reset_region_anchored(w as u16, (col as u16, row as u16));
        sim.term.feed(&bytes);
        sim.present_tail(w);
        sim.assert_committed_intact(&format!("no-reflow terminal at width {w}"));
    }
    sim.assert_document_exact(40);
}

/// The Atuin AI shape: the app draws its own cursor glyph and passes no
/// hint, so the hidden cursor parks on the region top and every resize
/// report is the erase target directly — exact even when the reflow
/// pins the content end (where a cursor left on the last row would
/// make the report useless).
#[test]
fn parked_cursor_drag_stays_exact_on_reflow_terminal() {
    let mut sim = Sim::start_with_hint(40, 12, None);
    for w in [38, 36, 34, 32, 30, 34, 38, 40] {
        sim.resize(w);
        sim.assert_committed_intact(&format!("parked drag at width {w}"));
    }
    sim.assert_document_exact(40);
}

/// Without a startup anchor (a custom driver that never reported the
/// cursor), the anchored resize can't detect reflow and must fall back
/// to the never-destructive assumption: committed rows survive narrow,
/// possibly at the cost of a stale fragment above the tail.
#[test]
fn unanchored_fallback_never_destroys() {
    let width = 40;
    let height = 12;
    let mut sim = Sim {
        term: TestTerminal::new(width, height),
        engine: Engine::new(width as u16, height as u16),
        height,
        hint: Some((3, 1)),
    };
    let committed: Vec<String> = COMMITTED.iter().map(|s| s.to_string()).collect();
    let bytes = sim
        .engine
        .commit(&buffer_with_lines(width as u16, &committed));
    sim.term.feed(&bytes);
    sim.present_tail(width);

    for w in [34, 30, 36, 40] {
        sim.resize(w);
        sim.assert_committed_intact(&format!("unanchored at width {w}"));
    }
}

/// Width and height changing together (the common case for a corner
/// drag). The reflow happens at the new width while the height shrinks
/// past the content.
#[test]
fn narrow_with_height_shrink_keeps_committed_rows() {
    let mut sim = Sim::start(40, 16);
    sim.term.resize_reflow(34, 11);
    sim.height = 11;
    sim.engine.set_terminal_height(11);
    let (row, col) = sim.term.cursor();
    let bytes = sim
        .engine
        .reset_region_anchored(34, (col as u16, row as u16));
    sim.term.feed(&bytes);
    sim.present_tail(34);
    sim.assert_committed_intact("after narrow + height shrink");
}

/// A tail taller than the screen (window shrunk under the input UI).
/// Resize repaints must not re-stream the overflow into scrollback —
/// before the resumed-repaint path, every drag event dumped a
/// screenful of duplicate rows into history and scrolled the screen.
#[test]
fn oversized_tail_resize_does_not_duplicate() {
    let width = 40;
    let height = 8;
    let mut term = TestTerminal::new(width, height);
    let mut engine = Engine::new(width as u16, height as u16);

    let tall: Vec<String> = (0..12).map(|i| format!("tail row {i}")).collect();
    term.feed(&engine.present(Frame::new(buffer_with_lines(width as u16, &tall)), None));
    let baseline = term.scrollback_lines().len();

    for w in [36_usize, 32, 36, 40] {
        term.resize_reflow(w, height);
        engine.set_terminal_height(height as u16);
        let (row, col) = term.cursor();
        let bytes = engine.reset_region_anchored(w as u16, (col as u16, row as u16));
        term.feed(&bytes);
        let bytes = engine.present(Frame::new(buffer_with_lines(w as u16, &tall)), None);
        term.feed(&bytes);

        // The visible screen is the bottom window of the tail.
        assert_eq!(
            term.viewport_lines(),
            tall[4..].to_vec(),
            "screen shows the tail's last rows at width {w}"
        );
    }
    // No overflow re-streamed: scrollback growth stays bounded (reflow
    // may shuffle a few rows, never a screenful per event).
    let growth = term.scrollback_lines().len().saturating_sub(baseline);
    assert!(
        growth <= 2,
        "scrollback grew by {growth} rows across 4 resizes:\n{}",
        term.scrollback_lines().join("\n")
    );
}
