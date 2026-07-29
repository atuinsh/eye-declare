//! A minimal VTE-backed terminal emulator for testing engine output.
//!
//! Feed it the bytes the engine produces and assert on the resulting
//! viewport and scrollback contents. Models exactly the terminal behaviors
//! the engine's correctness depends on: pending-wrap semantics, linefeed
//! scrolling into scrollback, and relative CSI cursor movement.
//!
//! Enabled with the `test-util` feature. Moved verbatim from
//! `eye_declare`'s inline-renderer test module so downstream users can
//! snapshot-test their own UIs the same way the engine tests itself.

/// A fake terminal: feed bytes, inspect viewport and scrollback.
pub struct TestTerminal {
    parser: vte::Parser,
    width: usize,
    height: usize,
    cursor_row: usize,
    cursor_col: usize,
    pending_wrap: bool,
    viewport: Vec<Vec<char>>,
    /// Per-viewport-row soft-wrap flag: `wrapped[r]` means row `r`
    /// overflowed and continues on row `r + 1` (set only by auto-wrap,
    /// never by an explicit linefeed). This is what lets
    /// [`resize_reflow`](TestTerminal::resize_reflow) rejoin lines the
    /// way reflowing terminals do.
    wrapped: Vec<bool>,
    /// Rows scrolled out the top, oldest first, with their soft-wrap
    /// flags — kept as raw cells so [`resize_reflow`](TestTerminal::resize_reflow)
    /// can rejoin them and pull them back on widen, as real reflowing
    /// terminals do.
    scrollback: Vec<(Vec<char>, bool)>,
}

impl TestTerminal {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            parser: vte::Parser::new(),
            width,
            height,
            cursor_row: 0,
            cursor_col: 0,
            pending_wrap: false,
            viewport: vec![vec![' '; width]; height],
            wrapped: vec![false; height],
            scrollback: Vec::new(),
        }
    }

    /// Process a chunk of terminal output (escape sequences and text).
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::replace(&mut self.parser, vte::Parser::new());
        parser.advance(self, bytes);
        self.parser = parser;
    }

    /// Lines that have scrolled out of the viewport, oldest first.
    /// Trailing whitespace is trimmed.
    pub fn scrollback_lines(&self) -> Vec<String> {
        self.scrollback
            .iter()
            .map(|(cells, _)| trimmed_line(cells))
            .collect()
    }

    /// The current viewport contents, top to bottom, trailing whitespace
    /// trimmed.
    pub fn viewport_lines(&self) -> Vec<String> {
        self.viewport
            .iter()
            .map(|line| trimmed_line(line))
            .collect()
    }

    /// Current cursor position as `(row, col)` within the viewport.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    fn linefeed(&mut self) {
        if self.height == 0 {
            return;
        }
        if self.cursor_row + 1 >= self.height {
            let top = self.viewport.remove(0);
            let flag = self.wrapped.remove(0);
            self.scrollback.push((top, flag));
            self.viewport.push(vec![' '; self.width]);
            self.wrapped.push(false);
            self.cursor_row = self.height - 1;
        } else {
            self.cursor_row += 1;
        }
        self.pending_wrap = false;
    }

    fn put_char(&mut self, c: char) {
        if self.height == 0 || self.width == 0 {
            return;
        }
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if char_width == 0 {
            // Combining marks and other zero-width input would need cell
            // clustering; the engine never emits them bare.
            return;
        }
        if char_width > self.width {
            return;
        }
        if self.pending_wrap || self.cursor_col + char_width > self.width {
            // Auto-wrap, including a wide glyph that would straddle the
            // right edge: terminals push it whole onto the next row. The
            // row being left records the soft wrap (before linefeed —
            // a scroll at the bottom shifts the flags too).
            self.wrapped[self.cursor_row] = true;
            self.linefeed();
            self.cursor_col = 0;
        }
        let (row, col) = (self.cursor_row, self.cursor_col);
        self.clear_glyph_at(row, col);
        if char_width == 2 {
            self.clear_glyph_at(row, col + 1);
        }
        self.viewport[row][col] = c;
        if char_width == 2 {
            self.viewport[row][col + 1] = WIDE_CONTINUATION;
        }
        if self.cursor_col + char_width >= self.width {
            self.cursor_col = self.width - 1;
            self.pending_wrap = true;
        } else {
            self.cursor_col += char_width;
            self.pending_wrap = false;
        }
    }

    /// Blank the whole glyph occupying `col`, so overwriting one half of
    /// a wide glyph orphans the other half as a space — what real
    /// terminals render.
    fn clear_glyph_at(&mut self, row: usize, col: usize) {
        match self.viewport[row][col] {
            WIDE_CONTINUATION => {
                self.viewport[row][col] = ' ';
                if col > 0 {
                    self.viewport[row][col - 1] = ' ';
                }
            }
            c if unicode_width::UnicodeWidthChar::width(c) == Some(2) => {
                self.viewport[row][col] = ' ';
                if col + 1 < self.width && self.viewport[row][col + 1] == WIDE_CONTINUATION {
                    self.viewport[row][col + 1] = ' ';
                }
            }
            _ => {}
        }
    }

    /// Change the terminal dimensions without reflow: rows are clipped or
    /// padded at the right edge, and on height shrink the top rows scroll
    /// into scrollback as needed to keep the cursor visible. (Real
    /// terminals differ on reflow; the engine promises nothing about
    /// committed content across a resize, so the simplest model is also
    /// the honest one.)
    pub fn resize(&mut self, width: usize, height: usize) {
        for row in &mut self.viewport {
            if row.len() > width {
                row.truncate(width);
                // A wide glyph cut in half at the new edge loses both halves.
                if let Some(last) = row.last_mut()
                    && (*last == WIDE_CONTINUATION
                        || unicode_width::UnicodeWidthChar::width(*last) == Some(2))
                {
                    *last = ' ';
                }
            } else {
                row.resize(width, ' ');
            }
        }
        self.width = width;

        while self.viewport.len() > height && self.cursor_row + 1 > height {
            let top = self.viewport.remove(0);
            let flag = self.wrapped.remove(0);
            self.scrollback.push((top, flag));
            self.cursor_row -= 1;
        }
        self.viewport.truncate(height);
        self.wrapped.truncate(height);
        while self.viewport.len() < height {
            self.viewport.push(vec![' '; width]);
            self.wrapped.push(false);
        }
        self.height = height;

        self.cursor_col = self.cursor_col.min(width.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(height.saturating_sub(1));
        self.pending_wrap = false;
    }

    /// Change dimensions the way reflowing terminals (Ghostty, kitty,
    /// iTerm2, WezTerm, tmux, …) do: scrollback and viewport re-wrap as
    /// one document — soft-wrapped rows rejoin into logical lines,
    /// logical lines re-wrap at the new width — and the *content end
    /// stays pinned to its screen row*: on narrow the growth scrolls out
    /// the top (even with blank rows below), on widen rows are pulled
    /// back from scrollback. The cursor follows its logical position.
    /// (Verified against tmux 3.6; Ghostty and kitty behave the same
    /// way.) Rows created by explicit linefeeds are hard lines and never
    /// rejoin.
    ///
    /// Model notes, matching common terminal behavior closely enough for
    /// engine tests:
    /// - Trailing blank cells of a hard line are trimmed before
    ///   re-wrapping (so a blank row stays one row at any width).
    /// - Trailing all-blank rows below the content and cursor don't
    ///   count as content (they never push anything into scrollback).
    /// - Wide glyphs may be split at the wrap point (real terminals push
    ///   them whole); keep test content narrow-glyph-only near edges.
    pub fn resize_reflow(&mut self, width: usize, height: usize) {
        // The whole document: scrollback rows, then viewport rows. The
        // cursor is a document row index from here on.
        let mut doc: Vec<(Vec<char>, bool)> = std::mem::take(&mut self.scrollback);
        let sb_len = doc.len();
        for (row, flag) in self.viewport.drain(..).zip(self.wrapped.drain(..)) {
            doc.push((row, flag));
        }
        let cursor_doc = sb_len + self.cursor_row;

        // Drop trailing blank rows that hold neither content nor cursor.
        let mut used = doc.len();
        while used > cursor_doc + 1
            && used > 0
            && trimmed_line(&doc[used - 1].0).is_empty()
            && !doc[used - 1].1
        {
            used -= 1;
        }
        // How far the content end sat above the bottom of the screen —
        // reflowing terminals preserve this gap exactly.
        let bottom_gap = (self.height + sb_len).saturating_sub(used);

        // Rejoin soft-wrapped rows into logical lines. A wrapped row
        // contributes its full width (it overflowed, so it is full);
        // the final hard fragment is trimmed.
        let mut lines: Vec<Vec<char>> = Vec::new();
        let mut cursor_line = 0;
        let mut cursor_offset = 0;
        let mut current: Vec<char> = Vec::new();
        for (row, (cells, wrapped)) in doc.iter().take(used).enumerate() {
            if row == cursor_doc {
                cursor_line = lines.len();
                cursor_offset = current.len() + self.cursor_col;
            }
            if *wrapped {
                current.extend(cells.iter().copied());
            } else {
                let mut tail = cells.clone();
                while tail.last() == Some(&' ') {
                    tail.pop();
                }
                current.extend(tail);
                lines.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if lines.is_empty() {
            lines.push(Vec::new());
        }

        // Re-wrap each logical line at the new width.
        let mut rows: Vec<(Vec<char>, bool)> = Vec::new();
        let mut cursor_new = (0, 0);
        for (i, line) in lines.iter().enumerate() {
            let start = rows.len();
            let mut chunks: Vec<&[char]> = line.chunks(width.max(1)).collect();
            if chunks.is_empty() {
                chunks.push(&[]);
            }
            let n = chunks.len();
            for (j, chunk) in chunks.into_iter().enumerate() {
                let mut row: Vec<char> = chunk.to_vec();
                row.resize(width, ' ');
                rows.push((row, j + 1 < n));
            }
            if i == cursor_line {
                let frag = (cursor_offset / width.max(1)).min(n - 1);
                cursor_new = (
                    start + frag,
                    (cursor_offset - frag * width.max(1)).min(width.saturating_sub(1)),
                );
            }
        }

        // Pin the content end: the first visible document row is chosen
        // so the last content row keeps its distance from the screen
        // bottom (until scrollback runs dry), while keeping the cursor
        // on screen.
        let visible_below_end = height.saturating_sub(bottom_gap).max(1);
        let mut first_visible = rows.len().saturating_sub(visible_below_end);
        first_visible = first_visible.min(cursor_new.0);
        if cursor_new.0 - first_visible >= height {
            first_visible = cursor_new.0 + 1 - height;
        }

        self.scrollback = rows.drain(..first_visible).collect();
        self.viewport = Vec::with_capacity(height);
        self.wrapped = Vec::with_capacity(height);
        for (row, flag) in rows {
            if self.viewport.len() == height {
                break;
            }
            self.viewport.push(row);
            self.wrapped.push(flag);
        }
        while self.viewport.len() < height {
            self.viewport.push(vec![' '; width]);
            self.wrapped.push(false);
        }

        self.width = width;
        self.height = height;
        self.cursor_row = cursor_new.0 - first_visible;
        self.cursor_col = cursor_new.1;
        self.pending_wrap = false;
    }

    fn csi_param(params: &vte::Params, default: usize) -> usize {
        params
            .iter()
            .next()
            .and_then(|values| values.first().copied())
            .map(usize::from)
            .filter(|&n| n > 0)
            .unwrap_or(default)
    }
}

/// Marks the trailing cell of a double-width glyph; never rendered.
const WIDE_CONTINUATION: char = '\0';

fn trimmed_line(chars: &[char]) -> String {
    let mut line: String = chars.iter().filter(|&&c| c != WIDE_CONTINUATION).collect();
    while line.ends_with(' ') {
        line.pop();
    }
    line
}

impl vte::Perform for TestTerminal {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => {
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            b'\n' => self.linefeed(),
            b'\x08' => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
    }

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let n = Self::csi_param(params, 1);
        match action {
            'A' => self.cursor_row = self.cursor_row.saturating_sub(n),
            'B' => {
                self.cursor_row = (self.cursor_row + n).min(self.height.saturating_sub(1));
                self.pending_wrap = false;
            }
            'C' => {
                self.cursor_col = (self.cursor_col + n).min(self.width.saturating_sub(1));
                self.pending_wrap = false;
            }
            'D' => {
                self.cursor_col = self.cursor_col.saturating_sub(n);
                self.pending_wrap = false;
            }
            'E' => {
                self.cursor_row = (self.cursor_row + n).min(self.height.saturating_sub(1));
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            'F' => {
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            'H' => {
                // CUP: 1-based `row;col`, both defaulting to 1.
                let mut values = params.iter();
                let row = values
                    .next()
                    .and_then(|v| v.first().copied())
                    .map(usize::from)
                    .filter(|&n| n > 0)
                    .unwrap_or(1);
                let col = values
                    .next()
                    .and_then(|v| v.first().copied())
                    .map(usize::from)
                    .filter(|&n| n > 0)
                    .unwrap_or(1);
                self.cursor_row = (row - 1).min(self.height.saturating_sub(1));
                self.cursor_col = (col - 1).min(self.width.saturating_sub(1));
                self.pending_wrap = false;
            }
            'K' => {
                // EL 0: erase cursor to end of line. Breaks the row's
                // continuation, like real reflowing terminals.
                for col in self.cursor_col..self.width {
                    self.viewport[self.cursor_row][col] = ' ';
                }
                self.wrapped[self.cursor_row] = false;
            }
            'J' => {
                for row in self.cursor_row..self.height {
                    let start_col = if row == self.cursor_row {
                        self.cursor_col
                    } else {
                        0
                    };
                    for col in start_col..self.width {
                        self.viewport[row][col] = ' ';
                    }
                    // Erasing a row's tail breaks its continuation onto
                    // the next row, as in real reflowing terminals.
                    self.wrapped[row] = false;
                }
            }
            'h' | 'l' | 'm' => {}
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}
