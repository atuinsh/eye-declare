//! The tokio driver: wraps [`Runtime`] in an async loop that multiplexes
//! terminal events, messages from spawned work, and animation ticks.
//!
//! The core crate stays executor-agnostic — this module is the only place
//! tokio appears (feature `tokio`, on by default). A custom driver for
//! another executor needs exactly what this one uses: `Runtime` (events in,
//! bytes out), [`Runtime::take_effects`], and [`Effect`]'s stream + cancel
//! pair.

use std::future::poll_fn;
use std::io::{self, Write};
use std::time::Duration;

use futures_core::Stream;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::app::App;
use crate::input::InputEvent;
use crate::runtime::{RawModeGuard, Runtime};
use crate::task::Effect;

/// Run an app on the attached terminal until it exits, executing spawned
/// streams on the ambient tokio runtime.
///
/// Raw mode + bracketed paste are enabled for the duration and restored on
/// exit (including panic unwind). If stdin closes, returns
/// `A::Output::default()`.
pub async fn run<A>(app: A) -> io::Result<A::Output>
where
    A: App,
    A::Msg: Clone + Send + 'static,
{
    let (width, height) = crossterm::terminal::size()?;
    let mut runtime = Runtime::new(app, width, height);
    let (tx, mut rx) = unbounded_channel::<A::Msg>();
    let mut stdout = io::stdout().lock();

    let _guard = RawModeGuard::enable()?;

    let bytes = runtime.present();
    stdout.write_all(&bytes)?;
    stdout.flush()?;

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
        let Effect::Spawn { mut stream, cancel } = effect;
        let tx = tx.clone();
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
}
