//! Port 1: `agent_turn_view` and its helpers, from
//! `~/src/atuin/crates/atuin-ai/src/tui/view/mod.rs` (lines ~334–530 and
//! ~805–930 at the time of porting). Faithful to the original's structure so
//! the two read side-by-side; deliberate differences are called out in
//! comments and logged in FINDINGS.md.
//!
//! Notable wholesale deletions vs the original: every `key:` prop (no
//! reconciliation in v2, so element identity is meaningless) and the
//! `turn_id` parameter that existed only to build keys.

use ratatui_core::style::{Color, Modifier, Style};

use crate::fixtures::*;
use crate::ui::*;

/// Placeholder app message type. These views are display-only and never emit.
pub enum Msg {}

/// Type-erased element alias, for heterogeneous match arms. These views
/// clone their strings, so `'static` is accurate here; borrowing views
/// (Port 3) use the lifetime.
type El = AnyElement<'static, Msg>;

/// Max output lines shown for a shell command preview.
const MAX_SHELL_PREVIEW_LINES: u16 = 5;

/// Max entries shown under a tool group header.
const MAX_GROUP_ENTRIES: usize = 5;

pub fn agent_turn_view(
    events: &[UiEvent],
    busy: bool,
    showing_ui: bool,
) -> impl Element<Msg> + use<> {
    let label_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    col()
        .child(text(" Atuin AI ").style(label_style.add_modifier(Modifier::REVERSED)))
        .children(events.iter().enumerate().map(|(i, event)| {
            col()
                .when(i > 0, |c| c.child(text("")))
                .child(event_view(event))
        }))
        .when(busy && !showing_ui, |c| {
            c.child(
                spinner("")
                    .spinner_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                    .pad_left(2)
                    .pad_top(1),
            )
        })
}

/// The per-event dispatch that was a `#(match ...)` block inline in the
/// original. Native `match` needs a named function (or an immediate closure)
/// plus `.any()` per arm to unify types.
fn event_view(event: &UiEvent) -> El {
    match event {
        UiEvent::Text { content } => markdown(content.clone()).pad_left(2).any(),
        UiEvent::ToolSummary(summary) => tool_summary_view(summary).any(),
        UiEvent::SuggestedCommand(details) => suggested_command_view(details).any(),
        UiEvent::ToolCall(details) => tool_call_view(details).pad_left(2).any(),
        UiEvent::ToolGroup(group) => group_view(group).pad_left(2).any(),
        UiEvent::Other => empty().any(),
    }
}

fn tool_summary_view(summary: &ToolSummary) -> impl Element<Msg> + use<> {
    spinner(summary.summary()).done(!summary.any_pending())
}

fn tool_call_view(details: &ToolCallDetails) -> El {
    use crate::ports::file_edit::{file_edit_tool_view, file_write_tool_view};

    match &details.render_data {
        ToolRenderData::Shell { command, preview } => shell_tool_view(command, preview.as_ref()),
        ToolRenderData::Remote => tool_status_view(&details.name, &details.status),
        ToolRenderData::FileEdit { path, preview } => {
            file_edit_tool_view(&details.status, path, preview.as_ref())
        }
        ToolRenderData::FileWrite { path, preview } => {
            file_write_tool_view(&details.status, path, preview.as_ref())
        }
        ToolRenderData::FileRead { .. }
        | ToolRenderData::HistorySearch { .. }
        | ToolRenderData::SkillLoad => empty().any(),
    }
}

/// Status indicator for a non-preview tool call.
fn tool_status_view(name: &str, status: &ToolResultStatus) -> El {
    match status {
        ToolResultStatus::Pending => spinner(format!("Running: {name}"))
            .label_style(Style::default().fg(Color::Yellow))
            .done(false)
            .any(),
        ToolResultStatus::Success => spinner(format!("Ran: {name}")).done(true).any(),
        ToolResultStatus::Error => text("✗ ")
            .style(Style::default().fg(Color::Red))
            .span(format!("{name}: denied"), Style::default().fg(Color::Red))
            .any(),
    }
}

/// Shell command execution with live output viewport.
fn shell_tool_view(command: &str, preview: Option<&ToolPreview>) -> El {
    let done = preview.is_some_and(|p| p.exit_code.is_some() || p.interrupted.is_some());

    match preview {
        Some(preview) => col()
            .child(
                spinner(if done {
                    format!("Ran: {command}")
                } else {
                    format!("Running: {command}")
                })
                .done(done)
                .hide_checkmark(),
            )
            .child(
                row().fixed(2, text("└ ")).fill(
                    viewport(preview.lines.clone())
                        .height((preview.lines.len() as u16).clamp(1, MAX_SHELL_PREVIEW_LINES))
                        .style(Style::default().fg(Color::Gray))
                        .wrap(false),
                ),
            )
            .child(shell_tool_footer(preview, done))
            .any(),
        None => spinner(format!("Running: {command}"))
            .label_style(Style::default().fg(Color::Yellow))
            .done(false)
            .any(),
    }
}

fn shell_tool_footer(preview: &ToolPreview, done: bool) -> El {
    if let Some(reason) = &preview.interrupted {
        let label = match reason {
            InterruptReason::User => "Interrupted".to_string(),
            InterruptReason::Timeout(secs) => format!("Timed out ({secs}s)"),
        };
        return text(label)
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .any();
    }
    if !done {
        return text("[Ctrl+C] Interrupt")
            .style(Style::default().fg(Color::DarkGray))
            .any();
    }
    if let Some(code) = preview.exit_code {
        let style = if code == 0 {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        return text(format!("Exit code: {code}")).style(style).any();
    }
    empty().any()
}

// ───────────────────────────────────────────────────────────────────
// Tool groups
// ───────────────────────────────────────────────────────────────────

fn group_view(group: &ToolGroup) -> El {
    match group.kind {
        ToolGroupKind::FileRead => file_read_group_view(group).any(),
        ToolGroupKind::HistorySearch => history_search_group_view(group).any(),
    }
}

/// Tree-connector marker: `└ ` for the first visible row, spaces after.
fn tree_marker(is_first: bool) -> &'static str {
    if is_first { "└ " } else { "  " }
}

/// 2-char status marker column: ✓ / ✗ / blank.
fn status_marker_view(status: &ToolResultStatus) -> El {
    match status {
        ToolResultStatus::Pending => text("  ").any(),
        ToolResultStatus::Success => text("✓ ").style(Style::default().fg(Color::Green)).any(),
        ToolResultStatus::Error => text("✗ ").style(Style::default().fg(Color::Red)).any(),
    }
}

fn visible_group_calls(group: &ToolGroup) -> &[ToolCallDetails] {
    let start = group.calls.len().saturating_sub(MAX_GROUP_ENTRIES);
    &group.calls[start..]
}

/// One row in a grouped list: `[tree marker][status][content]`.
///
/// The original was an HStack with three `View(width: ...)` wrappers; the
/// `Width` cell model collapses it to three calls.
fn group_row_view(is_first: bool, status: &ToolResultStatus, content: El) -> El {
    row()
        .fixed(2, text(tree_marker(is_first)))
        .fixed(2, status_marker_view(status))
        .fill(content)
        .any()
}

fn file_read_group_view(group: &ToolGroup) -> impl Element<Msg> + use<> {
    let count = group.calls.len();
    let label = if count == 1 {
        "Read 1 file".to_string()
    } else {
        format!("Read {count} files")
    };
    let done = !group.any_pending();

    col()
        .child(spinner(label).done(done).hide_checkmark())
        .children(
            visible_group_calls(group)
                .iter()
                .enumerate()
                .map(|(i, details)| file_read_row(i == 0, details)),
        )
}

fn file_read_row(is_first: bool, details: &ToolCallDetails) -> El {
    let path_str = match &details.render_data {
        ToolRenderData::FileRead { path } => format_path_for_display(path),
        _ => String::new(),
    };

    group_row_view(is_first, &details.status, text(path_str).any())
}

fn history_search_group_view(group: &ToolGroup) -> impl Element<Msg> + use<> {
    let done = !group.any_pending();

    col()
        .child(
            spinner("Searched Atuin history:")
                .done(done)
                .hide_checkmark(),
        )
        .children(
            visible_group_calls(group)
                .iter()
                .enumerate()
                .map(|(i, details)| history_search_row(i == 0, details)),
        )
}

fn history_search_row(is_first: bool, details: &ToolCallDetails) -> El {
    let (query, filter_modes) = match &details.render_data {
        ToolRenderData::HistorySearch {
            query,
            filter_modes,
        } => (query.as_str(), filter_modes.as_slice()),
        _ => ("", [].as_slice()),
    };

    let filter_label = format_filter_modes(filter_modes);
    let filter_style = Style::default().fg(Color::DarkGray);

    let content = if query.trim().is_empty() {
        text("recent commands")
            .style(
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::ITALIC),
            )
            .when(!filter_label.is_empty(), |t| {
                t.span(" ", Style::default())
                    .span(&filter_label, filter_style)
            })
            .any()
    } else {
        text(query)
            .when(!filter_label.is_empty(), |t| {
                t.span(" ", Style::default())
                    .span(&filter_label, filter_style)
            })
            .any()
    };

    group_row_view(is_first, &details.status, content)
}

fn filter_mode_label(mode: &HistorySearchFilterMode) -> &'static str {
    match mode {
        HistorySearchFilterMode::Global => "global",
        HistorySearchFilterMode::Host => "host",
        HistorySearchFilterMode::Session => "session",
        HistorySearchFilterMode::Directory => "directory",
        HistorySearchFilterMode::Workspace => "workspace",
    }
}

fn format_filter_modes(modes: &[HistorySearchFilterMode]) -> String {
    if modes.is_empty() {
        return String::new();
    }
    let parts: Vec<&'static str> = modes.iter().map(filter_mode_label).collect();
    format!("({})", parts.join(", "))
}

/// Simplified vs the original (no cwd/home relativization) — path formatting
/// is not what this port is testing.
fn format_path_for_display(path: &std::path::Path) -> String {
    path.display().to_string()
}

// ───────────────────────────────────────────────────────────────────
// Suggested command — the `.when()` stress test
// ───────────────────────────────────────────────────────────────────

fn suggested_command_view(details: &SuggestedCommandDetails) -> impl Element<Msg> + use<> {
    let is_dangerous = matches!(
        details.danger_level,
        DangerLevel::High(_) | DangerLevel::Medium(_)
    );
    let danger_notes = details.danger_level.notes();
    let danger_style = match details.danger_level {
        DangerLevel::High(_) => Style::default().fg(Color::Red),
        DangerLevel::Medium(_) => Style::default().fg(Color::Yellow),
        DangerLevel::Low(_) | DangerLevel::Unknown(_) => Style::default().fg(Color::Green),
    };
    let danger_text = match details.danger_level {
        DangerLevel::High(_) => "High",
        DangerLevel::Medium(_) => "Medium",
        DangerLevel::Low(_) => "Low",
        DangerLevel::Unknown(_) => "Unknown",
    };

    let low_confidence = matches!(
        details.confidence_level,
        ConfidenceLevel::Low(_) | ConfidenceLevel::Medium(_)
    );
    let confidence_level = match details.confidence_level {
        ConfidenceLevel::Low(_) => "Low",
        ConfidenceLevel::Medium(_) => "Medium",
        ConfidenceLevel::High(_) => "High",
        ConfidenceLevel::Unknown(_) => "Unknown",
    };
    let confidence_notes = details.confidence_level.notes();

    col()
        .child(text("  Suggested command:").style(Style::default().fg(Color::Cyan)))
        .child(
            row()
                .fixed(
                    2,
                    if is_dangerous || low_confidence {
                        text("! ").style(Style::default().fg(Color::Yellow))
                    } else {
                        text("$ ").style(Style::default().fg(Color::Blue))
                    },
                )
                .fill(text(details.command.clone()).style(Style::default().fg(Color::Green))),
        )
        .when(is_dangerous, |c| {
            c.child(
                text("Danger: ")
                    .style(danger_style)
                    .span(danger_text, danger_style.add_modifier(Modifier::BOLD))
                    .pad_left(2),
            )
        })
        // `.when_some()` replaces the original's `cond && x.is_some()` guard
        // followed by `x.unwrap()` inside the body.
        .when_some(
            is_dangerous.then_some(danger_notes).flatten(),
            |c, notes| c.child(row().fixed(2, text("└")).fill(markdown(notes)).pad_left(2)),
        )
        .when(low_confidence, |c| {
            c.child(
                text("Confidence: ")
                    .style(Style::default().fg(Color::Blue))
                    .span(
                        confidence_level,
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    )
                    .pad_left(2),
            )
        })
        .when_some(
            low_confidence.then_some(confidence_notes).flatten(),
            |c, notes| c.child(row().fixed(2, text("└")).fill(markdown(notes)).pad_left(2)),
        )
}
