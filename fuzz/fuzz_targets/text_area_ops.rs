//! Model/invariant fuzz for `TextAreaState`: arbitrary editing sequences
//! (including multi-byte, combining, and wide characters) must keep the
//! cursor on a real grapheme boundary of a real line, keep the line
//! structure consistent with `text()`, and render/measure without panics.
//!
//! Literal '\n'/'\r' in `Char` events are excluded: `insert_char` trusts
//! the keymap layer to route newlines through `insert_newline`.

#![no_main]

use arbitrary::Arbitrary;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eye_declare::{Element, InputEvent, TextAreaState, text_area};
use libfuzzer_sys::fuzz_target;
use ratatui_core::{buffer::Buffer, layout::Rect};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Arbitrary)]
enum Op {
    Char(char),
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Newline,
    Paste(String),
    SetText(String),
    Take,
}

fn key(code: KeyCode) -> InputEvent {
    InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fuzz_target!(|ops: Vec<Op>| {
    let mut state = TextAreaState::new();

    for op in ops {
        match op {
            Op::Char(c) => {
                if c == '\n' || c == '\r' {
                    continue;
                }
                state.handle(&key(KeyCode::Char(c)));
            }
            Op::Backspace => state.handle(&key(KeyCode::Backspace)),
            Op::Delete => state.handle(&key(KeyCode::Delete)),
            Op::Left => state.handle(&key(KeyCode::Left)),
            Op::Right => state.handle(&key(KeyCode::Right)),
            Op::Up => state.handle(&key(KeyCode::Up)),
            Op::Down => state.handle(&key(KeyCode::Down)),
            Op::Home => state.handle(&key(KeyCode::Home)),
            Op::End => state.handle(&key(KeyCode::End)),
            Op::Newline => state.insert_newline(),
            Op::Paste(s) => {
                let s: String = s.chars().filter(|&c| c != '\r').collect();
                state.handle(&InputEvent::Paste(s));
            }
            Op::SetText(s) => {
                let s: String = s.chars().filter(|&c| c != '\r').collect();
                state.set_text(&s);
            }
            Op::Take => {
                state.take_text();
            }
        }

        // Cursor points at a real position of a real line.
        let text = state.text();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(
            lines.len(),
            state.line_count(),
            "line_count disagrees with text(): {text:?}",
        );
        let (line, col) = state.cursor();
        assert!(line < lines.len(), "cursor line {line} out of bounds");
        let max_col = lines[line].graphemes(true).count();
        assert!(
            col <= max_col,
            "cursor col {col} past end ({max_col}) of line {line}: {text:?}",
        );

        // Measurement and rendering hold up at awkward widths.
        for width in [1u16, 7, 30] {
            let element = text_area(&state);
            let height = element.height(width);
            assert!(height > 0, "text area must always claim a row");
            let area = Rect::new(0, 0, width, height.min(200));
            let mut buf = Buffer::empty(area);
            element.render(area, &mut buf);
            element.cursor(area);
        }
    }
});
