# Mutation testing findings (2026-07)

First full `cargo-mutants` run over the workspace, to locate where the test
suite is weakest. Method: `cargo install cargo-mutants`, then per crate

```sh
cargo mutants -p eye_declare_engine   # 240 mutants, ~7 min
cargo mutants -p eye_declare          # 632 mutants, ~15 min
```

`.cargo/mutants.toml` sets `test_workspace = true`; without it the engine
crate is scored against only its own unit tests (the headless VTE tests live
in eye_declare) and the miss rate triples. Wall time measured on 4 cores
with `-j 2`.

## Scores

| Crate              | Caught | Missed | Unviable | Catch rate |
| ------------------ | -----: | -----: | -------: | ---------: |
| eye_declare_engine |    163 |     72 |        5 |        69% |
| eye_declare        |    361 |    167 |       94 |        68% |

(10 library timeouts, all in driver/runtime loops — expected.)

A ~70% catch rate with exact-screen assertions means the survivors are
concentrated in genuinely unexercised paths, not weak assertions. The
frame-diffing core (`Frame::diff_from`, `Diff::to_escape_sequences`) and the
commit path caught nearly everything — the headless-app story works for the
hot path.

## Gaps, in priority order

1. **Resize is untested end-to-end.** `Engine::reset`, `Engine::reset_region`,
   `Engine::set_terminal_height`, `Runtime::resize`, `Timeline::resize`, and
   the `Event::Resize` arm in `run_with` all survive full-body stub mutants.
   No test simulates a width or height change at any layer, despite the
   carefully documented reflow contracts on `reset_region`.
2. **Wide characters in committed rows.** `unicode_display_width` mutated to
   always return 1 survives: no test commits a row containing CJK/emoji, so
   the continuation-cell skip logic in `write_committed_row` is unverified.
3. **Style transitions in committed output.** The reset-vs-incremental branch
   in `write_style_diff` and all of `write_full_style` survive: no committed
   row ever *removes* a color/modifier mid-row (the `\x1b[0m` + re-apply
   path).
4. **TextArea editing edges.** `delete` (forward-delete) survives as a stub
   and its `KeyCode::Delete` arm is deletable; ditto `KeyCode::Down`,
   `move_left` across a line boundary, `set_text` cursor clamping, and
   `line_count`. Backspace/typing are well covered; the rest of the editing
   surface is not.
5. **Panel geometry.** `Panel::height` arithmetic (border + title math),
   degenerate sizes (`width < 2 || height < 2`), cursor offset translation,
   and `animated` passthrough all survive. Panel is rendered in tests but
   never with a cursor inside, never at degenerate sizes, and its height
   contract is never checked against its rendered output.
6. **Tall frames / shutdown.** First-present of a frame taller than the
   terminal (`present`'s `stream_until` math) and `finalize`'s
   cursor-repositioning arithmetic survive off-by-one flips.
7. **TestTerminal (meta-risk).** Whole CSI arms (`D`, `E`, `F`, `H`, `J`,
   backspace) are deletable from the VTE emulator without failures. The
   emulator is the oracle for every headless test; it deserves a few direct
   unit tests of its own.
8. **Markdown parse arithmetic.** ~20 `+=`/`>` flips in `markdown.rs::parse`
   survive — list indexing/indent accounting is only lightly asserted.

Known-untestable (fine to ignore): `run`/`run_with` real-TTY loops,
`RawModeGuard`, `sleep_opt`, `Drop` impls — these need a PTY harness, which
isn't worth it while the headless `Runtime` covers the same logic.

Full per-mutant lists: re-run `cargo mutants -p <crate>` and see
`mutants.out/missed.txt` (not committed; runs are deterministic).
