---
title: Performance
description: What frames cost, and the patterns that keep them cheap
---

# Performance

The design invariant is that re-presenting the tail is unconditionally
cheap — cheap enough to do every frame with no dirty tracking. Measured on
the library's benchmark scenario (a streaming chat app on a 100×40
terminal, release build):

| scenario | cost per frame |
|---|---|
| unchanged tail, 10KB of markdown showing | ~780µs, ~2.3K allocs |
| one streamed chunk arriving | ~900µs |
| a keystroke | ~790µs |
| a burst of 64 queued chunks | ~1.7ms **total** (one frame) |
| building the element tree | 0.5µs, 16 allocs |

The pipeline already does the heavy lifting for you: expensive built-ins
cache within the frame, the engine skips diffing rows that have scrolled
past the reachable region, and the tokio driver coalesces queued messages
into one frame per batch — a fast LLM stream costs one present per burst,
not one per chunk.

## What's yours to manage

**Per-frame cost is O(tail content).** The frame costs above scale
linearly with how much lives in the tail. The fix is architectural, not
micro: [push content when it's final](the-timeline.md). An app that seals
each turn as it completes has a tail that stays small no matter how long
the session runs. At ordinary sizes the cost is negligible — 10KB of live
markdown at animation cadence is roughly 1% of a core — so this matters
when tails get genuinely large, not as a habit of fear.

**Cache expensive custom elements within the frame.** Elements are rebuilt
per frame, and `height` runs more than once per frame (measure, then
container placement). If your element parses, wraps, or lays out anything
non-trivial, share that work across calls with a `RefCell<Option<…>>` —
no invalidation needed, the cache dies with the frame. The built-in
`markdown()` is the reference implementation of this pattern.

**View builders are not the place to optimize.** Tree construction
measured at half a microsecond and sixteen allocations for a realistic
tail. Cloning a `String` into a `text()` is fine. (This is also why
there's no arena allocator: the thing it would optimize is already
0.06% of the frame.)

## Measuring

The regression harness ships in the repo: `benches/frame.rs` (criterion)
and `examples/perf_report.rs`, which prints **exact, deterministic
allocation counts** per scenario via a counting global allocator. If you're
changing render-path code — yours or the library's — run the report before
and after; the numbers don't wiggle.
