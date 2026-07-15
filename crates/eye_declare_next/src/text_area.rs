//! Editable multi-line text: state as a plain model value (strict Elm,
//! bake-off O1), view as a borrowing element.
//!
//! Division of labor, as validated by the spike's Port 3A:
//!
//! - [`TextAreaState`] owns content + cursor and applies *ordinary editing*
//!   ([`handle`](TextAreaState::handle)): characters, backspace/delete,
//!   arrows, home/end, paste. Wire it to the keymap's `fallthrough`.
//! - **Policy keys are not its business.** Enter-submits vs
//!   Shift+Enter-newline vs Tab-completion are app keymap bindings whose
//!   `update` arms call [`take_text`](TextAreaState::take_text) /
//!   [`insert_newline`](TextAreaState::insert_newline) / etc.
//! - [`text_area`] renders the state, keeps the cursor row in view within
//!   [`max_height`](TextArea::max_height), and reports the hardware cursor
//!   while its focus handle is focused.
//!
//! Editing is grapheme-aware (the cursor never lands inside an emoji or
//! combining sequence) and cursor columns are display-width-aware (CJK,
//! wide emoji). Long lines are not soft-wrapped yet; they render truncated
//! at the width (soft wrap is a planned follow-up).

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::element::Element;
use crate::focus::FocusHandle;
use crate::input::InputEvent;

/// Multi-line editing state. Lives in the app model; `update` mutates it,
/// the view borrows it.
pub struct TextAreaState {
    /// Always at least one line.
    lines: Vec<String>,
    /// Cursor line index.
    line: usize,
    /// Cursor column as a *grapheme* index within the line.
    col: usize,
}

impl Default for TextAreaState {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            line: 0,
            col: 0,
        }
    }
}

fn grapheme_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Byte offset of the `col`-th grapheme (or end of string).
fn byte_at(s: &str, col: usize) -> usize {
    s.grapheme_indices(true)
        .nth(col)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

impl TextAreaState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply an ordinary editing event. Policy keys (Enter, Tab) and any
    /// key carrying Ctrl/Alt are ignored — those belong to the keymap.
    pub fn handle(&mut self, event: &InputEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        match event {
            InputEvent::Key(k) => {
                if k.modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return;
                }
                match k.code {
                    KeyCode::Char(c) => self.insert_char(c),
                    KeyCode::Backspace => self.backspace(),
                    KeyCode::Delete => self.delete(),
                    KeyCode::Left => self.move_left(),
                    KeyCode::Right => self.move_right(),
                    KeyCode::Up => self.move_vertical(-1),
                    KeyCode::Down => self.move_vertical(1),
                    KeyCode::Home => self.col = 0,
                    KeyCode::End => self.col = grapheme_count(&self.lines[self.line]),
                    _ => {}
                }
            }
            InputEvent::Paste(s) => self.insert_str(s),
        }
    }

    fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.line];
        let at = byte_at(line, self.col);
        line.insert(at, c);
        self.col += 1;
    }

    /// Insert text at the cursor; newlines split lines.
    pub fn insert_str(&mut self, s: &str) {
        for (i, part) in s.split('\n').enumerate() {
            if i > 0 {
                self.insert_newline();
            }
            if !part.is_empty() {
                let line = &mut self.lines[self.line];
                let at = byte_at(line, self.col);
                line.insert_str(at, part);
                self.col += grapheme_count(part);
            }
        }
    }

    /// Split the current line at the cursor.
    pub fn insert_newline(&mut self) {
        let line = &mut self.lines[self.line];
        let at = byte_at(line, self.col);
        let rest = line.split_off(at);
        self.lines.insert(self.line + 1, rest);
        self.line += 1;
        self.col = 0;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            let line = &mut self.lines[self.line];
            let start = byte_at(line, self.col - 1);
            let end = byte_at(line, self.col);
            line.replace_range(start..end, "");
            self.col -= 1;
        } else if self.line > 0 {
            // Join with the previous line.
            let removed = self.lines.remove(self.line);
            self.line -= 1;
            self.col = grapheme_count(&self.lines[self.line]);
            self.lines[self.line].push_str(&removed);
        }
    }

    fn delete(&mut self) {
        let count = grapheme_count(&self.lines[self.line]);
        if self.col < count {
            let line = &mut self.lines[self.line];
            let start = byte_at(line, self.col);
            let end = byte_at(line, self.col + 1);
            line.replace_range(start..end, "");
        } else if self.line + 1 < self.lines.len() {
            let next = self.lines.remove(self.line + 1);
            self.lines[self.line].push_str(&next);
        }
    }

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.line > 0 {
            self.line -= 1;
            self.col = grapheme_count(&self.lines[self.line]);
        }
    }

    fn move_right(&mut self) {
        if self.col < grapheme_count(&self.lines[self.line]) {
            self.col += 1;
        } else if self.line + 1 < self.lines.len() {
            self.line += 1;
            self.col = 0;
        }
    }

    fn move_vertical(&mut self, delta: isize) {
        let target = self.line.saturating_add_signed(delta);
        if target < self.lines.len() {
            self.line = target;
            self.col = self.col.min(grapheme_count(&self.lines[self.line]));
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = text.split('\n').map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.line = self.lines.len() - 1;
        self.col = grapheme_count(&self.lines[self.line]);
    }

    /// Take the content and reset to empty (the submit path).
    pub fn take_text(&mut self) -> String {
        let text = self.text();
        *self = Self::default();
        text
    }

    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Cursor position as `(line, grapheme column)`.
    pub fn cursor(&self) -> (usize, usize) {
        (self.line, self.col)
    }

    /// Display-width column of the cursor (CJK/emoji-aware).
    fn cursor_display_col(&self) -> u16 {
        let line = &self.lines[self.line];
        let at = byte_at(line, self.col);
        line[..at].width() as u16
    }
}

/// The borrowing view over a [`TextAreaState`].
pub struct TextArea<'a> {
    state: &'a TextAreaState,
    placeholder: String,
    style: Style,
    placeholder_style: Style,
    focus: Option<FocusHandle>,
    max_height: u16,
}

pub fn text_area(state: &TextAreaState) -> TextArea<'_> {
    TextArea {
        state,
        placeholder: String::new(),
        style: Style::default(),
        placeholder_style: Style::default(),
        focus: None,
        max_height: u16::MAX,
    }
}

impl<'a> TextArea<'a> {
    /// Shown (in `placeholder_style`) while the content is blank.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }

    /// Report the hardware cursor while this handle is focused.
    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    /// Cap the rendered height; the window scrolls to keep the cursor row
    /// visible.
    pub fn max_height(mut self, rows: u16) -> Self {
        self.max_height = rows.max(1);
        self
    }

    /// First visible line given the rendered height (keeps the cursor row
    /// in the window, pinned toward the bottom when content overflows).
    fn first_visible(&self, height: u16) -> usize {
        self.state
            .line
            .saturating_sub(height.saturating_sub(1) as usize)
    }
}

impl Element for TextArea<'_> {
    fn height(&self, _width: u16) -> u16 {
        (self.state.line_count() as u16).clamp(1, self.max_height)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if self.state.is_blank() && !self.placeholder.is_empty() {
            buf.set_stringn(
                area.x,
                area.y,
                &self.placeholder,
                area.width as usize,
                self.placeholder_style,
            );
            return;
        }

        let first = self.first_visible(area.height);
        for (row, line) in self.state.lines.iter().skip(first).enumerate() {
            if row as u16 >= area.height {
                break;
            }
            buf.set_stringn(
                area.x,
                area.y + row as u16,
                line,
                area.width as usize,
                self.style,
            );
        }
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        let focused = self.focus.as_ref().is_some_and(FocusHandle::is_focused);
        if !focused {
            return None;
        }
        let first = self.first_visible(area.height);
        let row = (self.state.line - first) as u16;
        let col = self
            .state
            .cursor_display_col()
            .min(area.width.saturating_sub(1));
        Some((col, row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn press(state: &mut TextAreaState, code: KeyCode) {
        state.handle(&InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn type_str(state: &mut TextAreaState, s: &str) {
        for c in s.chars() {
            press(state, KeyCode::Char(c));
        }
    }

    #[test]
    fn typing_and_text_roundtrip() {
        let mut s = TextAreaState::new();
        type_str(&mut s, "hello");
        assert_eq!(s.text(), "hello");
        assert_eq!(s.cursor(), (0, 5));
    }

    #[test]
    fn newline_split_and_backspace_join() {
        let mut s = TextAreaState::new();
        type_str(&mut s, "ab");
        press(&mut s, KeyCode::Left);
        s.insert_newline();
        assert_eq!(s.text(), "a\nb");
        assert_eq!(s.cursor(), (1, 0));

        press(&mut s, KeyCode::Backspace);
        assert_eq!(s.text(), "ab");
        assert_eq!(s.cursor(), (0, 1));
    }

    #[test]
    fn backspace_removes_whole_grapheme() {
        let mut s = TextAreaState::new();
        // Family emoji: one grapheme, multiple codepoints (ZWJ sequence).
        s.insert_str("a👩‍👩‍👦b");
        assert_eq!(s.cursor(), (0, 3));

        press(&mut s, KeyCode::Left); // before 'b'
        press(&mut s, KeyCode::Backspace); // removes the whole family
        assert_eq!(s.text(), "ab");
        assert_eq!(s.cursor(), (0, 1));
    }

    #[test]
    fn arrows_navigate_graphemes_and_lines() {
        let mut s = TextAreaState::new();
        s.insert_str("日本\nx");
        assert_eq!(s.cursor(), (1, 1));

        press(&mut s, KeyCode::Up);
        // Column clamps to grapheme count of the shorter... "日本" has 2.
        assert_eq!(s.cursor(), (0, 1));

        press(&mut s, KeyCode::End);
        assert_eq!(s.cursor(), (0, 2));

        press(&mut s, KeyCode::Right); // wraps to next line start
        assert_eq!(s.cursor(), (1, 0));
    }

    #[test]
    fn cursor_display_col_is_width_aware() {
        let mut s = TextAreaState::new();
        s.insert_str("日本x");
        press(&mut s, KeyCode::Home);
        press(&mut s, KeyCode::Right);
        press(&mut s, KeyCode::Right);
        // Two CJK chars = display width 4.
        assert_eq!(s.cursor_display_col(), 4);
    }

    #[test]
    fn ctrl_chars_are_ignored() {
        let mut s = TextAreaState::new();
        s.handle(&InputEvent::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )));
        assert!(s.is_blank());
    }

    #[test]
    fn paste_with_newlines() {
        let mut s = TextAreaState::new();
        s.handle(&InputEvent::Paste("one\ntwo".into()));
        assert_eq!(s.text(), "one\ntwo");
        assert_eq!(s.cursor(), (1, 3));
    }

    #[test]
    fn take_text_resets() {
        let mut s = TextAreaState::new();
        type_str(&mut s, "hi");
        assert_eq!(s.take_text(), "hi");
        assert!(s.is_blank());
        assert_eq!(s.cursor(), (0, 0));
    }

    #[test]
    fn max_height_window_follows_cursor() {
        let mut s = TextAreaState::new();
        s.insert_str("l0\nl1\nl2\nl3");
        let ta = text_area(&s).max_height(2);
        assert_eq!(Element::height(&ta, 10), 2);

        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        ta.render(area, &mut buf);
        // Cursor is on l3; window shows l2, l3.
        assert_eq!(buf[(1, 0)].symbol(), "2");
        assert_eq!(buf[(1, 1)].symbol(), "3");
    }

    #[test]
    fn cursor_reported_only_when_focused() {
        use crate::focus::Focus;

        let mut s = TextAreaState::new();
        type_str(&mut s, "hi");
        let focus = Focus::new();
        let handle = focus.handle();

        let area = Rect::new(0, 0, 10, 1);
        assert_eq!(text_area(&s).cursor(area), None);
        assert_eq!(text_area(&s).track_focus(&handle).cursor(area), None);

        handle.focus();
        assert_eq!(
            text_area(&s).track_focus(&handle).cursor(area),
            Some((2, 0))
        );
    }
}
