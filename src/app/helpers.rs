use crate::ui::{
    session_detail::SessionDetailMsg, session_list::SessionListMsg,
    tool_inspector_pane::ToolInspectorPaneMsg,
};

use super::types::{EscapeResolution, ReindexAction, UtilityPaneMode};

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

pub(super) fn transition_to_list(pane_mode: &mut UtilityPaneMode) {
    *pane_mode = UtilityPaneMode::Filters;
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
