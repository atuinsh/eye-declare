//! End-to-end: elements → Buffer → Engine → real escape bytes → VTE
//! terminal. Proves the new layer composes with the extracted engine, and
//! exercises the timeline shape (present a tail, commit rows above it)
//! ahead of the runtime existing.

use eye_declare_engine::Engine;
use eye_declare_engine::frame::Frame;
use eye_declare_engine::test_terminal::TestTerminal;
use eye_declare_next::{Element, Fluent, col, text};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

/// Render an element to an owned buffer at the given width.
fn to_frame(el: &impl Element, width: u16) -> Frame {
    let height = el.height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    el.render(area, &mut buf);
    Frame::new(buf)
}

#[test]
fn tail_reaches_the_terminal() {
    let tail = col().child(text("hello")).child(text("world"));

    let mut engine = Engine::new(10, 24);
    let mut term = TestTerminal::new(10, 24);
    term.feed(&engine.present(to_frame(&tail, 10), None));

    assert_eq!(term.viewport_lines()[0], "hello");
    assert_eq!(term.viewport_lines()[1], "world");
}

#[test]
fn tail_rerender_diffs_to_minimal_output() {
    let mut engine = Engine::new(10, 24);
    let mut term = TestTerminal::new(10, 24);

    let tail = |busy: bool| {
        col()
            .child(text("status"))
            .when(busy, |c| c.child(text("busy")))
    };

    term.feed(&engine.present(to_frame(&tail(true), 10), None));
    assert_eq!(term.viewport_lines()[1], "busy");

    // Identical frame → no bytes beyond cursor bookkeeping.
    let bytes = engine.present(to_frame(&tail(true), 10), None);
    assert!(bytes.len() <= 8, "identical tail should be near-empty diff");

    term.feed(&engine.present(to_frame(&tail(false), 10), None));
    assert_eq!(term.viewport_lines()[1], "");
}

#[test]
fn wrapped_text_measures_and_renders_consistently() {
    let tail = col().child(text("hello world, this wraps"));
    let width = 8;
    let frame = to_frame(&tail, width);
    assert!(frame.area().height >= 3);

    let mut engine = Engine::new(width, 24);
    let mut term = TestTerminal::new(8, 24);
    term.feed(&engine.present(frame, None));
    assert_eq!(term.viewport_lines()[0], "hello");
}

/// The v2 timeline shape, hand-driven: commit a block above a live tail by
/// presenting (block ++ tail) and slicing the block off tracking. This is
/// the mechanism the runtime's `ctx.push` will use.
#[test]
fn commit_block_above_live_tail() {
    let width = 12;
    let mut engine = Engine::new(width, 24);
    let mut term = TestTerminal::new(12, 24);

    // Frame 1: just the tail (an input placeholder).
    let tail = col().child(text("> input"));
    term.feed(&engine.present(to_frame(&tail, width), None));
    assert_eq!(term.viewport_lines()[0], "> input");

    // Commit a block: present block ++ tail, then slice the block rows.
    let block = col().child(text("you: hi"));
    let block_height = block.height(width);
    let stacked = col().child(block).child(text("> input"));
    term.feed(&engine.present(to_frame(&stacked, width), None));
    term.feed(&engine.commit_scrolled(block_height));

    assert_eq!(term.viewport_lines()[0], "you: hi");
    assert_eq!(term.viewport_lines()[1], "> input");

    // The tail keeps updating below the committed block.
    let tail2 = col().child(text("> inputx"));
    // After commit_scrolled the engine's frame origin moved up by
    // block_height; present the tail alone.
    term.feed(&engine.present(to_frame(&tail2, width), None));
    assert_eq!(term.viewport_lines()[0], "you: hi");
    assert_eq!(term.viewport_lines()[1], "> inputx");
}
