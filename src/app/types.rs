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
