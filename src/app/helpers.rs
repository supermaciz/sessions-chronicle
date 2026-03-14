use crate::ui::{
    session_detail::SessionDetailMsg, session_list::SessionListMsg,
    tool_inspector_pane::ToolInspectorPaneMsg,
};

use super::types::{
    AnalyticsIndexingOutcome, EscapeResolution, ReindexAction, UtilityPaneMode, Workspace,
    WorkspaceHeaderVisibility,
};

pub(super) fn active_search_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn search_query_update_messages(query: String) -> (SessionListMsg, SessionDetailMsg) {
    let detail_query = active_search_query(&query);

    (
        SessionListMsg::SetSearchQuery(query),
        SessionDetailMsg::UpdateSearchQuery(detail_query),
    )
}

pub(super) fn workspace_allows_search(workspace: Workspace) -> bool {
    !workspace.is_analytics()
}

pub(super) fn resolve_search_mode_change(workspace: Workspace, enabled: bool) -> bool {
    workspace_allows_search(workspace) && enabled
}

pub(super) fn parent_session_load_failure_messages() -> (SessionDetailMsg, ToolInspectorPaneMsg) {
    (SessionDetailMsg::Clear, ToolInspectorPaneMsg::Clear)
}

pub(super) fn resolve_escape_action(
    search_visible: bool,
    detail_visible: bool,
    pane_open: bool,
    pane_mode: UtilityPaneMode,
) -> EscapeResolution {
    if search_visible {
        EscapeResolution::CloseSearch
    } else if detail_visible && pane_open && pane_mode == UtilityPaneMode::ToolInspector {
        EscapeResolution::CloseInspector
    } else if detail_visible {
        EscapeResolution::NavigateBack
    } else {
        EscapeResolution::Noop
    }
}

pub(super) fn transition_to_detail(pane_mode: &mut UtilityPaneMode, pane_open: &mut bool) {
    *pane_mode = UtilityPaneMode::ToolInspector;
    *pane_open = false;
}

pub(super) fn transition_to_list(pane_mode: &mut UtilityPaneMode, pane_open: &mut bool) {
    *pane_mode = UtilityPaneMode::Filters;
    *pane_open = true;
}

pub(super) fn detail_pop_sync_decision(
    suppress_next_pop_sync: bool,
    detail_visible: bool,
) -> (bool, bool) {
    if suppress_next_pop_sync {
        (false, false)
    } else {
        (detail_visible, false)
    }
}

pub(super) fn decide_reindex_action(indexing: bool) -> ReindexAction {
    if indexing {
        ReindexAction::AlreadyRunning
    } else {
        ReindexAction::StartFull
    }
}

pub(super) fn workspace_header_visibility(
    workspace: Workspace,
    detail_visible: bool,
    parent_session_present: bool,
) -> WorkspaceHeaderVisibility {
    if workspace.is_analytics() {
        WorkspaceHeaderVisibility {
            search_ui_visible: false,
            pane_controls_visible: false,
            detail_actions_visible: false,
            indexing_progress_visible: true,
        }
    } else {
        WorkspaceHeaderVisibility {
            search_ui_visible: true,
            pane_controls_visible: true,
            detail_actions_visible: detail_visible || parent_session_present,
            indexing_progress_visible: true,
        }
    }
}

pub(super) fn analytics_indexing_completion_outcome(
    active_workspace: Workspace,
) -> AnalyticsIndexingOutcome {
    AnalyticsIndexingOutcome {
        mark_stale: true,
        refresh_immediately: active_workspace.is_analytics(),
    }
}
