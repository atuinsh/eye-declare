---
title: Context
description: Passing data through the component tree without prop-drilling
---

# Context

The context system lets ancestor components provide typed values to their descendants without passing props through every intermediate component. This is the primary mechanism for connecting components to app-level services like event channels, configuration, or themes.

## Root-level context

Register values on the application builder — they're available to every component in the tree:

```rust
let (mut app, handle) = Application::builder()
    .state(MyState::default())
    .view(my_view)
    .with_context(event_sender)       // available to all components
    .with_context(AppConfig::new())   // multiple types supported
    .build()?;
```

Each call to `.with_context()` registers a value keyed by its concrete type. You can register as many different types as you need.

## Component-level context

Components provide context to their descendants via `provide_context` in their function body:

```rust
#[props]
struct ThemeProvider {
    theme: Theme,
}

#[component(props = ThemeProvider, children = Elements)]
fn theme_provider(props: &ThemeProvider, hooks: &mut Hooks<ThemeProvider, ()>, children: Elements) -> Elements {
    hooks.provide_context(props.theme.clone());
    children
}
```

Any descendant of `ThemeProvider` can read the theme value.

## Consuming context

Components read context values with `use_context`:

```rust
#[derive(Default)]
struct ButtonState {
    fg_color: Color,
    bg_color: Color,
}

#[props]
struct ThemedButton {
    label: String,
}

#[component(props = ThemedButton, state = ButtonState)]
fn themed_button(props: &ThemedButton, state: &ButtonState, hooks: &mut Hooks<ThemedButton, ButtonState>) -> Elements {
    hooks.use_context::<Theme>(|theme, _props, state| {
        if let Some(t) = theme {
            state.fg_color = t.primary_color;
            state.bg_color = t.background_color;
        }
    });

    // ... return element tree using state.fg_color, state.bg_color
}
```

The handler receives:
- `Option<&T>` — the context value, or `None` if no ancestor provides type `T`
- `&P` — the component's props
- `&mut Tracked<S>` — the component's mutable tracked state

The handler **always fires** — even when no ancestor provides the type. Use the `Option` to handle the absent case gracefully.

### Timing

Context consumers fire **after** the component function returns. This means:
1. The component function runs — registers effects, provides context, registers consumers
2. Context propagation happens
3. `use_context` handlers fire with the current context map

## Shadowing

Inner providers shadow outer providers of the same type within their subtree:

```rust
element! {
    ThemeProvider(theme: dark_theme()) {
        // Everything here sees dark_theme

        ThemeProvider(theme: light_theme()) {
            // Everything here sees light_theme
            ThemedButton(label: "Light button")
        }

        ThemedButton(label: "Dark button")
    }
}
```

The inner `ThemeProvider`'s `Theme` value shadows the outer one for all descendants within its subtree.

## Common patterns

### Event channel

Pass a channel sender through context so any component can dispatch events:

```rust
// At the root
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();

let (mut app, handle) = Application::builder()
    .state(state)
    .view(view)
    .with_context(tx)
    .build()?;

// In a component
#[derive(Default)]
struct SubmitState {
    event_tx: Option<UnboundedSender<AppEvent>>,
}

#[props]
struct SubmitButton {
    label: String,
}

#[component(props = SubmitButton, state = SubmitState)]
fn submit_button(props: &SubmitButton, state: &SubmitState, hooks: &mut Hooks<SubmitButton, SubmitState>) -> Elements {
    hooks.use_context::<UnboundedSender<AppEvent>>(|sender, _props, state| {
        state.event_tx = sender.cloned();
    });

    hooks.use_event(|event, _props, state| {
        if let Event::Key(KeyEvent { code: KeyCode::Enter, kind: KeyEventKind::Press, .. }) = event {
            if let Some(ref tx) = state.read().event_tx {
                tx.send(AppEvent::ButtonClicked).ok();
            }
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    });

    // ... return element tree
}
```

### Configuration

Share app-wide configuration without prop-drilling:

```rust
struct AppConfig {
    debug_mode: bool,
    max_items: usize,
}

// Register once
.with_context(AppConfig { debug_mode: true, max_items: 100 })

// Read anywhere
hooks.use_context::<AppConfig>(|config, _props, state| {
    if let Some(c) = config {
        state.show_debug = c.debug_mode;
    }
});
```

## Key points

- Context is keyed by Rust's `TypeId` — each concrete type can have at most one value in the context at a given tree level
- Context propagates top-down during reconciliation
- Inner providers shadow outer providers of the same type
- `use_context` handlers always fire (with `None` if no provider exists)
- Root-level context (`.with_context()`) is available everywhere; component-level context is scoped to the subtree
