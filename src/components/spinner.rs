use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

use std::time::Duration;

use crate::component::Component;
use crate::hooks::Hooks;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// A built-in animated spinner component with a label.
///
/// Label and done state are props on the component itself.
/// Animation frame and styles are internal state.
///
/// ```ignore
/// Spinner::new("Loading...")
/// Spinner::new("Done").done("Completed!")
/// ```
pub struct Spinner {
    label: String,
    done: bool,
    done_label: Option<String>,
}

impl Spinner {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            done: false,
            done_label: None,
        }
    }

    /// Mark the spinner as already done, with a completion label.
    pub fn done(mut self, label: impl Into<String>) -> Self {
        self.done = true;
        self.done_label = Some(label.into());
        self
    }
}

/// State for a [`Spinner`] component.
///
/// Contains animation frame and styles (internal state).
/// Label and done status are props on the [`Spinner`] struct.
pub struct SpinnerState {
    /// Current animation frame index. Increment to animate.
    pub frame: usize,
    /// Style for the spinner character. Defaults to cyan.
    pub spinner_style: Style,
    /// Style for the label text. Defaults to dim italic.
    pub label_style: Style,
    /// Style for the done checkmark + label. Defaults to green.
    pub done_style: Style,
}

impl SpinnerState {
    pub fn new() -> Self {
        Self {
            frame: 0,
            spinner_style: Style::default().fg(Color::Cyan),
            label_style: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            done_style: Style::default().fg(Color::Green),
        }
    }

    /// Advance the animation by one frame.
    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }
}

impl Default for SpinnerState {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Spinner {
    type State = SpinnerState;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let line = if self.done {
            let label = self.done_label.as_deref().unwrap_or(&self.label);
            Line::from(vec![
                Span::styled("✓ ", state.done_style),
                Span::styled(label.to_string(), state.done_style),
            ])
        } else {
            let frame_str = FRAMES[state.frame % FRAMES.len()];
            Line::from(vec![
                Span::styled(format!("{} ", frame_str), state.spinner_style),
                Span::styled(self.label.clone(), state.label_style),
            ])
        };
        Paragraph::new(line).render(area, buf);
    }

    fn desired_height(&self, _width: u16, _state: &Self::State) -> u16 {
        1
    }

    fn initial_state(&self) -> SpinnerState {
        SpinnerState::new()
    }

    fn lifecycle(&self, hooks: &mut Hooks<SpinnerState>, _state: &SpinnerState) {
        if !self.done {
            hooks.use_interval(Duration::from_millis(80), |s| s.tick());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_height_is_one() {
        let spinner = Spinner::new("Loading...");
        let state = spinner.initial_state();
        assert_eq!(spinner.desired_height(80, &state), 1);
    }

    #[test]
    fn spinner_renders_frame() {
        let spinner = Spinner::new("Loading...");
        let state = spinner.initial_state();
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf, &state);
        assert_eq!(buf[(0, 0)].symbol(), "⠋");
    }

    #[test]
    fn spinner_done_shows_checkmark() {
        let spinner = Spinner::new("Loading...").done("Done!");
        let state = spinner.initial_state();
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        spinner.render(area, &mut buf, &state);
        assert_eq!(buf[(0, 0)].symbol(), "✓");
    }

    #[test]
    fn tick_advances_frame() {
        let mut state = SpinnerState::new();
        assert_eq!(state.frame, 0);
        state.tick();
        assert_eq!(state.frame, 1);
        state.tick();
        assert_eq!(state.frame, 2);
    }
}
