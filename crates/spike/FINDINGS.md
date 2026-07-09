# Bake-off findings

Running log, one section per port. Verdicts go in `.planning/REDESIGN.md` once
all ports land.

## Port 1: `agent_turn_view` (pure builders, display-only)

Source: `~/src/atuin/crates/atuin-ai/src/tui/view/mod.rs` → `src/ports/agent_turn.rs`.
Overall length is rough parity with the `element!` original (rustfmt expands
method chains about as much as the macro's nesting). The wins are structural,
not line count.

### Wins

- **Keys deleted wholesale.** ~11 `key:` props and the `turn_id` parameter
  (which existed only to build key strings) have no equivalent — reconciliation-
  free rendering makes element identity meaningless. This includes the
  `on_commit` key-parsing contract keys were load-bearing for.
- **Native control flow.** `match`, `if let`, early returns, and iterator
  chains replace the `#(...)` grammar. `shell_tool_view`'s `Option<&ToolPreview>`
  handling reads better as a plain `match` than the original's `#(if let ... } else {`.
- **`group_row_view` collapsed**: three nested `View(width: ...)` wrappers in
  an `HStack` became `row().fixed(2, ..).fixed(2, ..).fill(..)`.
- **Padding-only `View` wrappers** became `.pad_left(n)` on the child.
- **`.when_some()`** replaces the original's `#(if cond && x.is_some())` guard
  + `x.unwrap()` in the body (twice in `suggested_command_view`).
- **Plain Rust tooling.** Every error hit while porting was an ordinary rustc
  diagnostic pointing at the real line; rust-analyzer completes everything.

### Costs

- **`.any()` density is the #1 noise source.** Every heterogeneous match arm
  and every `El`-returning helper ends in `.any()` (~15 in this port). Same tax
  as GPUI's `.into_any_element()`. Tolerable; would be the first target if we
  add any sugar.
- **Match-in-children needs a named helper** (`event_view`) or immediate
  closure — the macro allowed `#(match ...)` inline. Arguably better factoring,
  but it is extra ceremony the original didn't have.
- **Multi-span text is the weakest leaf API.** `text(a).style(s1).span(b, s2)`
  works, but `.style()` mutating "the most recent span" is subtle, and the
  conditional trailing span in `history_search_row` needed `.when()` on `Text`
  plus a `.span(" ", Style::default())` spacer. Wants design attention
  (`span_unstyled`, tuple-list constructor, or a tiny `spans![]` macro).

### API design rules learned (bind for v2)

1. **Combinators must live on a `Msg`-free trait.** Display-only elements
   implement `Element<Msg>` for *every* `Msg`; a combinator on an
   `ElementExt<Msg>` trait whose signature doesn't mention `Msg`
   (`pad_left(self) -> Padded<Self>`) is uninferrable on such receivers
   (E0282). Hence the `Fluent` (Msg-free) / `ElementExt<Msg>` (only `any()`,
   which names `Msg` in its return type) split in `ui.rs`. Rule: a method may
   be `Msg`-parameterized only if `Msg` appears in its argument or return
   types.
2. **Edition-2024 implicit capture bites every `&data -> impl Element` helper.**
   Returning `impl Element<Msg>` from a function taking references captures
   those lifetimes, so `.any()` (needing `'static`) fails with E0521 even when
   the returned value is fully owned. Fix is `-> impl Element<Msg> + use<>`.
   Recurring paper cut; v2 docs must establish a house style (probably: helpers
   return `AnyElement<Msg>` and eat the box, or always `+ use<>`).

### Not yet validated here

This port keeps `agent_turn_view` as a view function. In the real v2 model,
completed turns are *pushed blocks* and only the active turn renders in the
tail — the block lifecycle is Port 4's (driver sketch) job. This port only
validates the DSL shape.

## Port 2: file-edit diff view — TODO

## Port 3: InputBox ×2 (widget state candidates) — TODO

## Port 4: driver-loop sketch (`update`/`push`/`spawn`) — TODO
