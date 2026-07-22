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
        let init_exit = ctx.exit;
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
            let mut ctx = Ctx {
                timeline: &mut self.timeline,
                output: &mut bytes,
                effects: &mut self.effects,
                exit: None,
            };
            self.app.update(msg, &mut ctx);
            if let Some(output) = ctx.exit {
                exit = Some(output);
                break;
            }
        }

        bytes.extend_from_slice(&self.present());
        if exit.is_some() {
            bytes.extend_from_slice(&self.timeline.finalize());
        }
        (bytes, exit)
    }

    /// Effects queued by `update` (spawned streams), for the driver to
    /// execute. Drain after every [`handle`](Runtime::handle) /
    /// [`process`](Runtime::process).
    pub fn take_effects(&mut self) -> Vec<Effect<A::Msg>> {
        std::mem::take(&mut self.effects)
    }

    /// Re-present the live tail (also called on animation ticks).
    pub fn present(&mut self) -> Vec<u8> {
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
    pub fn resize(&mut self, width: u16, terminal_height: u16) -> Vec<u8> {
        self.timeline.set_terminal_height(terminal_height);
        let mut bytes = self.timeline.resize(width);
        bytes.extend_from_slice(&self.present());
        bytes
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
}

impl RunOptions {
    pub fn keyboard(mut self, protocol: KeyboardProtocol) -> Self {
        self.keyboard = protocol;
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

    let _guard = RawModeGuard::enable(options.keyboard)?;

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
                Event::Resize(w, h) => runtime.resize(w, h),
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

/// Restores the terminal on drop, including panic unwind.
pub(crate) struct RawModeGuard {
    keyboard_enhanced: bool,
}

impl RawModeGuard {
    pub(crate) fn enable(keyboard: KeyboardProtocol) -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        let _ = crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste);

        // Only push if the terminal supports it — the silent-fallback
        // contract of KeyboardProtocol::Enhanced.
        let keyboard_enhanced = keyboard == KeyboardProtocol::Enhanced
            && crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        if keyboard_enhanced {
            // Disambiguation only: it's all Shift+Enter detection needs.
            // REPORT_EVENT_TYPES would add key-release events, which the
            // built-in drivers filter but a custom driver feeding
            // Runtime::handle directly could easily double-dispatch.
            let _ = crossterm::execute!(
                stdout,
                crossterm::event::PushKeyboardEnhancementFlags(
                    crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                )
            );
        }

        Ok(Self { keyboard_enhanced })
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.keyboard_enhanced {
            let _ = crossterm::execute!(stdout, crossterm::event::PopKeyboardEnhancementFlags);
        }
        let _ = crossterm::execute!(stdout, crossterm::event::DisableBracketedPaste);
        let _ = crossterm::terminal::disable_raw_mode();
        // The engine hides the cursor while no element hints one; make
        // sure the shell gets it back.
        let _ = stdout.write_all(b"\x1b[?25h");
        let _ = stdout.flush();
    }
}
