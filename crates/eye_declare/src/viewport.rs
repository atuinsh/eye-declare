//! A fixed-height window over line-oriented output, pinned to the tail —
//! the "live command output" widget (v1's Viewport, as a plain element).

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;
use ratatui_core::text::Text as RText;
use ratatui_core::widgets::Widget;

use crate::element::Element;

/// Shows the last N rows of its lines within a fixed height. With
/// [`wrap`](Viewport::wrap) enabled (default), lines word-wrap at the
/// render width and the window shows the last wrapped rows; otherwise
/// long lines truncate at the width.
pub struct Viewport {
    lines: Vec<String>,
    height: u16,
    style: Style,
    wrap: bool,
}

pub fn viewport(lines: impl IntoIterator<Item = impl Into<String>>) -> Viewport {
    Viewport {
        lines: lines.into_iter().map(Into::into).collect(),
        height: 1,
        style: Style::default(),
        wrap: true,
    }
}

impl Viewport {
    /// Fixed height of the window (rows). Defaults to 1.
    pub fn height(mut self, rows: u16) -> Self {
        self.height = rows.max(1);
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }
}

impl Element for Viewport {
    fn height(&self, width: u16) -> u16 {
        if width == 0 {
            return 0;
        }
        self.height
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if self.wrap {
            let text = RText::from(self.lines.join("\n")).style(self.style);
            let total = eye_declare_engine::wrap::wrapped_line_count(&text, area.width);
            let scroll = total.saturating_sub(area.height);
            eye_declare_engine::wrap::wrapping_paragraph(text)
                .scroll((scroll, 0))
                .render(area, buf);
        } else {
            let skip = self.lines.len().saturating_sub(area.height as usize);
            for (row, line) in self.lines.iter().skip(skip).enumerate() {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn shows_the_tail_of_lines() {
        let el = viewport(["one", "two", "three"]).height(2).wrap(false);
        assert_eq!(rendered(&el, 10), vec!["two", "three"]);
    }

    #[test]
    fn fixed_height_even_with_less_content() {
        let el = viewport(["only"]).height(3).wrap(false);
        assert_eq!(Element::height(&el, 10), 3);
        assert_eq!(rendered(&el, 10), vec!["only", "", ""]);
    }

    #[test]
    fn wrapping_shows_last_wrapped_rows() {
        // "hello world" wraps to 2 rows at width 6; with one more line,
        // total 3 wrapped rows; window of 2 shows the last two.
        let el = viewport(["hello world", "tail"]).height(2);
        assert_eq!(rendered(&el, 6), vec!["world", "tail"]);
    }

    #[test]
    fn no_wrap_truncates_long_lines() {
        let el = viewport(["abcdefghij"]).height(1).wrap(false);
        assert_eq!(rendered(&el, 4), vec!["abcd"]);
    }
}
