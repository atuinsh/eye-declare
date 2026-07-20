//! Port 3, Candidate A: InputBox with **strict-Elm widget state**.
//!
//! Original: `~/src/atuin/crates/atuin-ai/src/tui/components/input_box.rs`
//! plus the input-policy fragments of `components/atuin_ai.rs` (Tab-to-insert)
//! and `view/mod.rs` `input_view` (slash suggestion rows).
//!
//! The original's shape: `Arc<Mutex<TextArea>>` in component state (because
//! Canvas demands a `'static` closure), a `tx: Option<DriverEventSender>`
//! fetched via `use_context` so handlers can emit, an `active` prop shadowing
//! focus, and an `InputUpdated` message sent to the driver **on every
//! keystroke** so app state could know the text (for `is_input_blank` and
//! slash search).
//!
//! Candidate A's shape: `TextAreaState` is a plain value in the model.
//! `update` owns all mutation; the keymap owns all policy; the view borrows.
//! Consequences visible below:
//!
//! - **No `Arc<Mutex>`** — the view borrows `&TextAreaState` (this is what
//!   forced `AnyElement` to carry a lifetime).
//! - **No event-sender plumbing** — handlers ARE messages.
//! - **No `active` prop** — visuals key off `FocusHandle::is_focused()`.
//! - **The `InputUpdated` echo is deleted** — `slash_results()` and
//!   `is_blank()` are derived reads of model state.
//! - **Tab policy is conditional keymap data**, rebuilt per update — the
//!   Tab-to-insert vs slash-accept vs focus-cycle footgun becomes
//!   impossible-by-construction (all Tab meanings are in one place, ordered).

use crossterm::event::KeyCode;

use crate::ui::*;

/// Messages this slice of the app produces. Note the absence of any
/// "InputUpdated" — update mutates state directly, and reads are derived.
#[derive(Clone)]
pub enum Msg {
    /// Enter (unmodified) in the input.
    Submit,
    /// Shift+Enter or Ctrl+J.
    InputNewline,
    /// Tab while a slash suggestion matches.
    AcceptSlashSuggestion,
    /// Tab while input is blank and a suggested command exists.
    InsertSuggestedCommand,
    /// Ctrl+C.
    Interrupt,
    /// Everything else while the input is focused: ordinary editing.
    Edit(EditEvent),
}

/// Raw event wrapper so `Msg` can stay `Clone` and the fallthrough mapper
/// can be the plain enum constructor `Msg::Edit`.
#[derive(Clone)]
pub struct EditEvent(pub InputEvent);

pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

/// The app-model slice for the input area.
pub struct InputModel {
    pub input: TextAreaState,
    pub input_focus: FocusHandle,
    pub slash_registry: Vec<SlashCommand>,
    pub has_command: bool,
    /// Stand-in for "what submit does" (the real app pushes a turn + spawns
    /// a stream here).
    pub submitted: Vec<String>,
}

impl InputModel {
    /// Slash-command matches, **derived** from the input text on demand.
    /// In the original this was `slash_command_search_results` app state,
    /// kept in sync by the per-keystroke `InputUpdated` round-trip through
    /// the driver thread.
    pub fn slash_results(&self) -> Vec<&SlashCommand> {
        let text = self.input.text();
        let Some(query) = text.strip_prefix('/') else {
            return Vec::new();
        };
        self.slash_registry
            .iter()
            .filter(|c| c.name.starts_with(query))
            .collect()
    }

    fn best_slash(&self) -> Option<&SlashCommand> {
        self.slash_results().into_iter().next()
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Submit => {
                let text = self.input.take_text();
                if !text.trim().is_empty() {
                    // Real app: push the user turn as a block, spawn the
                    // agent stream, hold the Task in the model.
                    self.submitted.push(text);
                }
            }
            Msg::InputNewline => self.input.insert_newline(),
            Msg::AcceptSlashSuggestion => {
                if let Some(cmd) = self.best_slash() {
                    let name = cmd.name.clone();
                    self.input.set_text(&format!("/{name}"));
                }
            }
            Msg::InsertSuggestedCommand => {
                // Real app: ctx.exit(ExitAction::Insert(..)).
            }
            Msg::Interrupt => {
                // Real app: drop the streaming Task, or ctx.exit(Cancel).
            }
            Msg::Edit(EditEvent(ev)) => self.input.handle(&ev),
        }
    }

    /// All input policy in one place, conditional on model state, rebuilt
    /// each update. Compare: the original spread Tab across the InputBox
    /// bubble handler, the AtuinAi capture handler, and the framework's
    /// hardcoded Tab-cycling — and broke silently when a second focusable
    /// appeared.
    pub fn keymap(&self) -> Keymap<Msg> {
        keymap()
            .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Interrupt)
            .in_scope(&self.input_focus, key(KeyCode::Enter), Msg::Submit)
            .in_scope(
                &self.input_focus,
                key(KeyCode::Enter).shift(),
                Msg::InputNewline,
            )
            .in_scope(
                &self.input_focus,
                key(KeyCode::Char('j')).ctrl(),
                Msg::InputNewline,
            )
            .when(self.best_slash().is_some(), |k| {
                k.in_scope(
                    &self.input_focus,
                    key(KeyCode::Tab),
                    Msg::AcceptSlashSuggestion,
                )
            })
            .when(self.has_command && self.input.is_blank(), |k| {
                k.in_scope(
                    &self.input_focus,
                    key(KeyCode::Tab),
                    Msg::InsertSuggestedCommand,
                )
            })
            .fallthrough(&self.input_focus, |ev| Msg::Edit(EditEvent(ev)))
    }

    /// The live-tail view for the input area. Borrows the model.
    pub fn tail(&self) -> impl Element<Msg> + '_ {
        let results = self.slash_results();

        col()
            .child(
                text_area(&self.input)
                    .title("Generate a command or ask a question")
                    .title_right("Atuin AI")
                    .footer("[Enter] Send  [Shift+Enter] Newline")
                    .placeholder("Type a message...")
                    .max_height(7)
                    .track_focus(&self.input_focus),
            )
            .children(results.into_iter().take(4).enumerate().map(|(i, cmd)| {
                text(format!("/{}", cmd.name))
                    .span(" - ", Default::default())
                    .span(cmd.description.as_str(), Default::default())
                    .when(i == 0, |t| t.span(" [Tab] Insert", Default::default()))
            }))
    }
}
