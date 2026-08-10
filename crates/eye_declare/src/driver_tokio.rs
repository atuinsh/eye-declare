//! The tokio driver: wraps [`Runtime`] in an async loop that multiplexes
//! terminal events, messages from spawned work, and animation ticks.
//!
//! The core crate stays executor-agnostic — this module is the only place
//! tokio appears (feature `tokio`, on by default). A custom driver for
//! another executor needs exactly what this one uses: `Runtime` (events in,
//! bytes out), [`Runtime::take_effects`], and [`Effect`]'s stream + cancel
//! pair.

use std::collections::HashMap;
use std::future::poll_fn;
use std::io::{self, Write};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_core::Stream;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::app::App;
use crate::input::InputEvent;
use crate::runtime::{RawModeGuard, RunOptions, Runtime};
use crate::subscription::{SubKind, Subscriptions};
use crate::task::{Cancel, Effect, MsgStream};

/// Run an app on the attached terminal until it exits, executing spawned
/// streams on the ambient tokio runtime. Uses default [`RunOptions`].
///
/// Raw mode + bracketed paste are enabled for the duration and restored on
/// exit (including panic unwind). If stdin closes, returns
/// `A::Output::default()`.
pub async fn run<A>(app: A) -> io::Result<A::Output>
where
    A: App,
    A::Msg: Clone + Send + 'static,
{
    run_with(app, RunOptions::default()).await
}

/// [`run`] with explicit [`RunOptions`] (e.g. the enhanced keyboard
/// protocol, which apps need to distinguish Shift+Enter from Enter).
pub async fn run_with<A>(app: A, options: RunOptions) -> io::Result<A::Output>
where
    A: App,
    A::Msg: Clone + Send + 'static,
{
    let (width, height) = crossterm::terminal::size()?;
    let mut runtime = Runtime::new(app, width, height);
    let (tx, mut rx) = unbounded_channel::<A::Msg>();
    let mut stdout = io::stdout().lock();

    let mut guard = RawModeGuard::enable(options.keyboard, options.screen, options.mouse_capture)?;
    if options.screen != crate::runtime::ScreenMode::AltScreen {
        crate::runtime::normalize_start_column();
    }

    let (bytes, init_exit) = runtime.startup();
    stdout.write_all(&bytes)?;
    stdout.flush()?;
    if let Some(output) = init_exit {
        shutdown_mouse_sync(&mut guard, &mut stdout);
        return Ok(output);
    }
    spawn_effects(runtime.take_effects(), &tx);

    let mut subs = ActiveSubscriptions::new(tx.clone());
    subs.sync(runtime.app().subscriptions());

    let mut events = crossterm::event::EventStream::new();

    // Frame pacing: the stream delivers one event per poll through its
    // reader thread, so an event burst (a wheel flick queues dozens)
    // cannot be batched at the source — it arrives as a rapid trickle,
    // and presenting per event paints every intermediate state, which
    // reads as lag. Instead, events accumulate in `pending` and flush
    // through `handle_batch` at most once per FRAME. An event arriving
    // with the frame budget already spent (the common typing case)
    // flushes immediately: pacing only engages during bursts.
    const FRAME: Duration = Duration::from_millis(8);
    let mut pending: Vec<InputEvent> = Vec::new();
    let mut last_flush = tokio::time::Instant::now() - FRAME;

    loop {
        let anim = runtime.animation_interval();
        let flush_at = (!pending.is_empty()).then(|| last_flush + FRAME);

        enum Step<O> {
            Out(Vec<u8>, Option<O>),
            Flush,
        }

        let step = tokio::select! {
            biased;

            maybe_event = poll_fn(|cx| Pin::new(&mut events).poll_next(cx)) => {
                use crossterm::event::{Event, KeyEventKind};
                match maybe_event {
                    Some(Ok(first)) => match first {
                        Event::Key(k)
                            if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                        {
                            pending.push(InputEvent::Key(k));
                            Step::Flush
                        }
                        Event::Paste(s) => {
                            pending.push(InputEvent::Paste(s));
                            Step::Flush
                        }
                        Event::Mouse(m) => {
                            pending.push(InputEvent::Mouse(m));
                            Step::Flush
                        }
                        Event::Resize(w, h) => {
                            // Resizes repaint the whole region and may
                            // query the cursor; never paced. Flush pending
                            // input first so it lays out against the state
                            // the user last saw.
                            let (mut bytes, mut exit) =
                                runtime.handle_batch(pending.drain(..));
                            last_flush = tokio::time::Instant::now();
                            if exit.is_none() {
                                let (resize_bytes, resize_exit) =
                                    crate::runtime::resize_with_report(
                                        &mut runtime,
                                        w,
                                        h,
                                        options.screen,
                                    );
                                bytes.extend_from_slice(&resize_bytes);
                                exit = resize_exit;
                            }
                            Step::Out(bytes, exit)
                        }
                        _ => Step::Out(Vec::new(), None),
                    },
                    Some(Err(e)) => return Err(e),
                    // Terminal input ended (stdin closed): exit cleanly,
                    // with the shell handoff process_batch would have done.
                    None => Step::Out(runtime.finalize(), Some(A::Output::default())),
                }
            }

            Some(msg) = rx.recv() => {
                // Drain whatever else is already queued (a stream burst)
                // into one batch: many chunks, one frame.
                let mut batch = vec![msg];
                while batch.len() < 256 {
                    match rx.try_recv() {
                        Ok(m) => batch.push(m),
                        Err(_) => break,
                    }
                }
                let (bytes, exit) = runtime.process_batch(batch);
                Step::Out(bytes, exit)
            }

            _ = sleep_opt(flush_at.map(|at| at.saturating_duration_since(tokio::time::Instant::now()))), if flush_at.is_some() => {
                Step::Flush
            }

            _ = sleep_opt(anim), if anim.is_some() => Step::Out(runtime.present(), None),
        };

        let (bytes, exit) = match step {
            Step::Out(bytes, exit) => (bytes, exit),
            Step::Flush => {
                if !pending.is_empty() && tokio::time::Instant::now() >= last_flush + FRAME {
                    let out = runtime.handle_batch(pending.drain(..));
                    last_flush = tokio::time::Instant::now();
                    out
                } else {
                    (Vec::new(), None)
                }
            }
        };

        spawn_effects(runtime.take_effects(), &tx);
        subs.sync(runtime.app().subscriptions());

        if !bytes.is_empty() {
            stdout.write_all(&bytes)?;
            stdout.flush()?;
        }
        if let Some(output) = exit {
            shutdown_mouse(&mut guard, &mut stdout, &mut events).await;
            return Ok(output);
        }
    }
}

/// Disable mouse capture and drain in-flight reports through the stream.
///
/// Between the exit and the terminal processing the disable, the terminal
/// keeps emitting reports — a fast wheel that triggered the exit has
/// dozens still in flight. Undrained, they land in the parent shell's
/// input as escape-sequence garbage (and have been seen to wedge fragile
/// emulators). The guard's own drop-time drain can't do this here: the
/// stream's reader thread owns crossterm's shared reader, so synchronous
/// `event::poll` sees nothing while the reports flow into the stream.
/// Bounded: quiet for 5ms or 50ms total.
///
/// The spray is contiguous, so the first non-mouse event ends the drain:
/// bytes not yet read stay in the tty for the shell. The cap is coarser
/// than one event, though — crossterm reads the tty in chunks (up to
/// 1KiB) and queues everything it parses, so whatever shared a chunk
/// with that first non-mouse event is consumed along with it and cannot
/// be handed back (ttys have no peek, and TIOCSTI is disabled on modern
/// kernels). A true byte-granular drain requires owning the input path
/// instead of reading through crossterm.
async fn shutdown_mouse(
    guard: &mut RawModeGuard,
    stdout: &mut impl Write,
    events: &mut crossterm::event::EventStream,
) {
    if !guard.take_mouse_capture() {
        return;
    }
    let _ = crossterm::execute!(stdout, crossterm::event::DisableMouseCapture);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(50);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(
            Duration::from_millis(5),
            poll_fn(|cx| Pin::new(&mut *events).poll_next(cx)),
        )
        .await
        {
            Ok(Some(Ok(crossterm::event::Event::Mouse(_)))) => {}
            // Quiet, a non-mouse event, a read error, or stream end: done.
            _ => break,
        }
    }
}

/// The pre-loop exit (`App::init` exited): no stream exists yet, so the
/// synchronous drain works — no reader thread is holding the source.
/// Stops at the first non-mouse event, like [`shutdown_mouse`].
fn shutdown_mouse_sync(guard: &mut RawModeGuard, stdout: &mut impl Write) {
    if !guard.take_mouse_capture() {
        return;
    }
    let _ = crossterm::execute!(stdout, crossterm::event::DisableMouseCapture);
    let deadline = std::time::Instant::now() + Duration::from_millis(50);
    while std::time::Instant::now() < deadline
        && matches!(crossterm::event::poll(Duration::from_millis(5)), Ok(true))
    {
        if !matches!(
            crossterm::event::read(),
            Ok(crossterm::event::Event::Mouse(_))
        ) {
            break;
        }
    }
}

async fn sleep_opt(duration: Option<Duration>) {
    match duration {
        Some(d) => tokio::time::sleep(d).await,
        // The branch is disabled by its `if` guard when None; never polled.
        None => std::future::pending().await,
    }
}

/// Execute effects on the tokio runtime, feeding produced messages into
/// `tx`. Public so custom loops (tests, embeddings) can drive effects the
/// same way [`run`] does.
pub fn spawn_effects<Msg: Send + 'static>(effects: Vec<Effect<Msg>>, tx: &UnboundedSender<Msg>) {
    for effect in effects {
        let Effect::Spawn { stream, cancel } = effect;
        drive_stream(stream, cancel, tx.clone());
    }
}

/// Forward a stream's items into `tx` until it ends or `cancel` fires.
fn drive_stream<Msg: Send + 'static>(
    mut stream: MsgStream<Msg>,
    cancel: Arc<Cancel>,
    tx: UnboundedSender<Msg>,
) {
    tokio::spawn(async move {
        loop {
            if cancel.is_cancelled() {
                break;
            }
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                item = poll_fn(|cx| stream.as_mut().poll_next(cx)) => match item {
                    Some(msg) => {
                        if tx.send(msg).is_err() {
                            break;
                        }
                    }
                    None => break,
                },
            }
        }
    });
}

/// The running side of [`Subscriptions`]: diffs each declared set against
/// what's live, starting new keys and cancelling absent ones. Public so
/// custom loops can drive subscriptions the same way [`run`] does.
///
/// Dropping this cancels everything it started.
pub struct ActiveSubscriptions<Msg> {
    running: HashMap<String, RunningSub>,
    tx: UnboundedSender<Msg>,
}

struct RunningSub {
    cancel: Arc<Cancel>,
    fingerprint: Fingerprint,
}

#[derive(PartialEq, Eq)]
enum Fingerprint {
    Every(Duration),
    /// Streams are opaque: same key = same subscription.
    Stream,
}

/// What a [`ActiveSubscriptions::sync`] changed — for logging and tests.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub started: Vec<String>,
    pub stopped: Vec<String>,
}

impl<Msg: Send + 'static> ActiveSubscriptions<Msg> {
    pub fn new(tx: UnboundedSender<Msg>) -> Self {
        Self {
            running: HashMap::new(),
            tx,
        }
    }

    /// Reconcile the declared set with what's running. An `every` whose
    /// interval changed restarts; a `stream` under an unchanged key keeps
    /// running untouched.
    pub fn sync(&mut self, declared: Subscriptions<Msg>) -> SyncReport {
        let mut report = SyncReport::default();
        let mut seen: Vec<String> = Vec::new();

        for (key, kind) in declared.entries {
            seen.push(key.clone());
            let fingerprint = match &kind {
                SubKind::Every { interval, .. } => Fingerprint::Every(*interval),
                SubKind::Stream { .. } => Fingerprint::Stream,
            };

            match self.running.get(&key) {
                Some(running) if running.fingerprint == fingerprint => {}
                Some(_) => {
                    // Same key, different shape: restart.
                    self.stop(&key);
                    report.stopped.push(key.clone());
                    self.start(&key, kind, fingerprint);
                    report.started.push(key);
                }
                None => {
                    self.start(&key, kind, fingerprint);
                    report.started.push(key);
                }
            }
        }

        let absent: Vec<String> = self
            .running
            .keys()
            .filter(|k| !seen.contains(k))
            .cloned()
            .collect();
        for key in absent {
            self.stop(&key);
            report.stopped.push(key);
        }

        report
    }

    fn start(&mut self, key: &str, kind: SubKind<Msg>, fingerprint: Fingerprint) {
        let cancel = Arc::new(Cancel::new());

        match kind {
            SubKind::Every { interval, make } => {
                let tx = self.tx.clone();
                let cancel_task = Arc::clone(&cancel);
                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            biased;
                            _ = cancel_task.cancelled() => break,
                            _ = tokio::time::sleep(interval) => {
                                if tx.send(make()).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
            SubKind::Stream { make } => {
                drive_stream(make(), Arc::clone(&cancel), self.tx.clone());
            }
        }

        self.running.insert(
            key.to_string(),
            RunningSub {
                cancel,
                fingerprint,
            },
        );
    }

    fn stop(&mut self, key: &str) {
        if let Some(running) = self.running.remove(key) {
            running.cancel.cancel();
        }
    }
}

impl<Msg> Drop for ActiveSubscriptions<Msg> {
    fn drop(&mut self) {
        for running in self.running.values() {
            running.cancel.cancel();
        }
    }
}
