---
title: eye-declare
description: Inline-first terminal UIs for Rust
---

# eye-declare

A library for building inline-first terminal UIs in Rust: interfaces that
live in your terminal's normal flow, where finished output scrolls into
native scrollback and only a small live region updates in place.

eye-declare is built for CLI tools, AI agents, and interactive prompts: the
programs where output accumulates and history should stay visible, exactly as
if the program had printed it. When an app needs the whole terminal instead,
the same model runs fullscreen on the
[alternate screen](/book/reference/runtime/#fullscreen-the-alt-screen).

## Rust Docs

For the rustdoc documentation, visit [https://docs.rs/eye_declare](https://docs.rs/eye_declare).

## Example

The whole design in one small fragment: a streaming agent turn. Content lives in
the live tail while it's changing; the moment it's finished, `ctx.push`
commits it to scrollback, like `println!`:

```rust
struct Agent {
    response: String,
    streaming: Option<Task>,
}

enum Msg {
    Delta(String),
    Done,
}

impl App for Agent {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            // Still changing: lives in the tail.
            Msg::Delta(word) => self.response.push_str(&word),
            // Finished: commit to scrollback.
            Msg::Done => {
                let reply = std::mem::take(&mut self.response);
                ctx.push(markdown(reply));
                self.streaming = None;
            }
        }
    }

    // The live region: a pure view of the model, rebuilt every frame.
    fn tail(&self) -> impl Element + '_ {
        col()
            .when(self.streaming.is_some(), |c| {
                c.child(spinner("Thinking…"))
            })
            .child(text(self.response.as_str()))
    }
}
```

Messages arrive from terminal input through a keymap and from async work
through `ctx.spawn` — both shown in the [quick start](/book/getting-started/quick-start/),
which builds a complete runnable app in about sixty lines.

## Examples

The repository ships runnable examples that double as learning material:

- [`echo`](https://github.com/atuinsh/eye-declare/blob/main/crates/eye_declare/examples/echo.rs) — the smallest useful app: type, Enter commits, Ctrl+C exits.
- [`stream`](https://github.com/atuinsh/eye-declare/blob/main/crates/eye_declare/examples/stream.rs) — a mini agent: a streaming turn with a spinner, Esc cancels.
- [`openrouter`](https://github.com/atuinsh/eye-declare/blob/main/crates/eye_declare/examples/openrouter.rs) — a real streaming AI chat TUI in one commented file.

## Where to go next

Start with the [introduction](/book/getting-started/introduction/) for the idea
behind the design, or jump to the [quick start](/book/getting-started/quick-start/)
to build a working app. The [guide](/book/guide/the-timeline/) covers the concepts
in depth; the [reference](/book/reference/widgets/) documents the widgets, runtime,
and migration path.
