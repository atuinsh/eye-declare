---
title: Input, Keymaps & Focus
description: Keys as data, dispatch order, and focus the app owns
---

# Input, Keymaps & Focus

There are no event handlers in eye-declare. Keys resolve to messages
through a `Keymap` — a plain value your app rebuilds from the model on
every update:

```rust
fn keymap(&self) -> Keymap<Msg> {
    let mut km = keymap()
        .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Quit)
        .on(key(KeyCode::Esc), Msg::Cancel);

    if !self.busy() && !self.input.is_blank() {
        km = km.on(key(KeyCode::Enter), Msg::Submit);
    }

    km.fallthrough(&self.input_focus, Msg::Input)
}
```

Because the keymap is rebuilt from state, **conditional bindings are just
`if` statements**. Enter means "send" only when sending is meaningful; Tab
can mean "accept suggestion" only while a suggestion exists. Key conflicts
between modes become structurally impossible — there's never a stale
handler lying around from a state you've left.

## Dispatch order

For each key event, first match wins, in declaration order within each
tier:

1. **`on_override`** — fires regardless of focus. For Ctrl+C-tier chords
   only.
2. **`in_scope(&handle, …)`** — active while that `FocusHandle` is focused.
3. **`on`** — global bindings.
4. **`fallthrough(&handle, mapper)`** — everything unclaimed (keys *and*
   pastes) becomes a message while the handle is focused. This is how a
   text input receives ordinary typing without the framework owning any
   editing logic.

One rule of thumb from building real apps on this: **if a binding's
condition ladder keeps growing, the policy belongs in `update`.** Prefer
`Esc → Msg::Cancel` with `update` deciding what cancelling means in the
current state, over an Esc binding that re-derives the state machine in the
keymap.

## Focus is data

```rust
let focus = Focus::new();
let input_focus = focus.handle();   // handles share one current-focus cell
input_focus.focus();
```

A `Focus` system hands out `FocusHandle`s that share a single
"currently focused" cell, so exactly one handle is focused at a time by
construction. Handles live in your model; `focus()`/`blur()` are ordinary
calls in `update`. "Press `/` to focus search" is one binding and one line.
There's no registry, no autofocus lifecycle, no Tab order unless you bind
Tab to a message that cycles.

## Composing keymaps

Sub-models bring their own keymaps; two combinators embed them:

```rust
// Re-target a child keymap's messages into the parent's Msg…
let child = SelectState::keymap().map(Msg::Select);
// …and append it. Within each tier, earlier declarations win, so a
// parent merging after its own bindings keeps priority on contested keys.
km = km.on(key(KeyCode::Enter), Msg::Confirm).merge(child);
```

See [components](../components/) for the full sub-model pattern.

## Keyboard protocol

Some chords — Shift+Enter above all — are indistinguishable in the legacy
terminal keyboard protocol. Request the kitty protocol where available:

```rust
let options = RunOptions::default().keyboard(KeyboardProtocol::Enhanced);
driver_tokio::run_with(app, options).await?;
```

`Enhanced` silently falls back to legacy on terminals without support, so
bind a fallback chord (Ctrl+J is the convention) alongside Shift+Enter.
