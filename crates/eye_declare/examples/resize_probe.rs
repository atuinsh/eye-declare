//! Manual test bed for resize behavior.
//!
//!     cargo run -p eye_declare --example resize_probe
//!
//! Commits two fake conversation turns at startup, then sits on a
//! bordered-input tail. Resize the terminal every way you can — the
//! committed turns must survive and the tail must repaint cleanly.
//! Ctrl+C or Esc exits.

use crossterm::event::KeyCode;
use eye_declare::{
    App, Ctx, Element, ElementExt, Focus, FocusHandle, InputEvent, KeyboardProtocol, Keymap,
    RunOptions, TextAreaState, col, driver_tokio, key, keymap, panel, text, text_area,
};
use ratatui_core::style::{Color, Modifier, Style};

#[derive(Clone)]
enum Msg {
    Edit(InputEvent),
    Quit,
}

struct Probe {
    input_focus: FocusHandle,
    input: TextAreaState,
}

impl App for Probe {
    type Msg = Msg;
    type Output = ();

    fn init(&mut self, ctx: &mut Ctx<'_, Self>) {
        ctx.push(
            col()
                .child(text(" You ").style(Style::default().add_modifier(Modifier::REVERSED)))
                .child(text("Hi!").pad_left(2)),
        );
        ctx.push(
            col()
                .child(text(" Atuin AI ").style(Style::default().add_modifier(Modifier::REVERSED)))
                .child(
                    text("Hi! I'm the Atuin AI assistant. I can help you with shell commands.")
                        .pad_left(2),
                ),
        );
    }

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Edit(ev) => self.input.handle(&ev),
            Msg::Quit => ctx.exit(()),
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col()
            .child(
                panel(
                    text_area(&self.input)
                        .placeholder("Type a message...")
                        .placeholder_style(Style::default().fg(Color::DarkGray))
                        .track_focus(&self.input_focus),
                )
                .title("Generate a command or ask a question")
                .title_right("Atuin AI")
                .footer("[Enter] Send  [Shift+Enter] New line  [Esc] Exit")
                .border_style(Style::default().fg(Color::DarkGray))
                .pad_top(1),
            )
            .child(text("Model: balanced (/model to change)").pad_left(1))
    }

    fn keymap(&self) -> Keymap<Msg> {
        keymap()
            .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Quit)
            .on_override(key(KeyCode::Esc), Msg::Quit)
            .fallthrough(&self.input_focus, Msg::Edit)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let focus = Focus::new();
    let input_focus = focus.handle();
    input_focus.focus();

    driver_tokio::run_with(
        Probe {
            input_focus,
            input: TextAreaState::new(),
        },
        RunOptions::default().keyboard(KeyboardProtocol::Enhanced),
    )
    .await
}
