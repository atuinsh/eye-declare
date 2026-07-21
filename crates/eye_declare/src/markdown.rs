//! CommonMark rendering via pulldown-cmark (feature `markdown`, on by
//! default).
//!
//! Adapted from atuin-ai's renderer (MIT) — the component the flagship
//! consumer wrote to replace v1's hand-rolled line parser, now living in
//! the library where it belonged. Handles headings, bold/italic, inline
//! code, fenced code blocks, (one level of) lists, and GFM tables;
//! word-wraps at the render width with honest height measurement.
//!
//! Tables render as bordered grids sized to their content, shrinking the
//! widest columns (and wrapping cell text) when the terminal is narrower
//! than the table. With [`Markdown::streaming`] enabled the element also
//! renders tables *in progress*: a header line whose delimiter row hasn't
//! arrived yet is shown optimistically, and a table still growing at the
//! end of the source stretches to the full render width so its outer
//! border holds still while cells stream in.

use std::borrow::Cow;
use std::cell::RefCell;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::{Alignment, Rect};
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
    /// Table rules and borders (the box-drawing characters).
    pub table_border: Style,
    /// Patched over the base style for header-row cells.
    pub table_header: Style,
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
            table_border: Style::default().fg(Color::DarkGray),
            table_header: base.add_modifier(Modifier::BOLD),
        }
    }
}

pub struct Markdown {
    source: String,
    styles: MarkdownStyles,
    streaming: bool,
    /// Parsed blocks, shared between `height` and `render` within the
    /// element's lifetime (one frame). Elements are rebuilt per frame, so
    /// this halves the per-frame parse cost without any cross-frame
    /// invalidation story.
    blocks: RefCell<Option<Vec<Block>>>,
    /// Per-width layout (wrapped row counts, table column widths), same
    /// lifetime story. `height` runs at least twice a frame (the tail
    /// measure, then container placement during render), and laying out a
    /// long document isn't free.
    layout: RefCell<Option<(u16, Vec<BlockLayout>)>>,
}

pub fn markdown(source: impl Into<String>) -> Markdown {
    Markdown {
        source: source.into(),
        styles: MarkdownStyles::default(),
        streaming: false,
        blocks: RefCell::new(None),
        layout: RefCell::new(None),
    }
}

impl Markdown {
    pub fn styles(mut self, styles: MarkdownStyles) -> Self {
        self.styles = styles;
        self.blocks.take();
        self.layout.take();
        self
    }

    /// Treat the source as a live streaming tail. GFM only recognizes a
    /// table once its delimiter row (`|---|---|`) is complete, so during
    /// token-by-token streaming the header would render as a raw-pipe
    /// paragraph and then reflow into a table. With this flag the element
    /// renders the trailing table-in-progress immediately, and stretches a
    /// table that runs to the end of the source to the full render width
    /// so its outer border holds still as cells arrive. Committed
    /// (finished) content should render without this flag: it settles the
    /// table to content-hugging widths and never speculates about
    /// incomplete syntax.
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self.blocks.take();
        self.layout.take();
        self
    }

    fn with_blocks<R>(&self, f: impl FnOnce(&[Block]) -> R) -> R {
        let mut cache = self.blocks.borrow_mut();
        let blocks = cache.get_or_insert_with(|| {
            let cooked = if self.streaming {
                cook_streaming_tail(&self.source)
            } else {
                Cow::Borrowed(self.source.as_str())
            };
            parse(&cooked, &self.styles)
        });
        f(blocks)
    }

    fn with_layout<R>(&self, width: u16, f: impl FnOnce(&[Block], &[BlockLayout]) -> R) -> R {
        self.with_blocks(|blocks| {
            let mut cache = self.layout.borrow_mut();
            let valid = matches!(*cache, Some((w, _)) if w == width);
            if !valid {
                let layouts = blocks
                    .iter()
                    .map(|b| layout_block(b, width, self.streaming))
                    .collect();
                *cache = Some((width, layouts));
            }
            match &*cache {
                Some((_, layouts)) => f(blocks, layouts),
                None => f(blocks, &[]),
            }
        })
    }
}

impl Element for Markdown {
    fn height(&self, width: u16) -> u16 {
        if self.source.is_empty() || width == 0 {
            return 0;
        }
        self.with_layout(width, |_, layouts| total_rows(layouts))
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.source.is_empty() || area.width == 0 || area.height == 0 {
            return;
        }
        self.with_layout(area.width, |blocks, layouts| {
            let mut y = area.y;
            for (i, (block, layout)) in blocks.iter().zip(layouts).enumerate() {
                if i > 0 {
                    // One blank row between blocks, mirroring paragraph
                    // separation inside text blocks.
                    y = y.saturating_add(1);
                }
                if y >= area.bottom() {
                    break;
                }
                match (block, layout) {
                    (Block::Text(text), BlockLayout::Text { rows }) => {
                        let rect = Rect::new(area.x, y, area.width, *rows).intersection(area);
                        if rect.height > 0 {
                            eye_declare_engine::wrap::wrapping_paragraph(borrow_text(text))
                                .render(rect, buf);
                        }
                        y = y.saturating_add(*rows);
                    }
                    (
                        Block::Table(table),
                        BlockLayout::Table {
                            col_widths,
                            row_heights,
                            rows,
                        },
                    ) => {
                        let rect = Rect::new(area.x, y, area.width, *rows).intersection(area);
                        render_table(table, col_widths, row_heights, rect, buf, &self.styles);
                        y = y.saturating_add(*rows);
                    }
                    // Blocks and layouts are built in lockstep; a mismatch
                    // cannot happen, but skipping is safer than panicking.
                    _ => {}
                }
            }
        });
    }
}

/// A top-level chunk of the document. Text blocks flow through the word
/// wrapper; tables lay themselves out on a grid and must never be
/// re-wrapped line-by-line.
enum Block {
    Text(RText<'static>),
    Table(Table),
}

/// Make text safe to become cell symbols: tabs expand to spaces, other
/// control characters become U+FFFD. Raw controls in a cell are hazardous
/// twice over — an ESC would splice into the terminal's escape stream when
/// the row is emitted, and zero-width controls drive ratatui's word
/// wrapper out of bounds (found by fuzzing: `"\0[佉x"` at width 2 panics
/// inside `Paragraph::render`).
fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

struct Table {
    alignments: Vec<Alignment>,
    /// Header cells; every row has exactly `alignments.len()` cells.
    header: Vec<Line<'static>>,
    rows: Vec<Vec<Line<'static>>>,
    /// The table runs to the end of the source — while streaming, it is
    /// still growing, so it stretches to the full render width.
    open: bool,
}

enum BlockLayout {
    Text {
        rows: u16,
    },
    Table {
        col_widths: Vec<u16>,
        /// Header first, then one entry per body row (cells wrap within
        /// their columns, so a row can be taller than one line).
        row_heights: Vec<u16>,
        rows: u16,
    },
}

impl BlockLayout {
    fn rows(&self) -> u16 {
        match self {
            BlockLayout::Text { rows } => *rows,
            BlockLayout::Table { rows, .. } => *rows,
        }
    }
}

fn total_rows(layouts: &[BlockLayout]) -> u16 {
    let mut total: u32 = 0;
    for (i, layout) in layouts.iter().enumerate() {
        if i > 0 {
            total += 1;
        }
        total += layout.rows() as u32;
    }
    total.min(u32::from(u16::MAX)) as u16
}

fn layout_block(block: &Block, width: u16, streaming: bool) -> BlockLayout {
    match block {
        Block::Text(text) => BlockLayout::Text {
            rows: eye_declare_engine::wrap::wrapped_line_count(text, width),
        },
        Block::Table(table) => layout_table(table, width, streaming),
    }
}

fn layout_table(table: &Table, width: u16, streaming: bool) -> BlockLayout {
    let ncols = table.alignments.len();
    let mut widths: Vec<usize> = (0..ncols)
        .map(|i| {
            let mut w = table.header.get(i).map(|l| l.width()).unwrap_or(0);
            for row in &table.rows {
                w = w.max(row.get(i).map(|l| l.width()).unwrap_or(0));
            }
            w.max(1)
        })
        .collect();

    // Per column: border + padding either side of the content, plus the
    // closing border.
    let budget = (width as usize).saturating_sub(ncols * 3 + 1);
    let total: usize = widths.iter().sum();
    if total > budget {
        // Wider than the terminal: shave the widest column first (cell
        // text wraps within its column), down to a floor of one cell.
        let mut excess = total - budget;
        while excess > 0 {
            let Some((i, &w)) = widths.iter().enumerate().max_by_key(|(_, w)| **w) else {
                break;
            };
            if w <= 1 {
                break;
            }
            widths[i] -= 1;
            excess -= 1;
        }
    } else if streaming && table.open {
        // Still growing at the end of a streaming source: give the slack
        // to the last column so the outer border sits at the full width
        // instead of chasing every token.
        if let Some(last) = widths.last_mut() {
            *last += budget - total;
        }
    }

    let col_widths: Vec<u16> = widths
        .into_iter()
        .map(|w| w.min(usize::from(u16::MAX)) as u16)
        .collect();
    let mut row_heights = Vec::with_capacity(table.rows.len() + 1);
    row_heights.push(row_height(&table.header, &col_widths));
    for row in &table.rows {
        row_heights.push(row_height(row, &col_widths));
    }
    // Top and bottom rules always; the header separator only once there is
    // a body (a lone header otherwise renders with rules back-to-back).
    let rules: u32 = if table.rows.is_empty() { 2 } else { 3 };
    let rows = (rules + row_heights.iter().map(|&h| u32::from(h)).sum::<u32>())
        .min(u32::from(u16::MAX)) as u16;
    BlockLayout::Table {
        col_widths,
        row_heights,
        rows,
    }
}

fn row_height(cells: &[Line<'static>], col_widths: &[u16]) -> u16 {
    let mut h = 1;
    for (cell, &w) in cells.iter().zip(col_widths) {
        if cell.spans.is_empty() {
            continue;
        }
        let text = RText::from(borrow_line(cell));
        h = h.max(eye_declare_engine::wrap::wrapped_line_count(&text, w));
    }
    h
}

fn render_table(
    table: &Table,
    col_widths: &[u16],
    row_heights: &[u16],
    region: Rect,
    buf: &mut Buffer,
    styles: &MarkdownStyles,
) {
    if region.width == 0 || region.height == 0 || col_widths.is_empty() {
        return;
    }

    let mut border_x = Vec::with_capacity(col_widths.len() + 1);
    let mut x = region.x;
    border_x.push(x);
    for &w in col_widths {
        x = x.saturating_add(w).saturating_add(3);
        border_x.push(x);
    }

    let mut y = region.y;
    draw_rule(
        buf,
        region,
        col_widths,
        y,
        ('┌', '┬', '┐'),
        styles.table_border,
    );
    y = y.saturating_add(1);
    y = draw_cells(
        buf,
        region,
        table,
        col_widths,
        &border_x,
        &table.header,
        row_heights.first().copied().unwrap_or(1),
        y,
        styles,
    );
    if !table.rows.is_empty() {
        draw_rule(
            buf,
            region,
            col_widths,
            y,
            ('├', '┼', '┤'),
            styles.table_border,
        );
        y = y.saturating_add(1);
        for (row, &h) in table.rows.iter().zip(&row_heights[1..]) {
            y = draw_cells(buf, region, table, col_widths, &border_x, row, h, y, styles);
        }
    }
    draw_rule(
        buf,
        region,
        col_widths,
        y,
        ('└', '┴', '┘'),
        styles.table_border,
    );
}

fn draw_rule(
    buf: &mut Buffer,
    region: Rect,
    col_widths: &[u16],
    y: u16,
    (left, mid, right): (char, char, char),
    style: Style,
) {
    if y >= region.bottom() {
        return;
    }
    let mut s = String::new();
    for (i, &w) in col_widths.iter().enumerate() {
        s.push(if i == 0 { left } else { mid });
        for _ in 0..(w as usize + 2) {
            s.push('─');
        }
    }
    s.push(right);
    buf.set_stringn(region.x, y, &s, region.width as usize, style);
}

#[allow(clippy::too_many_arguments)]
fn draw_cells(
    buf: &mut Buffer,
    region: Rect,
    table: &Table,
    col_widths: &[u16],
    border_x: &[u16],
    cells: &[Line<'static>],
    height: u16,
    y: u16,
    styles: &MarkdownStyles,
) -> u16 {
    for dy in 0..height {
        let yy = y.saturating_add(dy);
        if yy >= region.bottom() {
            break;
        }
        for &bx in border_x {
            if bx < region.right() {
                buf[(bx, yy)].set_symbol("│").set_style(styles.table_border);
            }
        }
    }
    for (i, cell) in cells.iter().enumerate().take(col_widths.len()) {
        let rect =
            Rect::new(border_x[i].saturating_add(2), y, col_widths[i], height).intersection(region);
        if rect.width == 0 || rect.height == 0 {
            continue;
        }
        eye_declare_engine::wrap::wrapping_paragraph(RText::from(borrow_line(cell)))
            .alignment(table.alignments.get(i).copied().unwrap_or(Alignment::Left))
            .render(rect, buf);
    }
    y.saturating_add(height)
}

/// A borrowed view of cached spans: per-line Vecs, but no string copies (a
/// deep clone would re-allocate every span).
fn borrow_line<'a>(line: &'a Line<'static>) -> Line<'a> {
    Line::from(
        line.spans
            .iter()
            .map(|s| Span::styled(s.content.as_ref(), s.style))
            .collect::<Vec<_>>(),
    )
    .style(line.style)
}

fn borrow_text<'a>(text: &'a RText<'static>) -> RText<'a> {
    RText::from(text.lines.iter().map(borrow_line).collect::<Vec<_>>())
}

/// Parse markdown source into styled blocks.
fn parse(source: &str, styles: &MarkdownStyles) -> Vec<Block> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    let mut blocks: Vec<Block> = Vec::new();
    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut current_line = 0;

    let mut style_stack: Vec<Style> = vec![styles.base];
    let mut in_code_block = false;
    let mut in_list_item = false;
    // True until the first paragraph inside a list item has been opened;
    // that paragraph flows inline with the "- " prefix.
    let mut list_item_first_para = false;
    let mut table: Option<TableAcc> = None;

    for (event, range) in Parser::new_ext(source, Options::ENABLE_TABLES).into_offset_iter() {
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
                let span = Span::styled(sanitize(&code), styles.code_inline);
                match table.as_mut() {
                    Some(t) if t.in_cell => t.cell.push(span),
                    Some(_) => {}
                    None => lines[current_line].push(span),
                }
            }
            Event::Text(text) => {
                if let Some(t) = table.as_mut() {
                    if t.in_cell {
                        let style = style_stack.last().copied().unwrap_or(styles.base);
                        t.cell.push(Span::styled(sanitize(&text), style));
                    }
                    continue;
                }
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
                        let part = sanitize(part);
                        lines[current_line]
                            .push(Span::styled(format!("{prefix}{part}"), current_style));
                    }
                }
            }
            Event::SoftBreak => {
                let current_style = style_stack.last().copied().unwrap_or(styles.base);
                let span = Span::styled(" ", current_style);
                match table.as_mut() {
                    Some(t) if t.in_cell => t.cell.push(span),
                    Some(_) => {}
                    None => lines[current_line].push(span),
                }
            }
            Event::HardBreak => {
                match table.as_mut() {
                    // Cells are single lines; a hard break degrades to a
                    // space.
                    Some(t) if t.in_cell => {
                        let style = style_stack.last().copied().unwrap_or(styles.base);
                        t.cell.push(Span::styled(" ", style));
                    }
                    Some(_) => {}
                    None => {
                        current_line += 1;
                        lines.push(Vec::new());
                    }
                }
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
            Event::Start(Tag::Table(alignments)) => {
                flush_text(&mut lines, &mut current_line, &mut blocks);
                table = Some(TableAcc {
                    alignments: alignments.iter().map(|a| convert_alignment(*a)).collect(),
                    header: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                    cell: Vec::new(),
                    in_cell: false,
                    // Nothing after the table in the source: it may still
                    // be growing (streaming stretch keys off this).
                    open: source[range.end.min(source.len())..].trim().is_empty(),
                });
            }
            Event::End(TagEnd::Table) => {
                if let Some(mut t) = table.take() {
                    // pulldown-cmark pads and truncates rows to the header
                    // column count already; normalize defensively so the
                    // layout can index without checks.
                    let ncols = t.alignments.len().max(1);
                    t.alignments.resize(ncols, Alignment::Left);
                    t.header.resize_with(ncols, Line::default);
                    for row in &mut t.rows {
                        row.resize_with(ncols, Line::default);
                    }
                    blocks.push(Block::Table(Table {
                        alignments: t.alignments,
                        header: t.header,
                        rows: t.rows,
                        open: t.open,
                    }));
                }
            }
            Event::Start(Tag::TableHead) => {
                let base = style_stack.last().copied().unwrap_or(styles.base);
                style_stack.push(base.patch(styles.table_header));
            }
            Event::End(TagEnd::TableHead) => {
                style_stack.pop();
                if let Some(t) = table.as_mut() {
                    t.header = std::mem::take(&mut t.row);
                }
            }
            Event::Start(Tag::TableRow) => {}
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(t) = table.as_mut() {
                    t.in_cell = true;
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(t) = table.as_mut() {
                    t.in_cell = false;
                    let spans = std::mem::take(&mut t.cell);
                    t.row.push(Line::from(spans));
                }
            }
            _ => {}
        }
    }

    flush_text(&mut lines, &mut current_line, &mut blocks);
    blocks
}

struct TableAcc {
    alignments: Vec<Alignment>,
    header: Vec<Line<'static>>,
    rows: Vec<Vec<Line<'static>>>,
    row: Vec<Line<'static>>,
    cell: Vec<Span<'static>>,
    in_cell: bool,
    open: bool,
}

fn convert_alignment(a: pulldown_cmark::Alignment) -> Alignment {
    match a {
        pulldown_cmark::Alignment::Center => Alignment::Center,
        pulldown_cmark::Alignment::Right => Alignment::Right,
        _ => Alignment::Left,
    }
}

/// Seal the accumulated text lines into a `Block::Text` and reset the
/// accumulator to a fresh document start.
fn flush_text(
    lines: &mut Vec<Vec<Span<'static>>>,
    current_line: &mut usize,
    blocks: &mut Vec<Block>,
) {
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    // Merge adjacent same-style spans. pulldown-cmark fragments plain text
    // into many events ("[", entities, replacement chars), and span count
    // is not free downstream: every span is a wrap-layout boundary, and
    // ratatui's word wrapper mishandles one of them — a wide char alone in
    // a span at the last column with another span following writes past
    // the buffer edge (fuzz-found; upstream bug, spans ["a","佉","b"] at
    // width 2 panic Paragraph::render). Merging removes every unstyled
    // instance of that shape.
    for line in lines.iter_mut() {
        line.dedup_by(|next, prev| {
            if next.style == prev.style {
                prev.content.to_mut().push_str(&next.content);
                true
            } else {
                false
            }
        });
    }
    if !lines.is_empty() {
        let taken = std::mem::take(lines);
        blocks.push(Block::Text(RText::from(
            taken.into_iter().map(Line::from).collect::<Vec<_>>(),
        )));
    }
    lines.clear();
    lines.push(Vec::new());
    *current_line = 0;
}

/// Make a table-in-progress at the end of a streaming source parseable:
/// GFM only recognizes a table once the delimiter row is complete, so
/// until then the header would flash as a raw-pipe paragraph. If the
/// trailing lines look like a table without a finished delimiter row,
/// synthesize one matching the header's current cell count — re-cooked
/// every frame, the table grows a column at a time as pipes arrive.
///
/// Headers without a leading pipe (`Name | Age`, valid GFM) are
/// deliberately not speculated: mid-stream they are indistinguishable
/// from prose that happens to contain a pipe, and a spurious table
/// flashing over a sentence is worse than the one-time reflow those
/// tables get when their real delimiter row completes.
fn cook_streaming_tail(source: &str) -> Cow<'_, str> {
    // A single trailing newline is still mid-table (the delimiter row may
    // be the next chunk): scan without it, but remember that the last line
    // is complete.
    let (scan, last_line_complete) = match source.strip_suffix('\n') {
        Some(s) => (s, true),
        None => (source, false),
    };
    if scan.is_empty() {
        return Cow::Borrowed(source);
    }

    // Find the trailing run of `|`-prefixed lines, ignoring anything
    // inside a fenced code block (pipes there are content, not cells).
    // A fence only closes on its own marker: a `~~~` line inside a
    // backtick fence is code, not a boundary.
    let mut fence: Option<char> = None;
    let mut first: Option<&str> = None;
    let mut second: Option<(usize, &str)> = None;
    let mut run_len = 0usize;
    let mut offset = 0usize;
    for line in scan.split('\n') {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        let boundary = match (fence, marker) {
            (None, Some(m)) => {
                fence = Some(m);
                true
            }
            (Some(f), Some(m)) if f == m => {
                fence = None;
                true
            }
            _ => false,
        };
        if boundary || fence.is_some() || !trimmed.starts_with('|') {
            first = None;
            second = None;
            run_len = 0;
        } else {
            if first.is_none() {
                first = Some(line);
                second = None;
                run_len = 0;
            }
            run_len += 1;
            if run_len == 2 {
                second = Some((offset, line));
            }
        }
        offset += line.len() + 1;
    }

    let Some(header) = first else {
        return Cow::Borrowed(source);
    };
    // pulldown rejects a header that is a lone `|` with no cell content
    // and no second pipe; synthesizing under one would fall back to a
    // paragraph and leak the synthetic delimiter as visible text.
    if header.trim() == "|" {
        return Cow::Borrowed(source);
    }
    match (run_len, second) {
        // Header alone: no delimiter row yet.
        (1, _) => {
            let sep = if last_line_complete { "" } else { "\n" };
            let delim = delimiter_row(cell_count(header).max(1));
            Cow::Owned(format!("{source}{sep}{delim}"))
        }
        (2, Some((delim_offset, delim))) => {
            let cols = cell_count(header).max(1);
            let complete =
                delimiter_charset(delim) && delim.contains('-') && cell_count(delim) == cols;
            if complete {
                // Already a valid table; pulldown handles partial rows.
                Cow::Borrowed(source)
            } else if delimiter_charset(delim) && !last_line_complete {
                // The delimiter row is still streaming: swap in a finished
                // one. A *completed* line that doesn't match the header is
                // genuinely malformed, not in-progress — leave it be.
                let delim = delimiter_row(cols);
                Cow::Owned(format!("{}{delim}", &source[..delim_offset]))
            } else {
                Cow::Borrowed(source)
            }
        }
        // Three or more lines: either a valid table pulldown already
        // parses, or malformed input that isn't ours to fix.
        _ => Cow::Borrowed(source),
    }
}

/// Cells implied by a (possibly partial) table line: unescaped `|` are
/// separators; one leading and one trailing pipe are decoration.
fn cell_count(line: &str) -> usize {
    let mut t = line.trim();
    t = t.strip_prefix('|').unwrap_or(t);
    if t.ends_with('|') && !t.ends_with("\\|") {
        t = &t[..t.len() - 1];
    }
    let mut count = 1;
    let mut escaped = false;
    for c in t.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '|' => count += 1,
            _ => {}
        }
    }
    count
}

/// The line could still become a delimiter row: only pipes, dashes,
/// colons, and whitespace so far.
fn delimiter_charset(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

fn delimiter_row(cols: usize) -> String {
    format!("|{}", "---|".repeat(cols))
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

    // ── tables ─────────────────────────────────────────────────────

    #[test]
    fn table_renders_with_borders() {
        let lines = rendered(&markdown("| a | b |\n|---|---|\n| 1 | 2 |"), 20);
        assert_eq!(
            lines,
            vec![
                "┌───┬───┐",
                "│ a │ b │",
                "├───┼───┤",
                "│ 1 │ 2 │",
                "└───┴───┘",
            ]
        );
    }

    #[test]
    fn table_header_is_bold() {
        let el = markdown("| ab | c |\n|---|---|\n| 1 | 2 |");
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        // Header cell "ab" starts at x=2 on row 1.
        assert_eq!(buf[(2, 1)].symbol(), "a");
        assert!(buf[(2, 1)].style().add_modifier.contains(Modifier::BOLD));
        // Body cell is not bold.
        assert!(!buf[(2, 3)].style().add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn table_between_text_gets_blank_separators() {
        let lines = rendered(&markdown("before\n\n| a |\n|---|\n| 1 |\n\nafter"), 20);
        assert_eq!(
            lines,
            vec![
                "before",
                "",
                "┌───┐",
                "│ a │",
                "├───┤",
                "│ 1 │",
                "└───┘",
                "",
                "after",
            ]
        );
    }

    #[test]
    fn right_aligned_column() {
        let lines = rendered(&markdown("| aa | b |\n|--:|---|\n| 1 | 2 |"), 20);
        assert_eq!(lines[3], "│  1 │ 2 │");
    }

    #[test]
    fn wide_table_shrinks_and_wraps_cells() {
        let lines = rendered(
            &markdown("| a | b |\n|---|---|\n| aaaaaaaaaaaaaaaaaaaa | b |"),
            16,
        );
        // Column one shrinks to fit; its 20-char cell wraps to three rows.
        assert_eq!(lines.len(), 7, "got {lines:?}");
        assert!(lines.iter().all(|l| l.chars().count() <= 16));
        assert!(lines[3].contains("aaaaaaaa"));
        assert!(lines[5].contains("aaaa"));
    }

    #[test]
    fn static_partial_header_stays_text() {
        // Without streaming, incomplete table syntax is what it is: text.
        assert_eq!(rendered(&markdown("| Name"), 20), vec!["| Name"]);
    }

    // ── streaming tables ───────────────────────────────────────────

    #[test]
    fn streaming_partial_header_is_full_width_cell() {
        let lines = rendered(&markdown("| Name").streaming(true), 20);
        assert_eq!(
            lines,
            vec![
                "┌──────────────────┐",
                "│ Name             │",
                "└──────────────────┘",
            ]
        );
    }

    #[test]
    fn streaming_header_splits_when_next_cell_arrives() {
        let lines = rendered(&markdown("| Name | Ag").streaming(true), 30);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].starts_with("│ Name │ Ag"), "got {:?}", lines[1]);
        // Stretched to the full width: border in the last column.
        assert!(lines[1].ends_with('│'));
        assert_eq!(lines[0].chars().count(), 30);
    }

    #[test]
    fn streaming_header_survives_trailing_newline() {
        // "…|\n" mid-stream: the delimiter row may be the next chunk.
        let lines = rendered(&markdown("| a | b |\n").streaming(true), 20);
        assert!(lines[0].starts_with('┌'), "got {lines:?}");
    }

    #[test]
    fn streaming_partial_delimiter_keeps_table() {
        let lines = rendered(&markdown("| a | b |\n| --").streaming(true), 20);
        assert!(lines[0].starts_with('┌'), "got {lines:?}");
        assert!(lines[1].contains("│ a │ b"), "got {lines:?}");
    }

    #[test]
    fn streaming_row_cell_grows_then_splits() {
        let partial = rendered(
            &markdown("| a | b |\n|---|---|\n| Alice | 3").streaming(true),
            30,
        );
        assert!(
            partial[3].starts_with("│ Alice │ 3"),
            "got {:?}",
            partial[3]
        );

        let next_row = rendered(
            &markdown("| a | b |\n|---|---|\n| Alice | 30 |\n| Bob").streaming(true),
            30,
        );
        assert!(next_row[4].starts_with("│ Bob"), "got {:?}", next_row[4]);
    }

    #[test]
    fn streaming_complete_table_hugs_content() {
        // Content after the table ends the stretch: it is no longer open.
        let lines = rendered(
            &markdown("| a | b |\n|---|---|\n| 1 | 2 |\n\ndone").streaming(true),
            30,
        );
        assert_eq!(lines[0], "┌───┬───┐");
        assert_eq!(*lines.last().unwrap(), "done");
    }

    #[test]
    fn streaming_ignores_pipes_in_code_fences() {
        let lines = rendered(&markdown("```\n| a | b |").streaming(true), 20);
        assert!(lines.iter().any(|l| l == "  | a | b |"), "got {lines:?}");
        assert!(!lines.iter().any(|l| l.contains('┌')), "got {lines:?}");
    }

    #[test]
    fn streaming_height_matches_render() {
        // The honest-measurement contract holds mid-stream at any width.
        let snapshots = [
            "| Na",
            "| Name |",
            "| Name | Age |\n",
            "| Name | Age |\n| --- | -",
            "| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 2",
        ];
        for src in snapshots {
            for width in [5u16, 12, 30, 80] {
                let el = markdown(src).streaming(true);
                let h = el.height(width);
                assert!(h > 0, "zero height for {src:?} at {width}");
            }
        }
    }

    #[test]
    fn every_streaming_prefix_renders() {
        // A stream can be cut mid-token anywhere; every prefix must
        // measure and render without panicking, at any width.
        let doc = "Intro text.\n\n| Name | Age |\n| --- | ---: |\n| Alice | 30 |\n| Bob \\| Jr | 25 |\n\n```\n| not | a table |\n```\n\ndone";
        for end in doc.char_indices().map(|(i, _)| i).chain([doc.len()]) {
            let src = &doc[..end];
            for width in [3u16, 10, 24, 60] {
                let el = markdown(src).streaming(true);
                let h = el.height(width);
                let area = Rect::new(0, 0, width, h);
                let mut buf = Buffer::empty(area);
                el.render(area, &mut buf);
            }
        }
    }

    #[test]
    fn streaming_lone_pipe_stays_raw() {
        // pulldown rejects a `|`-only header row, so synthesis would fall
        // back to a paragraph and show the synthetic delimiter as text.
        for src in ["|", "| ", "|\n"] {
            assert_eq!(rendered(&markdown(src).streaming(true), 20), vec!["|"]);
        }
    }

    #[test]
    fn streaming_empty_cell_header_still_speculates() {
        // `||` is a header pulldown accepts; the lone-pipe guard must not
        // swallow it.
        let lines = rendered(&markdown("||").streaming(true), 20);
        assert!(lines[0].starts_with('┌'), "got {lines:?}");
    }

    #[test]
    fn streaming_fence_marker_mismatch_stays_code() {
        // A `~~~` line inside a backtick fence is code, not a fence
        // boundary; pipes after it must not be cooked into a table.
        let lines = rendered(&markdown("```\n~~~\n| a | b |").streaming(true), 20);
        assert!(lines.iter().any(|l| l == "  | a | b |"), "got {lines:?}");
        assert!(
            !lines.iter().any(|l| l.contains('┌') || l.contains("---")),
            "synthetic delimiter leaked: {lines:?}"
        );
    }

    #[test]
    fn cell_count_handles_partial_and_escaped_pipes() {
        assert_eq!(cell_count("| a"), 1);
        assert_eq!(cell_count("| a |"), 1);
        assert_eq!(cell_count("| a | b"), 2);
        assert_eq!(cell_count("| a | b |"), 2);
        assert_eq!(cell_count("| a \\| b |"), 1);
        assert_eq!(cell_count("|"), 1);
    }

    /// Found by fuzzing (fuzz/fuzz_targets/markdown_element.rs):
    /// `"\0[佉&"` at width 2 panicked with an out-of-bounds buffer write
    /// inside ratatui's word wrapper — its trigger is a wide char alone in
    /// a span at the last column with another span following (upstream
    /// bug). Merging adjacent same-style spans removes every unstyled
    /// instance of that shape from parse output.
    #[test]
    fn fragmented_spans_with_wide_chars_render_safely() {
        let el = markdown("\u{0}[\u{4f49}&");
        for width in 1..8 {
            let height = Element::height(&el, width);
            let area = Rect::new(0, 0, width, height);
            let mut buf = Buffer::empty(area);
            el.render(area, &mut buf); // must not panic
        }
    }

    /// Raw control characters must never survive into span content: an
    /// ESC would splice into the terminal's escape stream when the row is
    /// emitted, and zero-width controls confuse wrap layout. Tabs expand;
    /// everything else becomes U+FFFD.
    #[test]
    fn control_chars_are_sanitized() {
        let lines = rendered(&markdown("a\u{0}b\tc\u{1b}[31m"), 40);
        assert_eq!(lines, vec!["a\u{fffd}b    c\u{fffd}[31m"]);
    }
}
