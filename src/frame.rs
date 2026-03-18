use ratatui_core::{
    buffer::{Buffer, Cell},
    layout::Rect,
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

    /// Diff against a previous frame, producing the set of changed cells.
    ///
    /// Handles height mismatches: if the frames have different heights,
    /// the shorter one is logically padded with empty cells so that
    /// `Buffer::diff` can operate on matching dimensions.
    pub fn diff(&self, previous: &Frame) -> Diff {
        let new_area = self.buffer.area;
        let prev_area = previous.buffer.area;

        // Fast path: same dimensions, use Buffer::diff directly
        if new_area == prev_area {
            let changes = previous
                .buffer
                .diff(&self.buffer)
                .into_iter()
                .map(|(x, y, cell)| (x, y, cell.clone()))
                .collect();
            return Diff {
                cells: changes,
                new_area,
                prev_area,
            };
        }

        // Heights differ — we need matching buffers for diff.
        // Create padded versions at the max dimensions.
        let max_width = new_area.width.max(prev_area.width);
        let max_height = new_area.height.max(prev_area.height);
        let max_rect = Rect::new(0, 0, max_width, max_height);

        let padded_prev = pad_buffer(&previous.buffer, max_rect);
        let padded_new = pad_buffer(&self.buffer, max_rect);

        let changes = padded_prev
            .diff(&padded_new)
            .into_iter()
            .map(|(x, y, cell)| (x, y, cell.clone()))
            .collect();

        Diff {
            cells: changes,
            new_area,
            prev_area,
        }
    }
}

/// Pad a buffer to a larger rect, copying existing cells and filling
/// new space with default (empty) cells.
fn pad_buffer(src: &Buffer, target_area: Rect) -> Buffer {
    let mut padded = Buffer::empty(target_area);
    let src_area = src.area;

    for y in src_area.y..src_area.y.saturating_add(src_area.height) {
        for x in src_area.x..src_area.x.saturating_add(src_area.width) {
            if x < target_area.x + target_area.width && y < target_area.y + target_area.height {
                padded[(x, y)] = src[(x, y)].clone();
            }
        }
    }

    padded
}

/// A set of changed cells between two frames.
pub struct Diff {
    /// Changed cells: (x, y, new_cell).
    pub(crate) cells: Vec<(u16, u16, Cell)>,
    /// The area of the new (current) frame.
    pub(crate) new_area: Rect,
    /// The area of the previous frame.
    pub(crate) prev_area: Rect,
}

impl Diff {
    /// Whether there are no changes.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Number of changed cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// The area of the new frame.
    pub fn new_area(&self) -> Rect {
        self.new_area
    }

    /// The area of the previous frame.
    pub fn prev_area(&self) -> Rect {
        self.prev_area
    }

    /// Whether the frame grew (new frame is taller than previous).
    pub fn grew(&self) -> bool {
        self.new_area.height > self.prev_area.height
    }

    /// How many rows the frame grew by (0 if it didn't grow).
    pub fn growth(&self) -> u16 {
        self.new_area
            .height
            .saturating_sub(self.prev_area.height)
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
        // The removed row should show as changed (cleared to empty)
        let removed_row_cells: Vec<_> = diff.cells.iter().filter(|(_, y, _)| *y == 1).collect();
        assert!(!removed_row_cells.is_empty());
    }
}
