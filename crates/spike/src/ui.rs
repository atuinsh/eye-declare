//! Candidate v2 DSL — stub implementation for the bake-off.
//!
//! Fluent builders, no macro. The `Element` trait matches the REDESIGN.md
//! sketch: honest `height(width)` measurement, plain `render`, optional
//! self-animation. Rendering is unimplemented — only call-site shape is
//! under test here.

use std::time::Duration;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;

/// A renderable piece of UI. `Msg` is the app's message type (Elm-style);
/// display-only elements implement it for every `Msg`.
pub trait Element<Msg> {
    /// Honest measurement: rows needed at the given width.
    fn height(&self, _width: u16) -> u16 {
        0
    }

    fn render(&self, _area: Rect, _buf: &mut Buffer) {}

    /// Frame interval if self-animating (e.g. a live spinner). The runtime
    /// self-ticks the tail while any animated element is present.
    fn animated(&self) -> Option<Duration> {
        None
    }
}

/// A boxed, type-erased element. Match arms that produce different element
/// types converge on this via [`ElementExt::any`].
pub type AnyElement<Msg> = Box<dyn Element<Msg>>;

impl<Msg> Element<Msg> for Box<dyn Element<Msg>> {
    fn height(&self, width: u16) -> u16 {
        self.as_ref().height(width)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_ref().render(area, buf)
    }

    fn animated(&self) -> Option<Duration> {
        self.as_ref().animated()
    }
}

/// Type erasure. Separate from [`Fluent`] because `any()` mentions `Msg` in
/// its return type (so inference can pin it from context), while the
/// combinators must NOT be `Msg`-parameterized at all — display-only elements
/// implement `Element<Msg>` for every `Msg`, and a `Msg`-generic `pad_left`
/// on such a receiver is uninferrable (found the hard way; see FINDINGS.md).
pub trait ElementExt<Msg>: Element<Msg> + Sized + 'static {
    /// Type-erase, for heterogeneous branches.
    fn any(self) -> AnyElement<Msg> {
        Box::new(self)
    }
}

impl<Msg, E: Element<Msg> + 'static> ElementExt<Msg> for E {}

/// `Msg`-free combinators, available on every builder.
pub trait Fluent: Sized {
    /// Apply `f` only when `cond` holds. The `#(if ...)` analog.
    fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
    }

    /// Apply `f` with the value when present. The `#(if let Some ...)` analog.
    fn when_some<T>(self, value: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self {
        match value {
            Some(v) => f(self, v),
            None => self,
        }
    }

    fn pad_left(self, cols: u16) -> Padded<Self> {
        Padded {
            inner: self,
            left: cols,
            top: 0,
        }
    }

    fn pad_top(self, rows: u16) -> Padded<Self> {
        Padded {
            inner: self,
            left: 0,
            top: rows,
        }
    }
}

impl<T> Fluent for T {}

/// An element offset by padding. Produced by [`Fluent::pad_left`] /
/// [`Fluent::pad_top`].
pub struct Padded<E> {
    inner: E,
    left: u16,
    top: u16,
}

impl<Msg, E: Element<Msg>> Element<Msg> for Padded<E> {}

// ───────────────────────────────────────────────────────────────────
// Containers
// ───────────────────────────────────────────────────────────────────

/// Vertical stack. Children get full width; height is content-driven.
pub struct Col<Msg> {
    children: Vec<AnyElement<Msg>>,
    gap: u16,
}

pub fn col<Msg>() -> Col<Msg> {
    Col {
        children: Vec::new(),
        gap: 0,
    }
}

impl<Msg> Col<Msg> {
    /// Blank rows between children.
    pub fn gap(mut self, rows: u16) -> Self {
        self.gap = rows;
        self
    }

    pub fn child(mut self, child: impl Element<Msg> + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: Element<Msg> + 'static,
    {
        self.children
            .extend(children.into_iter().map(|c| Box::new(c) as AnyElement<Msg>));
        self
    }
}

impl<Msg> Element<Msg> for Col<Msg> {}

/// Column width within a [`Row`].
pub enum Width {
    Fixed(u16),
    /// Remaining space, split equally among `Fill` cells.
    Fill,
}

/// Horizontal layout. Cells declare `Fixed(n)` or `Fill` widths;
/// row height is the max of cell heights.
pub struct Row<Msg> {
    cells: Vec<(Width, AnyElement<Msg>)>,
}

pub fn row<Msg>() -> Row<Msg> {
    Row { cells: Vec::new() }
}

impl<Msg> Row<Msg> {
    pub fn fixed(mut self, cols: u16, child: impl Element<Msg> + 'static) -> Self {
        self.cells.push((Width::Fixed(cols), Box::new(child)));
        self
    }

    pub fn fill(mut self, child: impl Element<Msg> + 'static) -> Self {
        self.cells.push((Width::Fill, Box::new(child)));
        self
    }
}

impl<Msg> Element<Msg> for Row<Msg> {}

// ───────────────────────────────────────────────────────────────────
// Leaves
// ───────────────────────────────────────────────────────────────────

/// One word-wrapped run of styled spans.
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

    pub fn span(mut self, content: impl Into<String>, style: Style) -> Self {
        self.spans.push((content.into(), style));
        self
    }
}

impl<Msg> Element<Msg> for Text {}

/// The nothing element, for match arms with no output.
pub struct Empty;

pub fn empty() -> Empty {
    Empty
}

impl<Msg> Element<Msg> for Empty {}

pub struct Spinner {
    label: String,
    done: bool,
    hide_checkmark: bool,
    label_style: Style,
    spinner_style: Style,
}

pub fn spinner(label: impl Into<String>) -> Spinner {
    Spinner {
        label: label.into(),
        done: false,
        hide_checkmark: false,
        label_style: Style::default(),
        spinner_style: Style::default(),
    }
}

impl Spinner {
    pub fn done(mut self, done: bool) -> Self {
        self.done = done;
        self
    }

    pub fn hide_checkmark(mut self) -> Self {
        self.hide_checkmark = true;
        self
    }

    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    pub fn spinner_style(mut self, style: Style) -> Self {
        self.spinner_style = style;
        self
    }
}

impl<Msg> Element<Msg> for Spinner {
    fn animated(&self) -> Option<Duration> {
        (!self.done).then(|| Duration::from_millis(80))
    }
}

/// CommonMark rendering (pulldown-cmark behind the scenes in the real thing).
pub struct Markdown {
    source: String,
}

pub fn markdown(source: impl Into<String>) -> Markdown {
    Markdown {
        source: source.into(),
    }
}

impl<Msg> Element<Msg> for Markdown {}

/// Fixed-height tail window over a list of lines.
pub struct Viewport {
    lines: Vec<String>,
    height: u16,
    style: Style,
    wrap: bool,
}

pub fn viewport(lines: Vec<String>) -> Viewport {
    Viewport {
        lines,
        height: 1,
        style: Style::default(),
        wrap: true,
    }
}

impl Viewport {
    pub fn height(mut self, rows: u16) -> Self {
        self.height = rows;
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

impl<Msg> Element<Msg> for Viewport {}
