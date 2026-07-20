//! A complete streaming AI chat TUI against the OpenRouter API, in one
//! file. This is the flagship example: every pattern a real agent
//! interface needs, with none of the app-specific noise.
//!
//! ```sh
//! export OPENROUTER_API_KEY=sk-or-...
//! cargo run --release --example openrouter
//! # optionally: export OPENROUTER_MODEL=anthropic/claude-sonnet-4.5
//! ```
//!
//! What to look for:
//! - **The struct is the model** (strict Elm): conversation history, the
//!   live response, the input editor, and the in-flight request handle are
//!   all plain fields. No framework state anywhere.
//! - **Committed output is an effect**: finished turns leave the program
//!   through `ctx.push` — the welcome banner in `init`, your message at
//!   submit, the assistant's turn when its stream completes. Only the
//!   live tail re-renders.
//! - **Cancellation is drop**: the request is a `Task` held in the model;
//!   Esc sets it to `None` and the HTTP stream dies at its await point.
//! - **Keys are data**: `keymap()` is rebuilt from the model every
//!   update, so Enter only means "send" when there's something to send
//!   and no request in flight. What a key does never depends on hidden
//!   focus state.
//! - **The spinner costs zero code**: it declares its own animation via
//!   `Element::animated`; the runtime ticks while it's visible.

use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use eye_declare::{
    App, Ctx, Element, ElementExt, Fluent, Focus, FocusHandle, InputEvent, KeyboardProtocol,
    Keymap, RunOptions, Task, TextAreaState, col, key, keymap, markdown, panel, spinner, text,
    text_area,
};
use futures::StreamExt;
use ratatui_core::style::{Color, Modifier, Style};

// ───────────────────────────────────────────────────────────────────
// Model
// ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum Msg {
    /// Unclaimed key/paste — routed to the input editor.
    Input(InputEvent),
    /// Insert a newline (Shift+Enter, or Ctrl+J where unsupported).
    Newline,
    /// Send the typed message.
    Submit,
    /// A streamed piece of the assistant's reply.
    Chunk(String),
    /// The reply finished cleanly.
    StreamDone,
    /// The request failed (connection, HTTP error, bad key…).
    StreamFailed(String),
    /// Esc: cancel the in-flight request, or quit when idle.
    CancelOrQuit,
    /// Ctrl+C: leave immediately.
    Quit,
}

struct Chat {
    api_key: String,
    model: String,
    http: reqwest::Client,

    /// The conversation so far, as (role, content) — what the API sees.
    history: Vec<(&'static str, String)>,
    /// The assistant's reply as it streams in. Lives in the tail until
    /// the stream ends, then leaves through `ctx.push`.
    streaming: String,
    /// The in-flight request. Dropping it cancels the HTTP stream —
    /// Esc-cancels-generation is `self.request = None`.
    request: Option<Task>,
    started: Option<Instant>,

    input: TextAreaState,
    input_focus: FocusHandle,
    error: Option<String>,
}

impl Chat {
    fn new(api_key: String, model: String) -> Self {
        let input_focus = Focus::new().handle();
        input_focus.focus();
        Self {
            api_key,
            model,
            http: reqwest::Client::new(),
            history: Vec::new(),
            streaming: String::new(),
            request: None,
            started: None,
            input: TextAreaState::new(),
            input_focus,
            error: None,
        }
    }

    fn busy(&self) -> bool {
        self.request.is_some()
    }

    /// Seal the streamed reply into scrollback and record it in history.
    fn finish_reply(&mut self, note: Option<&str>, ctx: &mut Ctx<'_, Self>) {
        let reply = std::mem::take(&mut self.streaming);
        self.request = None;
        self.started = None;
        if reply.is_empty() && note.is_none() {
            return;
        }
        ctx.push(assistant_turn(&reply, note));
        if !reply.is_empty() {
            self.history.push(("assistant", reply));
        }
    }
}

// ───────────────────────────────────────────────────────────────────
// Update: all policy lives here
// ───────────────────────────────────────────────────────────────────

impl App for Chat {
    type Msg = Msg;
    type Output = ();

    fn init(&mut self, ctx: &mut Ctx<'_, Self>) {
        ctx.push(
            col()
                .child(
                    text("eye-declare chat")
                        .style(Style::default().add_modifier(Modifier::BOLD))
                        .span(
                            format!("  {}", self.model),
                            Style::default().fg(Color::DarkGray),
                        ),
                )
                .child(
                    text("Streaming chat over OpenRouter. Transcript stays in your terminal.")
                        .style(Style::default().fg(Color::DarkGray)),
                ),
        );
    }

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Input(ev) => self.input.handle(&ev),
            Msg::Newline => self.input.insert_newline(),
            Msg::Submit => {
                let prompt = self.input.take_text().trim().to_string();
                if prompt.is_empty() || self.busy() {
                    return;
                }
                self.error = None;

                // The user's turn is finished the moment it's sent:
                // commit it to scrollback, like a shell echoing a command.
                ctx.push(user_turn(&prompt));
                self.history.push(("user", prompt));

                // Spawn the reply as a stream of messages; hold the Task.
                self.started = Some(Instant::now());
                self.request = Some(ctx.spawn(chat_stream(
                    self.http.clone(),
                    self.api_key.clone(),
                    self.model.clone(),
                    &self.history,
                )));
            }
            Msg::Chunk(delta) => {
                // Cancellation is prompt but asynchronous: a chunk that
                // was already queued when Esc dropped the Task still
                // arrives. Validity comes from the model, not from the
                // assumption that a cancelled stream falls silent.
                if self.busy() {
                    self.streaming.push_str(&delta);
                }
            }
            Msg::StreamDone => self.finish_reply(None, ctx),
            Msg::StreamFailed(e) => {
                // Keep whatever streamed before the failure.
                self.finish_reply(None, ctx);
                self.error = Some(e);
            }
            Msg::CancelOrQuit => {
                if self.busy() {
                    // Dropping the Task cancels the HTTP request; the
                    // partial reply is still worth keeping.
                    self.finish_reply(Some("interrupted"), ctx);
                } else {
                    ctx.exit(());
                }
            }
            Msg::Quit => ctx.exit(()),
        }
    }

    // ───────────────────────────────────────────────────────────────
    // View: a pure function of the model, re-run every frame
    // ───────────────────────────────────────────────────────────────

    fn tail(&self) -> impl Element + '_ {
        let waiting = self.busy() && self.streaming.is_empty();

        col()
            .when(!self.streaming.is_empty(), |c| {
                c.child(
                    col()
                        .child(assistant_label())
                        .child(markdown(self.streaming.clone()).pad_left(2)),
                )
            })
            .when(waiting, |c| {
                // Wall-clock elapsed in the label: pure view-side time
                // dependence, refreshed by the spinner's own animation
                // ticks — no timer subscription, no messages.
                let elapsed = self.started.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                c.child(
                    spinner(format!("Waiting for the model… {elapsed}s"))
                        .label_style(Style::default().fg(Color::DarkGray))
                        .pad_top(1),
                )
            })
            .when_some(self.error.clone(), |c, e| {
                c.child(
                    text("Error: ")
                        .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                        .span(e, Style::default().fg(Color::Red))
                        .pad_top(1),
                )
            })
            .child(
                panel(
                    text_area(&self.input)
                        .placeholder("Ask anything…")
                        .track_focus(&self.input_focus)
                        .max_height(6),
                )
                .title("You")
                .title_right(&self.model)
                .footer(if self.busy() {
                    "[Esc] Cancel"
                } else {
                    "[Enter] Send  [Shift+Enter] Newline  [Esc] Quit"
                })
                .border_style(Style::default().fg(Color::DarkGray))
                .pad_x(1)
                .pad_top(1),
            )
    }

    fn keymap(&self) -> Keymap<Msg> {
        let mut km = keymap()
            .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Quit)
            .on(key(KeyCode::Esc), Msg::CancelOrQuit);

        // Enter means "send" only when sending is meaningful. Rebuilding
        // the keymap from the model each update is what makes this a
        // one-liner instead of a mode flag.
        if !self.busy() && !self.input.is_blank() {
            km = km.on(key(KeyCode::Enter), Msg::Submit);
        }
        km = km
            .on(key(KeyCode::Enter).shift(), Msg::Newline)
            .on(key(KeyCode::Char('j')).ctrl(), Msg::Newline);

        km.fallthrough(&self.input_focus, Msg::Input)
    }
}

// ───────────────────────────────────────────────────────────────────
// Turn views: plain functions returning elements
// ───────────────────────────────────────────────────────────────────

fn assistant_label() -> impl Element + use<> {
    text(" Assistant ").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::REVERSED),
    )
}

fn user_turn(content: &str) -> impl Element + use<> {
    col()
        .child(
            text(" You ").style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::REVERSED),
            ),
        )
        .child(text(content.to_string()).pad_left(2))
        .pad_top(1)
}

fn assistant_turn(content: &str, note: Option<&str>) -> impl Element + use<> {
    col()
        .child(assistant_label())
        .when(!content.is_empty(), |c| {
            c.child(markdown(content.to_string()).pad_left(2))
        })
        .when_some(note.map(String::from), |c, note| {
            c.child(
                text(format!("({note})"))
                    .style(
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )
                    .pad_left(2),
            )
        })
        .pad_top(1)
}

// ───────────────────────────────────────────────────────────────────
// The request as a stream of messages
// ───────────────────────────────────────────────────────────────────

/// POST the conversation with `stream: true` and translate the SSE
/// response into `Msg`s. Errors become messages too — the app decides
/// what they mean. Dropping the stream (via its `Task`) aborts the
/// request at whatever await point it's suspended on.
fn chat_stream(
    http: reqwest::Client,
    api_key: String,
    model: String,
    history: &[(&'static str, String)],
) -> impl futures::Stream<Item = Msg> + Send + use<> {
    let messages: Vec<serde_json::Value> = history
        .iter()
        .map(|(role, content)| serde_json::json!({ "role": role, "content": content }))
        .collect();

    async_stream::stream! {
        let response = http
            .post("https://openrouter.ai/api/v1/chat/completions")
            .bearer_auth(&api_key)
            .json(&serde_json::json!({
                "model": model,
                "messages": messages,
                "stream": true,
            }))
            .timeout(Duration::from_secs(300))
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                yield Msg::StreamFailed(e.to_string());
                return;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let body = body.chars().take(200).collect::<String>();
            yield Msg::StreamFailed(format!("{status}: {body}"));
            return;
        }

        // Minimal SSE parse: lines of `data: <json>`, ending with
        // `data: [DONE]`. Comment lines (`: keep-alive`) are skipped.
        let mut bytes = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(b) => buf.extend_from_slice(&b),
                Err(e) => {
                    yield Msg::StreamFailed(e.to_string());
                    return;
                }
            }
            while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line);
                let Some(payload) = line.trim().strip_prefix("data: ") else {
                    continue;
                };
                if payload == "[DONE]" {
                    yield Msg::StreamDone;
                    return;
                }
                // (Full SSE permits one event's data to span several
                // `data:` fields; chat completion APIs emit one JSON
                // object per field, so this parser keeps to one line.)
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(payload) {
                    // A 200 stream can still deliver an error payload
                    // mid-flight; surface it instead of ending "cleanly"
                    // with a silently truncated reply.
                    if let Some(error) = event.get("error") {
                        let message = error["message"].as_str().unwrap_or("provider error");
                        yield Msg::StreamFailed(message.to_string());
                        return;
                    }
                    if let Some(delta) = event["choices"][0]["delta"]["content"].as_str()
                        && !delta.is_empty()
                    {
                        yield Msg::Chunk(delta.to_string());
                    }
                }
            }
        }
        // The body ended without `data: [DONE]`: something upstream cut
        // the stream. The partial reply is kept either way, but the user
        // should know it may be incomplete.
        yield Msg::StreamFailed("stream ended before completion".to_string());
    }
}

// ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let Ok(api_key) = std::env::var("OPENROUTER_API_KEY") else {
        eprintln!("Set OPENROUTER_API_KEY to run this example:");
        eprintln!("  export OPENROUTER_API_KEY=sk-or-...   # https://openrouter.ai/keys");
        std::process::exit(1);
    };
    let model = std::env::var("OPENROUTER_MODEL").unwrap_or_else(|_| "openrouter/auto".to_string());

    let options = RunOptions::default().keyboard(KeyboardProtocol::Enhanced);
    eye_declare::driver_tokio::run_with(Chat::new(api_key, model), options).await?;
    Ok(())
}
