---
title: Elements & Layout
description: Fluent builders, honest measurement, and custom elements
---

# Elements & Layout

Views are built from plain Rust with fluent builders — no macro DSL, no
custom syntax, full rust-analyzer support:

```rust
fn tail(&self) -> impl Element + '_ {
    col()
        .gap(1)
        .when(self.streaming, |c| c.child(spinner("Thinking…")))
        .child(
            panel(text_area(&self.input).track_focus(&self.input_focus))
                .title("Ask")
                .footer("[Enter] Send"),
        )
        .children(self.results.iter().take(4).map(result_row))
}
```

`col()` stacks children vertically with content-driven height; `row()`
splits width between `fixed(n, el)` and `fill(el)` cells. Conditionals are
ordinary Rust — `if`, `match`, and iterators all work — plus two
conveniences from the `Fluent` trait available on every builder:
`.when(cond, |x| …)` and `.when_some(option, |x, value| …)`.

## The Element trait

```rust
pub trait Element {
    /// Exact height at the given width. Must be cheap and honest.
    fn height(&self, width: u16) -> u16;
    fn render(&self, area: Rect, buf: &mut Buffer);
    /// Frame interval if self-animating (Spinner returns ~80ms).
    fn animated(&self) -> Option<Duration> { None }
    /// Hardware-cursor position, if this element wants it.
    fn cursor(&self, area: Rect) -> Option<(u16, u16)> { None }
}
```

Two things are deliberately absent. There's no message type parameter —
elements describe pixels, and all message emission lives in the
[keymap](../input/). And there's no probe-render escape hatch: `height` is
part of the contract, exact, called before `render`. For wrapped text the
`wrap` helpers in the engine make it a one-liner.

Note that `animated()` covers *view-only* time dependence — pixels that
change while the model doesn't, like a spinner glyph or an elapsed-seconds
label. Time that should change your model goes through
[subscriptions](../async/) as messages. Animation ticks are not messages.

## Heterogeneous children: `AnyElement`

Match arms and mixed lists need type erasure; `.any()` (from `ElementExt`)
boxes into `AnyElement<'a>`, which carries a lifetime so views can borrow
the model:

```rust
fn event_view(event: &UiEvent) -> AnyElement<'_> {
    match event {
        UiEvent::Text(t) => markdown(t.clone()).pad_left(2).any(),
        UiEvent::Tool(t) => tool_view(t).any(),
    }
}
```

## Writing view helper functions

A helper that borrows data and returns `impl Element` needs Rust's precise
capturing syntax when it *doesn't* capture the borrow:

```rust
// Clones what it needs: tell the compiler nothing is captured.
fn label(name: &str) -> impl Element + use<> {
    text(name.to_string())
}

// Borrows into the element: the default capture is what you want.
fn preview(source: &str) -> impl Element + '_ {
    text(source)
}
```

Under edition 2024, `impl Trait` captures all input lifetimes by default;
`+ use<>` opts out. If you hit "borrowed data escapes" on a helper that
only clones, this is almost always the fix.

## Custom elements

Implement the trait directly; it's small on purpose. Two conventions from
the built-ins worth copying:

**Same-frame caching for expensive work.** Elements are values rebuilt
every frame, but *within* a frame, `height` runs more than once (the tail
measure, then container placement during render) and `render` follows. An
element that parses or lays out something non-trivial should do that work
once per lifetime with a `RefCell` cache — the built-in `markdown()` does
exactly this, and it needs no invalidation story because the element dies
with the frame:

```rust
struct Fancy {
    source: String,
    parsed: RefCell<Option<Layout>>,   // filled on first use, per frame
}
```

**Respect clipping in `cursor`.** If a container hands you less height than
you measured, `render` truncates — `cursor` must not report positions in
the truncated region. (Containers do this for you; it matters when you
write one.)

## Wrapping Ratatui widgets

Any Ratatui `Widget` adapts in a few lines: `height` from whatever
measurement the widget offers, `render` delegating to its `Widget` impl.
Widgets that measure through `&mut self` (tui-textarea's `measure`, for
example) sit behind the same `RefCell` pattern. See
[components](../components/) for a worked example.
