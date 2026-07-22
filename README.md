# eye-declare

[![Crates.io Version](https://img.shields.io/crates/v/eye_declare)](https://crates.io/crates/eye_declare) [![docs.rs](https://img.shields.io/docsrs/eye_declare)](https://docs.rs/eye_declare)

Inline terminal UIs for Rust, built on [Ratatui](https://ratatui.rs).

Inline interfaces live in your terminal's normal flow, where finished output scrolls into native scrollback and only a small live region updates in place. Ideal for CLI tools, AI agents, and interactive prompts: programs where output accumulates and history should stay visible, exactly as if the program had printed it.

![Demo](https://github.com/BinaryMuse/eye-declare/blob/main/assets/demo.gif?raw=true)

## The idea

eye-declare models inline applications as a **timeline**: an append-only sequence of finished output with a small live region at the bottom.

Emitting a finished block is I/O, irreversible like `println!`, so it happens in your update function (`ctx.push`). The live tail is the only thing that behaves like a screen, so it's the only thing described by a view function — re-run every frame, pure, cheap because it's small.

If you've written Elm, iced, or Redux, the rest is familiar: your struct is the model, messages drive `update`, async work arrives as streams of messages, and cancellation is dropping a handle. There's no reconciliation, no keys, no dirty tracking, no framework-owned widget state, and no hidden focus registry, because a timeline needs none of them.

## Example

```rust
use eye_declare::{App, Ctx, Element, Fluent, Keymap, Task, col, key, keymap, markdown, panel, spinner, text_area};

struct Chat {
    reply: String,            // the streaming response
    request: Option<Task>,    // in-flight work; dropping it cancels
    input: TextAreaState,
    input_focus: FocusHandle,
}

impl App for Chat {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Submit => {
                let prompt = self.input.take_text();
                // ctx.push() commits as permanent, unchangeable output:
                ctx.push(user_turn(&prompt));
                // async work is a stream of `Msg`s:
                self.request = Some(ctx.spawn(stream(prompt)));
            }
            Msg::Chunk(delta) if self.request.is_some() => self.reply.push_str(&delta),
            Msg::Done => {
                self.request = None;
                ctx.push(markdown(std::mem::take(&mut self.reply)));
            }
            // Simply drop the task to cancel async work:
            Msg::Cancel => self.request = None,
            _ => {}
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col()
            .when(!self.reply.is_empty(), |c| c.child(markdown(self.reply.clone())))
            .when(self.request.is_some() && self.reply.is_empty(), |c| c.child(spinner("Thinking…")))
            .child(panel(text_area(&self.input).track_focus(&self.input_focus)).title("Ask"))
    }

    fn keymap(&self) -> Keymap<Msg> {
        let mut km = keymap().on(key(KeyCode::Esc), Msg::Cancel);
        if self.request.is_none() && !self.input.is_blank() {
            // Enter is only bound to Submit when no request is in progress
            // and the input box is not blank.
            km = km.on(key(KeyCode::Enter), Msg::Submit);
        }
        km.fallthrough(&self.input_focus, Msg::Input)
    }
}
```

For a complete, runnable example of a streaming AI chat against the OpenRouter API in one commented file, see [`examples/openrouter.rs`](crates/eye_declare/examples/openrouter.rs):

```sh
export OPENROUTER_API_KEY=sk-or-...
cargo run --release --example openrouter
```

## Highlights

- **Fluent builders, no macro DSL** — views are plain Rust (`col()`, `.when(…)`, iterators), with full rust-analyzer and `cargo fmt` support.
- **Strict-Elm state** — widget state (text areas, selects) lives in _your_ model as plain values. Everything is constructible and testable.
- **Keys are data** — a `Keymap` rebuilt from the model each update makes conditional bindings simple one-liners, and makes mode conflicts impossible.
- **Cancel-on-drop async** — `ctx.spawn` returns a `Task`; cancellation and replacement are field assignments.
- **Headless testing** — the runtime is sync at its core (events in, bytes out); whole apps run in tests against a real VTE terminal emulator.
- **Measured performance** — no dirty tracking, and none needed: ~780µs and ~2.3K allocations per frame with 10KB of live markdown; bursts of stream chunks coalesce into single frames.
- **The hard part is invisible** — scrollback-safe diffing, burst streaming, cursor discipline, resize handling, and DEC synchronized output live in a dedicated engine crate with a VTE-based test suite.

## Documentation

The [eye-declare book](https://eye-declare.rs) covers everything: the [introduction](https://eye-declare.rs/getting-started/introduction) explains the design; the [quick start](https://eye-declare.rs/getting-started/quick-start) builds a working app in sixty lines; guide chapters cover the timeline, elements, input, async, components-as-a-convention, testing, and performance. Migrating from 0.5? There's a [dedicated chapter](https://eye-declare.rs/reference/migration).

## Status

0.6.0 is a ground-up redesign of the library around the timeline architecture. The 0.5.x component-model API is frozen and will not receive further changes. Expect the 0.6 API to settle toward 1.0 as it accumulates mileage.

## License

MIT
