---
title: The Timeline
description: Push semantics, sealing, and what "committed" means
---

# The Timeline

Every eye-declare app manages two kinds of output:

- **Committed blocks** — finished output, emitted from `update` via
  `ctx.push`. Rendered exactly once at the current width, then owned by the
  terminal. They scroll into native scrollback like anything printed by any
  program before yours ran.
- **The live tail** — everything below the last committed block, described
  by `tail(&self)` and replaced wholesale every frame.

## Push is `println!` for elements

```rust
ctx.push(markdown(&finished_turn));
```

Everything about `push` follows from treating it as I/O:

- **It's irreversible.** There is no un-push, no editing a committed block.
  If content might still change, it isn't finished — keep it in the tail.
- **It renders immediately**, inside `update`, at the current terminal
  width. Blocks can therefore borrow freely from locals and the model; no
  `'static` requirements, no cloning into the framework.
- **Order is program order.** Multiple pushes in one update land in call
  order, above the tail.

## The seal pattern

The characteristic move of a streaming app: content lives in the tail while
it's changing, and is pushed the moment it can no longer change.

```rust
Msg::Chunk(delta) => self.reply.push_str(&delta),   // still changing: tail
Msg::StreamDone => {
    let reply = std::mem::take(&mut self.reply);
    ctx.push(assistant_turn(&reply));               // finished: commit
}
```

The engine makes sealing cheap: when the pushed block's content is what the
tail was already showing (the usual case), the commit costs a single
newline — nothing repaints, nothing is re-sent. The transcript simply stops
being live.

## Keep the tail bounded

Per-frame cost is proportional to the tail's content, not the
conversation's. A tail carrying 10KB of streaming markdown costs well under
a millisecond a frame; the cost scales linearly from there. The design
consequence: **push content as soon as it's final**. A chat app pushes each
turn as it completes rather than accumulating the conversation in the tail;
an app streaming something enormous should seal finished portions at safe
boundaries. See [performance](performance.md) for measurements.

One hazard to know if you keep your own index into data you're
progressively pushing (a "frontier"): if the underlying data can shrink —
a session reset, a cleared list — the frontier must reset with it, or new
content will be sliced out of view. Derive the frontier's validity from the
data, not from history.

## Resizes

Committed blocks are immutable, so on a width change they keep whatever
reflow the terminal itself applies to old output — the same thing that
happens to your shell prompt history. Only the live tail is erased and
re-rendered at the new width. If a piece of output should stay
width-responsive for a while, keep it in the tail longer; push it when that
stops mattering.
