//! Raw buffer rendering component.
//!
//! [`Canvas`] is a leaf component that renders via a user-provided closure,
//! giving direct access to the ratatui [`Buffer`]. Use it for custom widgets,
//! charts, sparklines, or any rendering that the built-in components don't cover.
//!
//! Canvas is primarily used inside [`Component::view()`](crate::Component::view)
//! implementations to express raw rendering as part of an element tree.
//!
//! # Examples
//!
//! ```ignore
//! use eye_declare::{element, Canvas};
//! use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};
//! use ratatui_widgets::paragraph::Paragraph;
//!
//! // In element! macro
//! element! {
//!     Canvas(render_fn: |area: Rect, buf: &mut Buffer| {
//!         Paragraph::new("Hello from Canvas!").render(area, buf);
//!     })
//! }
//!
//! // With explicit height (skips probe measurement)
//! element! {
//!     Canvas(render_fn: |area, buf| { /* draw */ }, height: 3u16)
//! }
//! ```

use ratatui_core::{buffer::Buffer, layout::Rect};

use crate::component::Component;

type RenderFn = Box<dyn Fn(Rect, &mut Buffer) + Send + Sync>;
type DesiredHeightFn = Box<dyn Fn(u16) -> u16 + Send + Sync>;

fn noop_render(_: Rect, _: &mut Buffer) {}

/// A leaf component that renders via a user-provided closure.
///
/// See the [module-level docs](self) for examples.
#[derive(typed_builder::TypedBuilder)]
pub struct Canvas {
    #[builder(default = Box::new(noop_render), setter(transform = |f: impl Fn(Rect, &mut Buffer) + Send + Sync + 'static| Box::new(f) as RenderFn))]
    pub render_fn: RenderFn,
    #[builder(default, setter(into))]
    pub height: Option<u16>,
    /// Width-aware height function. Takes priority over `height`.
    /// When set, the framework calls this instead of probe rendering.
    #[builder(default, setter(transform = |f: impl Fn(u16) -> u16 + Send + Sync + 'static| Some(Box::new(f) as DesiredHeightFn)))]
    pub desired_height_fn: Option<DesiredHeightFn>,
}

impl Canvas {
    /// Create a new Canvas with the given render function.
    pub fn new(f: impl Fn(Rect, &mut Buffer) + Send + Sync + 'static) -> Self {
        Self {
            render_fn: Box::new(f),
            height: None,
            desired_height_fn: None,
        }
    }

    /// Set an explicit height hint, skipping probe-render measurement.
    ///
    /// Use this for components that fill their entire area (e.g., bordered
    /// widgets) where probe rendering would keep growing the buffer.
    pub fn with_height(mut self, h: u16) -> Self {
        self.height = Some(h);
        self
    }
}

impl Component for Canvas {
    type State = ();

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        (self.render_fn)(area, buf);
    }

    fn desired_height(&self, width: u16, _state: &()) -> Option<u16> {
        if let Some(ref f) = self.desired_height_fn {
            Some(f(width))
        } else {
            self.height
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::widgets::Widget;
    use ratatui_widgets::paragraph::Paragraph;

    #[test]
    fn canvas_renders_via_closure() {
        let canvas = Canvas::new(|area: Rect, buf: &mut Buffer| {
            Paragraph::new("hello").render(area, buf);
        });

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        canvas.render(area, &mut buf, &());

        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "h");
        assert_eq!(buf.cell((4, 0)).unwrap().symbol(), "o");
    }

    #[test]
    fn canvas_with_height_returns_desired_height() {
        let canvas = Canvas::new(|_, _| {}).with_height(5);
        assert_eq!(canvas.desired_height(80, &()), Some(5));
    }

    #[test]
    fn canvas_without_height_returns_none() {
        let canvas = Canvas::new(|_, _| {});
        assert_eq!(canvas.desired_height(80, &()), None);
    }
}
