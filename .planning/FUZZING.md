# Fuzzing (2026-07)

`cargo-fuzz` (libFuzzer) harness in `fuzz/`. Requires nightly:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run runtime_transcript   # or text_area_ops, markdown_element
cargo +nightly fuzz run <target> -- -max_total_time=300   # bounded run
```

The release profile keeps `debug-assertions` and `overflow-checks` on, so
`debug_assert!`s in the library fire during fuzzing. Corpus and crash
artifacts stay untracked (`fuzz/.gitignore`); a crashing input lands in
`fuzz/artifacts/<target>/` — decode it with
`cargo +nightly fuzz fmt <target> <artifact>` and shrink it with
`cargo +nightly fuzz tmin <target> <artifact>`.

## Why fuzzing fits here

The engine is a pure state machine (frames in, escape bytes out) with a
real oracle: the VTE `TestTerminal` can interpret the engine's own output.
That upgrades fuzzing from "doesn't panic" to differential testing —
arbitrary op sequences, and after every op the emulated screen must equal
a naive model of the transcript. Mutation testing (`.planning/MUTATION.md`)
pointed at the weak regions; the differential target reaches them with
arbitrary geometry and op interleavings that example-based tests miss.

## Targets

- `runtime_transcript` — the star. Drives a whole app headlessly
  (`Runtime` + `TestTerminal`) with arbitrary push/set-tail/re-present
  sequences and asserts the terminal always shows: every committed block,
  in order, exactly once, then the current tail, then blanks. Content is
  short width-1 ASCII and geometry is fixed per run because `TestTerminal`
  models neither wide glyphs nor resize — teaching it both would let this
  target cover the (mutation-flagged, still weakly tested) resize and
  wide-char paths.
- `text_area_ops` — arbitrary editing sequences on `TextAreaState`
  (arbitrary unicode: combining marks, wide glyphs) with invariants: the
  cursor stays on a real grapheme boundary of a real line, `line_count()`
  agrees with `text()`, and measure/render/cursor hold up at awkward
  widths. Literal `'\n'` in `Char` events is excluded by design —
  `insert_char` trusts the keymap layer to route newlines to
  `insert_newline`.
- `markdown_element` — arbitrary UTF-8 through pulldown-cmark and the
  layout pass at arbitrary widths: no panics, `height` deterministic
  (RefCell cache agrees with a fresh parse at a second width), render at
  exactly the claimed height.

## Findings

- **Engine cursor desync growing out of an empty region** (fixed, found in
  <30 s by `runtime_transcript`): a tail rendering at height 0, then 1,
  then 2 painted all subsequent content one row below the region origin.
  The grow path in `Engine::present` claimed a full newline per row even
  when the region was empty (where the cursor already occupies the first
  row — the first-render path subtracts it), leaving `cursor.row` one past
  the region bottom; the next grow then snapped tracking back without
  emitting movement. Regression test:
  `engine_compose::growing_tail_from_empty_keeps_cursor_sync`; the grow
  path now carries a `debug_assert!` for the invariant
  (`cursor.row <= current_bottom`).

## Not yet covered

- Resize and wide-char content in the differential target (blocked on
  `TestTerminal` support for both; the emulator treats every char as one
  column and has a fixed size).
- Styled content in the differential model (`viewport_lines` is
  plain text; styles pass through untested).
- The engine's `commit`/`finalize` ops directly — `runtime_transcript`
  exercises them only through `Timeline`'s usage.
