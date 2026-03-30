//! Data children: typed child collection with `#[component]`.
//!
//! Demonstrates using `children = DataChildren<T>` to accept typed
//! children in the `element!` macro. Unlike slot children (Elements),
//! data children are collected into a typed Vec and accessed by the
//! component function — useful for components that need structured
//! input rather than arbitrary element trees.
//!
//! Run with: cargo run --example data_children

use std::io::{self, Write};

use eye_declare::{
    BorderType, Canvas, DataChildren, Elements, InlineRenderer, VStack, View, component, element,
    props,
};
use ratatui_core::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use ratatui_widgets::paragraph::Paragraph;

// ---------------------------------------------------------------------------
// Item — a data child type for our Table component
// ---------------------------------------------------------------------------

/// A single row in the table. This is a data child, not a Component.
#[derive(Clone)]
struct Item {
    label: String,
    value: String,
    style: Style,
}

impl Item {
    fn builder() -> ItemBuilder {
        ItemBuilder {
            label: String::new(),
            value: String::new(),
            style: Style::default(),
        }
    }
}

struct ItemBuilder {
    label: String,
    value: String,
    style: Style,
}

impl ItemBuilder {
    fn label(mut self, l: impl Into<String>) -> Self {
        self.label = l.into();
        self
    }
    fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }
    fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    fn build(self) -> Item {
        Item {
            label: self.label,
            value: self.value,
            style: self.style,
        }
    }
}

// ---------------------------------------------------------------------------
// TableChild — the child enum + From conversions
// ---------------------------------------------------------------------------

enum TableChild {
    Item(Item),
}

impl From<Item> for TableChild {
    fn from(item: Item) -> Self {
        TableChild::Item(item)
    }
}

// ---------------------------------------------------------------------------
// Table — a #[component] with data children
// ---------------------------------------------------------------------------

#[props]
struct Table {
    title: String,
}

#[component(props = Table, children = DataChildren<TableChild>)]
fn table(props: &Table, children: &DataChildren<TableChild>) -> Elements {
    let items: Vec<&Item> = children
        .as_slice()
        .iter()
        .map(|c| match c {
            TableChild::Item(item) => item,
        })
        .collect();

    // Find the longest label for alignment
    let max_label = items.iter().map(|i| i.label.len()).max().unwrap_or(0);

    // Build styled lines for each item
    let lines: Vec<ratatui_core::text::Line<'static>> = items
        .iter()
        .map(|item| {
            let padded_label = format!("{:>width$}", item.label, width = max_label);
            ratatui_core::text::Line::from(vec![
                ratatui_core::text::Span::styled(
                    padded_label,
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                ratatui_core::text::Span::styled("  ", Style::default()),
                ratatui_core::text::Span::styled(item.value.clone(), item.style),
            ])
        })
        .collect();

    let title = props.title.clone();
    element! {
        View(
            border: BorderType::Rounded,
            border_style: Style::default().fg(Color::DarkGray),
            title: title,
            title_style: Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            padding_left: Some(eye_declare::Cells(1)),
            padding_right: Some(eye_declare::Cells(1)),
        ) {
            Canvas(render_fn: move |area: Rect, buf: &mut Buffer| {
                Paragraph::new(lines.clone()).render(area, buf);
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (width, _) = crossterm::terminal::size()?;
    let mut r = InlineRenderer::new(width);
    let mut stdout = io::stdout();

    let container = r.push(VStack);

    let els = element! {
        Table(title: "System Info") {
            Item(label: "OS", value: "macOS 15.3", style: Style::default().fg(Color::Cyan))
            Item(label: "Rust", value: "1.86.0", style: Style::default().fg(Color::Yellow))
            Item(label: "Terminal", value: format!("{}×cols", width), style: Style::default().fg(Color::Green))
        }

        // Table without children — uses default empty data
        Table(title: "Empty Table")

        Table(title: "Status") {
            Item(label: "Build", value: "passing", style: Style::default().fg(Color::Green))
            Item(label: "Tests", value: "243 passed", style: Style::default().fg(Color::Green))
            Item(label: "Coverage", value: "87%", style: Style::default().fg(Color::Yellow))
        }
    };

    r.rebuild(container, els);
    let output = r.render();
    stdout.write_all(&output)?;
    stdout.flush()?;

    println!();
    Ok(())
}
