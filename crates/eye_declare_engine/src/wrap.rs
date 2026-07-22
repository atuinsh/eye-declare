use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::text::Text;
use ratatui_widgets::paragraph::{Paragraph, Wrap};

/// Below this width the word wrapper is unsafe: at width 2 a multi-column
/// grapheme mid-word (plain CJK does it — `"a佉b"`) makes ratatui's
/// `WordWrapper` emit a line wider than the limit, and `Paragraph::render`
/// then writes past the buffer edge (upstream bug, fuzz-found; minimal
/// repros in `.planning/FUZZING.md`). Under this width both measuring and
/// rendering fall back to truncation, which is panic-free at any width.
const MIN_WRAP_WIDTH: u16 = 3;

/// Compute how many terminal rows `text` occupies at `width` with word wrapping.
///
/// Uses ratatui's `Paragraph` with `Wrap { trim: false }` to match the
/// rendering behavior of [`render_wrapped`], including its truncation
/// fallback below [`MIN_WRAP_WIDTH`].
pub fn wrapped_line_count(text: &Text<'_>, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    if width < MIN_WRAP_WIDTH {
        return text.lines.len() as u16;
    }
    let count = Paragraph::new(text.clone())
        .wrap(Wrap { trim: false })
        .line_count(width);
    count as u16
}

/// Render `text` word-wrapped into `area`, scrolled down by `scroll_rows`.
///
/// This is the framework's one wrap-render path: word wrap at the area
/// width preserving leading whitespace, except below [`MIN_WRAP_WIDTH`],
/// where lines truncate at the edge instead of feeding the word wrapper
/// widths it cannot handle. [`wrapped_line_count`] measures identically,
/// keeping the `height(width)` contract exact in both regimes.
pub fn render_wrapped(
    text: Text<'_>,
    alignment: ratatui_core::layout::Alignment,
    scroll_rows: u16,
    area: Rect,
    buf: &mut Buffer,
) {
    use ratatui_core::widgets::Widget;

    if area.width == 0 || area.height == 0 {
        return;
    }
    let paragraph = Paragraph::new(text).alignment(alignment);
    let paragraph = if area.width < MIN_WRAP_WIDTH {
        paragraph
    } else {
        paragraph.wrap(Wrap { trim: false })
    };
    paragraph.scroll((scroll_rows, 0)).render(area, buf);
}

/// Create a `Paragraph` with word wrapping enabled (no trim).
///
/// Prefer [`render_wrapped`], which also guards the degenerate widths the
/// word wrapper cannot handle; this constructor remains for callers that
/// need the `Paragraph` itself.
pub fn wrapping_paragraph<'a>(text: Text<'a>) -> Paragraph<'a> {
    Paragraph::new(text).wrap(Wrap { trim: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::text::Text;

    fn text_from(s: &str) -> Text<'_> {
        Text::from(s)
    }

    #[test]
    fn short_text_no_wrap() {
        let text = text_from("hello");
        assert_eq!(wrapped_line_count(&text, 80), 1);
    }

    #[test]
    fn text_wraps_at_width() {
        // "hello world" is 11 chars. At width 6, should wrap to 2 lines.
        let text = text_from("hello world");
        assert_eq!(wrapped_line_count(&text, 6), 2);
    }

    #[test]
    fn explicit_newlines_counted() {
        let text = text_from("line1\nline2\nline3");
        assert_eq!(wrapped_line_count(&text, 80), 3);
    }

    #[test]
    fn empty_text() {
        // ratatui's Paragraph counts an empty text as 1 line (the empty line).
        // Components should guard with is_empty() before calling wrapped_line_count.
        let text = text_from("");
        assert_eq!(wrapped_line_count(&text, 80), 1);
    }

    #[test]
    fn zero_width() {
        let text = text_from("hello");
        assert_eq!(wrapped_line_count(&text, 0), 0);
    }

    /// Found by fuzzing: at width 2 a multi-column grapheme mid-word
    /// ("a佉b", or a Cf prepend cluster like "\u{604}<") drives
    /// ratatui's word wrapper into an out-of-bounds buffer write. Below
    /// MIN_WRAP_WIDTH rendering must truncate instead, and measurement
    /// must agree with it.
    #[test]
    fn degenerate_widths_truncate_instead_of_wrapping() {
        use ratatui_core::buffer::Buffer;
        use ratatui_core::layout::{Alignment, Rect};
        use ratatui_core::text::Line;

        for source in ["a\u{4f49}b", "x\u{604}<!", "one\ntwo\nthree"] {
            let text = Text::from(source.split('\n').map(Line::raw).collect::<Vec<_>>());
            for width in 1..=4u16 {
                let rows = wrapped_line_count(&text, width);
                let area = Rect::new(0, 0, width, rows.max(1));
                let mut buf = Buffer::empty(area);
                render_wrapped(text.clone(), Alignment::Left, 0, area, &mut buf);
            }
        }
        // The truncation fallback measures hard lines.
        let text = Text::from(vec![
            ratatui_core::text::Line::raw("one"),
            ratatui_core::text::Line::raw("two"),
        ]);
        assert_eq!(wrapped_line_count(&text, 2), 2);
    }

    #[test]
    fn long_paragraph_wraps() {
        let text = text_from(
            "This is a longer paragraph that should wrap across multiple lines \
             when rendered at a narrow terminal width.",
        );
        let count = wrapped_line_count(&text, 40);
        assert!(count >= 3, "expected >= 3 lines at width 40, got {}", count);
    }

    #[test]
    fn wrap_with_newlines_and_long_lines() {
        let text = text_from("short\nthis line is longer than twenty characters");
        let count = wrapped_line_count(&text, 20);
        // "short" = 1 line, "this line is longer than twenty characters" wraps to 3+ lines
        assert!(count >= 3, "expected >= 3 lines, got {}", count);
    }
}
