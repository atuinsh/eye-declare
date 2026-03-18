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
/// Stores logical lines of styled text. Wrapping is computed at
/// render time based on the current width, so content reflows
/// automatically on resize.
///
/// Works for both static text and streaming content — push tokens
/// as they arrive and the component re-renders with correct wrapping.
pub struct TextBlock;

/// State for a [`TextBlock`] component.
///
/// Each entry is a logical line with its style. Use the convenience
/// methods to build content.
pub struct TextState {
    pub lines: Vec<(String, Style)>,
}

impl TextState {
    /// Add a styled line of text.
    pub fn push(&mut self, text: impl Into<String>, style: Style) {
        self.lines.push((text.into(), style));
    }

    /// Add an unstyled line of text (default style).
    pub fn push_unstyled(&mut self, text: impl Into<String>) {
        self.lines.push((text.into(), Style::default()));
    }

    /// Clear all text.
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Whether the text block is empty.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Build a ratatui `Text` from the current lines.
    fn to_text(&self) -> Text<'_> {
        let lines: Vec<Line> = self
            .lines
            .iter()
            .map(|(text, style)| Line::styled(text.as_str(), *style))
            .collect();
        Text::from(lines)
    }
}

impl Component for TextBlock {
    type State = TextState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        if state.is_empty() {
            return;
        }
        let text = state.to_text();
        wrap::wrapping_paragraph(text).render(area, buf);
    }

    fn desired_height(&self, width: u16, state: &Self::State) -> u16 {
        if state.is_empty() || width == 0 {
            return 0;
        }
        let text = state.to_text();
        wrap::wrapped_line_count(&text, width)
    }

    fn initial_state(&self) -> TextState {
        TextState { lines: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use ratatui_core::style::Color;

    #[test]
    fn empty_text_block_height_zero() {
        let tb = TextBlock;
        let state = tb.initial_state();
        assert_eq!(tb.desired_height(80, &state), 0);
    }

    #[test]
    fn single_short_line() {
        let tb = TextBlock;
        let mut state = tb.initial_state();
        state.push_unstyled("hello world");
        assert_eq!(tb.desired_height(80, &state), 1);
    }

    #[test]
    fn wraps_at_narrow_width() {
        let tb = TextBlock;
        let mut state = tb.initial_state();
        state.push_unstyled("hello world this is a long line that should wrap");
        // At width 20, this ~47 char line should wrap to 3 lines
        let height = tb.desired_height(20, &state);
        assert!(height >= 3, "expected >= 3, got {}", height);
    }

    #[test]
    fn multiple_lines_with_wrapping() {
        let tb = TextBlock;
        let mut state = tb.initial_state();
        state.push_unstyled("short");
        state.push_unstyled("this is a longer line that will wrap at narrow widths");
        // At width 20: "short" = 1 line, long line = 3+ lines
        let height = tb.desired_height(20, &state);
        assert!(height >= 4, "expected >= 4, got {}", height);
    }

    #[test]
    fn styled_text_wraps_correctly() {
        let tb = TextBlock;
        let mut state = tb.initial_state();
        state.push("important text that is fairly long", Style::default().fg(Color::Red));
        let height_wide = tb.desired_height(80, &state);
        let height_narrow = tb.desired_height(15, &state);
        assert_eq!(height_wide, 1);
        assert!(height_narrow >= 3, "expected >= 3 at width 15, got {}", height_narrow);
    }

    #[test]
    fn renders_into_buffer() {
        let tb = TextBlock;
        let mut state = tb.initial_state();
        state.push_unstyled("hello");

        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        tb.render(area, &mut buf, &state);

        assert_eq!(buf[(0, 0)].symbol(), "h");
        assert_eq!(buf[(4, 0)].symbol(), "o");
    }

    #[test]
    fn clear_empties_state() {
        let tb = TextBlock;
        let mut state = tb.initial_state();
        state.push_unstyled("hello");
        assert!(!state.is_empty());
        state.clear();
        assert!(state.is_empty());
        assert_eq!(tb.desired_height(80, &state), 0);
    }
}
