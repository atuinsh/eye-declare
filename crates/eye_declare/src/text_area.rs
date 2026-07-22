//! Editable multi-line text: state as a plain model value (strict Elm),
//! view as a borrowing element.
//!
//! Division of labor:
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
//! wide emoji). Long lines soft-wrap at the render width by default
//! (character wrapping, like the terminal's own; disable with
//! [`wrap(false)`](TextArea::wrap) to truncate instead). Wrap layout and
//! cursor mapping share one function, so they cannot disagree.

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
        // Recount instead of incrementing: a combining mark (e.g. U+0301
        // after `e`) merges into the previous grapheme, so the insertion
        // may not add a cursor position at all.
        self.col = grapheme_count(&line[..at + c.len_utf8()]);
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
                // Recount like insert_char: if the part starts with a
                // combining mark it merges into the preceding grapheme,
                // so adding grapheme_count(part) would overshoot the end
                // of the line.
                self.col = grapheme_count(&line[..at + part.len()]);
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
    wrap: bool,
}

pub fn text_area(state: &TextAreaState) -> TextArea<'_> {
    TextArea {
        state,
        placeholder: String::new(),
        style: Style::default(),
        placeholder_style: Style::default(),
        focus: None,
        max_height: u16::MAX,
        wrap: true,
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

    /// Soft-wrap long logical lines at the render width (default: on).
    /// Disabled, long lines truncate at the width.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// The cursor's visual position: `(visual row within all wrapped
    /// content, display column)`. The single source of truth that
    /// `render`, `cursor`, and the scroll window all derive from — wrap
    /// layout and cursor math can't disagree.
    fn visual_cursor(&self, width: u16) -> (usize, u16) {
        if !self.wrap {
            return (
                self.state.line,
                self.state.cursor_display_col().min(width.saturating_sub(1)),
            );
        }

        let rows_above: usize = self.state.lines[..self.state.line]
            .iter()
            .map(|l| wrap_line(l, width).len())
            .sum();

        let line = &self.state.lines[self.state.line];
        let segments = wrap_line(line, width);
        let offset = byte_at(line, self.state.col);

        // The segment containing the cursor: the first whose end is past
        // the offset. An offset at a full row's boundary belongs to the
        // next row (where the next grapheme would land) — except at the
        // very end of the line, where it stays on the last row, clamped.
        let mut segment = segments.len() - 1;
        for (i, &(_start, end)) in segments.iter().enumerate() {
            if offset < end {
                segment = i;
                break;
            }
        }

        let (start, _end) = segments[segment];
        let col = (line[start..offset].width() as u16).min(width.saturating_sub(1));
        (rows_above + segment, col)
    }

    /// First visible visual row: keeps the cursor row in the window,
    /// pinned toward the bottom when content overflows.
    fn window_start(&self, width: u16, height: u16) -> usize {
        let (cursor_row, _) = self.visual_cursor(width);
        cursor_row.saturating_sub(height.saturating_sub(1) as usize)
    }
}

impl Element for TextArea<'_> {
    fn height(&self, width: u16) -> u16 {
        if width == 0 {
            return 0;
        }
        let rows = if self.wrap {
            self.state
                .lines
                .iter()
                .map(|l| wrap_line(l, width).len())
                .sum::<usize>()
        } else {
            self.state.line_count()
        };
        // min() before the cast: content past 65,535 visual rows must
        // saturate, not wrap around to a tiny height.
        (rows.min(u16::MAX as usize) as u16).clamp(1, self.max_height)
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

        let first = self.window_start(area.width, area.height);
        let mut visual_row = 0usize;
        let mut y = 0u16;

        'lines: for line in &self.state.lines {
            let segments = if self.wrap {
                wrap_line(line, area.width)
            } else {
                vec![(0, line.len())]
            };
            for &(start, end) in &segments {
                if visual_row >= first {
                    if y >= area.height {
                        break 'lines;
                    }
                    buf.set_stringn(
                        area.x,
                        area.y + y,
                        &line[start..end],
                        area.width as usize,
                        self.style,
                    );
                    y += 1;
                }
                visual_row += 1;
            }
        }
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        let focused = self.focus.as_ref().is_some_and(FocusHandle::is_focused);
        if !focused {
            return None;
        }
        let (cursor_row, col) = self.visual_cursor(area.width);
        let first = self.window_start(area.width, area.height);
        Some((col, (cursor_row - first) as u16))
    }
}

/// Split one logical line into visual rows at `width`: `(byte_start,
/// byte_end)` per row. Character wrapping (like the terminal's own), never
/// inside a grapheme, display-width aware — a wide grapheme that doesn't
/// fit moves whole to the next row. An empty line is one empty row.
///
/// (Word wrap would be a drop-in replacement here if wanted later; keeping
/// the layout function tiny and cursor-exact won out for now.)
fn wrap_line(line: &str, width: u16) -> Vec<(usize, usize)> {
    if width == 0 {
        return vec![(0, line.len())];
    }

    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut used = 0u16;

    for (idx, grapheme) in line.grapheme_indices(true) {
        let gw = grapheme.width() as u16;
        // `used > 0` keeps a grapheme wider than the whole width on a row
        // of its own rather than looping forever.
        if used + gw > width && used > 0 {
            segments.push((start, idx));
            start = idx;
            used = 0;
        }
        used += gw;
    }

    segments.push((start, line.len()));
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::focus::Focus;

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

    // ── Soft wrap ──────────────────────────────────────────────────

    fn rendered(el: &TextArea<'_>, width: u16) -> Vec<String> {
        let height = Element::height(el, width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        (0..height)
            .map(|y| {
                let mut line: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
                while line.ends_with(' ') {
                    line.pop();
                }
                line
            })
            .collect()
    }

    #[test]
    fn wrap_line_splits_at_width() {
        assert_eq!(wrap_line("abcdef", 4), vec![(0, 4), (4, 6)]);
        assert_eq!(wrap_line("", 4), vec![(0, 0)]);
        assert_eq!(wrap_line("ab", 4), vec![(0, 2)]);
    }

    #[test]
    fn wrap_line_respects_wide_graphemes() {
        // Each CJK char is display width 2: "日本語" at width 4 → "日本" | "語".
        let s = "日本語";
        let segs = wrap_line(s, 4);
        assert_eq!(segs.len(), 2);
        assert_eq!(&s[segs[0].0..segs[0].1], "日本");
        assert_eq!(&s[segs[1].0..segs[1].1], "語");

        // Width 3 can't fit two wide chars per row: one per row.
        assert_eq!(wrap_line(s, 3).len(), 3);
    }

    #[test]
    fn long_line_wraps_in_render_and_height() {
        let mut s = TextAreaState::new();
        s.insert_str("abcdefgh");
        let ta = text_area(&s);
        assert_eq!(Element::height(&ta, 4), 2);
        assert_eq!(rendered(&ta, 4), vec!["abcd", "efgh"]);
    }

    #[test]
    fn cursor_maps_into_wrapped_rows() {
        let mut s = TextAreaState::new();
        s.insert_str("abcdefgh");
        // Move cursor to after "abcde" (col 5) → visual row 1, col 1.
        press(&mut s, KeyCode::Home);
        for _ in 0..5 {
            press(&mut s, KeyCode::Right);
        }

        let focus = Focus::new();
        let handle = focus.handle();
        handle.focus();
        let ta = text_area(&s).track_focus(&handle);
        let area = Rect::new(0, 0, 4, Element::height(&ta, 4));
        assert_eq!(ta.cursor(area), Some((1, 1)));
    }

    #[test]
    fn cursor_at_exact_row_boundary_lands_on_next_row() {
        let mut s = TextAreaState::new();
        s.insert_str("abcdefgh");
        press(&mut s, KeyCode::Home);
        for _ in 0..4 {
            press(&mut s, KeyCode::Right);
        }
        // Offset 4 is the boundary between full row 0 and row 1: the next
        // grapheme would land at row 1 col 0, so that's where the cursor is.
        let focus = Focus::new();
        let handle = focus.handle();
        handle.focus();
        let ta = text_area(&s).track_focus(&handle);
        let area = Rect::new(0, 0, 4, Element::height(&ta, 4));
        assert_eq!(ta.cursor(area), Some((0, 1)));
    }

    #[test]
    fn cursor_at_end_of_exactly_full_line_clamps() {
        let mut s = TextAreaState::new();
        s.insert_str("abcd");
        // End of a line that exactly fills its row: no next row exists, so
        // the cursor clamps to the last cell (documented edge).
        let focus = Focus::new();
        let handle = focus.handle();
        handle.focus();
        let ta = text_area(&s).track_focus(&handle);
        let area = Rect::new(0, 0, 4, 1);
        assert_eq!(ta.cursor(area), Some((3, 0)));
    }

    #[test]
    fn window_follows_visual_cursor_through_wrapped_content() {
        let mut s = TextAreaState::new();
        // One long line -> 4 visual rows at width 4, plus a second line.
        s.insert_str("aaaabbbbccccdddd\nend");
        // Cursor is at the end of "end": visual row 4 (rows 0-3 are the
        // wrapped first line... row 4 would be "dddd"'s boundary; "end" is
        // row 4). With max_height 2 the window shows the last two rows.
        let ta = text_area(&s).max_height(2);
        assert_eq!(Element::height(&ta, 4), 2);
        assert_eq!(rendered(&ta, 4), vec!["dddd", "end"]);
    }

    #[test]
    fn no_wrap_mode_truncates() {
        let mut s = TextAreaState::new();
        s.insert_str("abcdefgh");
        let ta = text_area(&s).wrap(false);
        assert_eq!(Element::height(&ta, 4), 1);
        assert_eq!(rendered(&ta, 4), vec!["abcd"]);
    }
}
#[cfg(test)]
mod combining_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn press(state: &mut TextAreaState, c: char) {
        state.handle(&crate::InputEvent::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }

    #[test]
    fn combining_mark_merges_without_advancing_cursor() {
        let mut state = TextAreaState::new();
        press(&mut state, 'e');
        assert_eq!(state.cursor(), (0, 1));
        // U+0301 combines with 'e' into one grapheme: same cursor cell.
        press(&mut state, '\u{0301}');
        assert_eq!(state.cursor(), (0, 1), "combining mark must not advance");
        // One backspace removes the whole grapheme.
        state.handle(&crate::InputEvent::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )));
        assert!(state.is_blank());
    }

    /// Found by fuzzing (fuzz/fuzz_targets/text_area_ops.rs): pasting text
    /// that begins with a combining mark merges it into the grapheme
    /// before the cursor, so advancing by the pasted text's own grapheme
    /// count overshoots the end of the line.
    #[test]
    fn paste_starting_with_combining_mark_keeps_cursor_in_bounds() {
        let mut state = TextAreaState::new();
        press(&mut state, 'e');
        state.handle(&InputEvent::Paste("\u{301}x".into()));

        let (line, col) = state.cursor();
        assert_eq!(line, 0);
        // "e" + U+0301 merge: the line is two graphemes, cursor at its end.
        assert_eq!(state.text(), "e\u{301}x");
        assert_eq!(col, 2, "cursor past the end of the line");
    }
}
