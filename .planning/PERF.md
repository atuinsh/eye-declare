# Performance pass (2026-07-20, stack layer mkt/v2-8-perf)

Measured with `cargo run --release -p eye_declare_next --example perf_report`
(deterministic: exact allocation counts via a wrapping global allocator) and
`benches/frame.rs` (criterion). Scenario: an atuin-shaped chat app — one
streaming markdown response of N bytes in the tail + panel/text-area input —
on a 100×40 terminal, driven headlessly.

## Results (per frame, 10KB markdown in the tail)

| scenario              | before            | after            |
|-----------------------|-------------------|------------------|
| steady present        | 2276µs / 10.1K allocs | 784µs / 2.3K allocs |
| streaming chunk       | 2560µs / 10.8K    | 901µs / 2.6K     |
| typing keystroke      | 2278µs / 10.1K    | 790µs / 2.3K     |
| 64-chunk burst        | 99ms (64 frames)  | 1.7ms (1 frame)  |

At 50KB the frame cost is ~3.7ms (was 11.3ms). Scaling with content size
remains linear — see "what we didn't do."

## What changed

1. **Markdown same-frame caches** (`markdown.rs`): parse once per element
   lifetime (= per frame) instead of once in `height` and again in
   `render`; wrapped row count memoized per width — containers re-ask
   `height()` at every layer, so this fired 3-4× per frame. Render uses a
   borrowed view of the cached parse (no per-span string clones). No
   cross-frame invalidation story needed: elements are rebuilt per frame.
2. **Diff clamped to reachable rows** (`Frame::diff_from`): rows past the
   scrollback boundary are physically immutable; the engine now skips
   comparing them instead of filtering them out afterward. With a tail
   much taller than the terminal this is diffing one screenful instead of
   the whole virtual frame.
3. **Message coalescing** (`Runtime::process_batch` + driver drain): the
   tokio driver drains everything queued (≤256) into one batch and
   presents once. A stream burst of 64 chunks costs one frame, not 64 —
   the 61× number above. This is the fix for streams being O(n²) in
   practice: per-chunk frames were the multiplier on the per-frame cost.

## The arena question: answered no

Building the element tree — the thing an arena allocator would optimize —
costs 0.5µs and 16 allocations per frame. It was never the problem; the
allocations were markdown span strings (fixed by caching/borrowing) and
per-frame re-parsing (fixed by memoization). An arena would complicate
`AnyElement`'s boxing story for a stage that is 0.06% of the frame.

## What we deliberately didn't do

- **O(visible) rendering.** `render` still paints the full virtual frame
  and `height` still wraps all content, so per-frame cost stays O(tail
  content). Fixing that means cooperative render clipping through the
  element tree, and it wouldn't help the dominant case anyway (a single
  tall markdown block has no children to cull). The architectural answer
  is the one the spec already gives: **keep tails bounded** — push content
  that can no longer change (the frontier pattern), and the per-frame cost
  follows the live content, not the conversation. 780µs at 10KB /
  12.5fps animation ≈ 1% CPU, which is acceptable for the unbounded case;
  apps that stream 100KB+ responses should seal at safe markdown
  boundaries.
- **Buffer recycling.** The engine retains each presented frame as `prev`,
  so reuse needs a swap API for ~10% of one stage. Not worth it today.
- **`Text` same-frame caches.** Leaf text is label-sized in practice;
  tree-build measurements say it's noise. The `Markdown` pattern applies
  directly if a profile ever disagrees.

## Regression harness

`benches/frame.rs` (criterion) and `examples/perf_report.rs` stay in the
tree. The report prints exact alloc counts — run it before and after
render-path changes; the numbers are deterministic.
