use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Text},
    widgets::Widget,
};

use crate::component::Component;
use crate::wrap;

/// A built-in text component with display-time word wrapping.
///
/// Stores logical lines of styled text as props on the component itself.
/// Wrapping is computed at render time based on the current width,
/// so content reflows automatically on resize.
///
/// Use the builder API to construct:
/// ```ignore
/// TextBlock::new()
///     .line("styled text", Style::default().fg(Color::Red))
///     .unstyled("plain text")
/// ```
pub struct TextBlock {
    lines: Vec<(String, Style)>,
}

impl TextBlock {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Add a styled line.
    pub fn line(mut self, text: impl Into<String>, style: Style) -> Self {
        self.lines.push((text.into(), style));
        self
    }

    /// Add an unstyled line (default style).
    pub fn unstyled(mut self, text: impl Into<String>) -> Self {
        self.lines.push((text.into(), Style::default()));
        self
    }

    fn to_text(&self) -> Text<'_> {
        let lines: Vec<Line> = self
            .lines
            .iter()
            .map(|(text, style)| Line::styled(text.as_str(), *style))
            .collect();
        Text::from(lines)
    }
}

impl Default for TextBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for TextBlock {
    type State = ();

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &Self::State) {
        if self.lines.is_empty() {
            return;
        }
        let text = self.to_text();
        wrap::wrapping_paragraph(text).render(area, buf);
    }

    fn desired_height(&self, width: u16, _state: &Self::State) -> u16 {
        if self.lines.is_empty() || width == 0 {
            return 0;
        }
        let text = self.to_text();
        wrap::wrapped_line_count(&text, width)
    }

    fn initial_state(&self) -> () {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Color;

    #[test]
    fn empty_text_block_height_zero() {
        let tb = TextBlock::new();
        assert_eq!(tb.desired_height(80, &()), 0);
    }

    #[test]
    fn single_short_line() {
        let tb = TextBlock::new().unstyled("hello world");
        assert_eq!(tb.desired_height(80, &()), 1);
    }

    #[test]
    fn wraps_at_narrow_width() {
        let tb = TextBlock::new()
            .unstyled("hello world this is a long line that should wrap");
        // At width 20, this ~47 char line should wrap to 3 lines
        let height = tb.desired_height(20, &());
        assert!(height >= 3, "expected >= 3, got {}", height);
    }

    #[test]
    fn multiple_lines_with_wrapping() {
        let tb = TextBlock::new()
            .unstyled("short")
            .unstyled("this is a longer line that will wrap at narrow widths");
        // At width 20: "short" = 1 line, long line = 3+ lines
        let height = tb.desired_height(20, &());
        assert!(height >= 4, "expected >= 4, got {}", height);
    }

    #[test]
    fn styled_text_wraps_correctly() {
        let tb = TextBlock::new()
            .line("important text that is fairly long", Style::default().fg(Color::Red));
        let height_wide = tb.desired_height(80, &());
        let height_narrow = tb.desired_height(15, &());
        assert_eq!(height_wide, 1);
        assert!(height_narrow >= 3, "expected >= 3 at width 15, got {}", height_narrow);
    }

    #[test]
    fn renders_into_buffer() {
        let tb = TextBlock::new().unstyled("hello");

        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        tb.render(area, &mut buf, &());

        assert_eq!(buf[(0, 0)].symbol(), "h");
        assert_eq!(buf[(4, 0)].symbol(), "o");
    }

    #[test]
    fn default_is_empty() {
        let tb = TextBlock::default();
        assert_eq!(tb.desired_height(80, &()), 0);
    }
}
