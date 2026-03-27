---
title: Layout
description: How eye-declare positions components vertically and horizontally
---

# Layout

eye-declare uses a simple, predictable layout model: vertical stacking is the default, with horizontal layout available via `HStack`.

## Vertical layout

By default, children stack top-to-bottom. Each child receives the full parent width and its `desired_height()`:

```rust
element! {
    "First line"
    "Second line"
    Spinner(label: "Third line with spinner")
}
```

```
┌──────────────────────────────┐
│ First line                   │  ← desired_height = 1
│ Second line                  │  ← desired_height = 1
│ ⠋ Third line with spinner    │  ← desired_height = 1
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

The `HStack` height is the maximum `desired_height()` of its children. Shorter children are top-aligned within the row.

## Content insets

Components that draw borders or padding declare `content_inset()` to reserve space around their children:

```rust
impl Component for Card {
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

## How desired_height works

The framework calls `desired_height(width, state)` on every component during layout:

- **Leaf components** return their actual height (e.g., a `TextBlock` with 3 wrapped lines returns `3`)
- **Container components** return `0` — the framework sums their children's heights plus any insets

The `width` parameter is the allocated width for that component. Use it to compute word-wrapped heights:

```rust
fn desired_height(&self, width: u16, state: &Self::State) -> u16 {
    let text = &state.content;
    let wrapped_lines = wrap_text(text, width as usize);
    wrapped_lines.len() as u16
}
```

## Width constraints on components

Components can declare their own width constraint by overriding `width_constraint()`:

```rust
impl Component for Sidebar {
    fn width_constraint(&self) -> WidthConstraint {
        WidthConstraint::Fixed(30)
    }
    // ...
}
```

This is equivalent to wrapping the component in a `Column(width: Fixed(30))` — but more convenient when the width is intrinsic to the component.
