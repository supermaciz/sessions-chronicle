use crate::{
    icon_names,
    models::{ProjectFilter, session::AiAssistant},
};

#[derive(Debug, Clone)]
pub(super) struct ActiveSessionRef {
    pub(super) id: String,
    pub(super) tool: AiAssistant,
    pub(super) project_name: String,
    pub(super) pinned: bool,
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

    pub(super) fn icon_name(self) -> &'static str {
        match self {
            Workspace::Sessions => "document-open-recent-symbolic",
            Workspace::Analytics => icon_names::GRAPH,
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

#[cfg(test)]
mod tests {
    use crate::icon_names;

    use super::Workspace;

    #[test]
    fn workspaces_expose_distinct_view_switcher_icons() {
        assert_eq!(
            Workspace::Sessions.icon_name(),
            "document-open-recent-symbolic"
        );
        assert_eq!(Workspace::Analytics.icon_name(), icon_names::GRAPH);
        assert_ne!(
            Workspace::Sessions.icon_name(),
            Workspace::Analytics.icon_name()
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WorkspaceHeaderVisibility {
    pub(super) search_ui_visible: bool,
    pub(super) date_filter_visible: bool,
    pub(super) pane_controls_visible: bool,
    pub(super) detail_actions_visible: bool,
    pub(super) summary_button_visible: bool,
    pub(super) indexing_progress_visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AnalyticsIndexingOutcome {
    pub(super) mark_stale: bool,
    pub(super) refresh_immediately: bool,
}

#[derive(Debug, Clone)]
pub(super) struct FilterState {
    pub(super) tools: Vec<AiAssistant>,
    pub(super) project_filter: ProjectFilter,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            tools: AiAssistant::ALL.to_vec(),
            project_filter: ProjectFilter::AllSessions,
        }
    }
}
