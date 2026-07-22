---
title: Runtime & Drivers
description: run functions, options, and the headless core
---

# Runtime & Drivers

## The async driver

```rust
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let output = eye_declare::driver_tokio::run(app).await?;
    // or, with options:
    let options = RunOptions::default().keyboard(KeyboardProtocol::Enhanced);
    let output = eye_declare::driver_tokio::run_with(app, options).await?;
    Ok(())
}
```

Multiplexes terminal events, messages from spawned tasks, animation ticks,
and subscriptions. Queued messages are drained in batches — one frame per
burst. Returns the app's `Output` after `ctx.exit(…)`; returns
`Output::default()` if stdin closes.

## The sync loop

`eye_declare::run(app)` / `run_with(app, options)` — a blocking loop for
keyboard-only apps with no executor dependency (build with
`default-features = false` if you want to guarantee that). Apps that spawn
tasks or declare subscriptions are rejected with an error directing you to
the tokio driver.

Both entry points enable raw mode and bracketed paste for the duration and
restore the terminal on exit, including panic unwind.

## RunOptions

| option     | values                                            | notes                                                                                                                                          |
| ---------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `keyboard` | `KeyboardProtocol::Legacy` (default) / `Enhanced` | `Enhanced` requests the kitty protocol's disambiguated escape codes (Shift+Enter vs Enter), falling back to legacy silently where unsupported. |

The struct is `#[non_exhaustive]`; construct with `Default` and the
setters.

## Runtime: the headless core

Both drivers are thin shells over `Runtime`, which is synchronous and does
no I/O — events in, escape bytes out:

```rust
let mut rt = Runtime::new(app, width, terminal_height);
let (bytes, exit) = rt.startup();               // App::init's output
let (bytes, exit) = rt.handle(input_event);     // keymap → update → present
let (bytes, exit) = rt.process(msg);            // update → present
let (bytes, exit) = rt.process_batch(msgs);     // many updates, one present
let bytes = rt.present();                        // re-present (animation tick)
let bytes = rt.resize(w, h);
let effects = rt.take_effects();                 // spawned work, for the driver
let interval = rt.animation_interval();          // Some(_) while tail animates
```

This is the surface for [headless testing](../../guide/testing/) and for
custom drivers (another executor, a remote terminal, an event-sourced
replay). A driver's obligations: write every byte returned, in order;
execute or reject effects; call `present` on `animation_interval`'s
cadence while `Some`.

## App lifecycle

```rust
impl App for MyApp {
    type Msg = Msg;
    type Output = ExitValue;                       // Default required

    fn init(&mut self, ctx: &mut Ctx<'_, Self>);   // optional: before first frame
    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>);
    fn tail(&self) -> impl Element + '_;
    fn keymap(&self) -> Keymap<Msg>;               // optional: default empty
    fn subscriptions(&self) -> Subscriptions<Msg>; // optional: default none
}
```

`Ctx` provides the effects: `push(element)`, `spawn(stream) -> Task`,
`perform(future) -> Task`, `exit(output)`. On exit the runtime reclaims
trailing blank rows and hands the cursor back to the shell directly below
your final output.
