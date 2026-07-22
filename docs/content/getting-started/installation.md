---
title: Installation
description: Adding eye-declare to your project
---

# Installation

```sh
cargo add eye_declare
```

The default features suit most applications:

| Feature    | Default | What it adds                                                                                                                        |
| ---------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `tokio`    | ✓       | The async driver (`driver_tokio::run`) — spawned tasks, subscriptions, and the event loop most apps want. Requires a tokio runtime. |
| `markdown` | ✓       | The `markdown()` element (via pulldown-cmark).                                                                                      |

With `default-features = false` you get the synchronous core: the full
element/keymap/timeline stack and the blocking `run()` loop, with no
executor dependency. Apps built that way can't use `ctx.spawn`,
`ctx.perform`, or subscriptions — the sync loop rejects them with an
error explaining exactly that.

eye-declare targets Rust edition 2024 and the current stable toolchain.

## Related crates

- **`eye_declare_engine`** — the low-level terminal-sync engine (frame
  diffing, scrollback streaming, cursor discipline). You don't depend on it
  directly, with one exception: its `TestTerminal` (behind the `test-util`
  feature) is the headless VTE terminal used for
  [testing whole apps](../../guide/testing/).
