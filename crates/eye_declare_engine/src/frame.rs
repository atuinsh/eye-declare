use ratatui_core::{
    buffer::{Buffer, Cell},
    layout::Rect,
    style::Style,
};

/// The output of a render pass. Owns the buffer.
pub struct Frame {
    buffer: Buffer,
}

impl Frame {
    pub fn new(buffer: Buffer) -> Self {
        Self { buffer }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn area(&self) -> Rect {
        self.buffer.area
    }

    pub fn write_committed_row(
        &self,
        row: u16,
        out: &mut Vec<u8>,
        cursor: &mut crate::escape::CursorState,
    ) {
        let area = self.buffer.area;
        if row >= area.height {
            return;
        }

        crate::escape::write_committed_row(
            out,
            (0..area.width).map(|x| &self.buffer[(x, row)]),
            cursor,
        );
    }

    /// The row's rendered width up to its last non-blank cell — the
    /// content a reflowing terminal re-wraps when the width shrinks
    /// (terminals drop trailing blanks when re-wrapping, whether the
    /// cells were written or erased).
    pub fn content_width_of_row(&self, row: u16) -> u16 {
        let area = self.buffer.area;
        if row >= area.height {
            return 0;
        }
        for x in (0..area.width).rev() {
            let symbol = self.buffer[(x, row)].symbol();
            if symbol != " " {
                let width = unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
                return (x + width).min(area.width);
            }
        }
        0
    }

    /// Diff against a previous frame, producing the set of changed cells.
    ///
    /// Handles height mismatches: if the frames have different heights,
    /// the shorter one is logically padded with empty cells so that
    /// `Buffer::diff` can operate on matching dimensions.
    pub fn diff(&self, previous: &Frame) -> Diff {
        self.diff_from(previous, 0)
    }

    /// [`diff`](Frame::diff), skipping rows above `start_row`.
    ///
    /// Rows that have scrolled into terminal scrollback can never be
    /// repainted, so comparing them is pure waste — the engine passes its
    /// scrollback boundary here. With a tail much taller than the
    /// terminal, this is the difference between diffing the whole virtual
    /// frame and diffing one screenful.
    pub fn diff_from(&self, previous: &Frame, start_row: u16) -> Diff {
        let new_area = self.buffer.area;
        let prev_area = previous.buffer.area;

        // Fast path: same dimensions, use Buffer::diff directly
        if new_area == prev_area && start_row == 0 {
            let changes = previous
                .buffer
                .diff(&self.buffer)
                .into_iter()
                .map(|(x, y, cell)| (x, y, cell.clone()))
                .collect();
            return self.build_diff(changes, prev_area);
        }

        // Heights differ — compare directly without allocating padded buffers.
        // Cells within both buffers are compared normally; cells only in one
        // buffer are compared against a default (empty) cell.
        let max_width = new_area.width.max(prev_area.width);
        let max_height = new_area.height.max(prev_area.height);
        let default_cell = Cell::default();
        let mut changes = Vec::new();

        for y in start_row..max_height {
            // Mirror `Buffer::diff`'s wide-glyph discipline (the fast path
            // gets it from ratatui): the trailing cells of a wide symbol
            // in the new frame must not be emitted as separate updates —
            // painting them would overwrite half the glyph just written —
            // and a width change at a cell invalidates its neighbors so
            // uncovered cells repaint even when they compare equal.
            let mut invalidated: usize = 0;
            let mut to_skip: usize = 0;
            for x in 0..max_width {
                let in_prev = x < prev_area.width && y < prev_area.height;
                let in_new = x < new_area.width && y < new_area.height;

                let prev_cell = if in_prev {
                    &previous.buffer[(x, y)]
                } else {
                    &default_cell
                };
                let new_cell = if in_new {
                    &self.buffer[(x, y)]
                } else {
                    &default_cell
                };

                if (prev_cell != new_cell || invalidated > 0) && to_skip == 0 {
                    changes.push((x, y, new_cell.clone()));
                }

                let new_width = unicode_width::UnicodeWidthStr::width(new_cell.symbol());
                let prev_width = unicode_width::UnicodeWidthStr::width(prev_cell.symbol());
                to_skip = new_width.saturating_sub(1);
                invalidated = invalidated.max(new_width.max(prev_width)).saturating_sub(1);
            }
        }

        self.build_diff(changes, prev_area)
    }

    /// Build a [`Diff`], lifting each row's trailing run of blank
    /// default-styled cell writes into a line clear.
    ///
    /// Writing spaces to clear old content plants real cells the
    /// terminal treats as content — they re-wrap on resize (phantom
    /// blank fragments) and pad committed rows in scrollback. An
    /// erase-to-end-of-line leaves genuinely empty cells (and is far
    /// fewer bytes). A run is only lifted when everything to its right
    /// in the new frame is blank and default-styled, so the erase can't
    /// eat styled cells that aren't repainted.
    fn build_diff(&self, cells: Vec<(u16, u16, Cell)>, prev_area: Rect) -> Diff {
        let new_area = self.buffer.area;
        // A default cell's style, for blankness checks — `Cell::style()`
        // reports explicit `Reset` colors, which a bare
        // `Style::default()` (all `None`) never equals.
        let blank_style: Style = Cell::default().style();
        // Per new-frame row: index one past the last cell that must
        // survive an erase (non-space symbol or non-default style).
        let boundary = |y: u16| -> u16 {
            if y >= new_area.height {
                return 0;
            }
            for x in (0..new_area.width).rev() {
                let cell = &self.buffer[(x, y)];
                if cell.symbol() != " " || cell.style() != blank_style {
                    return x + 1;
                }
            }
            0
        };

        let mut kept = Vec::with_capacity(cells.len());
        let mut line_clears: Vec<(u16, u16)> = Vec::new();
        let mut current_boundary: Option<(u16, u16)> = None;
        for (x, y, cell) in cells {
            let b = match current_boundary {
                Some((row, b)) if row == y => b,
                _ => {
                    let b = boundary(y);
                    current_boundary = Some((y, b));
                    b
                }
            };
            if x >= b && cell.symbol() == " " && cell.style() == blank_style {
                match line_clears.last_mut() {
                    Some((cx, cy)) if *cy == y => *cx = (*cx).min(x),
                    _ => line_clears.push((x, y)),
                }
            } else {
                kept.push((x, y, cell));
            }
        }

        Diff {
            cells: kept,
            line_clears,
            new_area,
            prev_area,
        }
    }
}

/// A set of changed cells between two frames.
pub struct Diff {
    /// Changed cells: (x, y, new_cell).
    pub cells: Vec<(u16, u16, Cell)>,
    /// Erase-to-end-of-line anchors, row-major: at `(x, y)`, everything
    /// from `x` rightward is blank in the new frame and cleared with EL
    /// instead of written spaces — written blanks are content a
    /// reflowing terminal re-wraps; erased cells are not.
    pub line_clears: Vec<(u16, u16)>,
    /// The area of the new (current) frame.
    pub new_area: Rect,
    /// The area of the previous frame.
    pub prev_area: Rect,
}

impl Frame {
    /// Create a new frame with the top `n` rows removed.
    ///
    /// Used for committed scrollback: committed rows are sliced off
    /// so subsequent diffs only cover the active region.
    pub fn slice_top_rows(&self, n: u16) -> Frame {
        let old_area = self.buffer.area;
        let new_height = old_area.height.saturating_sub(n);
        if new_height == 0 {
            return Frame::new(Buffer::empty(Rect::new(0, 0, old_area.width, 0)));
        }
        let new_area = Rect::new(0, 0, old_area.width, new_height);
        let mut new_buf = Buffer::empty(new_area);
        for y in 0..new_height {
            for x in 0..old_area.width {
                new_buf[(x, y)] = self.buffer[(x, y + n)].clone();
            }
        }
        Frame::new(new_buf)
    }
}

impl Diff {
    /// Whether there are no changes.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.line_clears.is_empty()
    }

    /// Number of changed cells.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the frame grew (new frame is taller than previous).
    pub fn grew(&self) -> bool {
        self.new_area.height > self.prev_area.height
    }

    /// How many rows the frame grew by (0 if it didn't grow).
    #[cfg(test)]
    pub fn growth(&self) -> u16 {
        self.new_area.height.saturating_sub(self.prev_area.height)
    }

    /// Remove cells that are above the visible area (in scrollback).
    ///
    /// Cells at row < `min_row` are in terminal scrollback and can't
    /// be modified. Filtering them prevents cursor tracking drift.
    pub fn retain_visible(&mut self, min_row: u16) {
        if min_row > 0 {
            self.cells.retain(|(_, y, _)| *y >= min_row);
            self.line_clears.retain(|(_, y)| *y >= min_row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_frame(lines: &[&str]) -> Frame {
        Frame::new(Buffer::with_lines(lines.iter().map(|s| s.to_string())))
    }

    #[test]
    fn diff_identical_frames_is_empty() {
        let f1 = make_frame(&["hello", "world"]);
        let f2 = make_frame(&["hello", "world"]);
        let diff = f2.diff(&f1);
        assert!(diff.is_empty());
    }

    #[test]
    fn diff_lifts_trailing_blank_writes_into_line_clear() {
        let f1 = make_frame(&["hello world"]);
        let f2 = make_frame(&["hi         "]);
        let diff = f2.diff(&f1);
        // 'i' is written; the old "llo world" tail becomes one erase
        // instead of nine space cells the terminal would treat as
        // content.
        assert_eq!(diff.line_clears, vec![(2, 0)]);
        assert!(
            diff.cells
                .iter()
                .all(|(x, _, c)| *x < 2 || c.symbol() != " "),
            "no blank writes past the content boundary"
        );
    }

    #[test]
    fn diff_keeps_styled_blank_cells() {
        use ratatui_core::style::{Color, Style};
        let f1 = make_frame(&["hello world"]);
        let mut f2 = make_frame(&["hi         "]);
        // A styled blank cell (e.g. highlighted padding) must be painted,
        // and the erase may only cover what lies right of it.
        f2.buffer[(5, 0)].set_style(Style::default().bg(Color::Blue));
        let diff = f2.diff(&f1);
        assert!(
            diff.cells
                .iter()
                .any(|(x, _, c)| *x == 5 && c.symbol() == " "),
            "styled blank is written"
        );
        assert!(diff.line_clears.iter().all(|&(x, _)| x > 5));
    }

    #[test]
    fn diff_single_cell_change() {
        let f1 = make_frame(&["hello"]);
        let f2 = make_frame(&["hallo"]);
        let diff = f2.diff(&f1);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff.cells[0].0, 1); // x=1 (the 'a' vs 'e')
        assert_eq!(diff.cells[0].1, 0); // y=0
    }

    #[test]
    fn diff_height_growth() {
        let f1 = make_frame(&["hello"]);
        let f2 = make_frame(&["hello", "world"]);
        let diff = f2.diff(&f1);
        assert!(diff.grew());
        assert_eq!(diff.growth(), 1);
        // The new row should have changed cells
        let new_row_cells: Vec<_> = diff.cells.iter().filter(|(_, y, _)| *y == 1).collect();
        assert!(!new_row_cells.is_empty());
    }

    #[test]
    fn diff_no_growth_same_height() {
        let f1 = make_frame(&["hello", "world"]);
        let f2 = make_frame(&["hello", "earth"]);
        let diff = f2.diff(&f1);
        assert!(!diff.grew());
        assert_eq!(diff.growth(), 0);
    }

    #[test]
    fn diff_height_shrink() {
        let f1 = make_frame(&["hello", "world"]);
        let f2 = make_frame(&["hello"]);
        let diff = f2.diff(&f1);
        assert!(!diff.grew());
        // The removed row is cleared with a single erase-to-end-of-line
        // from column 0, not a run of written spaces.
        assert!(diff.cells.iter().all(|(_, y, _)| *y != 1));
        assert_eq!(diff.line_clears, vec![(0, 1)]);
    }
}
