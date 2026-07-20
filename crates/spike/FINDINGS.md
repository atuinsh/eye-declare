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

## Port 2: `file_edit_tool_view` / `file_write_tool_view` (nesting stress test)

Source: view/mod.rs lines ~640–830 → `src/ports/file_edit.rs`. Compiled first
try; the Port 1 design rules (Msg-free `Fluent`, `+ use<>`) held with no new
friction.

### Wins

- **The escape-block workaround dissolved — the headline finding.** The
  original's diff body is `#(for ...)` wrapping `#({ ... })` which
  pre-collects `lines_rendered: Vec<(idx, prefix, text, style, gutter_text,
  gutter_style)>`, because the macro's `#(for)` cannot thread the mutable
  `before_pos`/`after_pos` line counters through iteration. In builder form
  the counters mutate inside an ordinary `FnMut` closure in
  `.children(hunk.lines.iter().map(...))` — the intermediate `Vec`, the
  6-tuple, and both levels of `#()` ceremony are gone. The gnarliest view
  code in atuin-ai became unremarkable Rust. This was the case the bake-off
  most needed to test, and builders won it outright.
- **The 6-tuple was partly macro-induced:** `gutter_style` was always equal
  to the line style; the pair collapses to one once you're in plain code.
- **Cross-view dedup became natural.** Edit and write views each carried a
  near-identical ~25-line pending/success/error status match; extracting
  `tool_status_line` was trivial because elements are just values. (Possible
  with `Elements` too, but the original didn't — block-macro syntax seems to
  discourage small extractions.)
- **Two more `key: &str` parameters** and ~6 `key:` props deleted.
- Guard clauses (`let Some(preview) else return status_line`) carry over
  verbatim — parity, since the original also used early returns.

### Costs

- None new. `.any()` density unchanged from Port 1; format-string gutter
  alignment identical to the original.

## Port 3: InputBox ×2 — the widget-state decision (O1)

Source: `components/input_box.rs` + the Tab policy from `components/atuin_ai.rs`
→ `src/ports/input_box_a.rs` (strict Elm) and `input_box_b.rs`
(framework-managed, egui-Memory style). Both cover submit, Shift+Enter/Ctrl+J
newline, Tab slash-accept, Tab insert-command, paste, and focus-driven visuals.

### Preliminary verdict: **Candidate A (strict Elm), decisively.**

Every pathology in the original maps to a structural fix in A and survives —
or returns — in B:

| Original pathology | A (state in model) | B (framework store) |
|---|---|---|
| `Arc<Mutex<TextArea>>` | plain model value, view borrows | store + `Any` downcasts |
| `InputUpdated` echo per keystroke | **deleted** — `is_blank`/slash results are derived reads | **reintroduced verbatim** (`input_text` mirror + `on_change`) |
| `tx` context plumbing | handlers are messages | `Box<dyn Fn>` callback props return (`on_submit`, `on_change`, `intercept_key`) |
| `active` prop shadowing focus | `FocusHandle::is_focused()` | same fix available |
| Tab spread across 3 dispatch layers | conditional keymap data, one place, rebuilt per update | config flags + escape hatches; slash-accept **hit a wall** — `intercept_key` can read widget text but can't write it (needs yet another read-write hook); A does it in 3 lines of `update` |
| widget measurement | `height()` reads borrowed state | `height()` needs store access — the managed model leaks into the core `Element` trait |

Fair credit to B: it's the right shape for ephemeral *view* state the app
genuinely never reads (scroll offsets, hover, spinner phase), and it avoids
A's per-widget routing arm (`Msg::Edit`). Worth revisiting a store for
Viewport-style scroll later — but text input is app data, and B forces the
app to either mirror it (the echo) or query the store (breaking view purity).

### API design rules learned (bind for v2)

3. **`AnyElement` must carry a lifetime.** A strict-Elm text area renders
   from `&TextAreaState` in the model; the previous implicit `'static` was
   an artifact of Ports 1–2 cloning small strings. Now
   `AnyElement<'a, Msg> = Box<dyn Element<Msg> + 'a>`, `ElementExt::any`
   generic over `'a` with `Self: 'a`. The tail is built/rendered/dropped in
   one frame, so model borrows are naturally scoped. Old ports annotate
   `'static` and needed no other changes.
4. **The blanket `Fluent` impl paid off**: `Keymap` got `.when(cond, ...)`
   conditional bindings for free — conditional Tab policy is one combinator.
5. **`Element` grew `fn cursor(&self, area) -> Option<(u16, u16)>`** — the
   focused-cursor runtime contract, settled naturally by this port.

### Open points for the real implementation

- `Keymap` stores `Msg` values, so `Msg: Clone` (hence the `EditEvent`
  wrapper for the raw-event variant). Alternative: store `Fn() -> Msg`
  constructors. Decide with the runtime.
- Two conditional Tab bindings can theoretically both be active; keymap
  needs documented precedence (first-match-wins in declaration order is the
  obvious rule).

## Port 4: driver-loop sketch (`update`/`push`/`spawn`)

Source: the *shape* of `driver.rs` (1,088 lines) + `commands/inline.rs`'s
channel/thread scaffolding → `src/ports/driver.rs` (~230 lines, honest
caveat: the sketch omits session persistence, permissions, and real SSE —
the claim is that the *adapters* died, not the business logic).

### The four adapters, checked off

- **`DriverEventSender`** — gone. Messages go straight to `update`;
  sub-models compose by enum embedding (`AppMsg::Input(input_box_a::Msg)`)
  plus `Keymap::map` / `map_msg`. Parent policy over child messages is
  plain pattern matching (parent claims `Submit`, delegates the rest) — no
  OutMsg machinery needed.
- **`sync_view_state`** — gone, structurally. There is no `ViewState` to
  clone the FSM into: the model holds only the live turn (5 fields vs 18,
  no `all_events`/`visible_events`/`archived_events`/`turns` clones per
  event, no `committed_turn_count` filtering per frame). The driver-thread
  turn pre-computation is unnecessary because per-frame work is bounded by
  the live turn, not conversation length.
- **`on_commit` key parsing** — gone. Sealing a turn is
  `ctx.push(agent_turn_view(&events, ..)); self.streaming = None;` — the
  turn leaves the model at push time; there is nothing to prune later.
- **The `spawn_blocking` driver thread + `mpsc` bridge** — gone; the
  runtime's loop delivers stream items as messages.

Bonus deletions: the `usize::MAX` sentinel pending-turn (the tail composes
a spinner), and `CancelGeneration` plumbing — Esc/Ctrl+C-while-busy is
`self.streaming = None` (cancel-on-drop `Task`), four lines of `update`.

### O6: resolved — `push` suffices

Sealing pushes the completed turn *above* the tail while the input box keeps
rendering *in* the tail below it — exactly the visual order the app wants,
with no `push_before` or ordering primitive. The streaming turn lives in the
tail until sealed; its committed position is where the tail was.

### New design fork discovered (→ spec as O7)

`map_msg` compiled as a **pure phantom re-wrap** — nothing on `Element`
actually carries `Msg` in the strict-Elm candidate, because all message
emission lives in the keymap (bindings + fallthrough). The `Msg` parameter
on `Element` is currently vestigial. Dropping it would dissolve the
`ElementExt<Msg>`/`Fluent` split from Port 1 and every `Msg`-inference
concern. Counterpoint: element-level emission may return with mouse support
(`on_click`) — though runtime hit-testing → `FocusHandle`/keymap routing is
a plausible alternative there too. Lean: try a `Msg`-free `Element` first.

### Engine contract addendum

`Element::cursor` (from Port 3) has to reach the terminal:
`Engine::present` needs a cursor-position parameter (or sibling call) —
`present(tail: &Buffer, cursor: Option<(u16, u16)>)`. Update the spec.

---

# Bake-off verdict (exit criteria from REDESIGN.md Phase 1)

1. **DSL: fluent builders, no macro.** Builders won every case including
   the hardest (Port 2's stateful diff numbering, where the macro needed an
   escape-block workaround). Keys/identity deleted throughout. Sugar
   candidates if ever wanted — `.any()` density, multi-span text — are
   method-level polish, not grounds for a parser.
2. **Widget state (O1): strict Elm** — controlled components. State lives
   in the model as plain values; views borrow it; policy is keymap data.
   Framework-managed state reintroduced the `InputUpdated` echo, callback
   props, and a store-leak into `Element::height`. Possible future
   carve-out: a B-style store for ephemeral view state the app never reads
   (scroll offsets) — uncontrolled components, explicitly opt-in.
3. **Event emission (O5): keymap-only.** No element-level key handlers were
   needed for any ported behavior; dispatch simplifies to
   override → focus-scoped → global → focus-fallthrough, first match in
   declaration order wins. (Whether `Element` keeps its `Msg` parameter at
   all is O7.)
4. **Block lifecycle (O6): `push` alone suffices.**
5. **API rules for the real implementation:** (1) combinators live on a
   `Msg`-free trait; (2) `&data -> impl Element` helpers need `+ use<>` or
   a boxed-return house style; (3) `AnyElement` carries a lifetime so views
   borrow the model; (4) `Element` exposes `cursor()`; (5) `Engine::present`
   takes the cursor hint.
6. **What the engine must expose** (unchanged from the spec otherwise):
   `commit(rows)`, `present(tail, cursor)`, `resize`, `resync`, `finalize`.
