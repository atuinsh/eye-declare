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

| option     | values                                                                     | notes                                                                                                                                                                             |
| ---------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `keyboard` | `KeyboardProtocol::Legacy` (default) / `Enhanced` / `Custom { flags, probe }` | `Enhanced` requests the kitty protocol's disambiguated escape codes (Shift+Enter vs Enter), falling back to legacy silently where unsupported. `Custom` pushes an explicit flag set; see [input](../../guide/input/). |
| `screen`   | `ScreenMode::Inline` (default) / `AltScreen`                                | Where the app runs: the main screen (the timeline model), or the alternate screen for fullscreen apps.                                                                             |
| `mouse_capture` | `bool` (default `false`)                                               | Capture mouse events and deliver them as `InputEvent::Mouse` through the keymap fallthrough. Capture takes over the terminal's native selection, so request it only if you handle the events. |
| `persist_grace` | `Option<Duration>` (default `None`)                                    | Cap the teardown wait for `ctx.persist` work. `None` waits until it completes; set a grace period when a wedged resource must not hang the exit. See [async](../../guide/async/). |

The struct is `#[non_exhaustive]`; construct with `Default` and the
setters.

## Fullscreen: the alt screen

`ScreenMode::AltScreen` runs the app on the alternate screen — for
"whole terminal" TUIs rather than inline ones. What changes:

- The prior screen contents are restored at teardown, like any fullscreen
  program.
- Startup homes the cursor explicitly instead of querying the start
  column, and resizes clear-and-repaint instead of anchoring on a cursor
  report — fullscreen startup and resizes cost **zero** position-query
  round-trips.
- Size the tail to the terminal with [`App::on_resize`](#sizes-and-resizes):
  a fullscreen tail should fill exactly the height it is told about.
- `ctx.push` still works but is of limited use: committed blocks scroll
  away within the alt screen and vanish with it. The timeline model is an
  inline-mode idea.

## Sizes and resizes

`App::on_resize(width, height) -> Option<Msg>` turns the terminal size
into a model input. It is called with the initial size during
`Runtime::new` and with the new size after every resize, before the
accompanying repaint — return a message and the frame that follows is
laid out with the size already in the model. The default returns `None`:
apps whose tails size themselves from content never need it.

This is how a fixed-height or fullscreen tail tracks the terminal;
content-sized inline tails should ignore it and keep measuring naturally.

## Cursor shape

`App::cursor_style() -> CursorStyle` is re-derived from the model at
every present, like `keymap()`: return the shape for the current mode
(`SteadyBlock` in a vim-normal mode, `BlinkingBar` in insert, …) and the
engine emits the change — only the change — as DECSCUSR. There is no
teardown reset: the terminal keeps the last shape presented, so an app
that changes shapes should end on the one it means to leave behind
(usually `CursorStyle::DefaultUserShape`).

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
let (bytes, exit) = rt.resize_msg(w, h, cursor); // inline resize + on_resize delivery
let (bytes, exit) = rt.resize_screen(w, h);      // alt-screen resize + on_resize delivery
let effects = rt.take_effects();                 // spawned work, for the driver
let interval = rt.animation_interval();          // Some(_) while tail animates
```

(`resize`/`resize_anchored` remain for compatibility; they repaint but do
not deliver `App::on_resize`.)

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
    fn on_resize(&self, w: u16, h: u16) -> Option<Msg>; // optional: default None
    fn cursor_style(&self) -> CursorStyle;         // optional: default user shape
}
```

`Ctx` provides the effects: `push(element)`, `spawn(stream) -> Task`,
`perform(future) -> Task`, `exit(output)`. On exit the runtime reclaims
trailing blank rows and hands the cursor back to the shell directly below
your final output.
