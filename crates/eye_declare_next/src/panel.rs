//! Bordered chrome around a child element (the v1 `View` border/title
//! surface, as a plain wrapper element).

use std::time::Duration;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;
use ratatui_core::text::Line;
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::Block;

use crate::element::{AnyElement, Element};

/// A bordered box: border, optional titles (top-left, top-right) and
/// footer (bottom-right), optional horizontal content padding.
pub struct Panel<'a> {
    child: AnyElement<'a>,
    title: String,
    title_right: String,
    footer: String,
    border_style: Style,
    title_style: Style,
    pad_x: u16,
}

pub fn panel<'a>(child: impl Element + 'a) -> Panel<'a> {
    Panel {
        child: Box::new(child),
        title: String::new(),
        title_right: String::new(),
        footer: String::new(),
        border_style: Style::default(),
        title_style: Style::default(),
        pad_x: 0,
    }
}

impl Panel<'_> {
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn title_right(mut self, title: impl Into<String>) -> Self {
        self.title_right = title.into();
        self
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Columns of blank padding between the border and the content.
    pub fn pad_x(mut self, cols: u16) -> Self {
        self.pad_x = cols;
        self
    }

    /// Content area within `area` (inside border + padding).
    fn inner(&self, area: Rect) -> Rect {
        let inset_x = 1 + self.pad_x;
        Rect::new(
            area.x.saturating_add(inset_x),
            area.y.saturating_add(1),
            area.width.saturating_sub(inset_x * 2),
            area.height.saturating_sub(2),
        )
    }
}

impl Element for Panel<'_> {
    fn height(&self, width: u16) -> u16 {
        let inner_width = width.saturating_sub(2 + self.pad_x * 2);
        if inner_width == 0 {
            return 0;
        }
        self.child.height(inner_width).max(1).saturating_add(2)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height < 2 {
            return;
        }

        let mut block = Block::bordered().border_style(self.border_style);
        if !self.title.is_empty() {
            block = block.title_top(
                Line::styled(format!(" {} ", self.title), self.title_style).left_aligned(),
            );
        }
        if !self.title_right.is_empty() {
            block = block.title_top(
                Line::styled(format!(" {} ", self.title_right), self.border_style).right_aligned(),
            );
        }
        if !self.footer.is_empty() {
            block = block.title_bottom(
                Line::styled(format!(" {} ", self.footer), self.border_style).right_aligned(),
            );
        }
        block.render(area, buf);

        let inner = self.inner(area);
        if inner.width > 0 && inner.height > 0 {
            self.child.render(inner, buf);
        }
    }

    fn animated(&self) -> Option<Duration> {
        self.child.animated()
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        let inner = self.inner(area);
        self.child.cursor(inner).map(|(col, row)| {
            (
                col.saturating_add(inner.x - area.x),
                row.saturating_add(inner.y - area.y),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn border_adds_two_rows_and_titles_render() {
        let el = panel(text("hi")).title("box");
        assert_eq!(el.height(12), 3);
        let lines = rendered(&el, 12);
        assert!(lines[0].contains(" box "));
        assert!(lines[1].contains("hi"));
        assert!(lines[2].starts_with("└"));
    }

    #[test]
    fn pad_x_narrows_content_and_offsets_it() {
        let el = panel(text("hello world")).pad_x(1);
        // inner width at 10 = 10 - 2 - 2 = 6 → wraps to 2 rows → height 4
        assert_eq!(el.height(10), 4);
        let lines = rendered(&el, 10);
        assert!(lines[1].contains("hello"));
        assert!(lines[1].starts_with("│ h"));
    }

    #[test]
    fn cursor_offsets_through_chrome() {
        struct CursorAt;
        impl Element for CursorAt {
            fn height(&self, _w: u16) -> u16 {
                1
            }
            fn render(&self, _a: Rect, _b: &mut Buffer) {}
            fn cursor(&self, _a: Rect) -> Option<(u16, u16)> {
                Some((3, 0))
            }
        }

        let el = panel(CursorAt).pad_x(1);
        let area = Rect::new(0, 0, 10, 3);
        // child col 3 + border 1 + pad 1 = 5; row 0 + border 1 = 1
        assert_eq!(el.cursor(area), Some((5, 1)));
    }
}
