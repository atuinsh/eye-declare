---
title: Components as a Convention
description: Reusable UI without a framework interface
---

# Components as a Convention

eye-declare has no component trait, and that's a considered position, not a
gap. A reusable piece of UI here is a *convention* — a bundle of ordinary
things:

- a **state struct** (plain value, lives in the parent's model),
- optionally a **`Msg` enum and an `update` method**,
- optionally a **keymap**,
- a **view function** that borrows the state.

```rust
// A reusable prompt box. No trait to implement, nothing to register.
pub struct PromptBox {
    pub editor: TextAreaState,
    pub focus: FocusHandle,
}

#[derive(Clone)]
pub enum PromptMsg {
    Edit(InputEvent),
    Submit,
}

impl PromptBox {
    pub fn update(&mut self, msg: PromptMsg) {
        match msg {
            PromptMsg::Edit(ev) => self.editor.handle(&ev),
            // Submit is *policy* — the parent claims it (below).
            PromptMsg::Submit => {}
        }
    }

    pub fn keymap(&self) -> Keymap<PromptMsg> {
        keymap()
            .in_scope(&self.focus, key(KeyCode::Enter), PromptMsg::Submit)
            .fallthrough(&self.focus, PromptMsg::Edit)
    }

    pub fn view(&self) -> impl Element + '_ {
        panel(text_area(&self.editor).track_focus(&self.focus)).title("Ask")
    }
}
```

## Embedding

The parent wraps the child's messages in its own enum and re-targets with
`map`; keymaps compose with `merge`:

```rust
enum Msg { Prompt(PromptMsg), /* … */ }

fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
    match msg {
        // The parent claims the child's policy message…
        Msg::Prompt(PromptMsg::Submit) => {
            let text = self.prompt.editor.take_text();
            self.send(text, ctx);
        }
        // …and forwards the rest.
        Msg::Prompt(m) => self.prompt.update(m),
    }
}

fn keymap(&self) -> Keymap<Msg> {
    my_bindings().merge(self.prompt.keymap().map(Msg::Prompt))
}
```

"Parent claims policy" is the load-bearing idea: the child knows *that*
Enter means submit; only the parent knows *what submitting does*. Pattern
matching on the wrapped message is the entire mechanism — no output-message
types, no callback props, no event bubbling.

## Why not a component trait?

The ecosystems that went furthest down the component-object road walked it
back. Elm removed signals-and-components in favor of "just functions" in
0.17, and its guidance since has been that nesting Elm-architecture
triples adds indirection without adding capability. iced deprecated its
`Component` trait for the same reason. The failure mode is consistent:
component interfaces force parent↔child message plumbing into the
framework, where it becomes generic and opaque, when a `match` in the
parent's update expresses it directly.

The convention keeps everything first-class Rust: sub-model state is
testable by constructing the struct, its keymap is inspectable data, and
composition is enum-wrapping you can read.

## Two supporting patterns

**Wrapping stateful Ratatui widgets.** A widget like
[tui-textarea](https://crates.io/crates/tui-textarea) is already a plain
value — store it in your model directly and feed it events in `update`.
The view side is a small `Element` adapter; if the widget measures through
`&mut self`, a `RefCell` bridges the gap (interior mutability for a cache
is fine — see [elements](../elements/)):

```rust
struct Editor<'a>(&'a RefCell<TextArea<'static>>);

impl Element for Editor<'_> {
    fn height(&self, width: u16) -> u16 {
        self.0.borrow_mut().measure(width).preferred_rows
    }
    fn render(&self, area: Rect, buf: &mut Buffer) {
        (&*self.0.borrow()).render(area, buf);
    }
}
```

**The resource actor.** IO that must serialize — a database session whose
writes must apply in order, say — gets a single owning task fed by a
channel. `update` sends jobs; the actor applies them in channel order. This
is the Elm-shaped answer to `&mut`-method resources generally: own the
resource in one place, talk to it in messages.
