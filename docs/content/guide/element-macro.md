---
title: The element! Macro
description: Full syntax reference for the element! macro
---

# The element! Macro

The `element!` macro is the primary way to build component trees in eye-declare. It provides a JSX-like syntax that compiles down to `Elements` builder calls.

## Basic syntax

```rust
use eye_declare::{element, Elements};

fn my_view(state: &MyState) -> Elements {
    element! {
        Text { "Hello, world!" }
    }
}
```

The macro returns an `Elements` value — a list of component descriptions that the framework uses to build and reconcile the component tree.

## Components with props

Props use struct-field syntax. For `#[props]` components, the macro generates builder calls with compile-time required field enforcement. For manual components, it uses struct init with `..Default::default()`:

```rust
element! {
    Spinner(label: "Loading...")
    Markdown(source: "# Hello")
}
```

For `#[props]` components, the macro generates builder calls:

```rust
let mut els = Elements::new();
els.add(Spinner::builder().label("Loading...").build());
els.add(Markdown::builder().source("# Hello").build());
els
```

For manual components that implement `Default`, the macro uses struct init:

```rust
els.add(MyComponent { field: value, ..Default::default() });
```

## Components with children

Curly braces after a component provide its children:

```rust
element! {
    VStack {
        "First item"
        "Second item"
        Spinner(label: "Working...")
    }
}
```

Children are collected into an `Elements` list and passed as slot children. With `#[component]`, use `children = Elements` to accept slot children. For manual components, implement `ChildCollector` (use the `impl_slot_children!` macro).

## Props and children together

```rust
element! {
    Card(title: "My Card".into()) {
        Spinner(label: "Loading...")
        "Some content"
    }
}
```

## String literals

Bare string literals are automatically wrapped in `Text`:

```rust
element! {
    "This becomes a Text"
    "So does this"
}
```

## Keys

The `key` prop gives a component a stable identity for reconciliation. It's separated from regular props with a special syntax:

```rust
element! {
    #(for item in &state.items {
        Markdown(key: item.id.clone(), source: item.text.clone())
    })
}
```

Keys are critical when rendering dynamic lists — without them, the framework matches components by position, which can cause state to "jump" between items when the list changes. See [Reconciliation](reconciliation.md) for details.

## Conditionals

Use `#(if ...)` for conditional rendering:

```rust
element! {
    #(if state.loading {
        Spinner(label: "Loading...")
    })

    #(if state.error.is_some() {
        "An error occurred"
    })
}
```

Pattern-matching conditionals with `if let`:

```rust
element! {
    #(if let Some(ref result) = state.result {
        Markdown(source: result.clone())
    })
}
```

When the condition is false, no component is emitted — the framework handles the absence during reconciliation (unmounting the component if it previously existed).

## Loops

Use `#(for ...)` for iterating over collections:

```rust
element! {
    #(for (i, msg) in state.messages.iter().enumerate() {
        Text(key: format!("msg-{i}")) {
            Span(text: msg.clone())
        }
    })
}
```

Always provide keys when rendering lists so that the framework can correctly track which items were added, removed, or reordered.

## Splicing

Use `#(expr)` to splice a pre-built `Elements` value inline:

```rust
fn footer(state: &AppState) -> Elements {
    element! {
        "---"
        Text {
            Span(text: format!("{} items", state.items.len()))
        }
    }
}

fn main_view(state: &AppState) -> Elements {
    element! {
        "Header"
        #(for item in &state.items {
            Markdown(key: item.id.clone(), source: item.text.clone())
        })
        #(footer(state))
    }
}
```

This is useful for composing view functions — you can break your UI into smaller functions that each return `Elements`, then splice them together.

## Syntax reference

| Syntax | Description |
|--------|-------------|
| `Component(prop: value)` | Component with props (struct field init) |
| `Component { ... }` | Component with children |
| `Component(props) { children }` | Both props and children |
| `"text"` | String literal — auto-wrapped as `Text` |
| `key: expr` | Special prop for stable identity across rebuilds |
| `#(if cond { ... })` | Conditional children |
| `#(if let pat = expr { ... })` | Pattern-matching conditional |
| `#(for pat in iter { ... })` | Loop children |
| `#(expr)` | Splice a pre-built `Elements` value inline |

## Without the macro

You can build `Elements` imperatively if you prefer:

```rust
fn my_view(state: &MyState) -> Elements {
    let mut els = Elements::new();

    for msg in &state.messages {
        els.add(Markdown::new(&msg.text)).key(msg.id.clone());
    }

    if state.loading {
        els.add(Spinner::new("Loading...")).key("spinner");
    }

    els
}
```

The macro and the imperative API produce identical results — use whichever is clearer for your use case.
