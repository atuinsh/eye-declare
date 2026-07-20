//! Deterministic allocation + byte-output report for the frame pipeline.
//!
//! Run with `cargo run --release -p eye_declare --example perf_report`.
//! Counts every heap allocation through a wrapping global allocator, so
//! the numbers are exact and reproducible — the arena-allocator question
//! gets answered with data, not vibes.

#[path = "../benches/support/scenario.rs"]
mod scenario;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use eye_declare::Runtime;
use scenario::{ChatApp, HEIGHT, Msg, WIDTH};

struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn measure<R>(label: &str, iters: u64, mut f: impl FnMut() -> R) {
    // Warm once so one-time setup doesn't pollute the steady state.
    let _ = f();
    let a0 = ALLOCS.load(Ordering::Relaxed);
    let b0 = BYTES.load(Ordering::Relaxed);
    let t0 = Instant::now();
    for _ in 0..iters {
        let _ = std::hint::black_box(f());
    }
    let dt = t0.elapsed();
    let allocs = (ALLOCS.load(Ordering::Relaxed) - a0) / iters;
    let bytes = (BYTES.load(Ordering::Relaxed) - b0) / iters;
    println!(
        "{label:<38} {:>9.1}µs {:>7} allocs {:>10} bytes",
        dt.as_secs_f64() * 1e6 / iters as f64,
        allocs,
        bytes,
    );
}

fn main() {
    println!(
        "{:<38} {:>11} {:>14} {:>16}",
        "scenario (per frame)", "time", "allocs", "alloc'd"
    );

    for size in [1_000usize, 10_000, 50_000] {
        let mut rt = Runtime::new(ChatApp::mid_stream(size), WIDTH, HEIGHT);
        let _ = rt.present();
        measure(&format!("steady_present {size}B"), 200, || rt.present());
    }

    for size in [1_000usize, 10_000, 50_000] {
        let mut rt = Runtime::new(ChatApp::mid_stream(size), WIDTH, HEIGHT);
        let _ = rt.present();
        measure(&format!("streaming_chunk {size}B"), 200, || {
            rt.process(Msg::Chunk("lorem ipsum dolor ".into()))
        });
    }

    {
        let mut rt = Runtime::new(ChatApp::mid_stream(10_000), WIDTH, HEIGHT);
        let _ = rt.present();
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::NONE,
        );
        measure("typing_keystroke (10KB on screen)", 200, || {
            rt.process(Msg::Input(eye_declare::InputEvent::Key(key)))
        });
    }

    // Coalescing: 64 stream chunks as one batch vs 64 frames.
    for &(label, batch) in &[("sequential", 1usize), ("batch=64", 64)] {
        let mut rt = Runtime::new(ChatApp::mid_stream(10_000), WIDTH, HEIGHT);
        let _ = rt.present();
        measure(&format!("64 chunks, {label} (10KB)"), 20, || {
            for _ in 0..(64 / batch) {
                let msgs = (0..batch).map(|_| Msg::Chunk("lorem ipsum dolor ".into()));
                let _ = std::hint::black_box(rt.process_batch(msgs));
            }
        });
    }

    // Stage isolation at 10KB: where does the frame go?
    {
        use eye_declare::{App, Element};
        let app = ChatApp::mid_stream(10_000);
        measure("stage: tree build only", 200, || {
            std::hint::black_box(app.tail()).animated()
        });
        let tail = app.tail();
        measure("stage: height only", 200, || tail.height(WIDTH));
        let h = tail.height(WIDTH);
        measure("stage: render only", 200, || {
            let area = ratatui_core::layout::Rect::new(0, 0, WIDTH, h);
            let mut buf = ratatui_core::buffer::Buffer::empty(area);
            tail.render(area, &mut buf);
            buf.area.height
        });
        let src = scenario::markdown_response(10_000);
        measure("stage: markdown height (1 parse)", 200, || {
            eye_declare::markdown(src.clone()).height(WIDTH)
        });
    }

    // Seal is not steady-state (each seal consumes the turn); report the
    // one-shot cost at each size.
    for size in [1_000usize, 10_000, 50_000] {
        let label = format!("seal {size}B (one-shot)");
        let mut rt = Runtime::new(ChatApp::mid_stream(size), WIDTH, HEIGHT);
        let _ = rt.present();
        let a0 = ALLOCS.load(Ordering::Relaxed);
        let t0 = Instant::now();
        let (bytes_out, _) = rt.process(Msg::Seal);
        let dt = t0.elapsed();
        let allocs = ALLOCS.load(Ordering::Relaxed) - a0;
        println!(
            "{label:<38} {:>9.1}µs {allocs:>7} allocs {:>10} bytes-out",
            dt.as_secs_f64() * 1e6,
            bytes_out.len(),
        );
    }
}
