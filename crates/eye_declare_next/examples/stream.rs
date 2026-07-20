//! A miniature streaming agent — the Atuin AI shape on the v2 stack.
//!
//!     cargo run -p eye_declare_next --example stream
//!
//! Type a prompt, Enter starts a fake agent turn that streams words back;
//! the streaming text and spinner live in the tail, and the finished turn
//! seals into scrollback. Esc cancels a turn mid-stream (dropping the Task
//! is the entire cancellation mechanism). Ctrl+C exits.

use std::time::Duration;

use crossterm::event::KeyCode;
use eye_declare_next::{
    App, Ctx, Element, ElementExt, Fluent, Focus, FocusHandle, InputEvent, Keymap, Task, col,
    driver_tokio, key, keymap, spinner, text,
};
use futures::StreamExt;
use ratatui_core::style::{Color, Modifier, Style};

#[derive(Clone)]
enum Msg {
    Typed(char),
    Backspace,
    Submit,
    Delta(String),
    Done,
    CancelTurn,
    Quit,
}

struct Agent {
    input_focus: FocusHandle,
    typed: String,
    response: String,
    streaming: Option<Task>,
}

impl Agent {
    fn seal_turn(&mut self, ctx: &mut Ctx<'_, Self>, note: &str) {
        let response = std::mem::take(&mut self.response);
        ctx.push(
            col()
                .child(text(" agent ").style(Style::default().add_modifier(Modifier::REVERSED)))
                .child(text(format!("{response}{note}")).pad_left(2)),
        );
        self.streaming = None;
    }
}

impl App for Agent {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Typed(c) => self.typed.push(c),
            Msg::Backspace => {
                self.typed.pop();
            }
            Msg::Submit => {
                if self.typed.trim().is_empty() || self.streaming.is_some() {
                    return;
                }
                let prompt = std::mem::take(&mut self.typed);
                ctx.push(
                    col()
                        .child(
                            text(" you ").style(Style::default().add_modifier(Modifier::REVERSED)),
                        )
                        .child(text(prompt.clone()).pad_left(2)),
                );
                self.streaming = Some(ctx.spawn(agent_turn(prompt)));
            }
            Msg::Delta(word) => self.response.push_str(&word),
            Msg::Done => self.seal_turn(ctx, ""),
            Msg::CancelTurn => {
                if self.streaming.take().is_some() {
                    self.seal_turn(ctx, " [cancelled]");
                }
            }
            Msg::Quit => ctx.exit(()),
        }
    }

    fn tail(&self) -> impl Element + '_ {
        let busy = self.streaming.is_some();

        col()
            .when(busy, |c| {
                c.child(
                    col()
                        .child(
                            text(" agent ")
                                .style(Style::default().add_modifier(Modifier::REVERSED)),
                        )
                        .child(text(self.response.as_str()).pad_left(2))
                        .child(
                            spinner("streaming — Esc to cancel")
                                .label_style(Style::default().fg(Color::DarkGray))
                                .pad_left(2),
                        ),
                )
            })
            .child(
                text("> ")
                    .style(Style::default().fg(Color::Cyan))
                    .span(&*self.typed, Style::default())
                    .pad_top(1),
            )
    }

    fn keymap(&self) -> Keymap<Msg> {
        keymap()
            .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Quit)
            .when(self.streaming.is_some(), |k| {
                k.on(key(KeyCode::Esc), Msg::CancelTurn)
            })
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

/// The fake turn: stream the prompt back word by word, slowly.
fn agent_turn(prompt: String) -> impl futures::Stream<Item = Msg> + Send {
    let words: Vec<String> = format!("you said: {prompt} — and that's all I know about it")
        .split_whitespace()
        .map(|w| format!("{w} "))
        .collect();

    futures::stream::iter(words)
        .then(|word| async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            Msg::Delta(word)
        })
        .chain(futures::stream::iter([Msg::Done]))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let focus = Focus::new();
    let input_focus = focus.handle();
    input_focus.focus();

    driver_tokio::run(Agent {
        input_focus,
        typed: String::new(),
        response: String::new(),
        streaming: None,
    })
    .await
}
