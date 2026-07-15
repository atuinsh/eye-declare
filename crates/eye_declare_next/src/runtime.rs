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
use crate::timeline::Timeline;

/// The pure core of the run loop: dispatch → update → flush pushes →
/// present. No terminal I/O; returns bytes for the caller to write.
pub struct Runtime<A: App> {
    app: A,
    timeline: Timeline,
    animate: Option<Duration>,
}

impl<A: App> Runtime<A>
where
    A::Msg: Clone,
{
    pub fn new(app: A, width: u16, terminal_height: u16) -> Self {
        Self {
            app,
            timeline: Timeline::new(width, terminal_height),
            animate: None,
        }
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
        let mut bytes = Vec::new();
        let mut ctx = Ctx {
            timeline: &mut self.timeline,
            output: &mut bytes,
            exit: None,
        };
        self.app.update(msg, &mut ctx);
        let exit = ctx.exit;

        bytes.extend_from_slice(&self.present());
        if exit.is_some() {
            bytes.extend_from_slice(&self.timeline.finalize());
        }
        (bytes, exit)
    }

    /// Re-present the live tail (also called on animation ticks).
    pub fn present(&mut self) -> Vec<u8> {
        let tail = self.app.tail();
        self.animate = tail.animated();
        self.timeline.present(&tail)
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

    /// The wrapped app, for inspection after exit (tests) or state peeks.
    pub fn app(&self) -> &A {
        &self.app
    }
}

/// Run an app on the attached terminal until it exits.
///
/// Raw mode + bracketed paste are enabled for the duration and restored on
/// exit (including panic unwind); the cursor is re-shown on teardown.
pub fn run<A: App>(app: A) -> io::Result<A::Output>
where
    A::Msg: Clone,
{
    let (width, height) = crossterm::terminal::size()?;
    let mut runtime = Runtime::new(app, width, height);
    let mut stdout = io::stdout().lock();

    let _guard = RawModeGuard::enable()?;

    let bytes = runtime.present();
    stdout.write_all(&bytes)?;
    stdout.flush()?;

    loop {
        let timeout = runtime
            .animation_interval()
            .unwrap_or(Duration::from_secs(3600));

        let bytes = if crossterm::event::poll(timeout)? {
            use crossterm::event::{Event, KeyEventKind};
            match crossterm::event::read()? {
                Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    let (bytes, exit) = runtime.handle(InputEvent::Key(k));
                    if let Some(output) = exit {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                        return Ok(output);
                    }
                    bytes
                }
                Event::Paste(s) => {
                    let (bytes, exit) = runtime.handle(InputEvent::Paste(s));
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

/// Restores the terminal on drop, including panic unwind.
struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        let _ = crossterm::execute!(stdout, crossterm::event::EnableBracketedPaste);
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = crossterm::execute!(stdout, crossterm::event::DisableBracketedPaste);
        let _ = crossterm::terminal::disable_raw_mode();
        // The engine hides the cursor while no element hints one; make
        // sure the shell gets it back.
        let _ = stdout.write_all(b"\x1b[?25h");
        let _ = stdout.flush();
    }
}
