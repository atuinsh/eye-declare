//! Port 3, Candidate B: InputBox with **framework-managed widget state**
//! (egui-Memory style). Self-contained sketch: the store, the managed
//! widget, and the app slice all live here so A and B read side by side.
//!
//! Shape: the app never holds textarea state. The view declares
//! `text_area_managed(id)` with message-mapping callbacks; the runtime owns
//! a `WidgetStore` keyed by id, routes focused input to the widget's
//! `handle`, and delivers whatever messages the callbacks produce.

use std::any::Any;
use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::{Element, InputEvent, TextAreaState};

/// Widget identity. With no model-side state to anchor identity, ids return
/// (Port 1 deleted keys; Candidate B reintroduces them for stateful widgets).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetId(pub &'static str);

/// Runtime-owned per-widget state, keyed by id. The framework would create,
/// persist, and garbage-collect entries; the app never sees inside except
/// through explicit queries.
#[derive(Default)]
pub struct WidgetStore {
    slots: HashMap<WidgetId, Box<dyn Any>>,
}

impl WidgetStore {
    pub fn get_or_default<T: Default + 'static>(&mut self, id: WidgetId) -> &mut T {
        self.slots
            .entry(id)
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut()
            .expect("widget state type mismatch for id")
    }

    /// App-side escape hatch to inspect widget state (needed for anything
    /// like `is_input_blank` unless the app mirrors via on_change).
    pub fn get<T: 'static>(&self, id: WidgetId) -> Option<&T> {
        self.slots.get(&id).and_then(|b| b.downcast_ref())
    }
}

pub enum Msg {
    /// Carries the submitted text — the app never touches the textarea.
    Submitted(String),
    /// The echo: apps that need to *know* the current text (blank checks,
    /// slash search) must mirror it into their model on every keystroke.
    InputChanged(String),
    InsertSuggestedCommand,
    Interrupt,
}

/// The managed text area element. Policy that Candidate A expressed as
/// keymap data must be expressed here as configuration flags and callback
/// props — and every app-specific behavior needs its own escape hatch.
pub struct ManagedTextArea<Msg> {
    id: WidgetId,
    title: String,
    footer: String,
    max_height: u16,
    submit_on_enter: bool,
    newline_on_shift_enter: bool,
    newline_on_ctrl_j: bool,
    on_submit: Option<Box<dyn Fn(String) -> Msg>>,
    on_change: Option<Box<dyn Fn(String) -> Msg>>,
    /// Escape hatch for behaviors the flags don't cover (the Tab
    /// slash-accept). Sees the key + current text, may consume with a Msg.
    /// This is the Select `on_select: Box<dyn Fn...>` pattern returning.
    intercept_key: Option<KeyInterceptFn<Msg>>,
}

/// Complex enough that clippy demands a name for it — which is itself
/// evidence for the Candidate A side of the comparison.
type KeyInterceptFn<Msg> = Box<dyn Fn(&KeyEvent, &str) -> Option<Msg>>;

pub fn text_area_managed<Msg>(id: WidgetId) -> ManagedTextArea<Msg> {
    ManagedTextArea {
        id,
        title: String::new(),
        footer: String::new(),
        max_height: u16::MAX,
        submit_on_enter: true,
        newline_on_shift_enter: true,
        newline_on_ctrl_j: false,
        on_submit: None,
        on_change: None,
        intercept_key: None,
    }
}

impl<Msg> ManagedTextArea<Msg> {
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    pub fn max_height(mut self, rows: u16) -> Self {
        self.max_height = rows;
        self
    }

    pub fn newline_on_ctrl_j(mut self, enabled: bool) -> Self {
        self.newline_on_ctrl_j = enabled;
        self
    }

    pub fn on_submit(mut self, f: impl Fn(String) -> Msg + 'static) -> Self {
        self.on_submit = Some(Box::new(f));
        self
    }

    pub fn on_change(mut self, f: impl Fn(String) -> Msg + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }

    pub fn intercept_key(mut self, f: impl Fn(&KeyEvent, &str) -> Option<Msg> + 'static) -> Self {
        self.intercept_key = Some(Box::new(f));
        self
    }

    /// The runtime calls this with the store while this widget is focused.
    /// (In a real design this hangs off a `ManagedElement` trait; a plain
    /// method keeps the sketch small.)
    pub fn handle(&self, event: &InputEvent, store: &mut WidgetStore) -> Option<Msg> {
        let state: &mut TextAreaState = store.get_or_default(self.id);

        if let InputEvent::Key(k) = event {
            if let Some(intercept) = &self.intercept_key
                && let Some(msg) = intercept(k, &state.text())
            {
                return msg.into();
            }
            if k.code == KeyCode::Enter {
                if self.newline_on_shift_enter && k.modifiers.contains(KeyModifiers::SHIFT) {
                    state.insert_newline();
                    return self.on_change.as_ref().map(|f| f(state.text()));
                }
                if self.submit_on_enter {
                    let text = state.take_text();
                    if text.trim().is_empty() {
                        return None;
                    }
                    return self.on_submit.as_ref().map(|f| f(text));
                }
            }
            if self.newline_on_ctrl_j
                && k.code == KeyCode::Char('j')
                && k.modifiers.contains(KeyModifiers::CONTROL)
            {
                state.insert_newline();
                return self.on_change.as_ref().map(|f| f(state.text()));
            }
        }

        state.handle(event);
        self.on_change.as_ref().map(|f| f(state.text()))
    }
}

impl<M> Element<M> for ManagedTextArea<M> {
    // height() needs the store to measure content — the Element trait
    // would have to grow store access (or heights get cached by id).
    // Left as the default stub; this wrinkle is a finding in itself.
}

// ───────────────────────────────────────────────────────────────────
// The app slice, Candidate B
// ───────────────────────────────────────────────────────────────────

pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

pub struct InputModelB {
    /// The mirror. `is_input_blank` and slash search need the text, the
    /// framework owns the text, so every keystroke echoes it back into the
    /// model via `Msg::InputChanged` — the exact `InputUpdated` round-trip
    /// the original atuin-ai driver had, reintroduced by the state model.
    pub input_text: String,
    pub slash_registry: Vec<SlashCommand>,
    pub has_command: bool,
    pub submitted: Vec<String>,
}

impl InputModelB {
    pub fn slash_results(&self) -> Vec<&SlashCommand> {
        let Some(query) = self.input_text.strip_prefix('/') else {
            return Vec::new();
        };
        self.slash_registry
            .iter()
            .filter(|c| c.name.starts_with(query))
            .collect()
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Submitted(text) => {
                self.submitted.push(text);
                self.input_text.clear();
            }
            Msg::InputChanged(text) => self.input_text = text,
            Msg::InsertSuggestedCommand | Msg::Interrupt => {}
        }
    }

    pub fn tail(&self) -> impl Element<Msg> + '_ {
        let suggestion = self.slash_results().first().map(|c| format!("/{}", c.name));

        text_area_managed(WidgetId("input"))
            .title("Generate a command or ask a question")
            .footer("[Enter] Send  [Shift+Enter] Newline")
            .max_height(7)
            .newline_on_ctrl_j(true)
            .on_submit(Msg::Submitted)
            .on_change(Msg::InputChanged)
            .intercept_key(move |key, _text| {
                if key.code == KeyCode::Tab
                    && let Some(s) = &suggestion
                {
                    // Can't set the textarea text from here — the store isn't
                    // in scope. The widget would need yet another hook
                    // (`on_tab_complete`?) that both reads AND writes state.
                    // Candidate A does this with three lines in update().
                    let _ = s;
                }
                None
            })
    }
}
