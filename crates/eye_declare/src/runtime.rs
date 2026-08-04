//! The runtime: a headlessly-testable [`Runtime`] core plus the [`run`]
//! terminal shell around it.
//!
//! `Runtime` speaks [`InputEvent`]s in and escape bytes out, so entire
//! apps can be driven in tests against the engine's `TestTerminal` — the
//! same way the framework tests itself.

use std::io::{self, Write};
use std::time::Duration;

use crate::app::{App, Ctx};
use crate::element::Element;
use crate::input::InputEvent;
use crate::task::Effect;
use crate::timeline::Timeline;

/// The pure core of the run loop: dispatch → update → flush pushes →
/// present. No terminal I/O; returns bytes for the caller to write.
pub struct Runtime<A: App> {
    app: A,
    timeline: Timeline,
    animate: Option<Duration>,
    effects: Vec<Effect<A::Msg>>,
    /// Bytes produced by [`App::init`] pushes, drained by the next
    /// [`present`](Runtime::present).
    pending: Vec<u8>,
    /// An exit requested from [`App::init`], delivered via
    /// [`startup`](Runtime::startup).
    init_exit: Option<A::Output>,
}

impl<A: App> Runtime<A>
where
    A::Msg: Clone,
{
    /// Construct the runtime. Runs [`App::init`]; its pushed blocks join
    /// the next `present`'s bytes and its spawned effects wait in
    /// [`take_effects`](Runtime::take_effects).
    pub fn new(mut app: A, width: u16, terminal_height: u16) -> Self {
        let mut timeline = Timeline::new(width, terminal_height);
        let mut pending = Vec::new();
        let mut effects = Vec::new();
        let mut ctx = Ctx {
            timeline: &mut timeline,
            output: &mut pending,
            effects: &mut effects,
            exit: None,
        };
        app.init(&mut ctx);
        let mut init_exit = ctx.exit;
        if init_exit.is_none()
            && let Some(msg) = app.on_resize(width, terminal_height)
        {
            let mut ctx = Ctx {
                timeline: &mut timeline,
                output: &mut pending,
                effects: &mut effects,
                exit: None,
            };
            app.update(msg, &mut ctx);
            init_exit = ctx.exit;
        }
        Self {
            app,
            timeline,
            animate: None,
            effects,
            pending,
            init_exit,
        }
    }

    /// First bytes to write and any exit [`App::init`] requested — what a
    /// driver calls once before its loop, instead of a bare `present`.
    ///
    /// An init-requested exit skips the message loop, so the shell
    /// handoff is appended here rather than by `process_batch`.
    pub fn startup(&mut self) -> (Vec<u8>, Option<A::Output>) {
        let mut bytes = self.present();
        let exit = self.init_exit.take();
        if exit.is_some() {
            bytes.extend_from_slice(&self.timeline.finalize());
        }
        (bytes, exit)
    }

    /// Feed one input event. Resolves it through the app's keymap; if a
    /// message results, processes it. Returns the bytes to write and the
    /// app's output when it exited.
    pub fn handle(&mut self, event: InputEvent) -> (Vec<u8>, Option<A::Output>) {
        match self.app.keymap().dispatch(&event) {
            Some(msg) => self.process(msg),
            None => (Vec::new(), None),
        }
    }

    /// Feed one message (from the keymap, or — in the async driver — from
    /// tasks and subscriptions).
    pub fn process(&mut self, msg: A::Msg) -> (Vec<u8>, Option<A::Output>) {
        self.process_batch(std::iter::once(msg))
    }

    /// Feed a batch of messages, presenting once at the end.
    ///
    /// Presenting is O(tail) regardless of how many messages arrived, so
    /// coalescing ready messages (the async driver drains its channel
    /// into one batch) collapses a burst of stream chunks into a single
    /// frame. Stops early if a message exits the app; the remainder is
    /// dropped, matching a terminated run loop.
    pub fn process_batch(
        &mut self,
        msgs: impl IntoIterator<Item = A::Msg>,
    ) -> (Vec<u8>, Option<A::Output>) {
        // Undelivered init bytes come first so init's blocks precede
        // these updates' pushes in scrollback.
        let mut bytes = std::mem::take(&mut self.pending);
        let mut exit = None;
        for msg in msgs {
            exit = self.apply_msg(msg, &mut bytes);
            if exit.is_some() {
                break;
            }
        }

        bytes.extend_from_slice(&self.present());
        if exit.is_some() {
            bytes.extend_from_slice(&self.timeline.finalize());
        }
        (bytes, exit)
    }

    /// Run one message through `update`, collecting pushed-block bytes into
    /// `bytes` and queued effects into the runtime. No present.
    fn apply_msg(&mut self, msg: A::Msg, bytes: &mut Vec<u8>) -> Option<A::Output> {
        let mut ctx = Ctx {
            timeline: &mut self.timeline,
            output: bytes,
            effects: &mut self.effects,
            exit: None,
        };
        self.app.update(msg, &mut ctx);
        ctx.exit
    }

    /// Effects queued by `update` (spawned streams), for the driver to
    /// execute. Drain after every [`handle`](Runtime::handle) /
    /// [`process`](Runtime::process).
    pub fn take_effects(&mut self) -> Vec<Effect<A::Msg>> {
        std::mem::take(&mut self.effects)
    }

    /// Re-present the live tail (also called on animation ticks).
    pub fn present(&mut self) -> Vec<u8> {
        self.timeline.set_cursor_style(self.app.cursor_style());
        let tail = self.app.tail();
        self.animate = tail.animated();
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend(self.timeline.present(&tail));
        bytes
    }

    /// How soon the tail wants re-presenting for animation, if at all.
    /// Refreshed by every [`present`](Runtime::present).
    pub fn animation_interval(&self) -> Option<Duration> {
        self.animate
    }

    /// Handle a terminal resize. Committed blocks keep the terminal's own
    /// reflow; the live region is erased and repainted at the new width.
    ///
    /// Prefer [`resize_anchored`](Runtime::resize_anchored) when the
    /// driver can query the cursor position (CSI 6n).
    pub fn resize(&mut self, width: u16, terminal_height: u16) -> Vec<u8> {
        self.timeline.set_terminal_height(terminal_height);
        let mut bytes = self.timeline.resize(width);
        bytes.extend_from_slice(&self.present());
        bytes
    }

    /// [`resize`](Runtime::resize) with the cursor's reported absolute
    /// position (`(col, row)`, 0-based) queried after the resize event:
    /// the erase re-anchors on it instead of pre-reflow row arithmetic,
    /// keeping committed blocks intact on reflowing terminals.
    pub fn resize_anchored(
        &mut self,
        width: u16,
        terminal_height: u16,
        cursor: (u16, u16),
    ) -> Vec<u8> {
        self.timeline.set_terminal_height(terminal_height);
        let mut bytes = self.timeline.resize_anchored(width, cursor);
        bytes.extend_from_slice(&self.present());
        bytes
    }

    /// [`resize_anchored`](Runtime::resize_anchored), plus
    /// [`App::on_resize`] delivery: the size message (if any) runs through
    /// `update` after the region reset and before the single repaint, so
    /// the new frame is laid out at the new size. Returns the app's output
    /// if the update exited.
    ///
    /// `cursor` is the post-reflow cursor position report when available
    /// (preferred); `None` falls back to stale-arithmetic erase.
    pub fn resize_msg(
        &mut self,
        width: u16,
        terminal_height: u16,
        cursor: Option<(u16, u16)>,
    ) -> (Vec<u8>, Option<A::Output>) {
        self.timeline.set_terminal_height(terminal_height);
        let mut bytes = match cursor {
            Some(pos) => self.timeline.resize_anchored(width, pos),
            None => self.timeline.resize(width),
        };
        let exit = self.deliver_resize(width, terminal_height, &mut bytes);
        bytes.extend_from_slice(&self.present());
        if exit.is_some() {
            bytes.extend_from_slice(&self.timeline.finalize());
        }
        (bytes, exit)
    }

    /// Resize for alt-screen (fullscreen) apps: clear the visible screen
    /// and repaint, with [`App::on_resize`] delivered in between. There is
    /// no committed content to preserve and no scrollback to protect, so
    /// no cursor report is needed.
    pub fn resize_screen(
        &mut self,
        width: u16,
        terminal_height: u16,
    ) -> (Vec<u8>, Option<A::Output>) {
        self.timeline.set_terminal_height(terminal_height);
        let mut bytes = self.timeline.reset_screen(width);
        let exit = self.deliver_resize(width, terminal_height, &mut bytes);
        bytes.extend_from_slice(&self.present());
        if exit.is_some() {
            bytes.extend_from_slice(&self.timeline.finalize());
        }
        (bytes, exit)
    }

    fn deliver_resize(
        &mut self,
        width: u16,
        terminal_height: u16,
        bytes: &mut Vec<u8>,
    ) -> Option<A::Output> {
        let msg = self.app.on_resize(width, terminal_height)?;
        self.apply_msg(msg, bytes)
    }

    /// Shell handoff for exits that bypass the message loop: park the
    /// cursor at column 0 below the content. Message-driven exits get
    /// this automatically from [`process_batch`](Runtime::process_batch);
    /// custom drivers that exit for their own reasons (e.g. stdin
    /// closing) write these bytes last.
    pub fn finalize(&mut self) -> Vec<u8> {
        self.timeline.finalize()
    }

    /// The wrapped app, for inspection after exit (tests) or state peeks.
    pub fn app(&self) -> &A {
        &self.app
    }
}

/// Which keyboard protocol interactive drivers request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardProtocol {
    /// Standard terminal key reporting (default). Compatible everywhere,
    /// but some chords are ambiguous (Shift+Enter vs Enter, Tab vs Ctrl+I).
    #[default]
    Legacy,
    /// The [kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
    /// when the terminal supports it, silently falling back to legacy.
    /// Disambiguates modified keys (Shift+Enter) in supporting terminals
    /// (kitty, WezTerm, foot, Ghostty, Windows Terminal, …).
    Enhanced,
    /// Push an explicit kitty flag set. With `probe: true` the terminal is
    /// asked first (one query round-trip) and unsupporting terminals fall
    /// back to legacy; with `probe: false` the flags are pushed blind —
    /// no round-trip, and terminals that ignore the protocol ignore the
    /// push (the pop at teardown is equally ignored).
    Custom {
        flags: crossterm::event::KeyboardEnhancementFlags,
        probe: bool,
    },
}

/// Which screen the interactive drivers run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenMode {
    /// The main screen: the region starts at the shell prompt, committed
    /// blocks flow into scrollback (the timeline model). The default.
    #[default]
    Inline,
    /// The alternate screen: fullscreen apps. The prior screen contents
    /// are restored at teardown, resizes clear-and-repaint (no cursor
    /// position query), and startup skips the start-column query — the
    /// cursor is homed explicitly. Committed blocks are of limited use
    /// here: they scroll away within the alt screen and vanish with it.
    AltScreen,
}

/// Terminal options for the interactive drivers ([`run_with`],
/// [`driver_tokio::run_with`](crate::driver_tokio::run_with)).
///
/// Construct with [`Default`] and the fluent setters; new options may be
/// added without a breaking change.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunOptions {
    pub keyboard: KeyboardProtocol,
    pub screen: ScreenMode,
    pub mouse_capture: bool,
}

impl RunOptions {
    pub fn keyboard(mut self, protocol: KeyboardProtocol) -> Self {
        self.keyboard = protocol;
        self
    }

    pub fn screen(mut self, screen: ScreenMode) -> Self {
        self.screen = screen;
        self
    }

    /// Capture mouse events and deliver them as [`InputEvent::Mouse`]
    /// through the keymap fallthrough. Off by default: capture takes over
    /// the terminal's native selection/copy behavior, so only apps that
    /// actually handle mouse events should request it.
    pub fn mouse_capture(mut self, capture: bool) -> Self {
        self.mouse_capture = capture;
        self
    }
}

/// Run an app on the attached terminal until it exits, with default
/// [`RunOptions`].
///
/// Raw mode + bracketed paste are enabled for the duration and restored on
/// exit (including panic unwind); the cursor is re-shown on teardown.
pub fn run<A: App>(app: A) -> io::Result<A::Output>
where
    A::Msg: Clone,
{
    run_with(app, RunOptions::default())
}

/// [`run`] with explicit [`RunOptions`].
pub fn run_with<A: App>(app: A, options: RunOptions) -> io::Result<A::Output>
where
    A::Msg: Clone,
{
    let (width, height) = crossterm::terminal::size()?;
    let mut runtime = Runtime::new(app, width, height);
    let mut stdout = io::stdout().lock();

    let _guard = RawModeGuard::enable(options.keyboard, options.screen, options.mouse_capture)?;
    if options.screen != ScreenMode::AltScreen {
        normalize_start_column();
    }

    let (bytes, init_exit) = runtime.startup();
    stdout.write_all(&bytes)?;
    stdout.flush()?;
    if let Some(output) = init_exit {
        return Ok(output);
    }
    reject_effects(&mut runtime)?;

    loop {
        let timeout = runtime
            .animation_interval()
            .unwrap_or(Duration::from_secs(3600));

        let bytes = if crossterm::event::poll(timeout)? {
            use crossterm::event::{Event, KeyEventKind};
            match crossterm::event::read()? {
                Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    let (bytes, exit) = runtime.handle(InputEvent::Key(k));
                    reject_effects(&mut runtime)?;
                    if let Some(output) = exit {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                        return Ok(output);
                    }
                    bytes
                }
                Event::Paste(s) => {
                    let (bytes, exit) = runtime.handle(InputEvent::Paste(s));
                    reject_effects(&mut runtime)?;
                    if let Some(output) = exit {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                        return Ok(output);
                    }
                    bytes
                }
                Event::Mouse(m) => {
                    let (bytes, exit) = runtime.handle(InputEvent::Mouse(m));
                    reject_effects(&mut runtime)?;
                    if let Some(output) = exit {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                        return Ok(output);
                    }
                    bytes
                }
                Event::Resize(w, h) => {
                    // Coalesce resize storms (see the tokio driver): drain
                    // queued events, process keys in order, and handle one
                    // resize at the latest size.
                    let (mut w, mut h) = (w, h);
                    let mut bytes = Vec::new();
                    let mut exited = None;
                    while crossterm::event::poll(Duration::ZERO)? {
                        match crossterm::event::read()? {
                            Event::Resize(nw, nh) => (w, h) = (nw, nh),
                            Event::Key(k)
                                if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                            {
                                let (b, e) = runtime.handle(InputEvent::Key(k));
                                reject_effects(&mut runtime)?;
                                bytes.extend_from_slice(&b);
                                if e.is_some() {
                                    exited = e;
                                    break;
                                }
                            }
                            Event::Paste(s) => {
                                let (b, e) = runtime.handle(InputEvent::Paste(s));
                                reject_effects(&mut runtime)?;
                                bytes.extend_from_slice(&b);
                                if e.is_some() {
                                    exited = e;
                                    break;
                                }
                            }
                            Event::Mouse(m) => {
                                let (b, e) = runtime.handle(InputEvent::Mouse(m));
                                reject_effects(&mut runtime)?;
                                bytes.extend_from_slice(&b);
                                if e.is_some() {
                                    exited = e;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(output) = exited {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                        return Ok(output);
                    }
                    let (resize_bytes, resize_exit) =
                        resize_with_report(&mut runtime, w, h, options.screen);
                    reject_effects(&mut runtime)?;
                    bytes.extend_from_slice(&resize_bytes);
                    if let Some(output) = resize_exit {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                        return Ok(output);
                    }
                    bytes
                }
                _ => Vec::new(),
            }
        } else {
            // Animation tick.
            runtime.present()
        };

        if !bytes.is_empty() {
            stdout.write_all(&bytes)?;
            stdout.flush()?;
        }
    }
}

/// Read the cursor position, discarding possibly-stale replies first.
///
/// Terminal replies are matched to queries only by arrival order. A
/// reply can be sitting in the input buffer before we ever ask — a zsh
/// prompt theme (powerlevel10k) also speaks CSI 6n, and a zle widget
/// hands us the tty with its last reply potentially unread — and a
/// timed-out query of our own leaves its late reply queued. Either way
/// every later read returns the *previous* query's answer, forever.
/// Reading twice (plus once per known orphan) makes the final value
/// describe the current screen: all reads happen against the same
/// screen state, and the extra reads consume queued strays.
pub(crate) fn read_cursor_position() -> std::io::Result<(u16, u16)> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    /// Replies orphaned by our own timed-out queries.
    static ORPHANS: AtomicUsize = AtomicUsize::new(0);

    let discard = ORPHANS.load(Ordering::Relaxed) + 1;
    for _ in 0..discard {
        if let Err(e) = crossterm::cursor::position() {
            ORPHANS.fetch_add(1, Ordering::Relaxed);
            return Err(e);
        }
    }
    match crossterm::cursor::position() {
        Ok(v) => Ok(v),
        Err(e) => {
            ORPHANS.fetch_add(1, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// Start the region on a fresh line if the embedding handed us the
/// terminal with the cursor mid-line — a zle widget leaves it at the end
/// of the still-painted prompt, and raw-ish tty modes mean even a prior
/// `println!` may not have carried a carriage return. The engine paints
/// relative to column 0. Called once per run, at startup; terminals that
/// never answer CSI 6n cost one timeout here.
pub(crate) fn normalize_start_column() {
    let Ok((col, _)) = read_cursor_position() else {
        return;
    };
    if col != 0 {
        // Stdout's lock is reentrant, so this is safe under the
        // driver's lock.
        let mut out = io::stdout().lock();
        let _ = out.write_all(b"\r\n");
        let _ = out.flush();
    }
}

/// Resize re-anchored by a fresh cursor position report — the terminal
/// has already reflowed by the time the resize event arrives, so the
/// report is post-reflow ground truth. Falls back to stale-arithmetic
/// erase if the terminal doesn't answer. On the alt screen there is
/// nothing to anchor: clear and repaint, no query.
///
/// The event's dimensions may be stale during a drag (events queue while
/// the terminal keeps resizing); painting at a stale width soft-wraps
/// for real and corrupts row tracking, so re-query the current size and
/// prefer it.
pub(crate) fn resize_with_report<A: App>(
    runtime: &mut Runtime<A>,
    w: u16,
    h: u16,
    screen: ScreenMode,
) -> (Vec<u8>, Option<A::Output>)
where
    A::Msg: Clone,
{
    let (w, h) = crossterm::terminal::size().unwrap_or((w, h));
    match screen {
        ScreenMode::AltScreen => runtime.resize_screen(w, h),
        ScreenMode::Inline => {
            let cursor = read_cursor_position().ok();
            runtime.resize_msg(w, h, cursor)
        }
    }
}

/// The sync loop can't execute async work — surface the mistake loudly
/// instead of silently dropping the app's spawned streams/subscriptions.
fn reject_effects<A: App>(runtime: &mut Runtime<A>) -> io::Result<()>
where
    A::Msg: Clone,
{
    if !runtime.take_effects().is_empty() {
        return Err(io::Error::other(
            "app spawned async work (ctx.spawn/perform); drive it with the tokio runtime \
             (eye_declare::driver_tokio::run) instead of the sync run()",
        ));
    }
    if !runtime.app().subscriptions().is_empty() {
        return Err(io::Error::other(
            "app declares subscriptions; drive it with the tokio runtime \
             (eye_declare::driver_tokio::run) instead of the sync run()",
        ));
    }
    Ok(())
}

/// The kitty flags to push for a protocol choice, or `None` for legacy.
/// Pure so the probe/blind/fallback logic is unit-testable.
fn keyboard_flags_to_push(
    keyboard: KeyboardProtocol,
    terminal_supports: impl FnOnce() -> bool,
) -> Option<crossterm::event::KeyboardEnhancementFlags> {
    match keyboard {
        KeyboardProtocol::Legacy => None,
        // Disambiguation only: it's all Shift+Enter detection needs.
        // REPORT_EVENT_TYPES would add key-release events, which the
        // built-in drivers filter but a custom driver feeding
        // Runtime::handle directly could easily double-dispatch.
        KeyboardProtocol::Enhanced => terminal_supports()
            .then_some(crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
        KeyboardProtocol::Custom { flags, probe } => {
            (!probe || terminal_supports()).then_some(flags)
        }
    }
}

/// Restores the terminal on drop, including panic unwind.
pub(crate) struct RawModeGuard {
    keyboard_enhanced: bool,
    alt_screen: bool,
    mouse_capture: bool,
}

impl RawModeGuard {
    pub(crate) fn enable(
        keyboard: KeyboardProtocol,
        screen: ScreenMode,
        mouse_capture: bool,
    ) -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();

        let alt_screen = screen == ScreenMode::AltScreen;
        if alt_screen {
            let _ = crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen);
            // The alt screen starts blank but the cursor position is
            // whatever the terminal felt like; home it so the engine's
            // (0,0) origin assumption holds without a position query.
            let _ = stdout.write_all(b"\x1b[2J\x1b[H");
            let _ = stdout.flush();
        }

        let _ = crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste);
        if mouse_capture {
            let _ = crossterm::execute!(stdout, crossterm::event::EnableMouseCapture);
        }

        // Only probe when the protocol asks for it — the silent-fallback
        // contract of KeyboardProtocol::Enhanced.
        let flags = keyboard_flags_to_push(keyboard, || {
            crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
        });
        let keyboard_enhanced = flags.is_some();
        if let Some(flags) = flags {
            let _ = crossterm::execute!(
                stdout,
                crossterm::event::PushKeyboardEnhancementFlags(flags)
            );
        }

        Ok(Self {
            keyboard_enhanced,
            alt_screen,
            mouse_capture,
        })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.keyboard_enhanced {
            let _ = crossterm::execute!(stdout, crossterm::event::PopKeyboardEnhancementFlags);
        }
        if self.mouse_capture {
            let _ = crossterm::execute!(stdout, crossterm::event::DisableMouseCapture);
            // The terminal keeps sending reports until it processes the
            // disable; anything already queued (a fast wheel queues dozens,
            // possibly the very events that triggered the exit) would land
            // in the parent shell's input as escape-sequence garbage — and
            // has been seen to wedge fragile emulators outright. Drain
            // until quiet, bounded, while raw mode is still on. Costs up to
            // 50ms at teardown, only for capture-enabled apps.
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
            while std::time::Instant::now() < deadline
                && matches!(
                    crossterm::event::poll(std::time::Duration::from_millis(5)),
                    Ok(true)
                )
            {
                let _ = crossterm::event::read();
            }
        }
        if self.alt_screen {
            let _ = crossterm::execute!(stdout, crossterm::terminal::LeaveAlternateScreen);
        }
        let _ = crossterm::execute!(stdout, crossterm::event::DisableBracketedPaste);
        let _ = crossterm::terminal::disable_raw_mode();
        // The engine hides the cursor while no element hints one; make
        // sure the shell gets it back.
        let _ = stdout.write_all(b"\x1b[?25h");
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyboardEnhancementFlags as Flags;

    #[test]
    fn legacy_pushes_nothing_and_never_probes() {
        let flags = keyboard_flags_to_push(KeyboardProtocol::Legacy, || {
            panic!("legacy must not query the terminal")
        });
        assert_eq!(flags, None);
    }

    #[test]
    fn enhanced_probes_and_falls_back() {
        assert_eq!(
            keyboard_flags_to_push(KeyboardProtocol::Enhanced, || true),
            Some(Flags::DISAMBIGUATE_ESCAPE_CODES)
        );
        assert_eq!(
            keyboard_flags_to_push(KeyboardProtocol::Enhanced, || false),
            None
        );
    }

    #[test]
    fn custom_blind_push_skips_the_probe_round_trip() {
        let flags = Flags::DISAMBIGUATE_ESCAPE_CODES
            | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            | Flags::REPORT_ALTERNATE_KEYS;
        let pushed = keyboard_flags_to_push(
            KeyboardProtocol::Custom {
                flags,
                probe: false,
            },
            || panic!("blind push must not query the terminal"),
        );
        assert_eq!(pushed, Some(flags));
    }

    #[test]
    fn custom_with_probe_respects_the_answer() {
        let flags = Flags::REPORT_ALTERNATE_KEYS;
        assert_eq!(
            keyboard_flags_to_push(KeyboardProtocol::Custom { flags, probe: true }, || true),
            Some(flags)
        );
        assert_eq!(
            keyboard_flags_to_push(KeyboardProtocol::Custom { flags, probe: true }, || false),
            None
        );
    }
}
