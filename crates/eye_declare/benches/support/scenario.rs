//! The deterministic benchmark app: an atuin-shaped chat TUI (markdown
//! turns, streaming assistant response, panel + text-area input) driven
//! headlessly. Shared by the criterion bench and the alloc-report example
//! via `#[path]` include.

use eye_declare::{
    App, Ctx, Element, ElementExt, Fluent, Focus, FocusHandle, InputEvent, Keymap, TextAreaState,
    col, key, keymap, markdown, panel, text, text_area,
};
use ratatui_core::style::{Color, Modifier, Style};

pub const WIDTH: u16 = 100;
pub const HEIGHT: u16 = 40;

/// Deterministic markdown "prose": headers, lists, inline styles, and a
/// code fence, cycled from a fixed word list. `bytes` is approximate.
pub fn markdown_response(bytes: usize) -> String {
    const WORDS: &[&str] = &[
        "terminal",
        "renders",
        "inline",
        "content",
        "with",
        "diffed",
        "escape",
        "sequences",
        "while",
        "the",
        "timeline",
        "commits",
        "finished",
        "blocks",
        "into",
        "scrollback",
    ];
    let mut out = String::with_capacity(bytes + 64);
    out.push_str("## Summary\n\n");
    let mut w = 0usize;
    let mut sentence = 0usize;
    while out.len() < bytes {
        match sentence % 7 {
            3 => {
                out.push_str("\n- ");
                out.push_str(WORDS[w % WORDS.len()]);
                out.push(' ');
                out.push_str(WORDS[(w + 3) % WORDS.len()]);
                out.push('\n');
            }
            5 => {
                out.push_str("\n```rust\nlet frame = engine.present(tail)?;\n```\n\n");
            }
            _ => {
                for i in 0..9 {
                    if i == 4 {
                        out.push_str("**");
                        out.push_str(WORDS[w % WORDS.len()]);
                        out.push_str("** ");
                    } else if i == 7 {
                        out.push('`');
                        out.push_str(WORDS[w % WORDS.len()]);
                        out.push_str("` ");
                    } else {
                        out.push_str(WORDS[w % WORDS.len()]);
                        out.push(' ');
                    }
                    w += 1;
                }
                out.push_str("of the\n");
            }
        }
        sentence += 1;
    }
    out
}

#[derive(Clone)]
pub enum Msg {
    /// Append one streamed chunk to the live response.
    Chunk(String),
    /// Seal the live response into scrollback.
    Seal,
    /// A key for the input editor.
    Input(InputEvent),
}

pub struct ChatApp {
    pub streaming: String,
    pub input: TextAreaState,
    pub input_focus: FocusHandle,
}

impl Default for ChatApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatApp {
    pub fn new() -> Self {
        let input_focus = Focus::new().handle();
        input_focus.focus();
        Self {
            streaming: String::new(),
            input: TextAreaState::new(),
            input_focus,
        }
    }

    /// An app mid-stream with ~`bytes` of markdown already received.
    pub fn mid_stream(bytes: usize) -> Self {
        let mut app = Self::new();
        app.streaming = markdown_response(bytes);
        app
    }
}

impl App for ChatApp {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Chunk(chunk) => self.streaming.push_str(&chunk),
            Msg::Seal => {
                let turn = std::mem::take(&mut self.streaming);
                ctx.push(turn_view(&turn));
            }
            Msg::Input(ev) => self.input.handle(&ev),
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col()
            .when(!self.streaming.is_empty(), |c| {
                c.child(turn_view(&self.streaming))
            })
            .child(
                panel(
                    text_area(&self.input)
                        .placeholder("Type a message...")
                        .track_focus(&self.input_focus)
                        .max_height(5),
                )
                .title("Ask")
                .footer("[Enter] Send")
                .border_style(Style::default().fg(Color::DarkGray))
                .pad_x(1),
            )
            .child(text(" Model: bench").style(Style::default().fg(Color::DarkGray)))
    }

    fn keymap(&self) -> Keymap<Msg> {
        keymap()
            .on(key(crossterm::event::KeyCode::Enter), Msg::Seal)
            .fallthrough(&self.input_focus, Msg::Input)
    }
}

pub fn turn_view(source: &str) -> impl Element + use<> {
    col()
        .child(
            text(" Assistant ").style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED),
            ),
        )
        .child(markdown(source.to_string()).pad_left(2))
}
