---
title: Built-in Widgets
description: The element vocabulary that ships with eye-declare
---

# Built-in Widgets

Every builder returns an `Element`; all compose freely.

## Text

Word-wrapped styled text; newlines start new lines. `.style(…)` styles the
current span, `.span(content, style)` appends further spans:

```rust
text("✓ ").style(green).span("done", Style::default())
```

## Markdown (requires 'markdown' feature enabled)

CommonMark via pulldown-cmark: headings, emphasis, inline code, fenced code
blocks, lists. Word-wraps at render width with exact height measurement,
and caches its parse within the frame. `.styles(MarkdownStyles { … })`
overrides the palette.

## Spinner

An animated activity indicator that requires zero app code to animate: it
reports `animated() → ~80ms` and the runtime ticks while it's visible.
The glyph derives from wall-clock time, so there's no state to store.
`.done(bool)` swaps the animation for a ✓ (`.hide_checkmark()` omits it),
`.label_style(…)` / `.spinner_style(…)` style the parts.

A label computed from wall-clock time (elapsed seconds, say) updates on the
spinner's own ticks — view-side time needs no subscription.

## Panel

Border-and-title chrome: `.title(…)` (top-left), `.title_right(…)`,
`.footer(…)` (bottom-right), `.border_style(…)`, `.title_style(…)`,
`.pad_x(cols)` for interior padding. Forwards its child's cursor through
the chrome, and suppresses it when there's no room to render.

## Text Area

The built-in multi-line input, strict-Elm style: contents and cursor live
in a `TextAreaState` in _your_ model; the element borrows it.

```rust
// model
input: TextAreaState,
// update — unclaimed keys and pastes route here via keymap fallthrough
Msg::Input(ev) => self.input.handle(&ev),
// view
text_area(&self.input)
    .placeholder("Type a message…")
    .track_focus(&self.input_focus)
    .max_height(6)
```

`TextAreaState` is grapheme-aware (combining marks, wide characters, emoji
clusters) and exposes `insert_str` / `insert_newline` / `take_text` /
`set_text` / `is_blank`. Soft wrap is on by default (`.wrap(false)` to
truncate); one wrap computation is the single source of truth for layout,
height, cursor position, and the scroll window. `handle` deliberately
ignores Ctrl/Alt chords and policy keys (Enter, Tab, Esc) — those belong
to your keymap.

`TextAreaState.handle` covers ordinary editing. If you want emacs
bindings, undo, and a kill ring, wrap
[tui-textarea](https://crates.io/crates/tui-textarea) instead — the
[components chapter](../../guide/components/) shows the adapter.

## Viewport

A fixed-height window over a list of lines showing the most recent content
— live command output, log tails. `.height(rows)`, `.style(…)`,
`.wrap(bool)`.

## Combinators

From `ElementExt` on any element: `.any()` (type-erase to
`AnyElement<'a>`), `.pad_left(cols)`, `.pad_top(rows)`. From `Fluent` on
any builder: `.when(cond, f)`, `.when_some(option, f)`.
