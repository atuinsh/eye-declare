---
title: Layout
description: How eye-declare positions components vertically and horizontally
---

# Layout

eye-declare uses a simple, predictable layout model: vertical stacking is the default, with horizontal layout available via `HStack`.

## Vertical layout

By default, children stack top-to-bottom. Each child receives the full parent width and its measured height:

```rust
element! {
    "First line"
    "Second line"
    Spinner(label: "Third line with spinner")
}
```

```
┌──────────────────────────────┐
│ First line                   │  ← height = 1
│ Second line                  │  ← height = 1
│ ⠋ Third line with spinner    │  ← height = 1
└──────────────────────────────┘
```

`VStack` makes this explicit, but it's the default behavior — you don't need `VStack` unless you want a named group:

```rust
element! {
    VStack {
        "These children"
        "stack vertically"
    }
}
```

## Horizontal layout

`HStack` lays children out left-to-right. Use `Column` to control how children claim horizontal space:

```rust
use eye_declare::WidthConstraint::Fixed;

element! {
    HStack {
        Column(width: Fixed(20)) {
            "Sidebar (20 cols)"
        }
        Column {
            "Main content (remaining space)"
        }
    }
}
```

```
┌────────────────────┬───────────────────────────────────────┐
│ Sidebar (20 cols)  │ Main content (remaining space)        │
└────────────────────┴───────────────────────────────────────┘
```

### Width constraints

| Constraint | Behavior |
|-----------|----------|
| `Fixed(n)` | Reserve exactly `n` columns |
| `Fill` (default) | Split remaining space equally among all `Fill` siblings |

If an `HStack` has three columns — `Fixed(10)`, `Fill`, `Fill` — and the terminal is 80 columns wide, the two `Fill` columns each get 35 columns.

### Height in horizontal layout

The `HStack` height is the maximum measured height of its children. Shorter children are top-aligned within the row.

## Content insets

Components that draw borders or padding declare `content_inset()` to reserve space around their children. In practice, most user components use the built-in `View` component for borders and padding rather than implementing `content_inset()` directly:

```rust
#[component(props = Card, children = Elements)]
fn card(props: &Card, children: Elements) -> Elements {
    element! {
        View(border: BorderType::Rounded, padding: 1u16) {
            #(children)
        }
    }
}
```

For primitive components that need custom chrome, `content_inset()` is available on the `Component` trait:

```rust
impl Component for MyPrimitive {
    fn content_inset(&self, _state: &()) -> Insets {
        Insets::all(1)
    }

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        // Draw border in the full `area`
        draw_border(area, buf);
    }
}
```

The framework handles the math: children are laid out inside the inset area, while the component renders in the full area.

```
┌──────────────────────────┐  ← component renders here (full area)
│ ┌──────────────────────┐ │
│ │ children render here │ │  ← children get inset area
│ └──────────────────────┘ │
└──────────────────────────┘
```

### Insets API

```rust
Insets::ZERO                          // no insets (default)
Insets::all(1)                        // 1 cell on every side
Insets::symmetric(1, 2)               // vertical 1, horizontal 2
Insets::new().top(2).left(1).right(1) // builder pattern
```

## Nesting layouts

Vertical and horizontal layouts nest freely:

```rust
element! {
    "Header"
    HStack {
        Column(width: Fixed(3)) {
            Spinner(label: "".into())
        }
        Column {
            VStack {
                "Task name"
                "Task details"
            }
        }
    }
    "Footer"
}
```

```
Header
⠋  Task name
   Task details
Footer
```

## Width constraints on components

Components can declare their own width constraint. In `#[component]` functions, use the `use_width_constraint` hook:

```rust
hooks.use_width_constraint(WidthConstraint::Fixed(30));
```

This is equivalent to wrapping the component in a `Column(width: Fixed(30))` — but more convenient when the width is intrinsic to the component.
