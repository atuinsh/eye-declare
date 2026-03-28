use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use eye_declare::{Component, InlineRenderer};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

/// A simple component that displays a growing list of messages.
struct MessageList;

impl Component for MessageList {
    type State = Vec<(String, Style)>;

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let lines: Vec<Line> = state
            .iter()
            .map(|(text, style)| Line::styled(text.as_str(), *style))
            .collect();
        Paragraph::new(lines).render(area, buf);
    }

    fn initial_state(&self) -> Option<Vec<(String, Style)>> {
        Some(vec![])
    }
}

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut renderer = InlineRenderer::new(width);
    let id = renderer.push(MessageList);

    let messages = vec![
        (
            "Thinking...",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ),
        (
            "Analyzing your codebase...",
            Style::default().fg(Color::Cyan),
        ),
        ("Found 3 relevant files.", Style::default().fg(Color::Green)),
        ("  src/lib.rs", Style::default().fg(Color::DarkGray)),
        ("  src/main.rs", Style::default().fg(Color::DarkGray)),
        ("  src/config.rs", Style::default().fg(Color::DarkGray)),
        (
            "Generating implementation plan...",
            Style::default().fg(Color::Cyan),
        ),
        (
            "Done!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    for (text, style) in messages {
        renderer
            .state_mut::<MessageList>(id)
            .push((text.to_string(), style));

        let output = renderer.render();
        io::stdout().write_all(&output)?;
        io::stdout().flush()?;

        thread::sleep(Duration::from_millis(600));
    }

    // Move to a new line so the shell prompt appears below
    println!();

    Ok(())
}
