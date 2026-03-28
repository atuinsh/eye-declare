---
title: Components
description: Building custom components with the Component trait
---

# Components

Components are the building blocks of an eye-declare UI. Every piece of your interface — from a single line of styled text to a complex multi-part layout — is a component.

## The Component trait

A component that renders directly implements `render()`:

```rust
use eye_declare::Component;
use ratatui_core::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};
use ratatui_widgets::paragraph::Paragraph;

#[derive(Default)]
struct Badge {
    pub label: String,
    pub color: Color,
}

impl Component for Badge {
    type State = (); // no internal state needed

    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        let style = Style::default().fg(self.color);
        Paragraph::new(Span::styled(&self.label, style)).render(area, buf);
    }
}
```

Then use it:

```rust
element! {
    Badge(label: "Online".into(), color: Color::Green)
}
```

## Props vs. State

eye-declare separates data into two categories:

**Props** are fields on `&self` — immutable data set by the parent:

```rust
struct StatusBadge {
    pub label: String,    // prop
    pub color: Color,     // prop
}
```

**State** is the associated `type State` — mutable data managed by the framework:

```rust
#[derive(Default)]
struct CounterState {
    count: u32,
}

struct Counter;

impl Component for Counter {
    type State = CounterState;
    // ...
}
```

State is automatically wrapped in `Tracked<S>`, which detects mutations and marks the component dirty for re-rendering. You never need to manage this manually — mutations through event handlers and lifecycle hooks trigger it automatically.

## Initial state

By default, state is initialized with `State::default()`. Override `initial_state()` to customize:

```rust
impl Component for Timer {
    type State = TimerState;

    fn initial_state(&self) -> Option<TimerState> {
        Some(TimerState {
            started_at: Instant::now(),
            elapsed: 0,
        })
    }

    // ...
}
```

## Rendering

`render()` receives a `Rect` (the allocated area) and a `Buffer` (the drawing surface). Use any Ratatui `Widget` to draw:

```rust
fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
    let text = format!("Count: {}", state.count);
    Paragraph::new(text).render(area, buf);
}
```

The framework only calls `render()` when the component is dirty (state changed) or the layout changed. You don't need to optimize for no-op renders.

### Height measurement

The framework measures each component's height automatically by examining the render output — you don't need to calculate it yourself. Most components just implement `render()` and the framework figures out the rest.

The exception is components that **fill their given area** rather than rendering a fixed amount of content (e.g., a bordered input box that stretches to fit). These should override `desired_height()` to declare their height explicitly:

```rust
fn desired_height(&self, _width: u16, _state: &Self::State) -> Option<u16> {
    Some(3) // border-top + content row + border-bottom
}
```

The default returns `None`, which means "measure from render output."

## Composite components

Components compose their visual output by overriding `view()`, which receives
slot children and returns an element tree:

```rust
impl Component for Card {
    type State = ();

    fn view(&self, _state: &(), children: Elements) -> Elements {
        let mut els = Elements::new();
        els.add_with_children(
            View {
                border: Some(BorderType::Rounded),
                title: Some(self.title.clone()),
                ..View::default()
            },
            children,
        );
        els
    }
}
```

The `children` parameter carries children provided externally (from the `element!` macro's brace syntax):

```rust
element! {
    Card(title: "My Card".into()) {
        "These children appear inside the card"
        Spinner(label: "Loading...")
    }
}
```

### Three composition patterns

1. **Pass through** (default) — `view()` returns `children` unchanged. Layout containers like `VStack` and `HStack` use this.

2. **Generate own tree** — `view()` ignores `children` and returns a custom `Elements`. A status badge generates its own Canvas rendering.

3. **Wrap children** — `view()` incorporates `children` into a larger tree. A `Card` component wraps children with borders via `View`.

## Accepting slot children

For your component to accept children in `element!`, it needs to implement `ChildCollector`. Use the `impl_slot_children!` macro:

```rust
#[derive(Default)]
struct Panel {
    pub title: String,
}

impl Component for Panel {
    type State = ();
    // view() default passes children through — no override needed
}

impl_slot_children!(Panel);

// Now this works:
element! {
    Panel(title: "Settings".into()) {
        "Option 1"
        "Option 2"
    }
}
```

Without `impl_slot_children!`, attempting to use brace children on your component will produce a compile error.

## Using `view()` for declarative composition

`view()` is the primary rendering method. Override it to compose your component's visual output as an element tree using `View` for chrome and `Canvas` for raw rendering:

```rust
#[derive(Default)]
struct Card {
    pub title: String,
}

impl Component for Card {
    type State = ();

    fn view(&self, _state: &(), children: Elements) -> Elements {
        let mut els = Elements::new();
        els.add_with_children(
            View {
                border: Some(BorderType::Rounded),
                title: Some(self.title.clone()),
                padding_left: Some(Cells(1)),
                padding_right: Some(Cells(1)),
                ..View::default()
            },
            children,
        );
        els
    }
}

impl_slot_children!(Card);
```

The default `view()` passes children through unchanged — layout containers like `VStack` and `HStack` use this behavior without overriding anything.

### Canvas for raw rendering

`Canvas` is a leaf component for raw buffer access, used inside `view()` when you need to render with ratatui widgets directly:

```rust
use eye_declare::Canvas;

let canvas = Canvas::new(|area: Rect, buf: &mut Buffer| {
    Paragraph::new("Hello!").render(area, buf);
});

// Optional: declare a fixed height to skip probe measurement
let canvas = Canvas::new(|area, buf| { /* ... */ }).with_height(3);
```

Canvas is added to element lists via `els.add(Canvas::new(...))`. It's useful for:
- Wrapping third-party ratatui widgets
- Custom rendering that built-in components don't cover
- Leaf components in a `view()` tree

### When to use `view()` vs `render()`

Use `view()` for components that **compose other components** — bordered cards, panels, layouts that wrap children. The framework handles measurement, insets, and reconciliation automatically.

Use `render()` for **leaf-level custom rendering** where you need precise control over buffer output, or where the overhead of element tree allocation isn't warranted (e.g., high-frequency animation components).

## Full Component trait reference

| Method | Required | Default | Purpose |
|--------|----------|---------|---------|
| `view()` | No | passthrough | Return element tree for this component |
| `render()` | No | no-op | Draw into buffer (primitive components only) |
| `handle_event_capture()` | No | `Ignored` | Intercept events during capture phase (root → focused) |
| `handle_event()` | No | `Ignored` | Handle events during bubble phase (focused → root) |
| `is_focusable()` | No | `false` | Participate in Tab cycling |
| `cursor_position()` | No | `None` | Position terminal cursor when focused |
| `initial_state()` | No | `State::default()` | Custom initial state |
| `desired_height()` | No | `None` | Height hint (primitive components only) |
| `content_inset()` | No | `Insets::ZERO` | Border/padding inset (primitive components only) |
| `layout()` | No | `Vertical` | Child layout direction |
| `width_constraint()` | No | `Fill` | Width in horizontal containers |
| `lifecycle()` | No | no-op | Declare effects (intervals, mount, etc.) |
