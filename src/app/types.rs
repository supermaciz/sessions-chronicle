use relm4::gtk::PackType;

use crate::models::session::Tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UtilityPaneMode {
    Filters,
    ToolInspector,
}

impl UtilityPaneMode {
    pub(super) fn stack_child_name(self) -> &'static str {
        match self {
            UtilityPaneMode::Filters => "filters",
            UtilityPaneMode::ToolInspector => "tool-inspector",
        }
    }

    pub(super) fn sidebar_position(self) -> PackType {
        match self {
            UtilityPaneMode::Filters => PackType::Start,
            UtilityPaneMode::ToolInspector => PackType::End,
        }
    }

    pub(super) fn sidebar_min_width(self) -> f64 {
        match self {
            UtilityPaneMode::Filters => 200.0,
            UtilityPaneMode::ToolInspector => 360.0,
        }
    }

    pub(super) fn sidebar_width_fraction(self) -> f64 {
        match self {
            UtilityPaneMode::Filters => 0.18,
            UtilityPaneMode::ToolInspector => 0.4,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ActiveSessionRef {
    pub(super) id: String,
    pub(super) tool: Tool,
    #[allow(dead_code)]
    pub(super) project_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReindexAction {
    AlreadyRunning,
    StartFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EscapeResolution {
    CloseSearch,
    CloseInspector,
    NavigateBack,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Workspace {
    Sessions,
    Analytics,
}

impl Workspace {
    pub(super) fn stack_name(self) -> &'static str {
        match self {
            Workspace::Sessions => "sessions",
            Workspace::Analytics => "analytics",
        }
    }

    pub(super) fn from_stack_name(name: &str) -> Option<Self> {
        match name {
            "sessions" => Some(Workspace::Sessions),
            "analytics" => Some(Workspace::Analytics),
            _ => None,
        }
    }

    pub(super) fn is_analytics(self) -> bool {
        self == Workspace::Analytics
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WorkspaceHeaderVisibility {
    pub(super) search_ui_visible: bool,
    pub(super) pane_controls_visible: bool,
    pub(super) detail_actions_visible: bool,
    pub(super) indexing_progress_visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AnalyticsIndexingOutcome {
    pub(super) mark_stale: bool,
    pub(super) refresh_immediately: bool,
}
