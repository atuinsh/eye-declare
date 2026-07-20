//! CommonMark rendering via pulldown-cmark (feature `markdown`, on by
//! default).
//!
//! Adapted from atuin-ai's renderer (MIT) — the component the flagship
//! consumer wrote to replace v1's hand-rolled line parser, now living in
//! the library where it belonged. Handles headings, bold/italic, inline
//! code, fenced code blocks, and (one level of) lists; word-wraps at the
//! render width with honest height measurement.

use std::cell::RefCell;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::text::{Line, Span, Text as RText};
use ratatui_core::widgets::Widget;

use crate::element::Element;

/// Style configuration for markdown rendering.
pub struct MarkdownStyles {
    pub base: Style,
    pub code_inline: Style,
    pub code_block: Style,
    pub bold: Style,
    pub italic: Style,
    pub heading: Style,
}

impl Default for MarkdownStyles {
    fn default() -> Self {
        let base = Style::default();
        Self {
            base,
            code_inline: Style::default().fg(Color::Yellow),
            code_block: Style::default().fg(Color::Green),
            bold: base.add_modifier(Modifier::BOLD),
            italic: base.add_modifier(Modifier::ITALIC),
            heading: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        }
    }
}

pub struct Markdown {
    source: String,
    styles: MarkdownStyles,
    /// Parsed text, shared between `height` and `render` within the
    /// element's lifetime (one frame). Elements are rebuilt per frame, so
    /// this halves the per-frame parse cost without any cross-frame
    /// invalidation story.
    parsed: RefCell<Option<RText<'static>>>,
    /// Wrapped row count per width, same lifetime story. `height` runs at
    /// least twice a frame (the tail measure, then container placement
    /// during render), and wrap-counting a long document isn't free.
    measured: RefCell<Option<(u16, u16)>>,
}

pub fn markdown(source: impl Into<String>) -> Markdown {
    Markdown {
        source: source.into(),
        styles: MarkdownStyles::default(),
        parsed: RefCell::new(None),
        measured: RefCell::new(None),
    }
}

impl Markdown {
    pub fn styles(mut self, styles: MarkdownStyles) -> Self {
        self.styles = styles;
        self.parsed.take();
        self.measured.take();
        self
    }

    fn with_parsed<R>(&self, f: impl FnOnce(&RText<'static>) -> R) -> R {
        let mut cache = self.parsed.borrow_mut();
        let parsed = cache.get_or_insert_with(|| parse(&self.source, &self.styles));
        f(parsed)
    }
}

impl Element for Markdown {
    fn height(&self, width: u16) -> u16 {
        if self.source.is_empty() || width == 0 {
            return 0;
        }
        if let Some((w, rows)) = *self.measured.borrow()
            && w == width
        {
            return rows;
        }
        let rows =
            self.with_parsed(|text| eye_declare_engine::wrap::wrapped_line_count(text, width));
        *self.measured.borrow_mut() = Some((width, rows));
        rows
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.source.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }
        self.with_parsed(|text| {
            // A borrowed view of the cached parse: per-line Vecs, but no
            // string copies (a deep clone would re-allocate every span).
            let borrowed = RText::from(
                text.lines
                    .iter()
                    .map(|line| {
                        Line::from(
                            line.spans
                                .iter()
                                .map(|s| Span::styled(s.content.as_ref(), s.style))
                                .collect::<Vec<_>>(),
                        )
                        .style(line.style)
                    })
                    .collect::<Vec<_>>(),
            );
            eye_declare_engine::wrap::wrapping_paragraph(borrowed).render(area, buf)
        });
    }
}

/// Parse markdown source into styled ratatui text.
fn parse(source: &str, styles: &MarkdownStyles) -> RText<'static> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut current_line = 0;

    let mut style_stack: Vec<Style> = vec![styles.base];
    let mut in_code_block = false;
    let mut in_list_item = false;
    // True until the first paragraph inside a list item has been opened;
    // that paragraph flows inline with the "- " prefix.
    let mut list_item_first_para = false;

    for event in Parser::new(source) {
        match event {
            Event::Start(Tag::Strong) => {
                let base = style_stack.last().copied().unwrap_or(styles.base);
                style_stack.push(base.add_modifier(Modifier::BOLD).patch(styles.bold));
            }
            Event::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            Event::Start(Tag::Emphasis) => {
                let base = style_stack.last().copied().unwrap_or(styles.base);
                style_stack.push(base.add_modifier(Modifier::ITALIC).patch(styles.italic));
            }
            Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                if !lines[current_line].is_empty() {
                    current_line += 1;
                    lines.push(Vec::new());
                    current_line += 1;
                    lines.push(Vec::new());
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if !lines[current_line].is_empty() {
                    current_line += 1;
                    lines.push(Vec::new());
                }
            }
            Event::Code(code) => {
                lines[current_line].push(Span::styled(code.to_string(), styles.code_inline));
            }
            Event::Text(text) => {
                let current_style = if in_code_block {
                    styles.code_block
                } else {
                    style_stack.last().copied().unwrap_or(styles.base)
                };
                let prefix = if in_code_block { "  " } else { "" };
                for (i, part) in text.split('\n').enumerate() {
                    if i > 0 {
                        current_line += 1;
                        lines.push(Vec::new());
                    }
                    if !part.is_empty() {
                        lines[current_line]
                            .push(Span::styled(format!("{prefix}{part}"), current_style));
                    }
                }
            }
            Event::SoftBreak => {
                let current_style = style_stack.last().copied().unwrap_or(styles.base);
                lines[current_line].push(Span::styled(" ", current_style));
            }
            Event::HardBreak => {
                current_line += 1;
                lines.push(Vec::new());
            }
            Event::Start(Tag::Paragraph) => {
                if in_list_item && list_item_first_para {
                    list_item_first_para = false;
                } else if current_line > 0 || !lines[0].is_empty() {
                    current_line += 1;
                    lines.push(Vec::new());
                    if !in_list_item {
                        // Blank separator between paragraphs.
                        current_line += 1;
                        lines.push(Vec::new());
                    }
                }
            }
            Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Heading { .. }) => {
                if current_line > 0 || !lines[0].is_empty() {
                    current_line += 1;
                    lines.push(Vec::new());
                    current_line += 1;
                    lines.push(Vec::new());
                }
                style_stack.push(styles.heading);
            }
            Event::End(TagEnd::Heading(_)) => {
                style_stack.pop();
            }
            Event::Start(Tag::Item) => {
                if current_line > 0 || !lines[0].is_empty() {
                    current_line += 1;
                    lines.push(Vec::new());
                }
                lines[current_line].push(Span::styled("- ", Style::default().fg(Color::DarkGray)));
                in_list_item = true;
                list_item_first_para = true;
            }
            Event::End(TagEnd::Item) => {
                in_list_item = false;
            }
            Event::Start(Tag::List(_)) if current_line > 0 || !lines[0].is_empty() => {
                current_line += 1;
                lines.push(Vec::new());
            }
            Event::End(TagEnd::List(_)) => {}
            _ => {}
        }
    }

    RText::from(lines.into_iter().map(Line::from).collect::<Vec<_>>())
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
    fn plain_paragraph() {
        assert_eq!(rendered(&markdown("hello world"), 20), vec!["hello world"]);
    }

    #[test]
    fn empty_source_is_zero_height() {
        assert_eq!(Element::height(&markdown(""), 20), 0);
    }

    #[test]
    fn paragraphs_separated_by_blank_line() {
        let lines = rendered(&markdown("one\n\ntwo"), 20);
        assert_eq!(lines, vec!["one", "", "two"]);
    }

    #[test]
    fn heading_is_styled() {
        let el = markdown("# Title");
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "T");
        assert_eq!(buf[(0, 0)].style().fg, Some(Color::Cyan));
    }

    #[test]
    fn bold_and_inline_code_styles() {
        let el = markdown("a **b** `c`");
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        // "a b c" — b bold, c yellow.
        assert!(buf[(2, 0)].style().add_modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(4, 0)].style().fg, Some(Color::Yellow));
    }

    #[test]
    fn code_block_indented_and_colored() {
        let lines = rendered(&markdown("para\n\n```\ncode here\n```"), 20);
        assert!(lines.contains(&"  code here".to_string()));
    }

    #[test]
    fn list_items_prefixed() {
        let lines = rendered(&markdown("- one\n- two"), 20);
        assert_eq!(lines, vec!["- one", "- two"]);
    }

    #[test]
    fn long_content_wraps_and_measures_consistently() {
        let el = markdown("this is a longer paragraph that should wrap");
        let h = Element::height(&el, 12);
        assert!(h >= 3, "expected wrapping at width 12, got {h}");
    }
}
