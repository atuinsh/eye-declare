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
  (`Runtime` + `TestTerminal`) with arbitrary push/set-tail/re-present/
  resize sequences — content mixes ASCII with double-width CJK — and
  asserts the terminal always shows: every committed block, in order,
  exactly once, then the current tail, then blanks. `TestTerminal` models
  wide glyphs (continuation cells, whole-glyph wrap at the right edge,
  orphaned halves on overwrite) and non-reflowing resize for this;
  the emulator has its own behavior suite in
  `eye_declare_engine/tests/test_terminal_behavior.rs`, since it is the
  oracle everything else leans on.
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
- **TextArea cursor past end-of-line on paste** (fixed, `text_area_ops`):
  pasting text that begins with a combining mark merges it into the
  grapheme before the cursor, so `insert_str`'s
  `col += grapheme_count(part)` overshot the line end (violating the
  cursor-on-a-real-grapheme invariant that all later `byte_at` slicing
  assumes). Now recounts from the actual line content, like
  `insert_char` already did. Regression test:
  `text_area::tests::paste_starting_with_combining_mark_keeps_cursor_in_bounds`.
- **Markdown control characters flowed into cell symbols** (fixed,
  `markdown_element` via the input `"\0[佉&"`): parse passed raw text
  through, so a NUL/ESC in a document became cell content — an ESC would
  splice straight into the terminal's escape stream when the row is
  emitted. `parse` now sanitizes text and code events (tabs expand to
  four spaces, other controls become U+FFFD).
- **Upstream: ratatui word wrapper writes out of bounds** (worked around,
  same input): a wide char alone in a span starting at the last column
  with another span following makes `Paragraph::render` (Wrap, ratatui
  0.1.2 core / 0.3 widgets) index past the buffer edge. Minimal repro,
  no eye-declare involved: spans `["a", "佉", "b"]`, `Wrap {trim:false}`,
  2×16 buffer — `line_count(2)` also disagrees with itself (claims 3, or
  9 on the fuzz input). Worth reporting upstream. eye-declare's parse now
  merges adjacent same-style spans (also an alloc/layout win), which
  removes every unstyled instance of the shape; styled wide chars
  (`**佉**x`) can still hit it, so `markdown_element` filters wide glyphs
  from its corpus until the upstream fix. Regression test:
  `markdown::tests::fragmented_spans_with_wide_chars_render_safely`.

- **Wide glyph destroyed by its own diff** (fixed, found seconds after
  teaching the differential target CJK content): `Frame::diff_from`'s
  height-mismatch path compared cells with no wide-glyph discipline, so
  the trailing continuation cell of a freshly written wide glyph (blank
  in the new frame, old content in the previous one) was emitted as its
  own update — painting a space over half the glyph. A tail changing
  height from 2 rows to 1 lost its 日 entirely. The hand-rolled path now
  mirrors `Buffer::diff`'s skip/invalidate rules. Same family as
  upstream ratatui#2652 (trailing cell stale on style-only change),
  which still affects the equal-dimensions fast path — see below.
  Regression test: `engine_compose::wide_glyph_survives_tail_height_change`.

- **Word wrapper out of bounds at width 2 with plain CJK** (guarded,
  found re-fuzzing after the GFM-table merge): ratatui's `WordWrapper`
  emits a line wider than the limit when a multi-column grapheme sits
  mid-word at width 2 — `"a佉b"` does it, as does a Cf prepend cluster
  (`"x\u{604}<!"`, where U+0604 merges with the *following* char into a
  width-2 cluster). `Paragraph::render` then writes past the buffer
  edge. Distinct from the multi-span trigger above and not fixable by
  span merging; table cells make width-2 wrap regions realistic (a
  four-column table in a 20-col terminal). `wrap.rs` now has one render
  path, `render_wrapped`, that falls back to truncation below
  `MIN_WRAP_WIDTH = 3`, with `wrapped_line_count` measuring identically
  so `height(width)` stays exact; all element render sites go through
  it. This also let `markdown_element` re-enable wide glyphs in its
  corpus. Regression tests:
  `wrap::tests::degenerate_widths_truncate_instead_of_wrapping`,
  `markdown::tests::fragmented_spans_with_wide_chars_render_safely`.

After the fixes, the extended differential (resize + CJK) runs clean:
~200k executions over 5 minutes with resize hitting `reset_region` and
`set_terminal_height` — the paths mutation testing found completely
untested now hold up under arbitrary interleavings.

## Not yet covered

- Styled content in the differential model (`viewport_lines` is
  plain text; styles pass through untested). This is what would catch
  upstream ratatui#2652 (`Buffer::diff` leaves the trailing cell of a
  wide glyph stale on style-only changes), which eye-declare inherits
  through the equal-dimensions diff fast path.
- The engine's `commit`/`finalize` ops directly — `runtime_transcript`
  exercises them only through `Timeline`'s usage.
- Terminal reflow on resize: the emulator clips/pads without reflowing,
  matching the engine's committed-content promise; reflowing terminals
  are out of model.
