//! Size delivery (`App::on_resize`), hardware cursor shapes
//! (`App::cursor_style`), and the resize variants that carry them.

use eye_declare::{App, Ctx, CursorStyle, Element, Runtime, text};
use eye_declare_engine::test_terminal::TestTerminal;

#[derive(Clone)]
enum Msg {
    Size(u16, u16),
    Style(CursorStyle),
}

struct Sizer {
    size: Option<(u16, u16)>,
    style: CursorStyle,
}

impl Sizer {
    fn new() -> Self {
        Self {
            size: None,
            style: CursorStyle::DefaultUserShape,
        }
    }
}

impl App for Sizer {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, msg: Msg, _ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Size(w, h) => self.size = Some((w, h)),
            Msg::Style(style) => self.style = style,
        }
    }

    fn tail(&self) -> impl Element + '_ {
        let (w, h) = self.size.unwrap_or((0, 0));
        text(format!("size {w}x{h}"))
    }

    fn cursor_style(&self) -> CursorStyle {
        self.style
    }

    fn on_resize(&self, width: u16, height: u16) -> Option<Msg> {
        Some(Msg::Size(width, height))
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn initial_size_arrives_through_on_resize() {
    let mut rt = Runtime::new(Sizer::new(), 80, 24);
    let mut term = TestTerminal::new(80, 24);

    let (bytes, exit) = rt.startup();
    assert!(exit.is_none());
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[0], "size 80x24");
}

#[test]
fn resize_msg_delivers_the_new_size_before_the_repaint() {
    let mut rt = Runtime::new(Sizer::new(), 80, 24);
    let mut term = TestTerminal::new(80, 24);
    let (bytes, _) = rt.startup();
    term.feed(&bytes);

    let (bytes, exit) = rt.resize_msg(100, 30, None);
    assert!(exit.is_none());
    term.resize_reflow(100, 30);
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[0], "size 100x30");
}

#[test]
fn resize_screen_clears_and_repaints_at_the_new_size() {
    let mut rt = Runtime::new(Sizer::new(), 80, 24);
    let mut term = TestTerminal::new(80, 24);
    let (bytes, _) = rt.startup();
    term.feed(&bytes);

    let (bytes, exit) = rt.resize_screen(60, 20);
    assert!(exit.is_none());
    assert!(
        contains(&bytes, b"\x1b[2J"),
        "alt-screen resize should clear the screen"
    );
    term.resize(60, 20);
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[0], "size 60x20");
}

#[test]
fn cursor_style_changes_emit_decscusr_exactly_once() {
    let mut rt = Runtime::new(Sizer::new(), 80, 24);

    // The default shape is never emitted unprompted.
    let (bytes, _) = rt.startup();
    assert!(!contains(&bytes, b" q"), "no DECSCUSR at default shape");

    // A change emits with the present that carries it.
    let (bytes, _) = rt.process(Msg::Style(CursorStyle::SteadyBlock));
    assert!(contains(&bytes, b"\x1b[2 q"));

    // Unchanged shape: no re-emission on later presents.
    let (bytes, _) = rt.process(Msg::Size(80, 24));
    assert!(!contains(&bytes, b" q"));

    // Returning to the default emits its code (there is no teardown
    // reset; the final shape is whatever the app last presented).
    let (bytes, _) = rt.process(Msg::Style(CursorStyle::DefaultUserShape));
    assert!(contains(&bytes, b"\x1b[0 q"));
}
