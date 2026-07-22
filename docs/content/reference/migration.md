---
title: Migrating from 0.5
description: From the component tree to the timeline
---

# Migrating from 0.5

0.6.0 is a redesign, not an increment. The scrollback engine — the part of
0.5 that was genuinely hard to get right — carries over intact underneath,
but the component model above it is replaced. There is no mechanical
migration; expect to restructure, and expect the result to be smaller —
adapter layers like event-channel bridges, view-state sync, and commit
detection by key parsing tend to delete outright.

## The conceptual change

0.5 modeled a component tree and bolted the timeline on (`freeze`,
`on_commit`, key-based commit detection). 0.6 inverts that:
[the timeline is the model](../../guide/the-timeline/), and committed
output is an effect you emit rather than a lifecycle the framework
detects. Most 0.5 machinery has no equivalent because the design makes it
unnecessary, not because it was dropped.

## Mapping

| 0.5 | 0.6 |
|---|---|
| `Application::builder().state(…).view(f)` | `impl App` — the struct is the state, `tail` is the view of the live region |
| `element!` macro | fluent builders (`col()`, `row()`, `.when(…)`) — [elements](../../guide/elements/) |
| `#[component]`, props, `Hooks` | plain functions and sub-model structs — [components](../../guide/components/) |
| `Tracked<T>` / dirty detection | gone; the tail re-renders every frame |
| keys (`key: "turn-3"`), reconciliation | gone; nothing is matched across frames |
| `on_commit` callbacks | `ctx.push` — you decide what's committed, when |
| `use_event` / capture & bubble phases | `Keymap` dispatch — [input](../../guide/input/) |
| `use_focusable` / `use_autofocus` | `FocusHandle` values in your model |
| `Handle` + cross-thread `update(…)` | messages: `ctx.spawn` streams feed `update` |
| `use_interval` / lifecycle hooks | `subscriptions()` / `App::init` |
| `Spinner` component state | `spinner()` element; animation is self-declared |
| `Viewport`, `Text`, `Markdown` components | same vocabulary as element builders |
| `InlineRenderer` (imperative API) | `Timeline` (`push` / `present` / `finalize`) |

## A worked translation

0.5, abridged:

```rust
// state lives in a Tracked-field struct, view reads it, events mutate
// it through the Handle from another thread
handle.update(|state| state.response.push_str(&token));
```

0.6:

```rust
// the stream IS the other thread; tokens arrive as messages
Msg::Token(t) => self.response.push_str(&t),
// …and the finished response is committed, not diffed away:
Msg::Done => ctx.push(markdown(std::mem::take(&mut self.response))),
```

## Porting strategy

Port outside-in, and let each piece land as plain data:

1. Start from your existing state type; it's probably already most of the
   model. Delete anything that mirrors framework state (focus flags,
   dirty markers, committed-count trackers).
2. Rewrite views as functions returning elements. This is mostly
   mechanical — the layout vocabulary survived.
3. Move every event handler's *decision* into `update` and its *trigger*
   into the keymap.
4. Turn each background-thread `Handle` writer into a stream passed to
   `ctx.spawn`.

If you can't move yet, 0.5.x remains on crates.io; it is frozen and will
receive no further changes.
