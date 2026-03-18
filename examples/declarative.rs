//! Declarative view-function pattern for building UIs.
//!
//! This example shows how to use `Elements` and `rebuild` to describe
//! the UI as a function of state, instead of imperative tree manipulation.
//!
//! Run with: cargo run --example declarative

use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use eye_declare::{Elements, InlineRenderer, MarkdownEl, NodeId, SpinnerEl, TextBlockEl, VStack};
use ratatui_core::style::{Color, Style};

// ---------------------------------------------------------------------------
// Application state — user-owned, not framework-managed
// ---------------------------------------------------------------------------

struct AppState {
    thinking: bool,
    messages: Vec<String>,
    tool_running: Option<String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            thinking: false,
            messages: Vec::new(),
            tool_running: None,
        }
    }
}

// ---------------------------------------------------------------------------
// View function: state in, elements out
// ---------------------------------------------------------------------------

fn chat_view(state: &AppState) -> Elements {
    let mut els = Elements::new();

    // Render all messages
    for msg in &state.messages {
        els.add(MarkdownEl::new(msg));
    }

    // Show thinking spinner if active
    if state.thinking {
        els.add(SpinnerEl::new("Thinking..."));
    }

    // Show tool call spinner if active
    if let Some(ref tool) = state.tool_running {
        els.add(SpinnerEl::new(format!("Running {}...", tool)));
    }

    // Separator at the bottom
    if !state.messages.is_empty() || state.thinking || state.tool_running.is_some() {
        els.add(TextBlockEl::new().line("---", Style::default().fg(Color::DarkGray)));
    }

    els
}

// ---------------------------------------------------------------------------
// Main: simulate an agent conversation
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    // Create a container for the declarative view
    let container = r.push(VStack);

    let mut state = AppState::new();

    // --- Phase 1: Start thinking ---
    state.thinking = true;
    r.rebuild(container, chat_view(&state));
    flush(&mut r, &mut stdout)?;

    // Animate spinner — tick the component state directly rather than
    // rebuilding, since rebuild would recreate the node and reset the frame.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1500) {
        let children = r.children(container).to_vec();
        if let Some(&spinner_id) = children.first() {
            if let Ok(spinner_state) = try_state_mut::<eye_declare::Spinner>(&mut r, spinner_id) {
                spinner_state.tick();
            }
        }
        flush(&mut r, &mut stdout)?;
        thread::sleep(Duration::from_millis(80));
    }

    // --- Phase 2: First response ---
    state.thinking = false;
    state.messages.push(
        "Here's a binary search implementation in Rust:\n\n\
         ```rust\n\
         fn binary_search(arr: &[i32], target: i32) -> Option<usize> {\n\
         \x20   let mut low = 0;\n\
         \x20   let mut high = arr.len();\n\
         \x20   while low < high {\n\
         \x20       let mid = low + (high - low) / 2;\n\
         \x20       match arr[mid].cmp(&target) {\n\
         \x20           std::cmp::Ordering::Less => low = mid + 1,\n\
         \x20           std::cmp::Ordering::Greater => high = mid,\n\
         \x20           std::cmp::Ordering::Equal => return Some(mid),\n\
         \x20       }\n\
         \x20   }\n\
         \x20   None\n\
         }\n\
         ```"
        .to_string(),
    );
    r.rebuild(container, chat_view(&state));
    flush(&mut r, &mut stdout)?;
    thread::sleep(Duration::from_millis(800));

    // --- Phase 3: Tool call ---
    state.tool_running = Some("cargo clippy".to_string());
    r.rebuild(container, chat_view(&state));
    flush(&mut r, &mut stdout)?;

    // Animate tool spinner
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(2000) {
        let children = r.children(container).to_vec();
        // The tool spinner is the second-to-last child (before separator)
        if children.len() >= 2 {
            let spinner_id = children[children.len() - 2];
            if let Ok(spinner_state) = try_state_mut::<eye_declare::Spinner>(&mut r, spinner_id) {
                spinner_state.tick();
            }
        }
        flush(&mut r, &mut stdout)?;
        thread::sleep(Duration::from_millis(80));
    }

    // --- Phase 4: Tool complete, add follow-up ---
    state.tool_running = None;
    state.messages.push(
        "The implementation passes **clippy** with no warnings. \
         The function takes a sorted slice and a target value, \
         returning `Some(index)` if found or `None` otherwise."
            .to_string(),
    );
    r.rebuild(container, chat_view(&state));
    flush(&mut r, &mut stdout)?;

    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn flush(r: &mut InlineRenderer, stdout: &mut impl Write) -> io::Result<()> {
    let output = r.render();
    if !output.is_empty() {
        stdout.write_all(&output)?;
        stdout.flush()?;
    }
    Ok(())
}

/// Try to get mutable state for a component, returning Err if type mismatch.
fn try_state_mut<C: eye_declare::Component>(
    r: &mut InlineRenderer,
    id: NodeId,
) -> Result<&mut eye_declare::Tracked<C::State>, ()> {
    // state_mut panics on type mismatch, so we just call it — in this
    // example we know the types. In production code you might want a
    // try_state_mut on Renderer.
    Ok(r.state_mut::<C>(id))
}
