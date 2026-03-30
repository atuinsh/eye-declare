---
title: Events and Focus
description: Keyboard/mouse event handling and focus management
---

# Events and Focus

eye-declare provides an event system for interactive TUIs. Components can handle keyboard and mouse events, participate in focus cycling, and control the terminal cursor.

## Event dispatch model

Events are dispatched in two phases, similar to the DOM:

1. **Capture** (root → focused): each component's capture handler is called, walking from the root toward the focused component. Returning `Consumed` stops propagation — the event never reaches the focused component or the bubble phase.

2. **Bubble** (focused → root): each component's event handler is called, starting at the focused component and walking up to the root. Returning `Consumed` stops propagation.

The focused component participates in both phases. Frozen components are skipped in both.

Use the capture phase for global shortcuts that should take priority over focused-component handling (e.g., Ctrl+N for "new item" regardless of what's focused). Use the bubble phase for normal input handling.

## Bubble phase: use_event

Components handle events during the bubble phase with the `use_event` hook:

```rust
#[props]
struct Input {
    #[default(String::new())]
    text: String,
}

#[derive(Default)]
struct InputState {
    text: String,
    cursor: usize,
}

#[component(props = Input, state = InputState)]
fn input(props: &Input, state: &InputState, hooks: &mut Hooks<Input, InputState>) -> Elements {
    hooks.use_event(|event, _props, state| {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
            let state = &mut **state;
            match code {
                KeyCode::Char(c) => {
                    state.text.insert(state.cursor, *c);
                    state.cursor += c.len_utf8();
                    EventResult::Consumed
                }
                KeyCode::Backspace => {
                    if state.cursor > 0 {
                        state.cursor -= 1;
                        state.text.remove(state.cursor);
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        } else {
            EventResult::Ignored
        }
    });

    // ... return element tree
}
```

### EventResult

- `EventResult::Consumed` — the event was handled; stop propagation
- `EventResult::Ignored` — propagation continues to the next node

During bubble, the focused component gets the event first, and if it returns `Ignored`, the parent gets a chance, and so on.

## Capture phase: use_event_capture

Components can intercept events *before* they reach the focused component with `use_event_capture`:

```rust
hooks.use_event_capture(|event, _props, state| {
    // Intercept Ctrl+N as a global shortcut
    if let Event::Key(KeyEvent {
        code: KeyCode::Char('n'),
        kind: KeyEventKind::Press,
        modifiers,
        ..
    }) = event
    {
        if modifiers.contains(KeyModifiers::CONTROL) {
            state.new_item();
            return EventResult::Consumed;
        }
    }
    EventResult::Ignored
});
```

This is useful for parent components that define global shortcuts — the focused child doesn't need to know about them or explicitly ignore them.

### Dirty tracking

State is wrapped in `Tracked` — only mutable access via `DerefMut` marks the component dirty for re-rendering. You don't need to signal re-renders manually.

**Reading state without marking dirty:** On `&mut Tracked<S>`, Rust's auto-deref uses `DerefMut` for all field access — even reads. Use `state.read()` to get a shared reference that doesn't set the dirty flag:

```rust
hooks.use_event(|event, _props, state| {
    // state.mode would trigger DerefMut — use read() for a clean read
    if state.read().mode == Mode::Insert {
        state.text.push('a');  // DerefMut → marks dirty
        EventResult::Consumed
    } else {
        EventResult::Ignored  // state stays clean
    }
});
```

This is especially useful for handlers that read state to call methods using interior mutability (e.g., sending on a channel) without triggering unnecessary re-renders.

When you know you will modify state, `let state = &mut **state;` unwraps `Tracked` in one `DerefMut` call, giving you direct `&mut State` access for the rest of the block:

```rust
KeyCode::Char(c) => {
    let state = &mut **state;  // one DerefMut, then plain field access
    state.text.insert(state.cursor, *c);
    state.cursor += c.len_utf8();
    EventResult::Consumed
}
```

## Focus

Components opt into focus with the `use_focusable` hook:

```rust
hooks.use_focusable(true);
```

### Tab cycling

In interactive mode (`run_loop()` or `run_interactive()`), Tab and Shift+Tab cycle focus through all focusable components in depth-first tree order.

### Autofocus

Components can request focus when they mount:

```rust
hooks.use_autofocus();
```

### Programmatic focus

With the imperative API, set focus directly:

```rust
let input_id = renderer.push(Input);
renderer.set_focus(input_id);
```

### Focus scopes

A focus scope confines Tab/Shift-Tab cycling to a subtree of the component tree. This is useful for modals, popups, and nested forms where Tab should not escape the container.

Mark a component as a focus scope boundary:

```rust
#[props]
struct Modal {
    title: String,
}

#[component(props = Modal, children = Elements)]
fn modal(props: &Modal, hooks: &mut Hooks<Modal, ()>, children: Elements) -> Elements {
    hooks.use_focus_scope();

    element! {
        View(border: BorderType::Rounded, title: props.title.clone()) {
            #(children)
        }
    }
}
```

**Scope behavior:**

- Tab/Shift-Tab cycle through focusable descendants within the scope only — they never escape to the parent tree
- Scopes nest: a form section within a modal can have its own scope, and the innermost scope takes precedence
- When a scope node is removed from the tree, focus is restored to whatever was focused before the scope captured it. If that node is gone too, the first focusable in the parent scope gets focus
- With 0 or 1 focusable nodes inside a scope, Tab falls through to normal event dispatch so components can handle it (e.g., inserting a tab character)
- Programmatic `set_focus()` ignores scope boundaries — it always works
- Event dispatch (capture/bubble) is unaffected by scopes

Children of `Modal` will have their own Tab cycle — focus won't leak to components outside the modal. When the modal is removed, focus returns to wherever it was before.

## Cursor position

Focused components can position the terminal's hardware cursor (the blinking cursor) with `use_cursor`:

```rust
hooks.use_cursor(|area: Rect, _props, state| {
    // Position cursor at the text insertion point
    let col = state.cursor as u16;
    let row = 0;
    Some((col, row))
});
```

Coordinates are relative to the component's render area (not absolute terminal coordinates). Return `None` to hide the cursor.

## Interactive modes

eye-declare offers two ways to handle events:

### Component-driven: run_loop()

Events are delivered to the focused component automatically:

```rust
app.run_loop().await?;
```

The framework enters raw mode, handles Tab cycling, and routes events through the component tree. Components handle their own input via `use_event`.

### Manual: run_interactive()

For direct access to terminal events:

```rust
app.run_interactive(|event, state| {
    match event {
        Event::Key(KeyEvent { code: KeyCode::Char('q'), .. }) => {
            ControlFlow::Exit
        }
        Event::Key(KeyEvent { code: KeyCode::Char(c), .. }) => {
            state.input.push(*c);
            ControlFlow::Continue
        }
        _ => ControlFlow::Continue,
    }
}).await?;
```

This gives you full control over event handling but bypasses the component event system. You mutate state directly in the closure, and the framework re-renders after each event.

### Non-interactive: run()

No event handling — the app runs until the handle is dropped and all effects stop:

```rust
app.run().await?;
```

## Example: interactive input

Here's a complete focusable input component:

```rust
#[derive(Default)]
struct InputState {
    text: String,
    cursor: usize,
}

#[props]
struct Input {
    #[default("Input".to_string())]
    label: String,
}

#[component(props = Input, state = InputState)]
fn input(props: &Input, state: &InputState, hooks: &mut Hooks<Input, InputState>) -> Elements {
    hooks.use_focusable(true);

    hooks.use_cursor({
        let label_width = props.label.len() as u16 + 2; // ": "
        move |_area: Rect, _props, state| {
            let col = label_width + state.cursor as u16;
            Some((col, 0))
        }
    });

    hooks.use_event(|event, _props, state| {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press, ..
        }) = event {
            let state = &mut **state;
            match code {
                KeyCode::Char(c) => {
                    state.text.insert(state.cursor, *c);
                    state.cursor += c.len_utf8();
                    EventResult::Consumed
                }
                KeyCode::Backspace if state.cursor > 0 => {
                    state.cursor -= 1;
                    state.text.remove(state.cursor);
                    EventResult::Consumed
                }
                KeyCode::Left if state.cursor > 0 => {
                    state.cursor -= 1;
                    EventResult::Consumed
                }
                KeyCode::Right if state.cursor < state.text.len() => {
                    state.cursor += 1;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        } else {
            EventResult::Ignored
        }
    });

    let label = props.label.clone();
    let text = state.text.clone();
    element! {
        Canvas(render_fn: move |area: Rect, buf: &mut Buffer| {
            let spans = vec![
                ratatui_core::text::Span::styled(
                    format!("{}: ", label),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                ratatui_core::text::Span::styled(&text, Style::default().fg(Color::White)),
            ];
            Paragraph::new(ratatui_core::text::Line::from(spans)).render(area, buf);
        })
    }
}
```

## Manual Component trait

For primitive components that need direct access to the `Component` trait methods, you can implement event handling and focus directly:

```rust
impl Component for MyPrimitive {
    type State = MyState;

    fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
        // bubble phase handler
        EventResult::Ignored
    }

    fn handle_event_capture(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
        // capture phase handler
        EventResult::Ignored
    }

    fn is_focusable(&self, _state: &Self::State) -> bool { true }

    fn cursor_position(&self, area: Rect, state: &Self::State) -> Option<(u16, u16)> {
        None
    }
}
```

Most user components should use `#[component]` with behavioral hooks instead.
