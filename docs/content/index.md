---
title: eye-declare
description: Inline terminal UIs for Rust — timeline-first, Elm-shaped
---

# eye-declare

A library for building inline terminal UIs in Rust — interfaces that live in
your terminal's normal flow, where finished output scrolls into native
scrollback and only a small live region updates in place.

eye-declare is built for CLI tools, AI agents, and interactive prompts: the
programs where output accumulates and history should stay visible, exactly as
if the program had printed it.

```rust
fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
    match msg {
        Msg::StreamDone => {
            // The finished reply becomes permanent terminal output…
            ctx.push(markdown(std::mem::take(&mut self.reply)));
        }
        // …
    }
}

fn tail(&self) -> impl Element + '_ {
    // …and the live region is a pure view of the model, rebuilt
    // every frame.
    col()
        .when(!self.reply.is_empty(), |c| c.child(markdown(self.reply.clone())))
        .child(panel(text_area(&self.input)).title("Ask"))
}
```

Start with the [introduction](/getting-started/introduction/) for the idea
behind the design, or jump to the [quick start](/getting-started/quick-start/)
to build a working app in about sixty lines. For a complete real-world
program, see the [OpenRouter chat example](https://github.com/atuinsh/eye-declare/blob/main/crates/eye_declare/examples/openrouter.rs)
— a streaming AI chat TUI in one commented file.
