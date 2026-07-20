# Phase 5: Atuin AI Port Plan

**Status:** planned 2026-07-16, against atuin `origin/main` (610e15ab).
**Where the work happens:** a branch in `~/src/atuin`, with

```toml
eye_declare = { package = "eye_declare_next", path = "../eye_declare/crates/eye_declare_next" }
```

so all code imports `eye_declare::` from day one; only this manifest line
changes at the 0.6.0 release. The branch stays a local draft until then
(CI can't see the path dep).

**Success criteria (the validation gate):** `DriverEventSender`,
`sync_view_state`, the key-parsing `on_commit`, and the `active`-prop focus
shadow all delete cleanly, and the TUI code shrinks.

## Shape of the port

The FSM is already Elm-shaped — `fsm.handle(event) -> Vec<Effect>` — so the
port collapses the three-thread relay (render thread ⇄ Handle ⇄ driver
thread ⇄ spawned tasks) into one `App`:

```rust
struct AiApp {
    fsm: AgentFsm,                      // unchanged
    io: IoContext,                      // unchanged (persistence goes async, below)
    input: TextAreaState,               // replaces Arc<Mutex<TextArea>>
    select: Option<SelectState>,        // permission prompt / model picker cursor
    slash: SlashState,                  // registry + live search results
    usage: Option<UsageSnapshot>,
    streaming: Option<Task>,            // cancel-on-drop; replaces stream_cancel_tx
    tool_interrupts: HashMap<String, oneshot::Sender<()>>,
    pushed_events: usize,               // frontier: events already in scrollback
    resume_notice: Option<String>,      // replaces the SessionContinue component
    // …
}

enum Msg {
    Input(InputEvent),                  // keymap fallthrough → self.input.handle()
    Submit, ExecuteCommand, InsertCommand, AcceptSlashSuggestion,
    CancelOrEsc, Interrupt, Retry,
    Select(SelectMsg),                  // Up / Down / Confirm
    Fsm(fsm::Event),                    // everything spawned work reports
    Usage(UsageSnapshot),
}
```

`type Output = ExitAction`-equivalent (Execute/Insert/Cancel, Cancel as
`Default`); `ctx.exit(..)` replaces the `exiting: AtomicBool` +
`handle.exit()` dance and the `exit_action` ViewState field.

### Commit model: on_commit inverted into push

v1 renders the entire conversation every frame and *detects* commits by
parsing `"turn-{id}"` keys as they scroll away. v2 inverts it:

- **Startup (resume):** all historical turns are final — render once via
  the shared turn view fns and `ctx.push` them. History is never re-rendered.
- **User turn:** final the moment the FSM accepts the submit → push
  immediately. (Push-as-effect in its purest form.)
- **Agent turn:** stays in the tail while `AgentState::Turn { .. }` (tool
  statuses and streaming text mutate it); on return to Idle/Error, push the
  completed turn and advance `pushed_events`.

Tail = turns built from `events[pushed_events..]` by the unchanged
`TurnBuilder` + streaming text + footer (pending banner / error line /
input panel or picker / status bar). Per-frame cost is O(current turn).

If a long multi-tool turn makes the tail uncomfortably tall, the
mitigation is the **frontier pattern**: within the live turn, push any
leading `UiEvent`s whose status can no longer change (resolved tools with
later events after them). Not planned up front — measure first.

### Keymap: the policy table

Rebuilt per update from the model, which makes every conditional binding
declarative — the Tab footgun fix falls out structurally:

- override: `Ctrl+C` → Interrupt (if a preview is executing) else Exit
- global: `Esc` → mode-dependent Cancel/Exit (the `AppMode` match from
  `atuin_ai.rs` becomes data)
- when a select is open: Up/Down/Enter → `Msg::Select(..)` (input keymap
  absent entirely — no `active` prop, no focus shadow)
- input mode: Enter → Submit, or ExecuteCommand when `has_command &&
  input.is_blank()`; Shift+Enter / Ctrl+J → newline; Tab bound *only when*
  a slash suggestion exists (accept) or `has_command && blank` (insert) —
  otherwise Tab reaches the text area and does nothing
- Error mode: Enter / `r` → Retry
- fallthrough: `Msg::Input(ev)` → `self.input.handle(ev)`; slash search
  recomputes right there in update (deletes the `InputUpdated` echo event)

### Effects: driver `execute_effect` arms become update arms

| FSM Effect | v2 execution |
|---|---|
| StartStream | `self.streaming = Some(ctx.spawn(stream_bridge(..)))` — the bridge becomes `impl Stream<Item = Msg>`; cancel-on-drop deletes `stream_cancel_tx` + the `cancel_rx` select arms |
| AbortStream | `self.streaming = None` |
| ExecuteTool (shell) | `ctx.spawn` of a stream merging preview-line batches + final outcome; interrupt stays a oneshot in the model (interrupt ≠ cancel: the tool must report its outcome) |
| ExecuteTool (read/edit/write) | inline in update, as today |
| ExecuteTool (history/output/skill), LoadSkill, FetchModels, WritePermissionRule, SaveModelSelection | `ctx.perform(..)`, detached where no reply matters |
| CheckPermission | sync fast paths inline; resolver via `ctx.perform` |
| ScheduleTimeout | `ctx.perform(sleep → Msg)` detached; the FSM's timeout_id staleness guard already handles late firing |
| Persist / ArchiveSession / usage cache | currently `block_on` on the driver thread — becomes a detached `perform` with cloned data; `SessionManager` needs to be shareable (Arc) — **port work item** |
| ExitApp | `ctx.exit(action)` |

## File-by-file mapping

| atuin-ai file | fate |
|---|---|
| `commands/inline.rs` (648) | setup/auth/resume logic keeps; the channel + `DriverEventSender` + `Application::builder` + `on_commit` + `spawn_blocking` wiring (~150 lines) replaced by `AiApp` construction + `driver_tokio::run` |
| `driver.rs` (1168) | **dissolves**: `DriverEvent` → `Msg`; `run_driver` → `App::update`; `translate_tui_event` → keymap + update arms; `sync_view_state` + `ViewState` + `build_view_state` → deleted (the FSM ctx *is* the model; `tail` borrows it); `run_stream_bridge` → `stream_bridge() -> impl Stream<Item = Msg>` |
| `tui/events.rs` | `AiTuiEvent` deleted (folded into `Msg`); `PermissionResult` keeps |
| `tui/state.rs` | unchanged (`ConversationEvent`, `events_to_messages`); `AppMode` dissolves into keymap derivation |
| `tui/view/mod.rs` (1103) | `ai_view` → `tail()` + plain `&data -> impl Element + use<>` view fns; `element!` → fluent builders; every `key:` prop deleted; the diff/write/shell/group views port near-1:1 (already proven in the Phase 1 spike) |
| `tui/view/turn.rs` | unchanged (pure) |
| `tui/components/atuin_ai.rs` (143) | **deleted** → `keymap()` |
| `tui/components/input_box.rs` (220) | **deleted** → `TextAreaState` in model + `panel(text_area(..))` in view + keymap arms; drops the `tui-textarea` dependency |
| `tui/components/select.rs` (95) | → `SelectState` value + view fn (~30 lines); candidate for promotion into the library during Phase 6 |
| `tui/components/session_continue.rs` (49) | **deleted** → a `String` computed once at startup |
| `tui/components/markdown.rs` (210) | **deleted** → `eye_declare::markdown` (already ported) |
| `tui/slash.rs` | unchanged; search called from update |
| `fsm/*`, `stream.rs`, tools/permissions/session/usage | unchanged |

## Port slices (one commit each, on the atuin branch)

1. **Skeleton** — branch, package-renamed dep, `AiApp` + `Msg`, `tail()`
   rendering a fixture conversation through `TurnBuilder`; headless test via
   `eye_declare_engine::test_terminal::TestTerminal`.
2. **Input path** — `TextAreaState`, full keymap policy table, submit →
   `fsm.handle(UserSubmit)`, user turns pushed; slash search in update.
3. **Streaming** — `stream_bridge` as a `Msg` stream, StartStream/AbortStream
   via `Task`, spinner (free via `animated()`), agent turn sealed on Idle.
4. **Tools** — shell preview streams, inline read/edit/write, permission
   select flow, interrupts, timeouts.
5. **Pickers + periphery** — model picker, usage bar, session resume push,
   exit actions, initial prompt + background usage fetch (needs startup
   effects — see below).
6. **Deletion audit** — remove the four adapters and every orphaned type,
   measure the LOC delta, headless integration tests for the key flows
   (submit→stream→seal, permission prompt, Esc semantics per mode).

## Expected library additions (driven by the port, built in eye_declare_next)

- **Startup effects** — the initial prompt and background usage fetch fire
  before any input arrives. Likely `App::init(&mut self, ctx)` (Elm's
  `init` with commands) or initial-message support in `driver_tokio::run`.
- **Keyboard enhancement flags** — Shift+Enter vs Enter requires the
  enhanced keyboard protocol; v1 had `KeyboardProtocol::Enhanced`, the v2
  runtime doesn't expose it yet.
- **`Keymap::merge` / `Subscriptions` merge** — expected when the select
  sub-model appears (recorded backlog item).
- Verify **Paste** flows through keymap fallthrough to `Msg::Input`.
- Verify **tall-tail behavior** (engine `present` with tail taller than the
  terminal) before relying on whole-turn tails; frontier pushing is the
  fallback.

## What this proves (beyond the four deletions)

- No per-transition world-cloning: `sync_view_state` cloned every event,
  the tool manager, and rebuilt all turns on every FSM step; v2 borrows.
- One thread of control: no Handle, no `blocking_recv`, no
  `update_tracked` dirty-avoidance tricks, no `exiting` AtomicBool.
- Esc-cancels-generation is `self.streaming = None`.
- History renders exactly once (pushed at startup), not every frame.
