---
title: Testing
description: Driving whole apps headlessly against a real terminal emulator
---

# Testing

Because the runtime core is synchronous — events in, escape bytes out —
entire apps are testable without a terminal, a TTY, or an executor. The
same harness eye-declare tests itself with is public:

```toml
[dev-dependencies]
eye_declare_engine = { version = "...", features = ["test-util"] }
```

```rust
use eye_declare::{InputEvent, Runtime};
use eye_declare_engine::test_terminal::TestTerminal;

#[test]
fn submit_commits_the_line() {
    let mut rt = Runtime::new(my_app(), 80, 24);
    let mut term = TestTerminal::new(80, 24);   // a real VTE emulator
    term.feed(&rt.present());

    let (bytes, _) = rt.handle(InputEvent::Key(enter()));
    term.feed(&bytes);

    let screen = term.viewport_lines().join("\n");
    assert!(screen.contains("✓ hello"));
}
```

`TestTerminal` is a real terminal emulator (VTE): assertions run against
what a user would actually see, including scrollback
(`term.scrollback_lines()`), not against an internal buffer.

## The one rule: feed every byte

The runtime diffs against what it believes is on screen. **Every** byte it
returns — from `handle`, `process`, `startup`, not just `present` — must
reach the `TestTerminal`, or the emulator's state diverges from the
runtime's and later assertions fail confusingly. Wrap the pair in a small
harness so it's impossible to forget:

```rust
struct Harness { rt: Runtime<MyApp>, term: TestTerminal }

impl Harness {
    fn press(&mut self, code: KeyCode) -> Option<Output> {
        let (bytes, exit) = self.rt.handle(key_event(code));
        self.term.feed(&bytes);
        exit
    }
}
```

## Async flows, synchronously

Spawned work delivers messages; tests deliver those messages themselves and
skip the executor entirely:

```rust
h.press(KeyCode::Enter);                    // submit → app spawns request
h.process(Msg::Chunk("hello ".into()));     // play the stream by hand
h.process(Msg::Chunk("world".into()));
h.process(Msg::StreamDone);
assert!(h.all_lines().contains("hello world"));
```

This makes timing-free tests of streaming, cancellation, and error paths:
the sequence of messages *is* the scenario. (Effects the app queued are
observable via `Runtime::take_effects` if a test cares; otherwise they're
inert.) For the stream-producing functions themselves, ordinary
`#[tokio::test]`s collecting the stream complete the picture.

## What to assert

- **Screen content** for rendering (`viewport_lines`), **scrollback +
  viewport** for committed output.
- **Model state** for logic — the app struct is right there:
  `rt.app().submitted == 2`.
- **Byte-level cost** when it matters: `present()` on an unchanged tail
  should return (nearly) nothing, and a seal of already-displayed content
  should not repaint it. Regressions in output efficiency show up as
  assertions on `bytes.len()`.
