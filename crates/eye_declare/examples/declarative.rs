//! Declarative view-function pattern for building UIs.
//!
//! This example shows how to use `Elements` and `rebuild` to describe
//! the UI as a function of state, instead of imperative tree manipulation.
//! Spinners animate automatically via the tick registration system —
//! no manual ticking needed.
//!
//! Also demonstrates `view()` components: `Card` (composite container
//! using View for borders) and `Badge` (leaf using Canvas for raw rendering).
//!
//! Run with: cargo run --example declarative

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use eye_declare::{
    BorderType, Canvas, Elements, InlineRenderer, Markdown, Spinner, VStack, View, component,
    element, props,
};
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::{buffer::Buffer, layout::Rect, text::Line, widgets::Widget};
use ratatui_widgets::paragraph::Paragraph;

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
// Card: composite container using #[component] + #[props]
// ---------------------------------------------------------------------------

/// A bordered card with a title.
#[props]
struct Card {
    title: String,
}

#[component(props = Card, children = Elements)]
fn card(props: &Card, children: Elements) -> Elements {
    element!(
        View(
            border: BorderType::Rounded,
            border_style: Style::default().fg(Color::DarkGray),
            title: props.title.clone(),
            title_style: Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            padding_left: Some(eye_declare::Cells(1)),
            padding_right: Some(eye_declare::Cells(1)),
        ) {
            #(children)
        }
    )
}

// ---------------------------------------------------------------------------
// Badge: leaf component using #[component] + Canvas
// ---------------------------------------------------------------------------

/// A colored status badge.
#[props]
struct Badge {
    label: String,
    #[default(Color::Green)]
    color: Color,
}

#[component(props = Badge)]
fn badge(props: &Badge) -> Elements {
    let label = props.label.clone();
    let color = props.color;

    element!(
        Canvas(render_fn: move |area: Rect, buf: &mut Buffer| {
            let line = Line::styled(
                format!(" {} ", label),
                Style::default().fg(Color::Black).bg(color),
            );
            Paragraph::new(line).render(area, buf);
        })
    )
}

// ---------------------------------------------------------------------------
// View function: state in, elements out
// ---------------------------------------------------------------------------

fn chat_view(state: &AppState) -> Elements {
    element!(
        #(for (i, msg) in state.messages.iter().enumerate() {
            Card(key: format!("msg-{i}"), title: "Response") {
                Markdown(key: format!("msg-{i}"), source: msg.clone())
            }
        })

        #(if state.thinking {
            Spinner(key: "thinking", label: "Thinking...")
        })

        #(if let Some(ref tool) = state.tool_running {
            Spinner(key: "tool", label: format!("Running {}...", tool))
        })

        #(if !state.messages.is_empty() && state.tool_running.is_none() && !state.thinking {
            Badge(key: "done", label: "Done", color: Color::Green)
        })
    )
}

// ---------------------------------------------------------------------------
// Main: simulate an agent conversation
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    let container = r.push(VStack);
    let mut state = AppState::new();

    // --- Phase 1: Thinking ---
    state.thinking = true;
    r.rebuild(container, chat_view(&state));
    // Spinner animates automatically — just tick and render
    animate_while_active(&mut r, &mut stdout, Duration::from_millis(1500))?;

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
    // Spinner auto-animates
    animate_while_active(&mut r, &mut stdout, Duration::from_millis(2000))?;

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

/// Tick and render while there are active animations, up to a max duration.
fn animate_while_active(
    r: &mut InlineRenderer,
    stdout: &mut impl Write,
    max_duration: Duration,
) -> io::Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < max_duration && r.has_active() {
        r.tick();
        flush(r, stdout)?;
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
