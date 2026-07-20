//! The first runnable v2 app: type a line, Enter commits it to the
//! timeline, Ctrl+C exits. Watch committed lines scroll into native
//! scrollback while the prompt stays live.
//!
//!     cargo run -p eye_declare_next --example echo

use crossterm::event::KeyCode;
use eye_declare_next::{
    App, Ctx, Element, Fluent, Focus, FocusHandle, InputEvent, Keymap, col, key, keymap, run,
    spinner, text,
};
use ratatui_core::style::{Color, Style};

#[derive(Clone)]
enum Msg {
    Typed(char),
    Backspace,
    Submit,
    Quit,
}

struct Echo {
    input_focus: FocusHandle,
    typed: String,
    submitted: usize,
}

impl App for Echo {
    type Msg = Msg;
    type Output = usize;

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Typed(c) => self.typed.push(c),
            Msg::Backspace => {
                self.typed.pop();
            }
            Msg::Submit => {
                if !self.typed.is_empty() {
                    let line = std::mem::take(&mut self.typed);
                    self.submitted += 1;
                    ctx.push(
                        text("✓ ")
                            .style(Style::default().fg(Color::Green))
                            .span(line, Style::default()),
                    );
                }
            }
            Msg::Quit => ctx.exit(self.submitted),
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col()
            .child(
                text("> ")
                    .style(Style::default().fg(Color::Cyan))
                    .span(&*self.typed, Style::default()),
            )
            .when(self.submitted > 0, |c| {
                c.child(
                    spinner(format!(
                        "{} line(s) echoed — Ctrl+C to quit",
                        self.submitted
                    ))
                    .done(false)
                    .spinner_style(Style::default().fg(Color::Yellow)),
                )
            })
    }

    fn keymap(&self) -> Keymap<Msg> {
        keymap()
            .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Quit)
            .in_scope(&self.input_focus, key(KeyCode::Enter), Msg::Submit)
            .fallthrough(&self.input_focus, |ev| match ev {
                InputEvent::Key(k) => match k.code {
                    KeyCode::Char(c) => Msg::Typed(c),
                    KeyCode::Backspace => Msg::Backspace,
                    _ => Msg::Typed(' '),
                },
                InputEvent::Paste(s) => Msg::Typed(s.chars().next().unwrap_or(' ')),
            })
    }
}

fn main() -> std::io::Result<()> {
    let focus = Focus::new();
    let input_focus = focus.handle();
    input_focus.focus();

    let submitted = run(Echo {
        input_focus,
        typed: String::new(),
        submitted: 0,
    })?;

    println!("echoed {submitted} line(s)");
    Ok(())
}
