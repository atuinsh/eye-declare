---
title: Introduction
description: Inline UIs are timelines, not trees
---

# Introduction

Most terminal UI libraries model a screen: a tree of components that the
framework re-renders and reconciles as state changes. That model fits
full-screen apps. It fits inline apps — the kind that share the terminal with
your shell — badly, because an inline app's output isn't a screen at all.
It's a **timeline**: an append-only sequence of finished output, with a small
live region at the bottom that's still changing.

eye-declare is built on one observation about that shape:

> **Committed output is an effect. The live tail is a view.**

Writing a block of finished output toward scrollback is I/O — irreversible
and append-only, like `println!`. So in eye-declare, emitting a block happens
in your update function, as an effect (`ctx.push`). The live tail is the only
thing that behaves like a screen, so it's the only thing described by a view
function — re-run every frame, pure, and cheap because it's small.

## What this dissolves

Because blocks render exactly once and the tail re-renders wholesale, whole
categories of framework machinery have nothing to do and don't exist here:

- **No reconciliation, no keys.** There is no retained tree to diff your
  elements against. `col().children(items.iter().map(row_view))` needs no
  identity annotations because nothing is ever matched up across frames.
- **No dirty tracking.** The tail is rebuilt every frame unconditionally.
  Identical tails diff to zero bytes at the terminal layer; that's the
  optimization, and it needs nothing from you.
- **No framework-owned widget state.** Your model — the struct implementing
  `App` — is the only state. Widget state (a text area's contents, a
  select's cursor) is a plain field you own and mutate in `update`.
- **No hidden focus registry.** Focus is a value in your model
  (`FocusHandle`), and keys resolve to messages through a `Keymap` you
  rebuild from the model each update. What a key does is always derivable
  from your state, never from what the framework last focused.

## The shape of an app

If you've written Elm, iced, or Redux, this is that architecture with one
addition — the timeline:

```text
        terminal events ─┐
     messages from tasks ─┼─▶ update(&mut model, msg, ctx) ─▶ ctx.push(block)   effect: permanent output
      subscription ticks ─┘                                   ctx.spawn(stream) effect: async work
                                        │
                                        ▼
                          tail(&model) ─▶ diff ─▶ terminal    view: the live region
```

The model is a plain struct. `update` takes `&mut self`; `tail` takes
`&self`. The borrow checker enforces the discipline for free.

## What it's not

eye-declare renders inline only. If you want a full-screen, alternate-screen
application, use [Ratatui](https://ratatui.rs) directly — eye-declare is
built on Ratatui's primitives and hands you its `Buffer` and `Style` types,
but it deliberately does not do full-screen layout.

Continue to [installation](../installation/), or read
[the timeline](../../guide/the-timeline/) for the semantics that follow from
the design.
