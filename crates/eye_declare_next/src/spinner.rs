//! Animated spinner. Stateless by design: the frame index derives from the
//! wall clock, because the runtime re-presents the tail continuously while
//! any element reports [`animated`](crate::Element::animated). No effect
//! system, no per-widget tick state — the v1 `use_interval` machinery has
//! no v2 equivalent because nothing needs it.

use std::time::Duration;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;

use crate::element::Element;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_MS: u64 = 80;

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

    /// When done, show only the label (no leading checkmark).
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

    fn marker(&self) -> Option<&'static str> {
        if self.done {
            if self.hide_checkmark {
                None
            } else {
                Some("✓")
            }
        } else {
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            Some(FRAMES[((millis / FRAME_MS) % FRAMES.len() as u64) as usize])
        }
    }
}

impl Element for Spinner {
    fn height(&self, width: u16) -> u16 {
        if width == 0 { 0 } else { 1 }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut x = area.x;
        if let Some(marker) = self.marker() {
            buf.set_stringn(x, area.y, marker, area.width as usize, self.spinner_style);
            x = x.saturating_add(2); // marker + space
        }
        if !self.label.is_empty() && x < area.right() {
            let max = (area.right() - x) as usize;
            buf.set_stringn(x, area.y, &self.label, max, self.label_style);
        }
    }

    fn animated(&self) -> Option<Duration> {
        (!self.done).then(|| Duration::from_millis(FRAME_MS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_spinner_is_animated() {
        assert_eq!(
            spinner("working").animated(),
            Some(Duration::from_millis(80))
        );
        assert_eq!(spinner("done").done(true).animated(), None);
    }

    #[test]
    fn done_renders_checkmark_and_label() {
        let el = spinner("built").done(true);
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "✓");
        assert_eq!(buf[(2, 0)].symbol(), "b");
    }

    #[test]
    fn hide_checkmark_starts_at_label() {
        let el = spinner("ran").done(true).hide_checkmark();
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        el.render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "r");
    }
}
