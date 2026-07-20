//! Behavioral tests for the sync Timeline runtime: a conversation-shaped
//! push/present flow observed through the VTE test terminal.

use eye_declare::{Element, Timeline, col, spinner, text};
use eye_declare_engine::test_terminal::TestTerminal;
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

#[test]
fn conversation_flow() {
    let mut tl = Timeline::new(20, 24);
    let mut term = TestTerminal::new(20, 24);

    // Idle: just the input placeholder in the tail.
    term.feed(&tl.present(&text("> ")));
    assert_eq!(term.viewport_lines()[0], ">");

    // User submits: the turn becomes a block; tail shows the working state.
    term.feed(&tl.push(text("you: hello")));
    term.feed(
        &tl.present(
            &col()
                .child(spinner("thinking").done(true))
                .child(text("> ")),
        ),
    );
    assert_eq!(term.viewport_lines()[0], "you: hello");
    assert_eq!(term.viewport_lines()[1], "✓ thinking");
    assert_eq!(term.viewport_lines()[2], ">");

    // Agent turn completes: sealed above the tail, tail returns to idle.
    term.feed(&tl.push(text("ai: hi there!")));
    term.feed(&tl.present(&text("> ")));
    assert_eq!(term.viewport_lines()[0], "you: hello");
    assert_eq!(term.viewport_lines()[1], "ai: hi there!");
    assert_eq!(term.viewport_lines()[2], ">");

    // The tail keeps editing below the committed history.
    term.feed(&tl.present(&text("> more")));
    assert_eq!(term.viewport_lines()[2], "> more");
    assert_eq!(term.viewport_lines()[0], "you: hello");
}

#[test]
fn history_outgrows_terminal_into_scrollback() {
    // A tiny 4-row terminal: pushed blocks must flow into scrollback
    // intact (the burst-streaming path) while the tail stays live.
    let mut tl = Timeline::new(10, 4);
    let mut term = TestTerminal::new(10, 4);

    term.feed(&tl.present(&text("> ")));
    for i in 0..5 {
        term.feed(&tl.push(text(format!("block {i}"))));
        term.feed(&tl.present(&text("> ")));
    }

    let scrollback = term.scrollback_lines();
    assert!(scrollback.contains(&"block 0".to_string()));
    assert!(scrollback.contains(&"block 1".to_string()));
    // The most recent content and the tail are still in the viewport.
    let viewport = term.viewport_lines();
    assert!(viewport.contains(&"block 4".to_string()));
    assert!(viewport.contains(&"> ".trim().to_string()) || viewport.contains(&">".to_string()));
}

#[test]
fn tail_cursor_hint_reaches_terminal() {
    struct CursorAfter(String);
    impl Element for CursorAfter {
        fn height(&self, _w: u16) -> u16 {
            1
        }
        fn render(&self, area: Rect, buf: &mut Buffer) {
            buf.set_stringn(
                area.x,
                area.y,
                &self.0,
                area.width as usize,
                ratatui_core::style::Style::default(),
            );
        }
        fn cursor(&self, _area: Rect) -> Option<(u16, u16)> {
            Some((self.0.len() as u16, 0))
        }
    }

    let mut tl = Timeline::new(20, 24);
    let mut term = TestTerminal::new(20, 24);

    term.feed(&tl.push(text("history line")));
    term.feed(&tl.present(&CursorAfter("> hi".into())));

    // Cursor sits after the typed text, on the tail row (row 1: below the
    // committed block).
    assert_eq!(term.cursor(), (1, 4));
}

#[test]
fn empty_tail_and_finalize_hand_back_to_shell() {
    let mut tl = Timeline::new(10, 24);
    let mut term = TestTerminal::new(10, 24);

    term.feed(&tl.push(text("done: ok")));
    term.feed(&tl.present(&text("> ")));
    // Exit: clear the tail, reclaim its rows.
    term.feed(&tl.present(&eye_declare::empty()));
    term.feed(&tl.finalize());

    assert_eq!(term.viewport_lines()[0], "done: ok");
    assert_eq!(term.viewport_lines()[1], "");
    // Cursor parked at the row right after content, column 0 — where the
    // shell prompt will land.
    assert_eq!(term.cursor(), (1, 0));
}

/// The streaming-agent seal shape with a turn taller than the terminal:
/// the tail grows past the screen (rows burst-stream into scrollback),
/// then the same content is pushed as a block when the turn completes.
/// Every line must land in the terminal exactly once — a stale seal path
/// that re-streams the overlap duplicates the transcript and jumps the
/// screen by the block height.
#[test]
fn sealing_a_tail_taller_than_the_terminal_does_not_duplicate() {
    let mut tl = Timeline::new(20, 6);
    let mut term = TestTerminal::new(20, 6);

    let lines: Vec<String> = (0..12).map(|i| format!("reply-line-{i:02}")).collect();

    // Stream: the turn grows row by row, input tail below.
    term.feed(&tl.present(&text("> ")));
    for shown in 1..=lines.len() {
        let turn = lines[..shown].join("\n");
        term.feed(&tl.present(&col().child(text(turn)).child(text("> "))));
    }

    // Seal: the finished turn becomes a committed block; tail returns
    // to just the input.
    term.feed(&tl.push(text(lines.join("\n"))));
    term.feed(&tl.present(&text("> ")));

    let all = [term.scrollback_lines(), term.viewport_lines()].concat();
    for line in &lines {
        let count = all.iter().filter(|l| *l == line).count();
        assert_eq!(
            count, 1,
            "{line} should appear exactly once, appears {count}×:\n{all:#?}"
        );
    }

    // The transcript ends with the tail directly under the last reply
    // line — no newline burst in between.
    let last_content = all
        .iter()
        .rposition(|l| l == "reply-line-11")
        .expect("last reply line missing");
    let tail_row = all
        .iter()
        .rposition(|l| l.starts_with('>'))
        .expect("tail missing");
    assert_eq!(
        tail_row,
        last_content + 1,
        "tail should sit right below the sealed turn:\n{all:#?}"
    );
}
