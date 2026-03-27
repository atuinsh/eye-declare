---
title: Terminal Options
description: Configuring terminal protocols, Ctrl+C behavior, and keyboard modes
---

# Terminal Options

The `Application` builder supports configuring terminal protocols that affect how interactive modes behave.

## Options

```rust
Application::builder()
    .state(state)
    .view(view)
    .ctrl_c(CtrlCBehavior::Deliver)
    .keyboard_protocol(KeyboardProtocol::Enhanced)
    .bracketed_paste(true)
    .build()?;
```

### ctrl_c

Controls what happens when the user presses Ctrl+C.

| Value | Behavior |
|-------|----------|
| `CtrlCBehavior::Exit` (default) | The framework intercepts Ctrl+C and exits the event loop |
| `CtrlCBehavior::Deliver` | Ctrl+C is delivered to components as a normal key event |

Use `Deliver` when your application needs to handle Ctrl+C itself — for example, to cancel a running operation without exiting the entire application.

```rust
// In a component's handle_event:
Event::Key(KeyEvent {
    code: KeyCode::Char('c'),
    modifiers: KeyModifiers::CONTROL,
    ..
}) => {
    state.cancel_operation();
    EventResult::Consumed
}
```

### keyboard_protocol

Selects the terminal keyboard protocol.

| Value | Behavior |
|-------|----------|
| `KeyboardProtocol::Legacy` (default) | Standard terminal key handling |
| `KeyboardProtocol::Enhanced` | Kitty keyboard protocol for key disambiguation |

The enhanced (Kitty) protocol distinguishes between key press, repeat, and release events, and can disambiguate keys that look identical in legacy mode (e.g., Ctrl+I vs Tab, Ctrl+M vs Enter). Use this when you need precise keyboard handling.

Not all terminals support the Kitty protocol. Terminals that don't support it will silently ignore the request and use legacy handling.

### bracketed_paste

Controls whether pasted text is distinguished from typed text.

| Value | Behavior |
|-------|----------|
| `false` (default) | Pasted text arrives as individual key events |
| `true` | Pasted text arrives as `Event::Paste(String)` |

When enabled, the terminal wraps pasted content in escape sequences that the framework translates into a single `Event::Paste` event. This is useful for text input components that need to handle paste differently from typing (e.g., inserting all pasted text at once rather than character-by-character).

```rust
// In a component's handle_event:
Event::Paste(text) => {
    state.text.insert_str(state.cursor, text);
    state.cursor += text.len();
    EventResult::Consumed
}
```

## Summary

| Option | Default | Description |
|--------|---------|-------------|
| `ctrl_c` | `Exit` | `Exit` intercepts Ctrl+C; `Deliver` routes it to components |
| `keyboard_protocol` | `Legacy` | `Enhanced` enables Kitty protocol for key disambiguation |
| `bracketed_paste` | `false` | `true` delivers pasted text as `Event::Paste(String)` |

## When to use these options

For **non-interactive apps** (`run()`), these options have no effect since no terminal events are captured.

For **component-driven apps** (`run_loop()`), set these options based on your input handling needs.

For **manual interactive apps** (`run_interactive()`), set these options to match what your event handler expects.
