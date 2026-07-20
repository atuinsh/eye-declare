//! Per-frame cost of the timeline pipeline, by scenario and by stage.
//!
//! Scenario benches run the full path (tree build → height → render →
//! diff → escape bytes) through `Runtime`; stage benches isolate the
//! phases so a regression is attributable from the numbers alone.

#[path = "support/scenario.rs"]
mod scenario;

use criterion::{Criterion, criterion_group, criterion_main};
use eye_declare_next::{Element, Runtime};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use scenario::{ChatApp, HEIGHT, Msg, WIDTH, markdown_response};
use std::hint::black_box;

const SIZES: &[usize] = &[1_000, 10_000, 50_000];

/// Steady tail: nothing changed — the diff-to-zero path the design
/// promises is "unconditionally cheap". Includes full tree rebuild.
fn steady_present(c: &mut Criterion) {
    let mut g = c.benchmark_group("steady_present");
    for &size in SIZES {
        let mut rt = Runtime::new(ChatApp::mid_stream(size), WIDTH, HEIGHT);
        let _ = rt.present();
        g.bench_function(format!("{size}B"), |b| {
            b.iter(|| black_box(rt.present()));
        });
    }
    g.finish();
}

/// One streamed chunk lands: update + full re-present of a tail carrying
/// `size` bytes of markdown. The streaming hot path.
fn streaming_chunk(c: &mut Criterion) {
    let mut g = c.benchmark_group("streaming_chunk");
    for &size in SIZES {
        let mut rt = Runtime::new(ChatApp::mid_stream(size), WIDTH, HEIGHT);
        let _ = rt.present();
        g.bench_function(format!("{size}B"), |b| {
            b.iter(|| black_box(rt.process(Msg::Chunk("lorem ipsum dolor ".into()))));
        });
    }
    g.finish();
}

/// Sealing the streamed turn into scrollback (push + re-present).
fn seal(c: &mut Criterion) {
    let mut g = c.benchmark_group("seal");
    for &size in SIZES {
        g.bench_function(format!("{size}B"), |b| {
            b.iter_batched(
                || {
                    let mut rt = Runtime::new(ChatApp::mid_stream(size), WIDTH, HEIGHT);
                    let _ = rt.present();
                    rt
                },
                |mut rt| black_box(rt.process(Msg::Seal)),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

/// One keystroke into the text area with a mid-size response on screen.
fn typing(c: &mut Criterion) {
    let mut rt = Runtime::new(ChatApp::mid_stream(10_000), WIDTH, HEIGHT);
    let _ = rt.present();
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::NONE,
    );
    c.bench_function("typing_keystroke", |b| {
        b.iter(|| {
            black_box(rt.process(Msg::Input(eye_declare_next::InputEvent::Key(key))));
        });
    });
}

/// Stage isolation: tree build, height, render, at 10KB.
fn stages(c: &mut Criterion) {
    let app = ChatApp::mid_stream(10_000);

    c.bench_function("stage_tree_build", |b| {
        b.iter(|| black_box(app.tail_element()));
    });

    c.bench_function("stage_height", |b| {
        let tail = app.tail_element();
        b.iter(|| black_box(tail.height(WIDTH)));
    });

    c.bench_function("stage_render", |b| {
        let tail = app.tail_element();
        let h = tail.height(WIDTH);
        b.iter(|| {
            let mut buf = Buffer::empty(Rect::new(0, 0, WIDTH, h));
            tail.render(buf.area, &mut buf);
            black_box(&buf);
        });
    });

    c.bench_function("stage_markdown_parse", |b| {
        let source = markdown_response(10_000);
        b.iter(|| {
            let el = eye_declare_next::markdown(source.clone());
            black_box(el.height(WIDTH))
        });
    });
}

trait TailExt {
    fn tail_element(&self) -> Box<dyn Element + '_>;
}
impl TailExt for ChatApp {
    fn tail_element(&self) -> Box<dyn Element + '_> {
        Box::new(eye_declare_next::App::tail(self))
    }
}

criterion_group!(
    benches,
    steady_present,
    streaming_chunk,
    seal,
    typing,
    stages
);
criterion_main!(benches);
