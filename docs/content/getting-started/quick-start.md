---
title: Quick Start
description: A working inline app in about sixty lines
---

# Quick Start

The smallest useful app: type a line, press Enter to commit it to the
terminal, Ctrl+C to quit. Every eye-declare concept appears once.

```rust
use crossterm::event::KeyCode;
use eye_declare::{
    App, Ctx, Element, Fluent, Focus, FocusHandle, InputEvent, Keymap,
    col, key, keymap, run, text,
};
use ratatui_core::style::{Color, Style};

#[derive(Clone)]
enum Msg {
    Typed(char),
    Backspace,
    Submit,
    Quit,
    // Fallthrough must produce a message; this one means "not for us".
    Noop,
}

struct Echo {
    input_focus: FocusHandle,
    typed: String,
    submitted: usize,
}

impl App for Echo {
    type Msg = Msg;
    // What `run` returns when the app exits.
    type Output = usize;

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Typed(c) => self.typed.push(c),
            Msg::Backspace => {
                self.typed.pop();
            }
            Msg::Submit => {
                if !self.typed.is_empty() {
                    let line = std::mem::take(&mut self.typed);
                    self.submitted += 1;
                    // An effect, like println!: this block becomes
                    // permanent terminal output and never renders again.
                    ctx.push(
                        text("✓ ")
                            .style(Style::default().fg(Color::Green))
                            .span(line, Style::default()),
                    );
                }
            }
            Msg::Quit => ctx.exit(self.submitted),
            Msg::Noop => {}
        }
    }

    // The live region: a pure view of the model, rebuilt every frame.
    fn tail(&self) -> impl Element + '_ {
        col().child(
            text("> ")
                .style(Style::default().fg(Color::Cyan))
                .span(&*self.typed, Style::default()),
        )
    }

    // Keys are data, rebuilt from the model each update.
    fn keymap(&self) -> Keymap<Msg> {
        keymap()
            .on_override(key(KeyCode::Char('c')).ctrl(), Msg::Quit)
            .in_scope(&self.input_focus, key(KeyCode::Enter), Msg::Submit)
            .fallthrough(&self.input_focus, |ev| match ev {
                InputEvent::Key(k) => match k.code {
                    KeyCode::Char(c) => Msg::Typed(c),
                    KeyCode::Backspace => Msg::Backspace,
                    _ => Msg::Noop,
                },
                InputEvent::Paste(_) => Msg::Noop,
            })
    }
}

fn main() -> std::io::Result<()> {
    let input_focus = Focus::new().handle();
    input_focus.focus();

    let submitted = run(Echo {
        input_focus,
        typed: String::new(),
        submitted: 0,
    })?;

    println!("echoed {submitted} lines");
    Ok(())
}
```

Run it, type a few lines, and scroll up: committed lines are ordinary
terminal output, indistinguishable from anything printed before the program
started. That's the entire idea.

## Where each piece is explained

- `ctx.push` and the block lifecycle — [the timeline](../guide/the-timeline.md)
- `tail`, `col`, `text`, and writing your own elements —
  [elements and layout](../guide/elements.md)
- `keymap`, dispatch order, and focus — [input](../guide/input.md)
- Streaming, background work, and cancellation (this example is entirely
  synchronous; most real apps aren't) — [async](../guide/async.md)
- Real input editing: this example hand-rolls character handling to stay
  small; use the built-in [`text_area`](../reference/widgets.md) (or wrap
  [tui-textarea](../guide/components.md)) in real apps.
