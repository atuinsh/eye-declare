//! A complete app driven headlessly: synthetic key events in, VTE-verified
//! terminal contents out. This is the snapshot-testing story downstream
//! users get — the framework tests itself the same way.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eye_declare::{
    App, Ctx, Element, ElementExt, Fluent, Focus, FocusHandle, InputEvent, Keymap, Runtime, col,
    key, keymap, text,
};
use eye_declare_engine::test_terminal::TestTerminal;

#[derive(Clone)]
enum Msg {
    Submit,
    Typed(char),
    Backspace,
    Quit,
}

#[derive(Default)]
enum Outcome {
    #[default]
    Cancelled,
    Finished(usize),
}

/// Minimal echo app: type a line, Enter commits it as a block, Ctrl+C
/// exits with the number of submitted lines.
struct Echo {
    input_focus: FocusHandle,
    typed: String,
    submitted: usize,
}

impl Echo {
    fn new(focus: &Focus) -> Self {
        let input_focus = focus.handle();
        input_focus.focus();
        Self {
            input_focus,
            typed: String::new(),
            submitted: 0,
        }
    }
}

impl App for Echo {
    type Msg = Msg;
    type Output = Outcome;

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Typed(c) => self.typed.push(c),
            Msg::Backspace => {
                self.typed.pop();
            }
            Msg::Submit => {
                if !self.typed.is_empty() {
                    let line = std::mem::take(&mut self.typed);
                    ctx.push(text(format!("* {line}")));
                    self.submitted += 1;
                }
            }
            Msg::Quit => ctx.exit(Outcome::Finished(self.submitted)),
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col()
            .child(text(format!("> {}", self.typed)))
            .when(self.submitted > 0, |c| {
                c.child(text(format!("({} sent)", self.submitted)))
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
                    _ => Msg::Typed('?'),
                },
                InputEvent::Paste(_) => Msg::Typed('P'),
            })
    }
}

fn press(code: KeyCode) -> InputEvent {
    InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn type_str(rt: &mut Runtime<Echo>, term: &mut TestTerminal, s: &str) {
    for c in s.chars() {
        let (bytes, exit) = rt.handle(press(KeyCode::Char(c)));
        assert!(exit.is_none());
        term.feed(&bytes);
    }
}

#[test]
fn echo_session_end_to_end() {
    let focus = Focus::new();
    let mut rt = Runtime::new(Echo::new(&focus), 20, 24);
    let mut term = TestTerminal::new(20, 24);

    term.feed(&rt.present());
    assert_eq!(term.viewport_lines()[0], ">");

    // Type and watch the tail echo.
    type_str(&mut rt, &mut term, "hello");
    assert_eq!(term.viewport_lines()[0], "> hello");

    // Backspace edits.
    let (bytes, _) = rt.handle(press(KeyCode::Backspace));
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[0], "> hell");

    // Enter commits the line as a block; tail resets below it.
    let (bytes, _) = rt.handle(press(KeyCode::Enter));
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[0], "* hell");
    assert_eq!(term.viewport_lines()[1], ">");
    assert_eq!(term.viewport_lines()[2], "(1 sent)");

    // Another line stacks below the first block.
    type_str(&mut rt, &mut term, "again");
    let (bytes, _) = rt.handle(press(KeyCode::Enter));
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[0], "* hell");
    assert_eq!(term.viewport_lines()[1], "* again");
    assert_eq!(term.viewport_lines()[2], ">");
    assert_eq!(term.viewport_lines()[3], "(2 sent)");

    // Ctrl+C exits with the outcome; committed history survives.
    let (bytes, exit) = rt.handle(InputEvent::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    term.feed(&bytes);
    match exit {
        Some(Outcome::Finished(n)) => assert_eq!(n, 2),
        _ => panic!("expected Finished exit"),
    }
    assert_eq!(term.viewport_lines()[0], "* hell");
    assert_eq!(term.viewport_lines()[1], "* again");

    // Exit parks the cursor on a fresh line below the still-visible tail,
    // so whatever main() prints after run() returns doesn't land on top
    // of it.
    assert_eq!(term.viewport_lines()[2], ">");
    assert_eq!(term.viewport_lines()[3], "(2 sent)");
    assert_eq!(term.cursor(), (4, 0));
    term.feed(b"echoed 2 line(s)\r\n");
    assert_eq!(term.viewport_lines()[3], "(2 sent)");
    assert_eq!(term.viewport_lines()[4], "echoed 2 line(s)");
}

#[test]
fn init_exit_parks_cursor_below_content() {
    // An exit requested from App::init bypasses the message loop, so the
    // shell handoff must come from startup() itself.
    struct PrintAndQuit;

    impl App for PrintAndQuit {
        type Msg = ();
        type Output = ();

        fn init(&mut self, ctx: &mut Ctx<'_, Self>) {
            ctx.push(text("one-shot output"));
            ctx.exit(());
        }

        fn update(&mut self, _msg: (), _ctx: &mut Ctx<'_, Self>) {}

        fn tail(&self) -> impl Element + '_ {
            text("tail line")
        }

        fn keymap(&self) -> Keymap<()> {
            keymap()
        }
    }

    let mut rt = Runtime::new(PrintAndQuit, 20, 24);
    let mut term = TestTerminal::new(20, 24);

    let (bytes, exit) = rt.startup();
    term.feed(&bytes);
    assert!(exit.is_some());

    assert_eq!(term.viewport_lines()[0], "one-shot output");
    assert_eq!(term.viewport_lines()[1], "tail line");
    assert_eq!(term.cursor(), (2, 0));
    term.feed(b"$ prompt");
    assert_eq!(term.viewport_lines()[2], "$ prompt");
}

#[test]
fn tail_hidden_before_deferred_exit_leaves_no_gap() {
    // The Atuin AI exit shape: one update hides the tail (the input box
    // disappears in its own frame), a later message actually exits. The
    // vacated rows must be reclaimed with no blank line left between the
    // committed content and the shell prompt.
    struct HideThenQuit {
        exiting: bool,
    }

    #[derive(Clone)]
    enum HMsg {
        Commit,
        Hide,
        Quit,
    }

    impl App for HideThenQuit {
        type Msg = HMsg;
        type Output = ();

        fn update(&mut self, msg: HMsg, ctx: &mut Ctx<'_, Self>) {
            match msg {
                HMsg::Commit => ctx.push(text("committed line")),
                HMsg::Hide => self.exiting = true,
                HMsg::Quit => ctx.exit(()),
            }
        }

        fn tail(&self) -> impl Element + '_ {
            if self.exiting {
                col()
            } else {
                col().child(text("| input box |")).child(text("footer"))
            }
        }

        fn keymap(&self) -> Keymap<HMsg> {
            keymap()
        }
    }

    let mut rt = Runtime::new(HideThenQuit { exiting: false }, 20, 24);
    let mut term = TestTerminal::new(20, 24);

    term.feed(&rt.present());
    let (bytes, _) = rt.process(HMsg::Commit);
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[1], "| input box |");
    assert_eq!(term.viewport_lines()[2], "footer");

    let (bytes, exit) = rt.process(HMsg::Hide);
    term.feed(&bytes);
    assert!(exit.is_none());
    let (bytes, exit) = rt.process(HMsg::Quit);
    term.feed(&bytes);
    assert!(exit.is_some());

    assert_eq!(term.viewport_lines()[0], "committed line");
    assert_eq!(term.viewport_lines()[1], "");
    assert_eq!(term.cursor(), (1, 0));
    term.feed(b"$ prompt");
    assert_eq!(term.viewport_lines()[1], "$ prompt");
}

#[test]
fn unclaimed_events_produce_no_output() {
    let focus = Focus::new();
    let mut rt = Runtime::new(Echo::new(&focus), 20, 24);
    let mut term = TestTerminal::new(20, 24);
    term.feed(&rt.present());

    // F5 matches nothing: no bindings, fallthrough maps it to Typed('?')…
    // actually fallthrough claims everything for the focused input. Blur
    // first so nothing is focused.
    focus.blur_all();
    let (bytes, exit) = rt.handle(press(KeyCode::F(5)));
    assert!(bytes.is_empty());
    assert!(exit.is_none());
}

#[test]
fn conditional_bindings_rebuild_per_update() {
    // The "(n sent)" line only appears after a submit — and so could a
    // binding. Verify keymap() is consulted fresh each event by toggling
    // focus between events.
    let focus = Focus::new();
    let app = Echo::new(&focus);
    let input_focus = app.input_focus.clone();
    let mut rt = Runtime::new(app, 20, 24);
    let mut term = TestTerminal::new(20, 24);
    term.feed(&rt.present());

    type_str(&mut rt, &mut term, "a");
    assert_eq!(term.viewport_lines()[0], "> a");

    input_focus.blur();
    let (bytes, _) = rt.handle(press(KeyCode::Char('b')));
    assert!(bytes.is_empty(), "typing while blurred does nothing");

    input_focus.focus();
    type_str(&mut rt, &mut term, "c");
    assert_eq!(term.viewport_lines()[0], "> ac");
}

#[test]
fn borrowed_blocks_push_cleanly() {
    // Ctx::push renders immediately, so blocks may borrow from update
    // locals — this is the API shape the driver port needed.
    struct OneShot {
        events: Vec<String>,
        done: bool,
    }

    impl App for OneShot {
        type Msg = ();
        type Output = ();

        fn update(&mut self, _msg: (), ctx: &mut Ctx<'_, Self>) {
            let events = std::mem::take(&mut self.events);
            ctx.push(col().children(events.iter().map(|e| text(e.as_str()).pad_left(2))));
            self.done = true;
            ctx.exit(());
        }

        fn tail(&self) -> impl Element + '_ {
            text(if self.done { "" } else { "working" })
        }
    }

    let mut rt = Runtime::new(
        OneShot {
            events: vec!["one".into(), "two".into()],
            done: false,
        },
        20,
        24,
    );
    let mut term = TestTerminal::new(20, 24);
    term.feed(&rt.present());

    let (bytes, exit) = rt.process(());
    term.feed(&bytes);
    assert!(exit.is_some());
    assert_eq!(term.viewport_lines()[0], "  one");
    assert_eq!(term.viewport_lines()[1], "  two");
}

#[test]
fn init_pushes_precede_first_frame_and_later_pushes() {
    struct Banner {
        submitted: bool,
    }

    impl App for Banner {
        type Msg = ();
        type Output = ();

        fn init(&mut self, ctx: &mut Ctx<'_, Self>) {
            ctx.push(text("welcome banner"));
        }

        fn update(&mut self, _msg: (), ctx: &mut Ctx<'_, Self>) {
            self.submitted = true;
            ctx.push(text("first message"));
        }

        fn tail(&self) -> impl Element + '_ {
            text(if self.submitted { "done" } else { "prompt" })
        }
    }

    // The driver path: startup() delivers init's block before the tail.
    let mut rt = Runtime::new(Banner { submitted: false }, 40, 10);
    let mut term = TestTerminal::new(40, 10);
    let (bytes, exit) = rt.startup();
    assert!(exit.is_none());
    term.feed(&bytes);
    let screen = term.viewport_lines().join("\n");
    let banner_row = screen.find("welcome banner").expect("banner missing");
    let prompt_row = screen.find("prompt").expect("tail missing");
    assert!(banner_row < prompt_row, "init block must precede the tail");

    // A later push lands below the banner.
    let (bytes, _) = rt.process(());
    term.feed(&bytes);
    let all = [term.scrollback_lines(), term.viewport_lines()]
        .concat()
        .join("\n");
    let banner = all.find("welcome banner").unwrap();
    let first = all.find("first message").unwrap();
    assert!(
        banner < first,
        "init block stays above later pushes:\n{all}"
    );
}

#[test]
fn init_bytes_survive_process_without_present() {
    struct P;
    impl App for P {
        type Msg = ();
        type Output = ();
        fn init(&mut self, ctx: &mut Ctx<'_, Self>) {
            ctx.push(text("from init"));
        }
        fn update(&mut self, _msg: (), ctx: &mut Ctx<'_, Self>) {
            ctx.push(text("from update"));
        }
        fn tail(&self) -> impl Element + '_ {
            text("tail")
        }
    }

    // Skipping startup() and going straight to process(): init's block
    // must still come out first.
    let mut rt = Runtime::new(P, 40, 10);
    let mut term = TestTerminal::new(40, 10);
    let (bytes, _) = rt.process(());
    term.feed(&bytes);
    let all = [term.scrollback_lines(), term.viewport_lines()]
        .concat()
        .join("\n");
    assert!(
        all.find("from init").unwrap() < all.find("from update").unwrap(),
        "wrong order:\n{all}"
    );
}
