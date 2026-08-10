---
title: Async, Tasks & Subscriptions
description: Streams of messages, cancel-on-drop, and declarative recurring input
---

# Async, Tasks & Subscriptions

All async work enters the app the same way everything else does: as
messages through `update`.

## Spawning work

```rust
// A stream of messages — the LLM-turn shape:
self.request = Some(ctx.spawn(chat_stream(prompt)));

// A one-shot future:
ctx.perform(async move { Msg::Loaded(fetch().await) }).detach();
```

`ctx.spawn` takes a `Stream<Item = Msg>`; each item is fed back into
`update`. `ctx.perform` is the one-shot convenience. Both return a
[`Task`].

## Cancellation is drop

A `Task` cancels its work when dropped. Held in the model, this turns
cancellation into ordinary state manipulation:

```rust
struct Model { request: Option<Task>, /* … */ }

// Esc cancels the stream — that's the whole implementation:
Msg::Cancel => self.request = None,
```

No cancellation tokens, no flags, no channels. Replacing the `Task` with a
new one cancels the old work the same way, which also closes a subtle bug
class: a replaced request can't finish later and clobber shared state,
because it was dropped at whatever await point it had reached.

**Interrupt is not cancel.** Some work must report its outcome even when
the user stops it — a shell command whose exit status the app records, for
example. Spawn that work with `.detach()` (it outlives its handle) and
deliver the interrupt through the work's own channel; the final
outcome-message still arrives.

## Staleness

Cancellation is prompt but asynchronous: a message the work had already
queued can still be delivered after you dropped its `Task`. Treat validity
as a property of the model, not of the channel:

```rust
Msg::Chunk(delta) => {
    if self.request.is_some() {          // still streaming?
        self.reply.push_str(&delta);
    }                                     // otherwise: stale, ignore
}
```

## Durable work

Cancel-on-drop has an inverse hazard: work that must *not* die with the
app. A delete the user confirmed, a file save, a config write — if the
run loop exits while that future is in flight, a detached task dies with
the process and the write is silently lost.

`ctx.persist` is `perform` + `detach` for exactly that work: no `Task` is
returned (work that must complete is uncancellable by definition), and
the driver waits for every persist future at teardown, before the run
loop returns:

```rust
Msg::Delete(entry) => {
    self.entries.remove(&entry);           // optimistic UI
    let store = self.store.clone();
    ctx.persist(async move {
        store.delete(entry).await;         // survives an immediate exit
        Msg::DeleteDone
    });
}
```

The message still delivers normally while the app runs, and is dropped
after exit. By default the teardown wait is unbounded; cap it with
`RunOptions::persist_grace` when a wedged resource must not hang the
exit. Custom drivers get the same via `Runtime::persists()` — spawn the
final effects, then `.wait().await` the tracker.

## Handing values to spawned work

Moving a value *into* a spawned task couples its delivery to that task's
survival: if a keystroke replaces (cancels) the task before it runs, the
value is dropped with it. The classic symptom is a mode switch the UI
shows but the backend never received.

`Mailbox<T>` decouples them — a shared slot where `update` posts and
whichever task actually runs takes:

```rust
struct Model { pending_engine: Mailbox<Engine>, /* … */ }

// update: park the swap; any future query installs it.
Msg::CycleMode => self.pending_engine.post(new_engine),

// inside the spawned query, under whatever lock guards the backend:
if let Some(engine) = pending_engine.take() {
    backend.engine = engine;
}
```

A cancelled task never reaches `take`, so the value simply waits for the
next one. Posting replaces an undelivered value — the slot is
latest-wins state, not a queue.

## Subscriptions

Recurring inputs are declared, not managed. After every update the driver
diffs what you declare against what's running — new keys start, missing
keys cancel, changed intervals restart:

```rust
fn subscriptions(&self) -> Subscriptions<Msg> {
    Subscriptions::new()
        .when(self.session_active, |s| {
            s.every("poll", Duration::from_secs(30), || Msg::Poll)
        })
        .stream("fs-events", || watch_files())
}
```

"Poll while a session is active" is a `when` on model state; stopping the
poll is *not declaring it*. The same staleness note applies: one queued
tick may arrive after a subscription is removed.

If you came from Elm before 0.17: subscriptions are what signals became —
declarative descriptions of external input, derived from the model, with
the runtime owning the plumbing.

## Drivers

`driver_tokio::run(app)` (feature `tokio`, default) multiplexes terminal
events, task messages, animation ticks, and subscriptions. It batches:
when a burst of messages is queued — a fast LLM stream — they're processed
as one batch and presented as one frame, so you never need to debounce
streams yourself.

The synchronous `run(app)` drives keyboard-only apps with no executor; it
rejects apps that spawn or subscribe, with an error saying to use the
tokio driver. Custom drivers build on `Runtime` — see
[testing](../testing/), which uses the same surface.

## Startup work

`App::init` runs once before the first frame — push a banner, spawn an
initial request, process a command-line prompt:

```rust
fn init(&mut self, ctx: &mut Ctx<'_, Self>) {
    ctx.push(welcome_banner());
    if let Some(prompt) = self.initial_prompt.take() {
        self.submit(prompt, ctx);
    }
}
```
