//! Candidate v2 DSL — stub implementation for the bake-off.
//!
//! Fluent builders, no macro. The `Element` trait matches the REDESIGN.md
//! sketch: honest `height(width)` measurement, plain `render`, optional
//! self-animation. Rendering is unimplemented — only call-site shape is
//! under test here.

use std::time::Duration;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;

/// A renderable piece of UI. `Msg` is the app's message type (Elm-style);
/// display-only elements implement it for every `Msg`.
pub trait Element<Msg> {
    /// Honest measurement: rows needed at the given width.
    fn height(&self, _width: u16) -> u16 {
        0
    }

    fn render(&self, _area: Rect, _buf: &mut Buffer) {}

    /// Frame interval if self-animating (e.g. a live spinner). The runtime
    /// self-ticks the tail while any animated element is present.
    fn animated(&self) -> Option<Duration> {
        None
    }

    /// Hardware cursor position (relative to `area`) when this element is
    /// focused. `None` hides the cursor.
    fn cursor(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

/// A boxed, type-erased element. Match arms that produce different element
/// types converge on this via [`ElementExt::any`].
///
/// Carries a lifetime: views borrow from the model (Port 3 finding — a
/// strict-Elm text area renders from `&TextAreaState` living in the app
/// model; an implicit `'static` bound here would force cloning every
/// stateful widget per frame). The tail is built, rendered, and dropped
/// within one frame, so model borrows are naturally scoped.
pub type AnyElement<'a, Msg> = Box<dyn Element<Msg> + 'a>;

impl<Msg> Element<Msg> for Box<dyn Element<Msg> + '_> {
    fn height(&self, width: u16) -> u16 {
        self.as_ref().height(width)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_ref().render(area, buf)
    }

    fn animated(&self) -> Option<Duration> {
        self.as_ref().animated()
    }

    fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_ref().cursor(area)
    }
}

/// Type erasure. Separate from [`Fluent`] because `any()` mentions `Msg` in
/// its return type (so inference can pin it from context), while the
/// combinators must NOT be `Msg`-parameterized at all — display-only elements
/// implement `Element<Msg>` for every `Msg`, and a `Msg`-generic `pad_left`
/// on such a receiver is uninferrable (found the hard way; see FINDINGS.md).
pub trait ElementExt<Msg>: Element<Msg> + Sized {
    /// Type-erase, for heterogeneous branches.
    fn any<'a>(self) -> AnyElement<'a, Msg>
    where
        Self: 'a,
    {
        Box::new(self)
    }
}

impl<Msg, E: Element<Msg>> ElementExt<Msg> for E {}

/// `Msg`-free combinators, available on every builder.
pub trait Fluent: Sized {
    /// Apply `f` only when `cond` holds. The `#(if ...)` analog.
    fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
    }

    /// Apply `f` with the value when present. The `#(if let Some ...)` analog.
    fn when_some<T>(self, value: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self {
        match value {
            Some(v) => f(self, v),
            None => self,
        }
    }

    fn pad_left(self, cols: u16) -> Padded<Self> {
        Padded {
            inner: self,
            left: cols,
            top: 0,
        }
    }

    fn pad_top(self, rows: u16) -> Padded<Self> {
        Padded {
            inner: self,
            left: 0,
            top: rows,
        }
    }
}

impl<T> Fluent for T {}

/// An element offset by padding. Produced by [`Fluent::pad_left`] /
/// [`Fluent::pad_top`].
pub struct Padded<E> {
    inner: E,
    left: u16,
    top: u16,
}

impl<Msg, E: Element<Msg>> Element<Msg> for Padded<E> {}

// ───────────────────────────────────────────────────────────────────
// Containers
// ───────────────────────────────────────────────────────────────────

/// Vertical stack. Children get full width; height is content-driven.
pub struct Col<'a, Msg> {
    children: Vec<AnyElement<'a, Msg>>,
    gap: u16,
}

pub fn col<'a, Msg>() -> Col<'a, Msg> {
    Col {
        children: Vec::new(),
        gap: 0,
    }
}

impl<'a, Msg> Col<'a, Msg> {
    /// Blank rows between children.
    pub fn gap(mut self, rows: u16) -> Self {
        self.gap = rows;
        self
    }

    pub fn child(mut self, child: impl Element<Msg> + 'a) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: Element<Msg> + 'a,
    {
        self.children.extend(
            children
                .into_iter()
                .map(|c| Box::new(c) as AnyElement<'a, Msg>),
        );
        self
    }
}

impl<Msg> Element<Msg> for Col<'_, Msg> {}

/// Column width within a [`Row`].
pub enum Width {
    Fixed(u16),
    /// Remaining space, split equally among `Fill` cells.
    Fill,
}

/// Horizontal layout. Cells declare `Fixed(n)` or `Fill` widths;
/// row height is the max of cell heights.
pub struct Row<'a, Msg> {
    cells: Vec<(Width, AnyElement<'a, Msg>)>,
}

pub fn row<'a, Msg>() -> Row<'a, Msg> {
    Row { cells: Vec::new() }
}

impl<'a, Msg> Row<'a, Msg> {
    pub fn fixed(mut self, cols: u16, child: impl Element<Msg> + 'a) -> Self {
        self.cells.push((Width::Fixed(cols), Box::new(child)));
        self
    }

    pub fn fill(mut self, child: impl Element<Msg> + 'a) -> Self {
        self.cells.push((Width::Fill, Box::new(child)));
        self
    }
}

impl<Msg> Element<Msg> for Row<'_, Msg> {}

// ───────────────────────────────────────────────────────────────────
// Leaves
// ───────────────────────────────────────────────────────────────────

/// One word-wrapped run of styled spans.
pub struct Text {
    spans: Vec<(String, Style)>,
}

/// Single-span text. `.style()` styles it; `.span()` appends further
/// styled spans.
pub fn text(content: impl Into<String>) -> Text {
    Text {
        spans: vec![(content.into(), Style::default())],
    }
}

impl Text {
    /// Style the most recently added span (the initial one if none added).
    pub fn style(mut self, style: Style) -> Self {
        if let Some(last) = self.spans.last_mut() {
            last.1 = style;
        }
        self
    }

    pub fn span(mut self, content: impl Into<String>, style: Style) -> Self {
        self.spans.push((content.into(), style));
        self
    }
}

impl<Msg> Element<Msg> for Text {}

/// The nothing element, for match arms with no output.
pub struct Empty;

pub fn empty() -> Empty {
    Empty
}

impl<Msg> Element<Msg> for Empty {}

pub struct Spinner {
    label: String,
    done: bool,
    hide_checkmark: bool,
    label_style: Style,
    spinner_style: Style,
}

pub fn spinner(label: impl Into<String>) -> Spinner {
    Spinner {
        label: label.into(),
        done: false,
        hide_checkmark: false,
        label_style: Style::default(),
        spinner_style: Style::default(),
    }
}

impl Spinner {
    pub fn done(mut self, done: bool) -> Self {
        self.done = done;
        self
    }

    pub fn hide_checkmark(mut self) -> Self {
        self.hide_checkmark = true;
        self
    }

    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    pub fn spinner_style(mut self, style: Style) -> Self {
        self.spinner_style = style;
        self
    }
}

impl<Msg> Element<Msg> for Spinner {
    fn animated(&self) -> Option<Duration> {
        (!self.done).then(|| Duration::from_millis(80))
    }
}

/// CommonMark rendering (pulldown-cmark behind the scenes in the real thing).
pub struct Markdown {
    source: String,
}

pub fn markdown(source: impl Into<String>) -> Markdown {
    Markdown {
        source: source.into(),
    }
}

impl<Msg> Element<Msg> for Markdown {}

/// Fixed-height tail window over a list of lines.
pub struct Viewport {
    lines: Vec<String>,
    height: u16,
    style: Style,
    wrap: bool,
}

pub fn viewport(lines: Vec<String>) -> Viewport {
    Viewport {
        lines,
        height: 1,
        style: Style::default(),
        wrap: true,
    }
}

impl Viewport {
    pub fn height(mut self, rows: u16) -> Self {
        self.height = rows;
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }
}

impl<Msg> Element<Msg> for Viewport {}

// ───────────────────────────────────────────────────────────────────
// Input: events, focus, keymap (added for Port 3)
// ───────────────────────────────────────────────────────────────────

/// A terminal input event as delivered to the app: key press or paste.
#[derive(Clone, Debug)]
pub enum InputEvent {
    Key(crossterm::event::KeyEvent),
    Paste(String),
}

/// A key pattern for keymap bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: crossterm::event::KeyCode,
    pub mods: crossterm::event::KeyModifiers,
}

pub fn key(code: crossterm::event::KeyCode) -> Key {
    Key {
        code,
        mods: crossterm::event::KeyModifiers::NONE,
    }
}

impl Key {
    pub fn ctrl(mut self) -> Self {
        self.mods |= crossterm::event::KeyModifiers::CONTROL;
        self
    }

    pub fn shift(mut self) -> Self {
        self.mods |= crossterm::event::KeyModifiers::SHIFT;
        self
    }
}

/// Focus identity, owned by the app and stored in its model (GPUI-style).
///
/// The runtime tracks which handle is currently focused; elements bind via
/// `track_focus`, keymap bindings scope via `in_scope`. Stub: real semantics
/// (single focused handle per runtime) live in the runtime, not here.
#[derive(Clone, Default)]
pub struct FocusHandle(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl FocusHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn focus(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn blur(&self) {
        self.0.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_focused(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

enum BindingScope {
    /// Fires before the focused element sees the event. For Ctrl+C-tier
    /// chords only.
    GlobalOverride,
    /// Fires if neither the focused element nor a focus-scoped binding
    /// consumed the event.
    Global,
    /// Fires when the given handle has focus (after the focused element).
    Focus(FocusHandle),
}

/// Key → message bindings, rebuilt from the model each update (so bindings
/// can be conditional on app state — see the Tab bindings in Port 3A).
///
/// Dispatch order for a key event:
/// 1. `on_override` bindings
/// 2. the focused element's own editing keys
/// 3. `in_scope` bindings for the focused handle
/// 4. `on` (global) bindings
/// 5. `fallthrough` mappers for the focused handle (raw event → Msg)
type FallthroughFn<Msg> = Box<dyn Fn(InputEvent) -> Msg>;

pub struct Keymap<Msg> {
    bindings: Vec<(BindingScope, Key, Msg)>,
    fallthrough: Vec<(FocusHandle, FallthroughFn<Msg>)>,
}

pub fn keymap<Msg>() -> Keymap<Msg> {
    Keymap {
        bindings: Vec::new(),
        fallthrough: Vec::new(),
    }
}

impl<Msg> Keymap<Msg> {
    pub fn on(mut self, key: Key, msg: Msg) -> Self {
        self.bindings.push((BindingScope::Global, key, msg));
        self
    }

    pub fn on_override(mut self, key: Key, msg: Msg) -> Self {
        self.bindings.push((BindingScope::GlobalOverride, key, msg));
        self
    }

    pub fn in_scope(mut self, focus: &FocusHandle, key: Key, msg: Msg) -> Self {
        self.bindings
            .push((BindingScope::Focus(focus.clone()), key, msg));
        self
    }

    /// Route unbound events to a message while `focus` is focused. This is
    /// how a text input receives ordinary typing without the framework
    /// owning any editing logic.
    pub fn fallthrough(
        mut self,
        focus: &FocusHandle,
        map: impl Fn(InputEvent) -> Msg + 'static,
    ) -> Self {
        self.fallthrough.push((focus.clone(), Box::new(map)));
        self
    }
}

// ───────────────────────────────────────────────────────────────────
// Text area (Candidate A: state is a plain value in the app model)
// ───────────────────────────────────────────────────────────────────

/// Editable multi-line text state. Lives in the app model; `update` mutates
/// it, the view borrows it.
///
/// Spike shim: stands in for tui-textarea behind the real ratatui interop
/// adapter (which needs measure + cursor + render on `&self` — all of which
/// tui-textarea provides). Editing behavior here is deliberately minimal.
#[derive(Default)]
pub struct TextAreaState {
    lines: Vec<String>,
    cursor: (usize, usize),
}

impl TextAreaState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply an ordinary editing event (typed char, backspace, arrows,
    /// paste). Policy keys (submit, newline chords) are the keymap's job,
    /// not this widget's.
    pub fn handle(&mut self, event: &InputEvent) {
        match event {
            InputEvent::Key(k) => {
                if let crossterm::event::KeyCode::Char(c) = k.code {
                    if self.lines.is_empty() {
                        self.lines.push(String::new());
                    }
                    self.lines[self.cursor.1].push(c);
                    self.cursor.0 += 1;
                }
                // Backspace/arrows/etc. elided in the shim.
            }
            InputEvent::Paste(s) => {
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                self.lines[self.cursor.1].push_str(s);
            }
        }
    }

    pub fn insert_newline(&mut self) {
        self.lines.push(String::new());
        self.cursor = (0, self.lines.len() - 1);
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = text.split('\n').map(String::from).collect();
        let last = self.lines.len().saturating_sub(1);
        self.cursor = (self.lines.get(last).map_or(0, String::len), last);
    }

    /// Take the content and clear (the submit path).
    pub fn take_text(&mut self) -> String {
        let text = self.text();
        self.lines.clear();
        self.cursor = (0, 0);
        text
    }

    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }

    /// Rows needed at the given content width (honest measurement).
    pub fn measure(&self, _width: u16) -> u16 {
        (self.lines.len() as u16).max(1)
    }
}

/// Bordered text area view. Borrows its state from the model.
pub struct TextArea<'a> {
    state: &'a TextAreaState,
    title: String,
    title_right: String,
    footer: String,
    placeholder: String,
    max_height: u16,
    focus: Option<FocusHandle>,
}

pub fn text_area(state: &TextAreaState) -> TextArea<'_> {
    TextArea {
        state,
        title: String::new(),
        title_right: String::new(),
        footer: String::new(),
        placeholder: String::new(),
        max_height: u16::MAX,
        focus: None,
    }
}

impl<'a> TextArea<'a> {
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn title_right(mut self, title: impl Into<String>) -> Self {
        self.title_right = title.into();
        self
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = footer.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn max_height(mut self, rows: u16) -> Self {
        self.max_height = rows;
        self
    }

    /// Bind focus: the runtime shows this element's cursor and routes
    /// events here while the handle is focused. Focus-dependent visuals
    /// (cursor visibility, placeholder) come from `handle.is_focused()` —
    /// no `active` prop shadowing focus in app state.
    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }
}

impl<Msg> Element<Msg> for TextArea<'_> {
    fn height(&self, width: u16) -> u16 {
        // content + border chrome, capped
        (self.state.measure(width.saturating_sub(4)) + 2).min(self.max_height)
    }

    fn cursor(&self, _area: Rect) -> Option<(u16, u16)> {
        let focused = self.focus.as_ref().is_some_and(FocusHandle::is_focused);
        focused.then_some((
            self.state.cursor.0 as u16 + 2,
            self.state.cursor.1 as u16 + 1,
        ))
    }
}

// ───────────────────────────────────────────────────────────────────
// Runtime surface: App, Ctx, Task (added for Port 4)
// ───────────────────────────────────────────────────────────────────

/// The application, Elm-shaped. The implementing struct IS the model:
/// `update` takes `&mut self`, `tail` takes `&self` — the borrow checker
/// enforces the discipline.
pub trait App: Sized + 'static {
    type Msg: 'static;
    /// What `run()` returns after `ctx.exit(..)`.
    type Output: Default;

    fn update(&mut self, msg: Self::Msg, ctx: &mut Ctx<Self>);

    /// The live tail. Re-run every frame; borrows the model.
    fn tail(&self) -> impl Element<Self::Msg> + '_;

    /// Key bindings, rebuilt from the model each update.
    fn keymap(&self) -> Keymap<Self::Msg> {
        keymap()
    }
}

/// Effect context handed to `update`. Blocks pushed here are rendered once
/// (at current width) and flow toward scrollback — committed output is an
/// effect; only the tail is a view.
pub struct Ctx<A: App> {
    pushed: Vec<AnyElement<'static, A::Msg>>,
    exit_with: Option<A::Output>,
}

impl<A: App> Ctx<A> {
    /// Append a finished block to the timeline. Irreversible, like
    /// `println!`. Blocks are `'static`: they leave the model at push time.
    pub fn push(&mut self, block: impl Element<A::Msg> + 'static) {
        self.pushed.push(Box::new(block));
    }

    /// Spawn a stream of messages (the LLM-turn shape); each item feeds
    /// back into `update`. The returned Task cancels on drop.
    #[must_use]
    pub fn spawn<S>(&mut self, stream: S) -> Task
    where
        S: futures::Stream<Item = A::Msg> + Send + 'static,
    {
        let _ = stream;
        Task(())
    }

    /// End the run loop; `run()` returns this value after teardown.
    pub fn exit(&mut self, output: A::Output) {
        self.exit_with = Some(output);
    }
}

/// Handle to spawned work. **Dropping it cancels the work** — holding it in
/// the model makes cancellation a plain assignment (`self.streaming = None`).
pub struct Task(());

impl Task {
    /// Fire-and-forget: run to completion even after the handle is gone.
    pub fn detach(self) {
        std::mem::forget(self);
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        // Stub: the real runtime aborts the spawned work here.
    }
}

impl<Msg: 'static> Keymap<Msg> {
    /// Re-target bindings to a parent message type (Elm's `Html.map` for
    /// keymaps) — how a sub-model's keymap embeds into the app's.
    pub fn map<M2>(self, f: impl Fn(Msg) -> M2 + Clone + 'static) -> Keymap<M2> {
        Keymap {
            bindings: self
                .bindings
                .into_iter()
                .map(|(scope, key, msg)| (scope, key, f(msg)))
                .collect(),
            fallthrough: self
                .fallthrough
                .into_iter()
                .map(|(focus, g)| {
                    let f = f.clone();
                    let mapped: FallthroughFn<M2> = Box::new(move |ev| f(g(ev)));
                    (focus, mapped)
                })
                .collect(),
        }
    }
}

/// Adapt an element from one message type to another (Elm's `Html.map`).
///
/// Trivially a phantom re-wrap today, because nothing on `Element` actually
/// carries `Msg` in the strict-Elm candidate — emission happens in the
/// keymap. See FINDINGS: this is evidence the `Msg` parameter on `Element`
/// itself may be vestigial.
pub fn map_msg<'a, M1: 'a, M2>(el: impl Element<M1> + 'a) -> impl Element<M2> + 'a {
    struct Map<E, M1>(E, std::marker::PhantomData<fn() -> M1>);

    impl<M1, M2, E: Element<M1>> Element<M2> for Map<E, M1> {
        fn height(&self, width: u16) -> u16 {
            self.0.height(width)
        }

        fn render(&self, area: Rect, buf: &mut Buffer) {
            self.0.render(area, buf)
        }

        fn animated(&self) -> Option<Duration> {
            self.0.animated()
        }

        fn cursor(&self, area: Rect) -> Option<(u16, u16)> {
            self.0.cursor(area)
        }
    }

    Map(el, std::marker::PhantomData)
}
