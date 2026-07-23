# AGENTS.md
This file provides guidance to AI coding assistants working in this repository.

**Note:** CLAUDE.md, .clinerules, .cursorrules, .windsurfrules, .replit.md, GEMINI.md, .github/copilot-instructions.md, and .idx/airules.md are symlinks to AGENTS.md.

# eye-declare

Inline terminal UI library for Rust, built on Ratatui. Models inline UIs as a
**timeline**: committed blocks are emitted as effects (`ctx.push`) and flow
into native scrollback; only a small live tail re-renders, described by a pure
view function. Elm-shaped app architecture (model/`Msg`/`update`), strict-Elm
widget state, keymap-as-data input. Design history and decisions:
`.planning/REDESIGN.md`; performance notes: `.planning/PERF.md`.

## Crate Structure

```
crates/
  eye_declare/         The library: App/Ctx, elements, keymap/focus, widgets,
                       Timeline, sync run() + driver_tokio (ratatui-core,
                       crossterm, tokio, pulldown-cmark)
  eye_declare_engine/  Terminal-sync engine: frame diffing, escape generation,
                       scrollback streaming, cursor discipline; VTE TestTerminal
                       behind the `test-util` feature (no crossterm dependency)
```

## Build & Commands

```sh
# Build / check
cargo build                          # Build all workspace crates
cargo check -p eye_declare --no-default-features   # Executor-agnostic core must stay green
cargo clippy --workspace --all-targets -- -D warnings

# Test
cargo test --workspace               # All tests
cargo test -p eye_declare test_name  # One test by name

# Examples (all in crates/eye_declare/examples/)
cargo run -p eye_declare --example echo        # Minimal: type, Enter commits, Ctrl+C exits
cargo run -p eye_declare --example stream      # Mini agent: streaming turn, Esc cancels
OPENROUTER_API_KEY=... cargo run --release -p eye_declare --example openrouter
                                               # Flagship: real streaming AI chat
cargo run --release -p eye_declare --example perf_report
                                               # Deterministic perf/alloc report

# Benchmarks
cargo bench -p eye_declare           # criterion (benches/frame.rs)

# Docs site: root is the standalone explainer (docs/index.html); the book
# (docs/content/, config docs/undox.yaml) lives under /book
cd docs && undox build && cp index.html _site/index.html
```

## Code Style

- **Edition**: Rust 2024. Standard `rustfmt`; clippy clean at `-D warnings`.
- **Elements are `Msg`-free values**: they describe pixels only (`height`,
  `render`, optional `animated`/`cursor`). All message emission lives in the
  keymap. `height(width)` must be exact and cheap — no probe rendering.
- **Strict Elm**: widget state is plain values in the app's model; views
  borrow (`AnyElement<'a>`). No framework-owned state, no dirty tracking —
  re-presenting the tail must stay unconditionally cheap (an invariant, not
  an optimization target).
- **View helpers** that clone need `-> impl Element + use<>` (edition-2024
  implicit capture); helpers that borrow use `+ '_`.
- **Expensive custom elements** cache work per frame in a `RefCell` (see
  `markdown.rs`); `height` runs more than once per frame.
- **Error handling**: `std::io::Result` at app boundaries; no `unwrap()` in
  library code.
- **Comments** explain why, not what; doc comments carry contracts
  (`None` semantics, units, invariants), not restatements.

## Testing

- Whole apps test headlessly: `Runtime` (events in, bytes out) against
  `eye_declare_engine::test_terminal::TestTerminal` (a real VTE emulator).
  **Feed every byte the runtime returns to the terminal** — from `handle`,
  `process`, and `startup`, not just `present` — or emulator state diverges.
- Async flows are tested synchronously by delivering messages via
  `Runtime::process`; stream-producing functions get `#[tokio::test]`s.
- Engine changes: run `examples/perf_report.rs` (exact, deterministic alloc
  counts) before and after; `benches/frame.rs` for wall time.
- When tests fail, fix the code, not the test.

## Configuration

- Features on `eye_declare`: `tokio` (default; the async driver) and
  `markdown` (default; pulldown-cmark element). The core must keep compiling
  with `--no-default-features`.
- No environment variables or config files beyond `Cargo.toml`
  (`OPENROUTER_API_KEY` for the flagship example only).

## CI/CD

- GitHub Actions deploys docs (`.github/workflows/docs.yml`) built with
  [undox](https://github.com/undox-rs/undox) to GitHub Pages.
- Publish order when releasing: `eye_declare_engine` first, then
  `eye_declare` (the library depends on the engine by version).

## Commits

- Conventional commit format; run `cargo fmt` and
  `cargo clippy --workspace --all-targets -- -D warnings` before committing.
- Never push `main` — branch and open draft PRs; use GH Stacks
  (`gh stack …`) for multi-concern work.
