//! Component composition, the strict-Elm way: a reusable `PromptBox`
//! sub-model with its own Msg, update, keymap, and view — embedded in a
//! parent app by enum wrapping. The parent claims the child's `Submit`
//! (app policy) and delegates the rest. No context system, no event
//! phases, no OutMsg machinery.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eye_declare::{
    App, Ctx, Element, Focus, FocusHandle, InputEvent, Keymap, Runtime, TextAreaState, col, key,
    keymap, panel, text, text_area,
};
use eye_declare_engine::test_terminal::TestTerminal;

// ───────────────────────────────────────────────────────────────────
// The reusable component: state + messages + keymap + view
// ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum PromptMsg {
    Submit,
    Newline,
    Edit(InputEvent),
}

struct PromptBox {
    input: TextAreaState,
    focus: FocusHandle,
}

impl PromptBox {
    fn new(focus: &Focus) -> Self {
        Self {
            input: TextAreaState::new(),
            focus: focus.handle(),
        }
    }

    /// Everything except Submit — that's the parent's policy to claim.
    fn update(&mut self, msg: PromptMsg) {
        match msg {
            PromptMsg::Submit => unreachable!("parent claims Submit"),
            PromptMsg::Newline => self.input.insert_newline(),
            PromptMsg::Edit(ev) => self.input.handle(&ev),
        }
    }

    fn keymap(&self) -> Keymap<PromptMsg> {
        keymap()
            .in_scope(&self.focus, key(KeyCode::Enter), PromptMsg::Submit)
            .in_scope(&self.focus, key(KeyCode::Enter).shift(), PromptMsg::Newline)
            .in_scope(
                &self.focus,
                key(KeyCode::Char('j')).ctrl(),
                PromptMsg::Newline,
            )
            .fallthrough(&self.focus, PromptMsg::Edit)
    }

    fn view(&self) -> impl Element + '_ {
        panel(
            text_area(&self.input)
                .placeholder("Type a message...")
                .track_focus(&self.focus)
                .max_height(5),
        )
        .title("Ask")
        .pad_x(1)
    }
}

// ───────────────────────────────────────────────────────────────────
// The parent app
// ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum AppMsg {
    Prompt(PromptMsg),
}

struct Chat {
    prompt: PromptBox,
    submitted: Vec<String>,
}

impl App for Chat {
    type Msg = AppMsg;
    type Output = ();

    fn update(&mut self, msg: AppMsg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            // Parent policy: Submit starts a "turn".
            AppMsg::Prompt(PromptMsg::Submit) => {
                let content = self.prompt.input.take_text();
                if !content.trim().is_empty() {
                    ctx.push(text(format!("you: {}", content.replace('\n', " / "))));
                    self.submitted.push(content);
                }
            }
            AppMsg::Prompt(msg) => self.prompt.update(msg),
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col().child(self.prompt.view())
    }

    fn keymap(&self) -> Keymap<AppMsg> {
        self.prompt.keymap().map(AppMsg::Prompt)
    }
}

// ───────────────────────────────────────────────────────────────────

fn press(code: KeyCode) -> InputEvent {
    InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn press_mod(code: KeyCode, mods: KeyModifiers) -> InputEvent {
    InputEvent::Key(KeyEvent::new(code, mods))
}

fn setup() -> (Runtime<Chat>, TestTerminal, Focus) {
    let focus = Focus::new();
    let prompt = PromptBox::new(&focus);
    prompt.focus.focus();
    let rt = Runtime::new(
        Chat {
            prompt,
            submitted: Vec::new(),
        },
        30,
        24,
    );
    (rt, TestTerminal::new(30, 24), focus)
}

fn feed(rt: &mut Runtime<Chat>, term: &mut TestTerminal, ev: InputEvent) {
    let (bytes, _) = rt.handle(ev);
    term.feed(&bytes);
}

#[test]
fn embedded_component_session() {
    let (mut rt, mut term, _focus) = setup();
    term.feed(&rt.present());

    // Placeholder shows inside the panel chrome.
    assert!(term.viewport_lines()[0].contains(" Ask "));
    assert!(term.viewport_lines()[1].contains("Type a message..."));

    // Typing routes: keymap.map → AppMsg::Prompt(Edit) → TextAreaState.
    for c in "hi there".chars() {
        feed(&mut rt, &mut term, press(KeyCode::Char(c)));
    }
    assert!(term.viewport_lines()[1].contains("hi there"));

    // Shift+Enter: the component's own Newline binding.
    feed(
        &mut rt,
        &mut term,
        press_mod(KeyCode::Enter, KeyModifiers::SHIFT),
    );
    for c in "line2".chars() {
        feed(&mut rt, &mut term, press(KeyCode::Char(c)));
    }
    assert!(term.viewport_lines()[1].contains("hi there"));
    assert!(term.viewport_lines()[2].contains("line2"));

    // Enter: claimed by the parent → block committed, prompt reset.
    feed(&mut rt, &mut term, press(KeyCode::Enter));
    assert_eq!(term.viewport_lines()[0], "you: hi there / line2");
    assert!(term.viewport_lines()[1].contains(" Ask "));
    assert!(term.viewport_lines()[2].contains("Type a message..."));
    assert_eq!(rt.app().submitted, vec!["hi there\nline2"]);
}

#[test]
fn hardware_cursor_lands_inside_the_panel() {
    let (mut rt, mut term, _focus) = setup();
    term.feed(&rt.present());

    for c in "abc".chars() {
        feed(&mut rt, &mut term, press(KeyCode::Char(c)));
    }

    // Panel border (1) + pad_x (1) + 3 chars = col 5; content row = 1.
    assert_eq!(term.cursor(), (1, 5));
}

#[test]
fn blurred_component_ignores_input() {
    let (mut rt, mut term, focus) = setup();
    term.feed(&rt.present());

    focus.blur_all();
    let (bytes, _) = rt.handle(press(KeyCode::Char('x')));
    assert!(bytes.is_empty());

    // And the hardware cursor hides (present reports no hint).
    term.feed(&rt.present());
    assert!(rt.app().prompt.input.is_blank());
}
