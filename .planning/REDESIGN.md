# eye-declare v2: Timeline Architecture

**Status:** Phase 1 (bake-off) complete — 2026-07-14. Verdicts merged below;
evidence in `crates/spike/FINDINGS.md`, candidate API in `crates/spike/src/ui.rs`.
Next: Phase 2, engine extraction.
**Context:** `.binarymuse/library-redesign.md` (assessment, Atuin AI evidence, hazard list).
**Decisions already made:** new unpublished crate in this workspace; bake-off before engine extraction; current `eye_declare` API fully frozen.

## Thesis

Inline UIs are not trees, they are timelines: an append-only sequence of blocks where old
content freezes and scrolls away, and only a small live tail is ever dynamic. The current
library models a general component tree and bolts the timeline on (`freeze`, `commit`,
`on_commit`); the redesign inverts that.

The conceptual core, and the sentence everything below follows from:

> **Committed output is an effect. The live tail is a view.**

Writing a block toward scrollback is I/O — irreversible, append-only, like `println!`. So
block emission happens in `update`, as an effect. The tail is the only thing that behaves
like a screen, so it is the only thing described by a view function — re-run every frame,
pure, cheap because it is small.

This dissolves rather than solves several current problems: no reconciliation (nothing to
reconcile — blocks render once, the tail rerenders wholesale), no keys, no dirty tracking,
no `NodeId`s, no commit detection by key parsing, and per-frame cost bounded by the tail
regardless of how long the app runs.

## Core types

Sketches, not signatures-of-record. Names are placeholders.

### App

```rust
pub trait App: Sized + 'static {
    /// Messages driving the app. Everything that happens becomes one of these.
    type Msg: Send + 'static;
    /// What `run()` returns when the app exits (e.g. Atuin's Execute/Insert/Cancel).
    type Output: Default;

    /// Handle a message: mutate the model, emit effects via ctx.
    fn update(&mut self, msg: Self::Msg, ctx: &mut Ctx<'_, Self>);

    /// Describe the live tail. Re-run every frame. Pure, borrows the model.
    fn tail(&self) -> impl Element<Self::Msg>;

    /// Declarative recurring inputs, diffed by key after each update.
    fn subscriptions(&self) -> Subscriptions<Self::Msg> {
        Subscriptions::none()
    }
}
```

No separate Model type — the implementing struct *is* the model. `update` takes `&mut
self`; `tail` takes `&self`. The borrow checker enforces the Elm discipline for free.

### Ctx: effects

```rust
impl<A: App> Ctx<'_, A> {
    /// Append a finished block to the timeline. Rendered once at current
    /// width, then owned by the engine; flows into scrollback. Irreversible.
    pub fn push(&mut self, block: impl Element<A::Msg>);

    /// Spawn a stream of messages (the LLM-turn shape). Each item is fed
    /// back into update(). Returns a cancel-on-drop Task.
    #[must_use]
    pub fn spawn(&mut self, s: impl Stream<Item = A::Msg> + Send + 'static) -> Task;

    /// One-shot future convenience over spawn().
    #[must_use]
    pub fn perform(&mut self, f: impl Future<Output = A::Msg> + Send + 'static) -> Task;

    /// End the run loop; run() returns this value after teardown.
    pub fn exit(&mut self, output: A::Output);
}
```

```rust
/// Handle to spawned work. Dropping it cancels the work.
pub struct Task(/* … */);
impl Task {
    pub fn detach(self);      // fire-and-forget, never cancelled
}
```

Held in the model: `streaming: Option<Task>`. Esc-cancels-generation is
`self.streaming = None;` — no CancelGeneration plumbing, no AtomicBools.

### Subscriptions

```rust
Subscriptions::none()
    .every("autosave", Duration::from_secs(30), || Msg::Autosave)
    .stream("fs-events", || watch_files())          // keyed; diffed across updates
```

Note what is *absent*: spinner ticks. Widgets declare their own animation
(`Spinner` reports `animated: Some(80ms)` via the Element trait), and the runtime
self-ticks the tail while any animated widget is present. Spinners require zero user code.

### Focus

```rust
pub struct FocusHandle(/* Arc-backed identity */);

impl FocusHandle {
    pub fn focus(&self);
    pub fn blur(&self);
    pub fn is_focused(&self) -> bool;
}
```

Created by the app, stored in the model, bound in the tail
(`.track_focus(&self.input_focus)`). Focus is plain data the app owns; "press `/` to focus
search" is one line in `update`. There is no framework focus registry to fall out of sync
with, no autofocus lifecycle, no Tab cycling unless the app binds Tab to a cycling message.

### Keymap

```rust
fn keymap(&self) -> Keymap<Msg> {
    Keymap::new()
        .on(key!(ctrl-c), Msg::Interrupt).global_override()  // fires before widgets; rare
        .on(key!(esc), Msg::Cancel)                          // global fallback
        .in_scope(&self.input_focus, key!(enter), Msg::Submit)
}
```

Dispatch order for a key event:
1. `global_override` bindings (explicitly marked; for Ctrl+C-tier chords)
2. the focused element's own handlers (a focused textarea gets Tab, arrows, chars)
3. bindings scoped to the focused handle
4. global bindings

This makes the Atuin Tab-to-insert footgun structurally impossible: Tab means whatever the
keymap says in the current focus context, and a focused editor can always take it first.

### Elements and the DSL

```rust
pub trait Element<Msg> {
    /// Honest measurement. No probe rendering, no 512-row cliff.
    fn height(&self, width: u16) -> u16;
    fn render(&self, area: Rect, buf: &mut Buffer);
    /// Frame interval if self-animating (Spinner → Some(80ms)).
    fn animated(&self) -> Option<Duration> { None }
    /// Key handling when focused / message emission — exact shape TBD in bake-off.
    fn on_key(&self, key: KeyEvent) -> Option<Msg> { None }
}
```

Authoring is **fluent builders first** (GPUI-style), macros only as thin sugar if the
bake-off shows specific pain:

```rust
fn tail(&self) -> impl Element<Msg> {
    col()
        .gap(1)
        .when(self.streaming.is_some(), |c| c.child(spinner("Thinking…")))
        .child(
            text_area(&self.input)
                .border(Rounded)
                .title("Ask")
                .track_focus(&self.input_focus)
                .on_submit(Msg::Submit),
        )
        .children(self.slash_results.iter().take(4).map(slash_row))
}
```

Native `if`/`for`/`match` work inside plain Rust; `.when()` and `.children(iter)` cover
the inline cases. No custom parser, no `#()` grammar, full rust-analyzer support. Layout
vocabulary carries over from v1: `col`/`row`, `Fixed`/`Fill` widths, content-driven
height, insets/borders via builder methods.

### Widget state — the open question

Two candidates; the bake-off ports InputBox both ways.

**Candidate A — strict Elm (current lean).** Widget state is a plain value in the model;
the widget ships a state type with a `handle` method; update owns all mutation:

```rust
struct Model { input: TextAreaState, /* … */ }

// tail:  text_area(&self.input).on_event(Msg::Input)
// update:
Msg::Input(ev) => {
    if let Some(TextAreaEvent::Submitted(s)) = self.input.apply(ev) {
        // …
    }
}
```

Maximum explicitness and testability; the cost is one routing arm per stateful widget.

**Candidate B — framework-managed.** Runtime owns per-id widget state (egui-Memory
style): `ui.text_area(id!(), …)`. Less boilerplate, but reintroduces hidden state,
identity, and lifetime questions — the things this redesign exists to remove.

The `Arc<Mutex<TextArea>>` in Atuin's InputBox is the case study: whichever candidate
makes that component honest *and* short wins.

### Engine contract

What Phase 2 extracts, shaped by what the new layer needs — note how small it is:

```rust
pub struct Engine { /* cursor, emitted_rows, terminal_height, prev tail frame */ }

impl Engine {
    pub fn new(width: u16, terminal_height: u16) -> Self;
    /// Append final rows above the tail; they flow into scrollback. Immutable after.
    pub fn commit(&mut self, rows: &Buffer) -> Vec<u8>;
    /// Replace the live tail. Diffs against the previous tail frame.
    /// `cursor` is the focused element's hardware-cursor hint.
    pub fn present(&mut self, tail: &Buffer, cursor: Option<(u16, u16)>) -> Vec<u8>;
    pub fn resize(&mut self, width: u16) -> Vec<u8>;
    /// Full repaint of the tail region; recovery from cursor-state drift.
    pub fn resync(&mut self) -> Vec<u8>;
    /// Reclaim trailing blanks, park cursor for shell handoff.
    pub fn finalize(&mut self) -> Vec<u8>;
}
```

Sync, no runtime dependency, bytes out (caller writes them). All the hard-won v1 behavior
lives behind `commit`/`present`: burst-row streaming into scrollback, `retain_visible`,
DEC 2026 sync output, relative-only cursor movement, SGR diffing. The VTE `TestTerminal`
moves with it and becomes a public testing story.

Semantic simplification vs v1: committed rows are immutable immediately (like printed
output). On width resize only the tail reflows; committed content behaves like any other
scrollback text. (v1 re-wraps still-visible frozen content; see O3.)

### Runtime

```rust
let outcome = eye_declare::run(model, RunOptions {
    keyboard: KeyboardProtocol::Enhanced,
    bracketed_paste: true,
    ctrl_c: CtrlC::Deliver,
    // …
}).await?;   // tokio driver; core stays sync/runtime-agnostic
```

Loop shape: wait on (terminal events → keymap/focused element → Msg) ∪ (task/stream
messages) ∪ (subscription ticks) ∪ (animation tick when tail has animated widgets) →
`update` → render any pushed blocks → `engine.commit` → build tail → measure → render →
`engine.present`. No dirty tracking anywhere: identical tails diff to zero bytes, and the
tail is small enough that rebuilding it every frame is noise.

## Design rules (day one, non-negotiable)

- **No dirty tracking.** Rerendering the tail must be unconditionally cheap; that is an
  invariant to preserve, not an optimization to add.
- **Honest measurement.** `height(width)` is part of the Element contract; ship wrapping
  helpers that make it one line for text. No probe rendering.
- **Grapheme-correct text.** `unicode-segmentation` alongside `unicode-width` from the
  first line of the text stack.
- **Runtime-agnostic core.** Engine and element layers are sync; tokio is one driver.
- **Ratatui interop is first-class.** Wrapping a `StatefulWidget` (tui-textarea) must be
  a small, honest adapter — measure + render + `&mut` state access, no `'static` closures.
- **Markdown:** pulldown-cmark behind a feature flag, or nothing. No hand-rolled parser.
- **The engine's test terminal ships** as a public snapshot-testing API.

## Phase 1: bake-off protocol

Compile-only spike in a scratch crate (`crates/spike/`, never published, allowed to be
ugly). `cargo check` is the bar; no runtime needed.

1. Port Atuin AI's `agent_turn_view` and the file-edit diff view
   (`~/src/atuin/crates/atuin-ai/src/tui/view/mod.rs`) to pure fluent builders. Record
   every place the builder form reads worse than the `element!` original — that list
   defines what macro sugar must do, or shows none is needed.
2. Port `InputBox` (`tui/components/input_box.rs`) twice: Candidate A and Candidate B.
   Same behavior, side by side, including the tui-textarea wrapping and submit/slash
   handling.
3. Sketch the Atuin driver loop (`update`/`Msg`/`push`/`spawn`) far enough to confirm
   `DriverEventSender`, `sync_view_state`, and the key-parsing `on_commit` all disappear.

**Exit criteria:** chosen DSL shape (+ macro sugar list, possibly empty), chosen widget-
state model, validated keymap dispatch order, and a written list of everything the engine
must expose — which parameterizes the Phase 2 extraction.

## Open questions

Resolved by the bake-off (evidence: `crates/spike/FINDINGS.md`):

- **O1 — widget state: RESOLVED → strict Elm** (controlled components).
  State is a plain model value, views borrow it, policy is keymap data.
  Framework-managed state reintroduced the `InputUpdated` echo, callback
  props, and leaked the store into `Element::height`. Possible later
  carve-out: an opt-in store for ephemeral view state the app never reads
  (scroll offsets) — uncontrolled components, explicitly second-class.
- **O2 — keymap dispatch: RESOLVED, simplified** →
  override → focus-scoped → global → focus-fallthrough, first match in
  declaration order. The "focused element's own editing keys" layer proved
  unnecessary: editing reaches the widget as fallthrough messages
  (`Msg::Edit(ev)` → `state.handle(ev)` in update).
- **O5 — element-level emission: RESOLVED → none needed.** Every ported
  behavior routed through the keymap. See O7.
- **O6 — sealing: RESOLVED → `push` suffices.** A sealed turn lands above
  the tail while the input box keeps rendering below; no `push_before`.

Still open:

- **O3 — resize semantics:** committed-is-immutable means still-visible sealed blocks
  don't re-wrap on width change (v1 re-wraps them). Acceptable? (Matches plain-println
  behavior; massive simplification.)
- **O4 — naming:** working names `eye_declare_engine` / `eye_declare_next`; real names
  (and whether v2 is `eye_declare 1.0` or a rename) decided at the end.
- **O7 — RESOLVED → `Element` is `Msg`-free** (built that way in
  `eye_declare_next`; dissolved the `ElementExt<Msg>`/`Fluent` split and
  all `Msg`-inference concerns). Revisit only if mouse support wants
  element-level emission; runtime hit-testing → focus/keymap is the
  planned alternative.

## Bake-off outcomes (Phase 1 exit criteria)

- **DSL: fluent builders, no macro.** Won every case including the diff
  view's stateful line numbering, where `element!` needed an escape-block
  workaround. Keys and identity props deleted throughout. If sugar is ever
  added, it targets `.any()` density and multi-span text — method-level
  polish, not a parser.
- **API rules:** combinators live on a `Msg`-free trait; `&data -> impl
  Element` helpers need `+ use<>` (or a boxed-return house style);
  `AnyElement<'a, Msg>` carries a lifetime so views borrow the model;
  `Element` exposes `cursor()`; `Engine::present` takes the cursor hint:
  `present(tail: &Buffer, cursor: Option<(u16, u16)>)`.
- **Composition pattern:** sub-model messages embed by enum; `Keymap::map`
  and element `map_msg` re-target them; parents claim child messages by
  pattern matching (no OutMsg machinery).

## Phases

1. **Spec** — this document; argue until stable. ✅
2. **Bake-off** — protocol above; throwaway code, keeper decisions. ✅ (2026-07-14)
3. **Engine extraction** — `inline`/`frame`/`escape`/`wrap` + `TestTerminal` →
   engine crate with the contract above; pure-refactor PRs; v1 tests keep passing.
   ✅ (2026-07-15) `eye_declare_engine` now owns frame diffing, escape
   generation, wrap measurement, the `Engine` terminal-sync state machine
   (`present(frame, cursor_hint)` / `reset` / `commit_scrolled` / `finalize`),
   and `test_terminal::TestTerminal` (behind `test-util`). `InlineRenderer`
   is a tree-facing wrapper. v1-compat notes: `present` takes whole frames
   and commit is slice-off-top (`commit_scrolled`) — the v2 append-`commit`
   surface layers on this in Phase 4; `resync()` doesn't exist yet (`reset`
   is the clear-and-redraw fallback). No crossterm dependency in the engine.
   **← Phase 4 next**
4. **Build the new layer** — new crate, driven by ports of the current `examples/`.
   **In progress** (`crates/eye_declare_next`, working name):
   - Slice 1 ✅ (2026-07-15): `Msg`-free element layer — `Element` with
     required `height(width)`, `AnyElement<'a>` borrowing views, `Col`/`Row`/
     `Padded`/`Empty`/`Text`/wall-clock `Spinner`, engine-composition tests.
   - Slice 2 ✅ (2026-07-15): `Engine::commit(rows)` (the v2 append-commit,
     built on present + `commit_scrolled`), `commit_scrolled` hardened to
     return cursor-repositioning bytes (the parked-cursor invariant lives in
     the engine now), and the sync `Timeline` runtime (`push`/`present`/
     `finalize`) with conversation-flow tests through the VTE terminal.
   - Next slices: keymap/focus/events + `App`/`update` loop → async driver
     (tokio, `spawn`/`Task`/subscriptions, animation self-tick) → widgets
     (`TextAreaState`, feature-flagged markdown, viewport) → resize story
     (needs O3 decided).
5. **Atuin AI port** — the validation gate. Success = the four adapters
   (`DriverEventSender`, `sync_view_state`, key-parsing `on_commit`, `active`-prop focus
   shadow) delete cleanly and the TUI code shrinks.
6. **Ship** — naming, docs, migration story for v1 users, decide v1's fate.

## Non-goals

- Full-screen / alternate-screen mode. Inline only; ratatui already serves full-screen.
- General tree reconciliation, keys, or preserved-by-position component state.
- Vertical flex layout. Content-driven height is correct for unbounded scrollback.
- Supporting the v1 `Component`/hooks API on the new core. v1 stays frozen as-is.

## What carries over unchanged

The scrollback engine's behavior and its VTE test harness; content-driven layout
(`Fixed`/`Fill` horizontal, natural vertical); the widget vocabulary (Text, Spinner,
Viewport, borders/padding); the documentation discipline; the name.
