//! Layout containers: vertical [`Col`] and horizontal [`Row`].
//!
//! Layout is content-driven (heights come from children; no vertical flex),
//! matching v1's model: correct for inline UIs where height is unbounded
//! scrollback. `Row` allocates widths by `Fixed`/`Fill`, semantics ported
//! from v1's `allocate_widths`.

use std::time::Duration;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

use crate::element::{AnyElement, Element};

/// Vertical stack. Children get the full width; heights are summed.
pub struct Col<'a> {
    children: Vec<AnyElement<'a>>,
    gap: u16,
}

pub fn col<'a>() -> Col<'a> {
    Col {
        children: Vec::new(),
        gap: 0,
    }
}

impl<'a> Col<'a> {
    /// Blank rows between children.
    pub fn gap(mut self, rows: u16) -> Self {
        self.gap = rows;
        self
    }

    pub fn child(mut self, child: impl Element + 'a) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: Element + 'a,
    {
        self.children
            .extend(children.into_iter().map(|c| Box::new(c) as AnyElement<'a>));
        self
    }
}

impl Element for Col<'_> {
    fn height(&self, width: u16) -> u16 {
        let content: u16 = self
            .children
            .iter()
            .map(|c| c.height(width))
            .fold(0, u16::saturating_add);
        let gaps = self
            .gap
            .saturating_mul(self.children.len().saturating_sub(1) as u16);
        content.saturating_add(gaps)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        let bottom = area.bottom();
        for (i, child) in self.children.iter().enumerate() {
            if i > 0 {
                y = y.saturating_add(self.gap);
            }
            if y >= bottom {
                break;
            }
            let h = child.height(area.width).min(bottom - y);
            if h > 0 {
                child.render(Rect::new(area.x, y, area.width, h), buf);
            }
            y = y.saturating_add(h);
        }
    }

    fn animated(&self) -> Option<Duration> {
        self.children.iter().filter_map(|c| c.animated()).min()
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        // First child that wants the cursor wins; offset by its position.
        // Mirrors render's clipping: a child (or cursor row) below the
        // given height must not place the hardware cursor on a row that
        // was never painted.
        let mut y_offset: u16 = 0;
        for (i, child) in self.children.iter().enumerate() {
            if i > 0 {
                y_offset = y_offset.saturating_add(self.gap);
            }
            if y_offset >= area.height {
                break;
            }
            let h = child
                .height(area.width)
                .min(area.height.saturating_sub(y_offset));
            let child_area = Rect::new(area.x, area.y.saturating_add(y_offset), area.width, h);
            if let Some((col, row)) = child.cursor(child_area)
                && row < h
            {
                return Some((col, row.saturating_add(y_offset)));
            }
            y_offset = y_offset.saturating_add(h);
        }
        None
    }
}

/// Cell width within a [`Row`].
#[derive(Clone, Copy)]
pub enum Width {
    Fixed(u16),
    /// Remaining space, split equally among `Fill` cells (leftover columns
    /// go one each to the leftmost fills).
    Fill,
}

/// Horizontal layout. Row height is the max of cell heights at their
/// allocated widths; cells are top-aligned.
pub struct Row<'a> {
    cells: Vec<(Width, AnyElement<'a>)>,
}

pub fn row<'a>() -> Row<'a> {
    Row { cells: Vec::new() }
}

impl<'a> Row<'a> {
    pub fn fixed(mut self, cols: u16, child: impl Element + 'a) -> Self {
        self.cells.push((Width::Fixed(cols), Box::new(child)));
        self
    }

    pub fn fill(mut self, child: impl Element + 'a) -> Self {
        self.cells.push((Width::Fill, Box::new(child)));
        self
    }

    /// v1 `allocate_widths` semantics: fixed cells reserve their width in
    /// order (clamped to what remains); fills split the remainder equally,
    /// with leftover columns distributed one each from the left.
    fn allocate(&self, total: u16) -> Vec<u16> {
        let mut widths = vec![0u16; self.cells.len()];
        let mut remaining = total;
        let mut fills = Vec::new();

        for (i, (w, _)) in self.cells.iter().enumerate() {
            match w {
                Width::Fixed(n) => {
                    let take = (*n).min(remaining);
                    widths[i] = take;
                    remaining -= take;
                }
                Width::Fill => fills.push(i),
            }
        }

        if !fills.is_empty() {
            let each = remaining / fills.len() as u16;
            let extra = remaining % fills.len() as u16;
            for (j, &i) in fills.iter().enumerate() {
                widths[i] = each + u16::from((j as u16) < extra);
            }
        }

        widths
    }
}

impl Element for Row<'_> {
    fn height(&self, width: u16) -> u16 {
        let widths = self.allocate(width);
        self.cells
            .iter()
            .zip(&widths)
            .map(|((_, c), &w)| if w == 0 { 0 } else { c.height(w) })
            .max()
            .unwrap_or(0)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let widths = self.allocate(area.width);
        let mut x = area.x;
        for ((_, child), &w) in self.cells.iter().zip(&widths) {
            if w > 0 {
                child.render(Rect::new(x, area.y, w, area.height), buf);
            }
            x = x.saturating_add(w);
        }
    }

    fn animated(&self) -> Option<Duration> {
        self.cells.iter().filter_map(|(_, c)| c.animated()).min()
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        let widths = self.allocate(area.width);
        let mut x_offset: u16 = 0;
        for ((_, child), &w) in self.cells.iter().zip(&widths) {
            let cell_area = Rect::new(area.x.saturating_add(x_offset), area.y, w, area.height);
            if w > 0
                && let Some((col, row)) = child.cursor(cell_area)
            {
                return Some((col.saturating_add(x_offset), row));
            }
            x_offset = x_offset.saturating_add(w);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::ElementExt;
    use crate::text::text;

    fn rendered(el: &impl Element, width: u16) -> Vec<String> {
        let height = el.height(width);
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
    fn col_stacks_children() {
        let el = col().child(text("one")).child(text("two"));
        assert_eq!(el.height(10), 2);
        assert_eq!(rendered(&el, 10), vec!["one", "two"]);
    }

    #[test]
    fn col_gap_inserts_blank_rows() {
        let el = col().gap(1).child(text("a")).child(text("b"));
        assert_eq!(el.height(10), 3);
        assert_eq!(rendered(&el, 10), vec!["a", "", "b"]);
    }

    #[test]
    fn col_sums_wrapped_heights() {
        let el = col().child(text("hello world")).child(text("x"));
        assert_eq!(el.height(6), 3); // 2 wrapped + 1
    }

    #[test]
    fn row_fixed_and_fill() {
        let el = row().fixed(2, text("> ")).fill(text("hello"));
        assert_eq!(el.height(10), 1);
        assert_eq!(rendered(&el, 10), vec!["> hello"]);
    }

    #[test]
    fn row_fill_gets_remaining_width_for_wrapping() {
        // Fill cell gets 10 - 4 = 6 cols: "hello world" wraps to 2 rows.
        let el = row().fixed(4, text("gut ")).fill(text("hello world"));
        assert_eq!(el.height(10), 2);
        assert_eq!(rendered(&el, 10), vec!["gut hello", "    world"]);
    }

    #[test]
    fn row_splits_fill_equally_with_leftmost_remainder() {
        let el = row()
            .fill(empty_cell())
            .fill(empty_cell())
            .fill(empty_cell());
        assert_eq!(el.allocate(10), vec![4, 3, 3]);
    }

    fn empty_cell() -> crate::element::Empty {
        crate::element::Empty
    }

    #[test]
    fn row_fixed_clamps_to_available() {
        let el = row().fixed(8, text("aaaaaaaa")).fixed(8, text("b"));
        assert_eq!(el.allocate(10), vec![8, 2]);
    }

    #[test]
    fn col_cursor_offsets_by_child_position() {
        struct CursorAt(u16, u16);
        impl Element for CursorAt {
            fn height(&self, _w: u16) -> u16 {
                1
            }
            fn render(&self, _a: Rect, _b: &mut Buffer) {}
            fn cursor(&self, _a: Rect) -> Option<(u16, u16)> {
                Some((self.0, self.1))
            }
        }

        let el = col().child(text("above")).child(CursorAt(3, 0).pad_left(2));
        let area = Rect::new(0, 0, 10, 2);
        assert_eq!(el.cursor(area), Some((5, 1)));
    }

    #[test]
    fn cursor_of_clipped_child_is_suppressed() {
        struct CursorAt(u16, u16);
        impl Element for CursorAt {
            fn height(&self, _w: u16) -> u16 {
                1
            }
            fn render(&self, _a: Rect, _b: &mut Buffer) {}
            fn cursor(&self, _a: Rect) -> Option<(u16, u16)> {
                Some((self.0, self.1))
            }
        }

        // The cursor-bearing child sits on row 2, but only 2 rows are
        // rendered — the hardware cursor must not land on a hidden row.
        let el = col()
            .child(text("a"))
            .child(text("b"))
            .child(CursorAt(0, 0));
        assert_eq!(el.cursor(Rect::new(0, 0, 10, 2)), None);
        // With enough height it comes back.
        assert_eq!(el.cursor(Rect::new(0, 0, 10, 3)), Some((0, 2)));
    }
}
