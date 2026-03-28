# API Simplification — Working Design Document

Status: **Active exploration** (2026-03-27)

This document captures the current state of our thinking about simplifying
the eye-declare component API. It's a living reference — not a spec.
Each section has a status: **exploring**, **decided**, or **parked**.

Comparisons drawn from [iocraft](https://github.com/ccbrown/iocraft)
and real-world usage in [atuin-ai TUI](/Users/binarymuse/src/atuin/crates/atuin-ai/src/tui).

---

## Pain Points (observed)

1. **`desired_height` duplication** — Leaf components compute the same
   layout in both `render()` and `desired_height()`. Markdown parses
   content twice per frame. InputBox computes chrome height in both.

2. **`children()` / `impl_slot_children!` split** — Three concepts tangled:
   `impl_slot_children!` (macro collection), `children()` (framework
   rendering), `content_inset()` (child placement). Confusing for authors.

3. **Composite vs leaf mental model** — Writing a leaf (render to buffer)
   and a composite (return Elements) are fundamentally different code
   paths. A bordered card that contains children needs all three of
   `render()`, `content_inset()`, and `impl_slot_children!`.

4. **TextBlock verbosity** — `TextBlock { Line { Span(text: "hello") } }`
   for a single styled line. Unstyled text has string literal sugar, but
   styled text still requires full nesting.

5. **Component boilerplate** — Every component needs a props struct,
   optional state struct, `impl Component`, `impl Default`. Even trivial
   stateless components are ~20 lines.

---

## 1. Render-and-Measure Unification

**Status: exploring**

### Problem

`desired_height(width, state)` and `render(area, buf, state)` duplicate
work. Every leaf does the same calculation twice per frame.

### Proposal

Combine measuring and rendering into a single pass. The renderer calls
`render()` into a scratch buffer, measures what was produced, then places
it. For containers, children heights are still summed from their
individual measurements.

Since content grows downward into scrollback (not a fixed viewport),
there's no layout-effect problem — we never need to measure first and
then render differently based on the measurement. The scratch buffer
*is* the final output.

### Concrete approach

```rust
// Before (two methods, duplicated work):
fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State);
fn desired_height(&self, width: u16, state: &Self::State) -> u16;

// After (one method, framework measures the result):
fn render(&self, area: Rect, buf: &mut Buffer, state: &Self::State);
// desired_height removed — framework renders into scratch buffer at
// full width and measures used rows
```

The framework changes:
- `measure_height()` for leaves: render into a scratch buffer, measure
  how many rows were used, return that as the height.
- `render_node()`: use the cached scratch buffer, blit into final buffer
  at the correct position.
- Containers still work the same: sum children heights + insets.

### Tracking buffer — smarter than scanning

Rather than rendering into a full-sized buffer and scanning for the last
non-empty row, we can wrap `Buffer` (or provide our own) to track where
the component actually draws. The wrapper intercepts writes and records
the max row touched. This gives us the measured height with zero
scanning overhead.

Could also make the buffer virtual/lazy — only allocate backing storage
for rows that get written to, similar to how a `Vec` grows. This avoids
the "how big should the scratch buffer be?" problem entirely:

```rust
// Conceptual sketch
struct MeasuringBuffer {
    width: u16,
    rows: Vec<Option<Vec<Cell>>>,  // sparse — None until written
    max_row: u16,                  // tracked on every write
}
```

The component writes wherever it wants; we track the extent. When it's
time to blit into the real buffer, only materialized rows are copied.

### Open questions

- Performance: rendering into a scratch buffer to measure is more
  expensive than a dedicated `desired_height()`. For simple components
  (Spinner: always 1 row) this is wasteful. Consider keeping
  `desired_height()` as an optional override hint — if provided, skip
  the scratch render during measure.
- Integration with ratatui: ratatui widgets expect `&mut Buffer`. We'd
  need our measuring wrapper to either implement the same interface or
  wrap a real Buffer with tracking. Wrapping a real Buffer and tracking
  `set_string`/`set_cell` calls via a newtype is likely simplest.

---

## 2/3. Unified `view()` — Merging Composite and Leaf Rendering

**Status: exploring**

### Problem

Today, leaf components paint into a `Buffer` via `render()`, while
composite components declare child trees via `children()`. A component
that does both (bordered card with children) must implement three things:
`render()` for chrome, `content_inset()` for child placement, and
`children()` / `impl_slot_children!` for the children themselves.

### Direction

Replace `render()` + `children()` + `content_inset()` with a single
`view()` method that returns an element tree. Leaf rendering becomes
a special `Canvas` element for raw buffer access.

```rust
trait Component: Send + Sync + 'static {
    type State: Send + Sync + Default + 'static;

    /// Return the element tree for this component.
    /// `children` contains slot children passed by the parent.
    fn view(&self, state: &Self::State, children: Elements) -> Elements;

    // lifecycle, events, focus — unchanged
    fn lifecycle(&self, hooks: &mut Hooks<Self::State>, state: &Self::State) {}
    fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult { ... }
    fn is_focusable(&self, state: &Self::State) -> bool { false }
    fn cursor_position(&self, area: Rect, state: &Self::State) -> Option<(u16, u16)> { None }
    fn initial_state(&self) -> Option<Self::State> { None }
}
```

### What this eliminates

- `render()` — replaced by returning elements from `view()`
- `desired_height()` — tree handles measurement (see section 1)
- `children()` — slot children arrive as a parameter to `view()`
- `content_inset()` — use layout components (View, Padding) in the tree
- `impl_slot_children!` — unnecessary; `view()` receives children directly

### What stays

- `lifecycle()` — hooks are orthogonal to rendering
- Event handling (`handle_event`, `handle_event_capture`)
- Focus (`is_focusable`, `cursor_position`)
- `initial_state()` for components with state
- Data children — see "Rethinking data children" below

### Rethinking data children (ChildCollector)

Today, data children (Line/Span for TextBlock) use `ChildCollector` with
separate collector types (`LineChildren`, `TextBlockChildren`). This is
type-safe but requires per-component trait implementations.

Alternative: a collector component could define a child enum and accept
multiple types via `From` conversions:

```rust
// The component defines what children it accepts
enum TextChild {
    Line(Line),
    Span(Span),
    StyledText { text: String, style: Style },
}

impl From<Line> for TextChild { ... }
impl From<Span> for TextChild { ... }
impl From<&str> for TextChild { ... }
```

Children passed in the `element!` macro would be converted via `Into`,
and the component matches against enum variants at render time to decide
layout. This is more flexible than the current approach — a component
can accept heterogeneous children without separate collector types.

Could be powered by a special trait (e.g., `DataCollector`) that
defines the accepted child type, distinct from slot children:

```rust
trait DataCollector {
    type Child;  // the enum type
    fn collect(self, children: Vec<Self::Child>) -> Self;
}
```

The `element!` macro would call `.into()` on each child before
collecting. Compile error if a child type doesn't implement
`Into<Self::Child>`.

This needs more design work but the direction feels right — From/Into
is idiomatic Rust and avoids the current proliferation of collector
types.

### Examples

**Leaf component (today vs proposed):**
```rust
// Today
impl Component for Badge {
    type State = ();
    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        let line = Line::from(Span::styled(&self.label, self.style));
        Paragraph::new(line).render(area, buf);
    }
    fn desired_height(&self, _width: u16, _state: &()) -> u16 { 1 }
}

// Proposed
impl Component for Badge {
    type State = ();
    fn view(&self, _state: &(), _children: Elements) -> Elements {
        element! {
            TextBlock { Line { Span(text: self.label.clone(), style: self.style) } }
        }
    }
}
```

**Composite container (today vs proposed):**
```rust
// Today
struct Card { title: String }
impl Component for Card {
    type State = ();
    fn render(&self, area: Rect, buf: &mut Buffer, _state: &()) {
        // manually draw borders and title
    }
    fn desired_height(&self, width: u16, _state: &()) -> u16 {
        2 // top + bottom border (children measured separately)
    }
    fn content_inset(&self, _state: &()) -> Insets {
        Insets { top: 1, bottom: 1, left: 1, right: 1 }
    }
    fn children(&self, _state: &(), slot: Option<Elements>) -> Option<Elements> { slot }
}
impl_slot_children!(Card);

// Proposed
impl Component for Card {
    type State = ();
    fn view(&self, _state: &(), children: Elements) -> Elements {
        element! {
            View(border: BorderStyle::Single, title: self.title.clone()) {
                #(children)
            }
        }
    }
}
```

**Raw buffer access (escape hatch):**
```rust
impl Component for CustomWidget {
    type State = ();
    fn view(&self, state: &(), _children: Elements) -> Elements {
        let data = self.data.clone();
        element! {
            Canvas(render: move |area: Rect, buf: &mut Buffer| {
                // raw ratatui widget rendering
            })
        }
    }
}
```

### Insets and containing children

Rather than a separate `content_inset()` method, insets become part of
the tree via layout components. The interesting question is how a
component that draws its own chrome positions children within it.

Idea — `Insets::containing()`:
```rust
fn view(&self, _state: &(), children: Elements) -> Elements {
    element! {
        Canvas(render: |area, buf| { /* draw border chrome */ })
        // Children laid out inside the inset region
        Inset(top: 1, bottom: 1, left: 1, right: 1) {
            #(children)
        }
    }
}
```

This is effectively what `content_inset()` does today, but expressed
declaratively in the tree rather than as a separate trait method.

### Multi-slot components

A card with separate title and body slots:

```rust
struct Card {
    title: Elements,  // title slot
}

impl Component for Card {
    type State = ();
    fn view(&self, _state: &(), body: Elements) -> Elements {
        element! {
            View(border: BorderStyle::Single) {
                // Title area
                View(padding_bottom: 1) {
                    #(self.title.clone())
                }
                // Body area
                #(body)
            }
        }
    }
}

// Usage:
element! {
    Card(title: element! { TextBlock(text: "My Card") }) {
        TextBlock(text: "Card body content here")
    }
}
```

Here the primary slot is `children` (body), and additional slots are
props containing `Elements`. Each slot's height is computed by the
framework as part of the normal tree layout — no special handling needed.

### Open questions

- **Canvas component**: How does `Canvas` interact with measurement?
  If `view()` eliminates `desired_height`, Canvas needs to know its own
  height. Options:
  - `Canvas(height: 1, render: ...)` — explicit height prop
  - Framework renders Canvas into measuring buffer and tracks extent
    (section 1's tracking buffer approach)
  - Canvas could have an optional `measure: |width| -> u16` prop
  - The tracking buffer from section 1 may solve this naturally — Canvas
    renders into it, framework reads max_row, done.

  The exact mechanics of how Canvas works in a mixed-mode tree (some
  nodes are element trees, Canvas nodes are raw buffer draws) needs
  more thought. The framework already distinguishes container vs leaf
  nodes; Canvas would be a special leaf that the framework renders
  directly rather than reconciling as a subtree.

- **Performance**: Every component now builds an element tree instead of
  rendering directly. For deep trees, this adds allocation overhead.
  Mitigated by reconciliation (unchanged subtrees reused). Worth
  benchmarking.

- **Backward compatibility**: This is a breaking change to the Component
  trait. Only atuin-ai uses eye-declare currently, and we're at v0.x,
  so a clean break is fine. No need for migration shims.

- **`cursor_position`**: Currently takes `area: Rect`. With `view()`,
  the component doesn't know its own area. May need to become relative
  coordinates, or receive area from the framework after layout.

---

## 4. View Component (Consolidating Layout)

**Status: exploring**

### Problem

VStack, HStack, Column, and manual border rendering are separate
concepts. In iocraft, a single `View` component handles direction,
borders, padding, and colors.

### Proposal

Add a `View` component that consolidates layout + chrome:

```rust
element! {
    View(direction: Row, border: BorderStyle::Single, padding: 1) {
        View(width: Fixed(20)) { /* sidebar */ }
        View { /* main content, fills remaining space */ }
    }
}
```

This is additive — VStack/HStack/Column remain as lightweight aliases.
View is sugar that composes them.

### Props sketch

```rust
struct View {
    direction: Direction,        // Column (default) or Row
    border: Option<BorderStyle>, // ratatui border styles
    border_style: Style,         // border color/modifiers
    title: Option<String>,       // top border label
    padding: Option<u16>,        // all sides
    padding_top: Option<u16>,    // specific overrides
    padding_bottom: Option<u16>,
    padding_left: Option<u16>,
    padding_right: Option<u16>,
    width: WidthConstraint,      // for Row children
    background: Option<Color>,
}
```

### Relationship to `view()` unification

If we do the `view()` unification (section 2/3), then View becomes the
primary way to express layout in any component's `view()` method. Without
the unification, View is still useful as a convenience component in
`element!` trees and view functions.

---

## 5. Component Boilerplate Reduction

**Status: exploring**

### Problem

Even trivial stateless components require:
- Props struct with `#[derive(Default)]`
- `impl Component` with `type State = ()`
- At minimum `render()` + `desired_height()` (or `view()` if unified)

### Near-term: `simple_component!` macro

A declarative macro for stateless components that produce an element tree:

```rust
simple_component!(Badge { label: String, style: Style } => |self| {
    element! {
        TextBlock {
            Line { Span(text: self.label.clone(), style: self.style) }
        }
    }
});
```

Expands to the struct definition, Default impl, and Component impl.

### Future: `#[component]` attribute macro

More comprehensive, handles stateful components too:

```rust
#[component]
fn Badge(label: String, style: Style) -> Elements {
    element! {
        TextBlock { Line { Span(text: label.clone(), style: style) } }
    }
}

#[component]
fn Counter(hooks: &mut Hooks<CounterState>) -> Elements {
    let count = hooks.use_state(|| 0);
    element! {
        TextBlock(text: format!("Count: {}", count))
    }
}
```

This is a larger undertaking. The `simple_component!` macro is a good
stepping stone — it validates the ergonomics before committing to a
proc macro.

---

## 6. Text Model Rethink

**Status: exploring**

### Problem

Styled text is verbose:
```rust
TextBlock { Line { Span(text: "hello", style: Style::default().bold()) } }
```

Unstyled text has string literal sugar (`"hello"` → TextBlock), but
styled single-line text still requires the full Line/Span nesting.

### Reframing: TextBlock is really just a View with text

The more you think about it, TextBlock is doing two things:
1. Vertical layout of lines (which is what VStack/View does)
2. Word wrapping of text content

If we have a `Text` component (inline text with optional style) and
`View` handles vertical stacking, then:

```rust
// Vertical text lines — just a View with Text children
View {
    Text(content: "Line one")
    Text(content: "Line two")
    Text(content: "Line three")
}

// Single styled text
Text(content: "hello", style: Style::default().bold())
```

### The inline styling problem

The hard case is mixed inline styling — "My **Title**" where different
spans within a single line have different styles. This is what
Line/Span solves today:

```rust
// Today: explicit span composition
TextBlock {
    Line {
        Span(text: "My ")
        Span(text: "Title", style: Style::default().bold())
    }
}
```

With a `Text` component, how do you express this? Options:

**Option A: Text accepts styled children (data collector)**
```rust
Text {
    "My "
    Span(text: "Title", style: Style::default().bold())
}
```
Text collects children as spans and lays them out inline (horizontally,
with word wrapping). String literals become unstyled spans. This uses
the data children / collector system — Text defines what it accepts.

**Option B: Text with inline markup**
```rust
Text(content: "My **Title**")  // markdown-ish
```
Implicit styling from lightweight markup. Limited but covers common
cases. Could combine with explicit style prop for the base style.

**Option C: HStack-like inline flow**
```rust
// Inline layout = horizontal text flow with wrapping
InlineText {
    Text(content: "My ")
    Text(content: "Title", style: Style::default().bold())
}
```
A container that lays out text children inline with word wrapping.
Distinct from HStack (which allocates column widths, not text flow).

### Where this lands

Option A feels most natural — `Text` is both a leaf (single styled
string) and a container (collects spans for inline layout):

```rust
// Simple — leaf
Text(content: "hello")
Text(content: "hello", style: Style::default().bold())

// Composed — inline spans with wrapping
Text {
    "My "
    Span(text: "Title", style: Style::default().bold())
}

// Multi-line — View handles vertical stacking
View {
    Text(content: "Line one")
    Text { "Line " Span(text: "two", style: bold) }
}
```

This would replace TextBlock entirely. The string literal sugar in
`element!` would produce `Text` instead of `TextBlock`.

### Open questions

- Word wrapping: a `Text` with mixed spans needs to wrap across span
  boundaries. Today `TextBlock` handles this via `wrap.rs`. `Text`
  would need the same logic.
- Does `Text` with children subsume `Line`? If Text lays out spans
  inline, there's no need for a separate `Line` concept — each `Text`
  *is* a line (or a wrapping paragraph).
- Relationship to `Markdown`: Markdown renders to Lines/Spans
  internally. With the new model, it would render to `Text` elements
  in a `View`. This is a deeper change but more consistent.

---

## 7. Effects and Async in Components

**Status: exploring (long-term)**

### Problem

The current hook system has `use_interval`, `use_mount`, and
`use_unmount` — all synchronous callbacks. There's no `use_effect`
(run a side effect when dependencies change) and no way to do async
work inside a component.

In atuin, async work (streaming responses, API calls) lives entirely
outside the component tree — spawned as tokio tasks that communicate
back via `Handle::update()`. This works, but it means the component
can't own its own async lifecycle. If a component is unmounted while
its async work is in flight, there's no automatic cancellation.

### What iocraft does

- `use_future()` — run an async closure tied to the component's
  lifetime. Cancelled on unmount.
- `use_effect()` — run a side effect when hash-based dependencies
  change. Synchronous, but combined with `use_future` covers most
  patterns.

### Questions to explore

**`use_effect(deps, callback)`:**
- When deps change, run callback. Classic React pattern.
- Dependency tracking: hash-based (like iocraft)? Explicit dep list?
  Or rely on the fact that `lifecycle()` is already re-called when
  props/state change, so effects declared conditionally already
  "react" to changes?
- Cleanup: should `use_effect` return a cleanup function that runs
  before the next effect or on unmount?

**Async in components:**
- A `use_async` or `use_future` hook that spawns a tokio task tied
  to the component's lifetime. On unmount, the task is cancelled
  (AbortHandle).
- The task would need a way to update the component's state — either
  a channel back to the framework, or a `State<T>` handle (like
  iocraft's copy-based state).
- This is deeply tied to the runtime model. Today, state mutation
  happens through `Tracked<S>` with framework-driven dirty checking.
  Async tasks would need a way to trigger re-renders from outside
  the normal lifecycle.

**Relationship to Handle:**
- `Handle::update()` already solves "async work updates state" at
  the application level. The question is whether per-component async
  is worth the complexity, or if the Handle pattern is good enough.
- Per-component async is most valuable when components are reusable
  and self-contained (e.g., a `StreamingMarkdown` component that
  owns its own streaming connection). If components are always
  app-specific, Handle is fine.

### No concrete proposal yet

This needs more real-world pain to drive the design. Capturing it
here so we don't lose the thread.

---

## 8. Domain Events as a First-Class Concept

**Status: exploring (long-term)**

### Problem

Today, domain events (component signals something application-specific)
are handled via callback props or context-injected channels. In atuin,
components capture an `mpsc::Sender<AiTuiEvent>` from context in their
`lifecycle()` hook, store it in state, then `.send()` events through it
in event handlers. This works but is boilerplate-heavy:

```rust
// Every component that emits domain events:
struct MyComponentState {
    tx: Option<mpsc::Sender<AppEvent>>,  // stored from context
}

fn lifecycle(&self, hooks: &mut Hooks<Self::State>, _state: &Self::State) {
    hooks.use_context::<mpsc::Sender<AppEvent>>(|tx, state| {
        state.tx = tx.cloned();
    });
}

fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
    // ... match event ...
    if let Some(ref tx) = state.read().tx {
        let _ = tx.send(AppEvent::Something);
    }
    EventResult::Consumed
}
```

This pattern repeats in every component that needs to communicate
upward. The DESIGN.md currently says "callback props first, context
when drilling becomes painful" — but in practice, context channels
became the go-to pattern in atuin immediately.

### Directions to consider

**Callback props (current recommendation):**
```rust
struct InputBox {
    on_submit: Option<Box<dyn Fn(String) + Send + Sync>>,
}
```
Pro: simple, traceable, no framework support needed.
Con: verbose type signatures, prop drilling through intermediate
components, closures that capture `Handle` are awkward.

**Framework-integrated event emission:**
What if the framework understood "this component emits events of type T"
and provided sugar for it?

```rust
// Hypothetical
impl Component for InputBox {
    type Event = InputEvent;  // associated type

    fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
        // ...
        self.emit(InputEvent::Submit(text));  // framework-provided
        EventResult::Consumed
    }
}

// Parent handles it:
element! {
    InputBox(on: |event: InputEvent| { /* ... */ })
}
```

Pro: eliminates channel boilerplate, type-safe.
Con: adds complexity to Component trait and element system. How does
this compose through intermediate components?

**Typed event bubbling:**
Domain events bubble up through the tree (like terminal events do
today), with parents optionally intercepting them:

```rust
// Child emits
self.emit(InputEvent::Submit(text));

// Any ancestor can handle
fn handle_domain_event(&self, event: &dyn Any, state: &mut Tracked<Self::State>) -> EventResult {
    if let Some(input_event) = event.downcast_ref::<InputEvent>() {
        // handle it
        EventResult::Consumed
    } else {
        EventResult::Ignored  // let it bubble
    }
}
```

Pro: no prop drilling, no context setup, natural tree-based routing.
Con: type erasure, harder to trace, similar debuggability concerns
as event buses (which DESIGN.md explicitly avoids).

**Enhanced context pattern:**
Keep the context-channel approach but reduce boilerplate with
framework support:

```rust
// Framework provides a typed event emitter
fn lifecycle(&self, hooks: &mut Hooks<Self::State>, _state: &Self::State) {
    hooks.use_emitter::<AppEvent>();  // auto-captures from context
}

// In event handler, emitter available directly:
fn handle_event(&self, event: &Event, state: &mut Tracked<Self::State>) -> EventResult {
    state.emit(AppEvent::Something);  // sugar over channel send
    EventResult::Consumed
}
```

This is a lighter touch — just reducing the boilerplate around the
pattern that already works, rather than introducing a new event model.

### No concrete proposal yet

The callback-vs-channel-vs-bubbling tradeoff needs more thought.
The right answer probably depends on whether we lean toward
self-contained reusable components (callbacks/bubbling) or
app-specific components (context channels). In practice, atuin's
components are mostly app-specific, which is why the context channel
pattern emerged naturally.

---

## Implementation Priority

Roughly ordered by risk/impact:

| # | Item | Risk | Impact | Depends on |
|---|------|------|--------|------------|
| 1 | View component | Low | High | Nothing |
| 2 | Render-and-measure unification | Medium | High | Nothing |
| 3 | Unified `view()` method | Medium-High | Very high | #2 |
| 4 | `impl_slot_children!` elimination | — | — | Comes free with #3 |
| 5 | Text model rethink (replace TextBlock) | Low-Med | High | #4 (View) |
| 6 | `simple_component!` macro | Low | Moderate | #3 (ideally) |
| 7 | `#[component]` attribute macro | High effort | High | #3, #6 |
| 8 | Effects / async in components | High | Moderate | #3 |
| 9 | Domain events | Medium | Moderate | #3 (likely) |

View component (#1) and TextBlock convenience (#5) can be done
independently at any time. The core architectural work is #2 → #3.
Items #8 and #9 are longer-term — captured to not lose the thread,
but need more real-world pain to drive concrete designs.

---

## References

- iocraft: https://github.com/ccbrown/iocraft
- iocraft calculator example: examples/calculator.rs
- atuin-ai TUI: /Users/binarymuse/src/atuin/crates/atuin-ai/src/tui
- Current DESIGN.md: .planning/DESIGN.md
