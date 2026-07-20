//! Port 4: the driver loop. What `driver.rs` (1,088 lines) +
//! `commands/inline.rs`'s channel/thread scaffolding reduce to when the
//! runtime is Elm-shaped and the timeline owns block lifecycle.
//!
//! Infrastructure from the original that has **no equivalent here**:
//!
//! - `DriverEventSender` + `with_context` smuggling — messages go straight
//!   to `update`; sub-components compose by enum embedding + `Keymap::map`.
//! - `sync_view_state` — there is no `ViewState` to clone the FSM into;
//!   the model IS the state, and it holds only the *live* turn. The
//!   original cloned `all_events` + `visible_events` + `archived_events` +
//!   rebuilt `turns` on every FSM event, then filtered by
//!   `committed_turn_count` per frame.
//! - `on_commit` key-string parsing (`"turn-{id}"` → counter) — completed
//!   turns leave the model at `ctx.push` time; there is nothing to prune.
//! - The `spawn_blocking` driver thread + `std::sync::mpsc` bridge — the
//!   runtime's own loop delivers messages.
//! - The `usize::MAX` sentinel turn for the pending banner — the tail just
//!   composes a spinner.
//! - The driver-thread turn pre-computation ("so the render-thread view
//!   function doesn't redo O(n) work every frame") — per-frame work is
//!   bounded by the live turn, not conversation length, by construction.
//!
//! O6 check (does `push` suffice, or do we need `push_before`?): sealing a
//! completed turn pushes it *above* the tail while the input box continues
//! to render *in* the tail below — exactly the visual order the app wants.
//! `push` suffices; no ordering primitive needed.

use futures::StreamExt;

use crate::fixtures::{ToolCallDetails, ToolRenderData, ToolResultStatus, UiEvent};
use crate::ports::agent_turn;
use crate::ports::input_box_a::{self, InputModel};
use crate::ui::*;

/// The app's message type. Sub-model messages embed by enum; stream events
/// arrive from the spawned task.
#[derive(Clone)]
pub enum AppMsg {
    Input(input_box_a::Msg),
    Stream(StreamEvent),
    CancelStream,
}

#[derive(Clone)]
pub enum StreamEvent {
    Delta(String),
    ToolStarted { id: String, command: String },
    ToolFinished { id: String, ok: bool },
    Completed,
    Failed(String),
}

/// What the app returns to the shell integration on exit.
#[derive(Default)]
pub enum ExitAction {
    #[default]
    Cancel,
    Execute(String),
    Insert(String),
}

/// The whole app model. Compare `driver.rs`'s `ViewState` (18 fields, most
/// of them clones of FSM history): this holds only what is *live*.
pub struct AtuinApp {
    input: InputModel,
    /// Events of the in-progress agent turn only. Completed turns are
    /// pushed as blocks and leave the model.
    active: Vec<UiEvent>,
    /// Streaming text not yet committed to `active`.
    current_response: String,
    /// Cancel-on-drop handle for the running turn. Cancellation is
    /// `self.streaming = None` — no CancelGeneration plumbing, no atomics.
    streaming: Option<Task>,
    error: Option<String>,
}

impl App for AtuinApp {
    type Msg = AppMsg;
    type Output = ExitAction;

    fn update(&mut self, msg: AppMsg, ctx: &mut Ctx<Self>) {
        match msg {
            // Parent policy for a child message: plain pattern matching, no
            // OutMsg machinery. Submit is app policy (start a turn), so the
            // parent claims it and delegates the rest.
            AppMsg::Input(input_box_a::Msg::Submit) => {
                let prompt = self.input.input.take_text();
                if !prompt.trim().is_empty() {
                    self.begin_turn(prompt, ctx);
                }
            }
            AppMsg::Input(input_box_a::Msg::Interrupt) => {
                if self.streaming.is_some() {
                    self.streaming = None; // cancel-on-drop
                } else {
                    ctx.exit(ExitAction::Cancel);
                }
            }
            AppMsg::Input(imsg) => self.input.update(imsg),

            AppMsg::Stream(ev) => self.on_stream_event(ev, ctx),

            AppMsg::CancelStream => {
                self.streaming = None;
            }
        }
    }

    fn tail(&self) -> impl Element<AppMsg> + '_ {
        let busy = self.streaming.is_some();

        col()
            .when(busy || !self.active.is_empty(), |c| {
                c.child(live_turn_view(&self.active, &self.current_response, busy))
            })
            .when_some(self.error.as_deref(), |c, e| {
                c.child(text(format!("Error: {e}")).pad_left(2).pad_top(1))
            })
            .child(map_msg(self.input.tail()))
    }

    fn keymap(&self) -> Keymap<AppMsg> {
        self.input
            .keymap()
            .map(AppMsg::Input)
            .when(self.streaming.is_some(), |k| {
                k.on(key(crossterm::event::KeyCode::Esc), AppMsg::CancelStream)
            })
    }
}

impl AtuinApp {
    fn begin_turn(&mut self, prompt: String, ctx: &mut Ctx<Self>) {
        // The user turn is finished the moment it exists: push, gone.
        ctx.push(user_turn_block(&prompt));
        self.error = None;
        self.streaming = Some(ctx.spawn(agent_stream(prompt)));
    }

    fn on_stream_event(&mut self, ev: StreamEvent, ctx: &mut Ctx<Self>) {
        match ev {
            StreamEvent::Delta(s) => self.current_response.push_str(&s),
            StreamEvent::ToolStarted { id, command } => {
                self.flush_response();
                self.active.push(UiEvent::ToolCall(ToolCallDetails {
                    tool_use_id: id,
                    name: "shell".into(),
                    status: ToolResultStatus::Pending,
                    render_data: ToolRenderData::Shell {
                        command,
                        preview: None,
                    },
                }));
            }
            StreamEvent::ToolFinished { id, ok } => {
                for ev in &mut self.active {
                    if let UiEvent::ToolCall(d) = ev
                        && d.tool_use_id == id
                    {
                        d.status = if ok {
                            ToolResultStatus::Success
                        } else {
                            ToolResultStatus::Error
                        };
                    }
                }
            }
            StreamEvent::Completed => {
                // Seal the turn: it becomes a block, above the tail (which
                // still holds the input box below — the O6 case), and every
                // trace of it leaves the model.
                self.flush_response();
                let events = std::mem::take(&mut self.active);
                ctx.push(map_msg(agent_turn::agent_turn_view(&events, false, false)));
                self.streaming = None;
            }
            StreamEvent::Failed(e) => {
                self.error = Some(e);
                self.streaming = None;
            }
        }
    }

    fn flush_response(&mut self) {
        if !self.current_response.is_empty() {
            self.active.push(UiEvent::Text {
                content: std::mem::take(&mut self.current_response),
            });
        }
    }
}

/// The in-progress agent turn, rendered in the tail. Reuses Port 1's
/// per-event views; the pending "banner" is just the spinner branch — no
/// `usize::MAX` sentinel turn.
fn live_turn_view<'a>(
    events: &'a [UiEvent],
    current_response: &str,
    busy: bool,
) -> impl Element<AppMsg> + 'a {
    let streaming_md = (!current_response.is_empty()).then(|| current_response.to_string());

    col()
        .child(text(" Atuin AI "))
        .children(events.iter().map(|ev| map_msg(agent_turn::event_view(ev))))
        .when_some(streaming_md, |c, md| c.child(markdown(md).pad_left(2)))
        .when(busy, |c| c.child(spinner("").pad_left(2)))
}

/// A submitted user prompt as a block: rendered once, then scrollback.
fn user_turn_block(prompt: &str) -> impl Element<AppMsg> + use<> {
    col()
        .child(text(" You "))
        .child(text(prompt.to_string()).pad_left(2))
        .pad_top(1)
}

/// Stand-in for the real agent turn (SSE stream → StreamEvents → AppMsg).
fn agent_stream(prompt: String) -> impl futures::Stream<Item = AppMsg> + Send {
    futures::stream::iter([
        StreamEvent::Delta(format!("Thinking about: {prompt}")),
        StreamEvent::Completed,
    ])
    .map(AppMsg::Stream)
}
