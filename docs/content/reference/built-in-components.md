---
title: Built-in Components
description: Reference for TextBlock, Spinner, Markdown, VStack, HStack, and Column
---

# Built-in Components

eye-declare ships with a small set of built-in components that cover common TUI patterns.

## TextBlock

Styled text with display-time word wrapping. The most common component for displaying text.

### Basic usage

```rust
// Simple unstyled text
element! { "Hello, world!" }

// Styled text (imperative)
TextBlock::new()
    .line("Bold header", Style::default().add_modifier(Modifier::BOLD))
    .line("Normal text", Style::default())
    .unstyled("")  // empty line
```

### With Line and Span children

For multi-styled lines, use `Line` and `Span` children in the macro:

```rust
element! {
    TextBlock {
        Line {
            Span(
                text: "Status: ".into(),
                style: Style::default().fg(Color::DarkGray)
            )
            Span(
                text: "Online".into(),
                style: Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            )
        }
        Line {
            Span(text: "Last updated: 2 minutes ago".into())
        }
    }
}
```

`Line` and `Span` are data children — they're collected at build time and stored on the `TextBlock`, not as child components in the tree.

### Word wrapping

TextBlock automatically wraps text at word boundaries when content exceeds the available width. The wrapping is computed at render time using the actual allocated width, so it responds correctly to terminal resizes.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| (lines) | Built via `.line()` or `Line`/`Span` children | The text content |

## Spinner

An animated Braille spinner with a label. Commonly used to indicate ongoing work.

### Usage

```rust
// Basic
element! {
    Spinner(label: "Loading...".into())
}

// With key for reconciliation
element! {
    Spinner(key: "task-1", label: "Processing...".into())
}

// Done state — shows checkmark instead of animation
element! {
    Spinner(key: "task-1", label: "Complete".into(), done: true)
}
```

### Imperative construction

```rust
let spinner = Spinner::new("Loading...")
    .done("Completed!")
    .label_style(Style::default().fg(Color::Cyan))
    .spinner_style(Style::default().fg(Color::Yellow));
```

### How it animates

The Spinner registers an 80ms `use_interval` in its `lifecycle()` method. Each tick advances through a sequence of Braille characters. When `done` is `true`, it displays a checkmark symbol instead.

The animation state is preserved across rebuilds (via reconciliation), so changing the label doesn't restart the animation.

### Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `label` | `String` | `""` | Text displayed next to the spinner |
| `done` | `bool` | `false` | Show checkmark instead of animation |
| `label_style` | `Style` | default | Style for the label text |
| `spinner_style` | `Style` | default | Style for the spinner symbol |

## Markdown

Renders a subset of Markdown with terminal styling.

### Usage

```rust
element! {
    Markdown(source: "# Heading\n\nThis is **bold** and `inline code`.".into())
}
```

### Supported syntax

| Element | Syntax | Default style |
|---------|--------|---------------|
| Heading 1 | `# Title` | Bold + Cyan |
| Heading 2 | `## Title` | Bold + Cyan |
| Heading 3 | `### Title` | Bold + Cyan |
| Bold | `**text**` | Bold modifier |
| Italic | `*text*` | Italic modifier |
| Inline code | `` `code` `` | Yellow |
| Code block | ` ``` ` ... ` ``` ` | Green |
| List item | `- item` | Gray bullet |

### Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `source` | `String` | `""` | The Markdown text to render |

The component maintains a `MarkdownState` with default styles that can be customized.

## VStack

Vertical container — children stack top-to-bottom.

```rust
element! {
    VStack {
        "First"
        "Second"
        "Third"
    }
}
```

VStack renders nothing itself — it exists purely to group children with vertical layout. This is the default layout direction, so you only need VStack when you want an explicit named group.

## HStack

Horizontal container — children lay out left-to-right.

```rust
element! {
    HStack {
        Column(width: Fixed(20)) {
            "Sidebar"
        }
        Column {
            "Main content"
        }
    }
}
```

HStack renders nothing itself. Children declare their widths via `WidthConstraint`: `Fixed(n)` for exact columns, `Fill` (default) for remaining space split equally.

## Column

A width-constrained wrapper for children inside an `HStack`.

```rust
element! {
    HStack {
        Column(width: Fixed(3)) {
            Spinner(label: "".into())
        }
        Column {
            "Takes remaining space"
        }
    }
}
```

### Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `width` | `WidthConstraint` | `Fill` | How this column claims horizontal space |
