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

    let _guard = RawModeGuard::enable(options.keyboard)?;

    let bytes = runtime.present();
    stdout.write_all(&bytes)?;
    stdout.flush()?;

    let mut subs = ActiveSubscriptions::new(tx.clone());
    subs.sync(runtime.app().subscriptions());

    let mut events = crossterm::event::EventStream::new();

    loop {
        let anim = runtime.animation_interval();

        let (bytes, exit) = tokio::select! {
            biased;

            maybe_event = poll_fn(|cx| Pin::new(&mut events).poll_next(cx)) => {
                use crossterm::event::{Event, KeyEventKind};
                match maybe_event {
                    Some(Ok(Event::Key(k)))
                        if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                    {
                        runtime.handle(InputEvent::Key(k))
                    }
                    Some(Ok(Event::Paste(s))) => runtime.handle(InputEvent::Paste(s)),
                    Some(Ok(Event::Resize(w, h))) => (runtime.resize(w, h), None),
                    Some(Ok(_)) => (Vec::new(), None),
                    Some(Err(e)) => return Err(e),
                    // Terminal input ended (stdin closed): exit cleanly.
                    None => (Vec::new(), Some(A::Output::default())),
                }
            }

            Some(msg) = rx.recv() => runtime.process(msg),

            _ = sleep_opt(anim), if anim.is_some() => (runtime.present(), None),
        };

        spawn_effects(runtime.take_effects(), &tx);
        subs.sync(runtime.app().subscriptions());

        if !bytes.is_empty() {
            stdout.write_all(&bytes)?;
            stdout.flush()?;
        }
        if let Some(output) = exit {
            return Ok(output);
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
