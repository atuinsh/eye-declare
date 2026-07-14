//! Port 2: `file_edit_tool_view` / `file_write_tool_view`, from
//! `~/src/atuin/crates/atuin-ai/src/tui/view/mod.rs` (lines ~640–830 at the
//! time of porting). This is the nesting stress test: the original's diff
//! body is a `#(for ...)` containing a `#({ ... })` escape block that
//! pre-collects a `Vec` of 6-tuples, because the macro's `#(for)` cannot
//! thread mutable line-number state through iteration. In builder form the
//! stateful numbering runs directly inside a `.children(iter.map(...))`
//! closure and the intermediate collection disappears.
//!
//! As in Port 1: the `key: &str` parameters and every `key:` prop are
//! deleted outright.

use ratatui_core::style::{Color, Style};

use crate::fixtures::*;
use crate::ports::agent_turn::Msg;
use crate::ui::*;

type El = AnyElement<Msg>;

/// File edit tool call with diff preview.
pub fn file_edit_tool_view(
    status: &ToolResultStatus,
    path: &std::path::Path,
    preview: Option<&EditPreview>,
) -> El {
    let display_path = format_path_for_display(path);
    let status_line = tool_status_line(status, "Editing", "Edited", "Edit", &display_path, "");

    let Some(preview) = preview else {
        return status_line;
    };
    if preview.hunks.is_empty() {
        return status_line;
    }

    let gutter_w = gutter_width(preview.max_line_number());

    col()
        .child(status_line)
        .child(
            col()
                .children(preview.hunks.iter().map(|hunk| hunk_view(hunk, gutter_w)))
                .pad_left(2),
        )
        .any()
}

/// One diff hunk: gutter-numbered context/removed/added lines.
///
/// The original pre-collected `lines_rendered: Vec<(idx, prefix, text, style,
/// gutter_text, gutter_style)>` in an escape block, then looped over it with
/// `#(for)`. Here the `before_pos`/`after_pos` counters mutate inside the
/// `map` closure directly. (The original's separate `gutter_style` was always
/// equal to the line style, so the pair collapses to one.)
fn hunk_view(hunk: &DiffHunk, gutter_w: u16) -> impl Element<Msg> + use<> {
    let mut before_pos = hunk.before_start;
    let mut after_pos = hunk.after_start;
    let num_w = (gutter_w - 1) as usize;

    col().children(hunk.lines.iter().map(move |line| {
        let (prefix, content, style, gutter) = match line {
            DiffLine::Context(t) => {
                let num = format!("{after_pos:>num_w$}");
                before_pos += 1;
                after_pos += 1;
                (" ", t, Style::default().fg(Color::DarkGray), num)
            }
            DiffLine::Removed(t) => {
                let num = format!("{before_pos:>num_w$}");
                before_pos += 1;
                ("-", t, Style::default().fg(Color::Red), num)
            }
            DiffLine::Added(t) => {
                let num = format!("{after_pos:>num_w$}");
                after_pos += 1;
                ("+", t, Style::default().fg(Color::Green), num)
            }
        };

        row()
            .fixed(gutter_w, text(gutter).style(style))
            .fill(text(prefix).style(style).span(content.clone(), style))
    }))
}

/// File write tool call with content preview.
pub fn file_write_tool_view(
    status: &ToolResultStatus,
    path: &std::path::Path,
    preview: Option<&WritePreview>,
) -> El {
    let display_path = format_path_for_display(path);
    let line_info = match (status, preview) {
        (ToolResultStatus::Success, Some(p)) => format!(" ({} lines)", p.total_lines),
        _ => String::new(),
    };
    let status_line = tool_status_line(
        status,
        "Writing",
        "Wrote",
        "Write",
        &display_path,
        &line_info,
    );

    let Some(preview) = preview else {
        return status_line;
    };
    if preview.lines.is_empty() {
        return status_line;
    }

    let gutter_w = gutter_width(preview.total_lines);
    let num_w = (gutter_w - 1) as usize;
    let remaining = preview.remaining_lines();
    let dim = Style::default().fg(Color::DarkGray);

    col()
        .child(status_line)
        .child(
            col()
                .children(preview.lines.iter().enumerate().map(|(idx, line)| {
                    row()
                        .fixed(gutter_w, text(format!("{:>num_w$}", idx + 1)).style(dim))
                        .fill(text(line.clone()).style(dim))
                }))
                .when(remaining > 0, |c| {
                    c.child(text(format!("     ... +{remaining} more lines")).style(dim))
                })
                .pad_left(2),
        )
        .any()
}

/// Line-number gutter width for the highest displayed number, plus spacing.
fn gutter_width(max_line_num: usize) -> u16 {
    max_line_num.to_string().len().max(2) as u16 + 1
}

/// Shared pending/success/error status line for edit and write.
///
/// The original duplicated this ~25-line match in both views (differing only
/// in verbs and the write view's `line_info` suffix); the builder form makes
/// the extraction natural since the result is just a value.
fn tool_status_line(
    status: &ToolResultStatus,
    doing: &str,
    did: &str,
    noun: &str,
    display_path: &str,
    suffix: &str,
) -> El {
    match status {
        ToolResultStatus::Pending => spinner(format!("{doing}: {display_path}"))
            .label_style(Style::default().fg(Color::Yellow))
            .done(false)
            .any(),
        ToolResultStatus::Success => spinner(format!("{did}: {display_path}{suffix}"))
            .done(true)
            .any(),
        ToolResultStatus::Error => text("✗ ")
            .style(Style::default().fg(Color::Red))
            .span(
                format!("{noun} {display_path}: failed"),
                Style::default().fg(Color::Red),
            )
            .any(),
    }
}

/// Simplified vs the original (no cwd/home relativization), as in Port 1.
fn format_path_for_display(path: &std::path::Path) -> String {
    path.display().to_string()
}
