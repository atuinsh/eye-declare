use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use eye_delcare::{InlineRenderer, TextBlock};
use ratatui_core::style::{Color, Modifier, Style};

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // Header
    let header = r.push(TextBlock);
    {
        let s = r.state_mut::<TextBlock>(header);
        s.push(
            "Display-Time Wrapping Demo",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        s.push(
            format!("Terminal width: {} columns — resize to see reflow! Press q or Ctrl+C to exit.", width),
            Style::default().fg(Color::DarkGray),
        );
        s.push("", Style::default());
    }
    flush(&mut r, &mut stdout)?;
    r.freeze(header);

    // Long paragraphs that will wrap
    let para1 = r.push(TextBlock);
    {
        let s = r.state_mut::<TextBlock>(para1);
        s.push(
            "This is a long paragraph that demonstrates display-time word wrapping. \
             The text is stored as a single logical line, and the framework wraps it \
             at render time based on the current terminal width. Try resizing your \
             terminal window — the text will reflow automatically. This is the same \
             approach used by Codex's tui2 architecture.",
            Style::default().fg(Color::White),
        );
    }

    let spacer = r.push(TextBlock);
    { r.state_mut::<TextBlock>(spacer).push("", Style::default()); }
    r.freeze(spacer);

    let para2 = r.push(TextBlock);
    {
        let s = r.state_mut::<TextBlock>(para2);
        s.push(
            "Here's a second paragraph with different styling to show that \
             multiple components each wrap independently. Each component computes \
             its own height based on wrapped line count, and the framework stacks \
             them vertically. The total content height adjusts as wrapping changes.",
            Style::default().fg(Color::Yellow),
        );
    }

    let spacer2 = r.push(TextBlock);
    { r.state_mut::<TextBlock>(spacer2).push("", Style::default()); }
    r.freeze(spacer2);

    let code_block = r.push(TextBlock);
    {
        let s = r.state_mut::<TextBlock>(code_block);
        s.push(
            "fn main() {",
            Style::default().fg(Color::Green),
        );
        s.push(
            "    println!(\"Short lines like code don't wrap unless the terminal is very narrow.\");",
            Style::default().fg(Color::Green),
        );
        s.push(
            "}",
            Style::default().fg(Color::Green),
        );
    }

    let spacer3 = r.push(TextBlock);
    { r.state_mut::<TextBlock>(spacer3).push("", Style::default()); }
    r.freeze(spacer3);

    let status = r.push(TextBlock);

    // Initial render
    update_status(&mut r, status, width);
    flush(&mut r, &mut stdout)?;

    // Enable raw mode for event polling (but we'll handle it gracefully)
    crossterm::terminal::enable_raw_mode()?;

    loop {
        // Poll for events with a timeout
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Resize(new_width, _new_height) => {
                    let output = r.resize(new_width);
                    // Update status line with new width
                    update_status(&mut r, status, new_width);
                    let status_output = r.render();

                    stdout.write_all(&output)?;
                    stdout.write_all(&status_output)?;
                    stdout.flush()?;
                }
                Event::Key(key) => {
                    if key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    println!();
    Ok(())
}

fn update_status(r: &mut InlineRenderer, id: eye_delcare::NodeId, width: u16) {
    let s = r.state_mut::<TextBlock>(id);
    s.clear();
    s.push(
        format!("Current width: {} columns — press q to exit", width),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    );
}

fn flush(r: &mut InlineRenderer, stdout: &mut impl Write) -> io::Result<()> {
    let output = r.render();
    if !output.is_empty() {
        stdout.write_all(&output)?;
        stdout.flush()?;
    }
    Ok(())
}
