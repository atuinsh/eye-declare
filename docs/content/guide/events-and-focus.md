---
title: Events and Focus
description: Keyboard/mouse event handling and focus management
---

# Events and Focus

eye-declare provides an event system for interactive TUIs. Components can handle keyboard and mouse events, participate in focus cycling, and control the terminal cursor.

## Event handling

Components handle events by implementing `handle_event()`:

```rust
impl Component for Input {
    type State = InputState;

    fn handle_event(
        &self,
        event: &crossterm::event::Event,
        state: &mut Self::State,
    ) -> EventResult {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
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
    }
}
```

### EventResult

- `EventResult::Consumed` — the event was handled; stop propagation
- `EventResult::Ignored` — pass the event to the parent component

Events bubble up the tree: the focused component gets the event first, and if it returns `Ignored`, the parent gets a chance, and so on.

State mutations through `&mut State` automatically mark the component dirty — you don't need to signal re-renders manually.

## Focus

Components opt into focus by returning `true` from `is_focusable()`:

```rust
fn is_focusable(&self, _state: &Self::State) -> bool {
    true
}
```

### Tab cycling

In interactive mode (`run_loop()` or `run_interactive()`), Tab and Shift+Tab cycle focus through all focusable components in depth-first tree order.

### Autofocus

Components can request focus when they mount:

```rust
fn lifecycle(&self, hooks: &mut Hooks<Self::State>, _state: &Self::State) {
    hooks.use_autofocus();
}
```

### Programmatic focus

With the imperative API, set focus directly:

```rust
let input_id = renderer.push(Input);
renderer.set_focus(input_id);
```

## Cursor position

Focused components can position the terminal's hardware cursor (the blinking cursor):

```rust
fn cursor_position(&self, area: Rect, state: &Self::State) -> Option<(u16, u16)> {
    // Position cursor at the text insertion point
    let col = state.cursor as u16;
    let row = 0;
    Some((col, row))
}
```

Coordinates are relative to the component's render area (not absolute terminal coordinates). Return `None` to hide the cursor.

## Interactive modes

eye-declare offers two ways to handle events:

### Component-driven: run_loop()

Events are delivered to the focused component automatically:

```rust
app.run_loop().await?;
```

The framework enters raw mode, handles Tab cycling, and routes events through the component tree. Components handle their own input via `handle_event()`.

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

Here's a complete focusable input component from the `interactive` example:

```rust
struct Input;

#[derive(Default)]
struct InputState {
    text: String,
    cursor: usize,
    label: String,
}

impl Component for Input {
    type State = InputState;

    fn is_focusable(&self, _state: &Self::State) -> bool {
        true
    }

    fn cursor_position(&self, area: Rect, state: &Self::State) -> Option<(u16, u16)> {
        let label_width = state.label.len() as u16 + 2; // ": "
        let col = label_width + state.cursor as u16;
        Some((col, 0))
    }

    fn handle_event(&self, event: &Event, state: &mut Self::State) -> EventResult {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press, ..
        }) = event {
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
    }

    fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State) {
        let spans = vec![
            Span::styled(
                format!("{}: ", state.label),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(&state.text, Style::default().fg(Color::White)),
        ];
        Paragraph::new(Line::from(spans)).render(area, buf);
    }

    fn desired_height(&self, _width: u16, _state: &Self::State) -> u16 {
        1
    }

    fn initial_state(&self) -> Option<InputState> {
        Some(InputState {
            text: String::new(),
            cursor: 0,
            label: "Input".into(),
        })
    }
}
```
