//! Simplified copies of the atuin-ai data types the ported views consume.
//! Shapes mirror `~/src/atuin/crates/atuin-ai/src/tui/view/turn.rs` closely
//! enough to preserve the control flow the views exercise (nested matches,
//! optional previews, grouped calls). Field-level fidelity is not the point.

use std::path::PathBuf;

pub enum UiEvent {
    Text { content: String },
    ToolSummary(ToolSummary),
    SuggestedCommand(SuggestedCommandDetails),
    ToolCall(ToolCallDetails),
    ToolGroup(ToolGroup),
    Other,
}

pub struct ToolSummary {
    pub label: String,
    pub pending: usize,
}

impl ToolSummary {
    pub fn summary(&self) -> String {
        self.label.clone()
    }

    pub fn any_pending(&self) -> bool {
        self.pending > 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolResultStatus {
    Pending,
    Success,
    Error,
}

pub struct ToolCallDetails {
    pub tool_use_id: String,
    pub name: String,
    pub status: ToolResultStatus,
    pub render_data: ToolRenderData,
}

pub enum ToolRenderData {
    Shell {
        command: String,
        preview: Option<ToolPreview>,
    },
    FileEdit {
        path: PathBuf,
    },
    FileWrite {
        path: PathBuf,
    },
    Remote,
    FileRead {
        path: PathBuf,
    },
    HistorySearch {
        query: String,
        filter_modes: Vec<HistorySearchFilterMode>,
    },
    SkillLoad,
}

#[derive(Clone)]
pub struct ToolPreview {
    pub lines: Vec<String>,
    pub exit_code: Option<i32>,
    pub interrupted: Option<InterruptReason>,
}

#[derive(Clone)]
pub enum InterruptReason {
    User,
    Timeout(u64),
}

pub enum HistorySearchFilterMode {
    Global,
    Host,
    Session,
    Directory,
    Workspace,
}

pub struct ToolGroup {
    pub kind: ToolGroupKind,
    pub calls: Vec<ToolCallDetails>,
}

impl ToolGroup {
    pub fn any_pending(&self) -> bool {
        self.calls
            .iter()
            .any(|c| c.status == ToolResultStatus::Pending)
    }
}

pub enum ToolGroupKind {
    FileRead,
    HistorySearch,
}

pub enum DangerLevel {
    High(Option<String>),
    Medium(Option<String>),
    Low(Option<String>),
    Unknown(Option<String>),
}

impl DangerLevel {
    pub fn notes(&self) -> Option<&str> {
        match self {
            Self::High(n) | Self::Medium(n) | Self::Low(n) | Self::Unknown(n) => n.as_deref(),
        }
    }
}

pub enum ConfidenceLevel {
    High(Option<String>),
    Medium(Option<String>),
    Low(Option<String>),
    Unknown(Option<String>),
}

impl ConfidenceLevel {
    pub fn notes(&self) -> Option<&str> {
        match self {
            Self::High(n) | Self::Medium(n) | Self::Low(n) | Self::Unknown(n) => n.as_deref(),
        }
    }
}

pub struct SuggestedCommandDetails {
    pub command: String,
    pub danger_level: DangerLevel,
    pub confidence_level: ConfidenceLevel,
}
