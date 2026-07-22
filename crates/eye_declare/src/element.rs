//! The core `Element` trait and universal combinators.
//!
//! Elements are `Msg`-free: they describe structure and pixels. Message
//! emission lives in the keymap layer, so display code never names the
//! app's message type.

use std::time::Duration;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;

/// A renderable piece of UI.
///
/// The contract is honest measurement: [`height`](Element::height) must
/// return exactly the rows [`render`](Element::render) will use at the
/// given width, and must be cheap — the runtime calls it every frame for
/// every element in the live tail.
pub trait Element {
    /// Rows needed at the given width. Exact, cheap, no side effects.
    fn height(&self, width: u16) -> u16;

    /// Draw into `area` of `buf`. `area` is sized by the caller from
    /// [`height`](Element::height) (possibly clamped at the buffer edge).
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Frame interval if self-animating (e.g. a live spinner). The runtime
    /// re-presents the tail at the smallest returned interval while any
    /// animated element is present.
    fn animated(&self) -> Option<Duration> {
        None
    }

    /// Hardware cursor position relative to `area`, when this element is
    /// focused. `None` hides the cursor.
    fn cursor(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

/// A boxed, type-erased element. Heterogeneous match arms converge on this
/// via [`ElementExt::any`].
///
/// Carries a lifetime so views can borrow the app model: the tail is
/// built, rendered, and dropped within one frame, so model borrows are
/// naturally scoped. Fully-owned trees are `AnyElement<'static>`.
pub type AnyElement<'a> = Box<dyn Element + 'a>;

impl Element for Box<dyn Element + '_> {
    fn height(&self, width: u16) -> u16 {
        self.as_ref().height(width)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_ref().render(area, buf)
    }

    fn animated(&self) -> Option<Duration> {
        self.as_ref().animated()
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_ref().cursor(area)
    }
}

/// Combinators available on every element.
pub trait ElementExt: Element + Sized {
    /// Type-erase, for heterogeneous branches.
    fn any<'a>(self) -> AnyElement<'a>
    where
        Self: 'a,
    {
        Box::new(self)
    }

    /// Offset by `cols` columns of left padding.
    fn pad_left(self, cols: u16) -> Padded<Self> {
        Padded {
            inner: self,
            left: cols,
            top: 0,
        }
    }

    /// Offset by `rows` rows of top padding.
    fn pad_top(self, rows: u16) -> Padded<Self> {
        Padded {
            inner: self,
            left: 0,
            top: rows,
        }
    }
}

impl<E: Element> ElementExt for E {}

/// Conditional-builder combinators, available on every builder type
/// (elements, keymaps, anything chainable).
pub trait Fluent: Sized {
    /// Apply `f` only when `cond` holds.
    fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
    }

    /// Apply `f` with the value when present.
    fn when_some<T>(self, value: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self {
        match value {
            Some(v) => f(self, v),
            None => self,
        }
    }
}

impl<T> Fluent for T {}

/// An element offset by padding. Produced by [`ElementExt::pad_left`] /
/// [`ElementExt::pad_top`]; stack them for both.
pub struct Padded<E> {
    inner: E,
    left: u16,
    top: u16,
}

impl<E: Element> Element for Padded<E> {
    fn height(&self, width: u16) -> u16 {
        let inner_width = width.saturating_sub(self.left);
        if inner_width == 0 {
            return self.top;
        }
        self.top.saturating_add(self.inner.height(inner_width))
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let inner = Rect::new(
            area.x.saturating_add(self.left),
            area.y.saturating_add(self.top),
            area.width.saturating_sub(self.left),
            area.height.saturating_sub(self.top),
        );
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        self.inner.render(inner, buf);
    }

    fn animated(&self) -> Option<Duration> {
        self.inner.animated()
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        let inner = Rect::new(
            area.x.saturating_add(self.left),
            area.y.saturating_add(self.top),
            area.width.saturating_sub(self.left),
            area.height.saturating_sub(self.top),
        );
        self.inner
            .cursor(inner)
            .map(|(col, row)| (col.saturating_add(self.left), row.saturating_add(self.top)))
    }
}

/// The nothing element: zero height, renders nothing. For match arms and
/// conditionals with no output.
pub struct Empty;

pub fn empty() -> Empty {
    Empty
}

impl Element for Empty {
    fn height(&self, _width: u16) -> u16 {
        0
    }

    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::text;

    #[test]
    fn empty_is_zero_height() {
        assert_eq!(Element::height(&empty(), 80), 0);
    }

    #[test]
    fn padded_adds_top_rows_and_left_cols() {
        let el = text("hi").pad_left(2).pad_top(1);
        assert_eq!(el.height(10), 2); // 1 top pad + 1 text row

        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        assert_eq!(buf[(2, 1)].symbol(), "h");
        assert_eq!(buf[(3, 1)].symbol(), "i");
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn padded_narrows_wrap_width() {
        // "hello world" wraps at width 6; with pad_left(4) the inner
        // width at outer width 10 is 6 → 2 rows.
        let el = text("hello world").pad_left(4);
        assert_eq!(el.height(10), 2);
    }

    #[test]
    fn any_erases_heterogeneous_branches() {
        let branch =
            |b: bool| -> AnyElement<'static> { if b { text("yes").any() } else { empty().any() } };
        assert_eq!(branch(true).height(80), 1);
        assert_eq!(branch(false).height(80), 0);
    }
}
