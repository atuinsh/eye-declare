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
    scrollback: Vec<String>,
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
        self.scrollback.clone()
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
            self.scrollback.push(trimmed_line(&top));
            self.viewport.push(vec![' '; self.width]);
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
        if self.pending_wrap {
            self.linefeed();
            self.cursor_col = 0;
        }
        self.viewport[self.cursor_row][self.cursor_col] = c;
        if self.cursor_col + 1 >= self.width {
            self.pending_wrap = true;
        } else {
            self.cursor_col += 1;
            self.pending_wrap = false;
        }
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

fn trimmed_line(chars: &[char]) -> String {
    let mut line: String = chars.iter().collect();
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
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.pending_wrap = false;
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
                }
            }
            'h' | 'l' | 'm' => {}
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}
