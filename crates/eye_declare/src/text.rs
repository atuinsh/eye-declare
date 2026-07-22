//! Word-wrapped styled text, the workhorse leaf element.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;
use ratatui_core::text::{Line, Span, Text as RText};

use crate::element::Element;

/// A run of styled spans, word-wrapped at the render width. Newlines inside
/// span content start new lines.
pub struct Text {
    spans: Vec<(String, Style)>,
}

/// Single-span text. `.style()` styles it; `.span()` appends further
/// styled spans.
pub fn text(content: impl Into<String>) -> Text {
    Text {
        spans: vec![(content.into(), Style::default())],
    }
}

impl Text {
    /// Style the most recently added span (the initial one if none added).
    pub fn style(mut self, style: Style) -> Self {
        if let Some(last) = self.spans.last_mut() {
            last.1 = style;
        }
        self
    }

    /// Append a styled span.
    pub fn span(mut self, content: impl Into<String>, style: Style) -> Self {
        self.spans.push((content.into(), style));
        self
    }

    /// Build the ratatui text, splitting span content on newlines so `\n`
    /// starts a new line (a `Span` must not contain newlines).
    fn to_ratatui(&self) -> RText<'_> {
        let mut lines: Vec<Line<'_>> = vec![Line::default()];
        for (content, style) in &self.spans {
            for (i, part) in content.split('\n').enumerate() {
                if i > 0 {
                    lines.push(Line::default());
                }
                if !part.is_empty() {
                    let line = lines.last_mut().expect("lines starts non-empty");
                    line.spans.push(Span::styled(part, *style));
                }
            }
        }
        RText::from(lines)
    }
}

impl Element for Text {
    fn height(&self, width: u16) -> u16 {
        eye_declare_engine::wrap::wrapped_line_count(&self.to_ratatui(), width)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        eye_declare_engine::wrap::render_wrapped(
            self.to_ratatui(),
            ratatui_core::layout::Alignment::Left,
            0,
            area,
            buf,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Color;

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
    fn single_line() {
        assert_eq!(rendered(&text("hello"), 10), vec!["hello"]);
    }

    #[test]
    fn wraps_words_at_width() {
        assert_eq!(rendered(&text("hello world"), 6), vec!["hello", "world"]);
    }

    #[test]
    fn newlines_split_lines() {
        assert_eq!(rendered(&text("a\nb"), 10), vec!["a", "b"]);
    }

    #[test]
    fn spans_concatenate_and_carry_style() {
        let el = text("ab").span("cd", Style::default().fg(Color::Red));
        assert_eq!(rendered(&el, 10), vec!["abcd"]);

        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        assert_eq!(buf[(2, 0)].style().fg, Some(Color::Red));
        assert_ne!(buf[(0, 0)].style().fg, Some(Color::Red));
    }

    #[test]
    fn newline_inside_later_span() {
        let el = text("a").span("b\nc", Style::default());
        assert_eq!(rendered(&el, 10), vec!["ab", "c"]);
    }

    #[test]
    fn empty_text_is_one_blank_row() {
        // Matches v1 Text: an empty text still occupies a row (a blank
        // line is content). Use `empty()` for true zero-height.
        assert_eq!(text("").height(10), 1);
    }
}
