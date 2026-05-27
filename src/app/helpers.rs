use crate::models::{PerSourceResult, ProjectFilter, ProjectInfo, SourceStatus};
use crate::ui::{session_detail::SessionDetailMsg, session_list::SessionListMsg};

use super::types::{
    AnalyticsIndexingOutcome, EscapeResolution, ReindexAction, Workspace, WorkspaceHeaderVisibility,
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

pub(super) fn parent_session_load_failure_message() -> SessionDetailMsg {
    SessionDetailMsg::Clear
}

pub(super) fn resolve_escape_action(
    search_visible: bool,
    detail_visible: bool,
    inspector_open: bool,
) -> EscapeResolution {
    if search_visible {
        EscapeResolution::CloseSearch
    } else if detail_visible && inspector_open {
        EscapeResolution::CloseInspector
    } else if detail_visible {
        EscapeResolution::NavigateBack
    } else {
        EscapeResolution::Noop
    }
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
            date_filter_visible: false,
            pane_controls_visible: false,
            detail_actions_visible: false,
            indexing_progress_visible: true,
        }
    } else {
        WorkspaceHeaderVisibility {
            search_ui_visible: true,
            date_filter_visible: !detail_visible,
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

pub(super) fn should_reload_sessions_after_indexing(
    indexed: usize,
    removed: usize,
    pending_reindex_feedback: bool,
) -> bool {
    indexed > 0 || removed > 0 || pending_reindex_feedback
}

pub(super) fn banner_title(results: &[PerSourceResult]) -> Option<String> {
    let problematic = results
        .iter()
        .filter(|r| matches!(r.status, SourceStatus::Degraded | SourceStatus::Failed))
        .count();

    match problematic {
        0 => None,
        1 => Some("1 session source has indexing issues".to_string()),
        n => Some(format!("{n} session sources have indexing issues")),
    }
}

pub(super) fn banner_button_label(results: &[PerSourceResult]) -> Option<&'static str> {
    if banner_title(results).is_some() {
        Some("Details")
    } else {
        None
    }
}

pub(super) fn completion_toast_title(indexed: usize, errors: usize) -> String {
    if errors == 0 {
        format!("Index rebuilt — {indexed} sessions")
    } else {
        format!("Indexed {indexed} sessions with {errors} errors")
    }
}

pub(super) fn resolve_pin_shortcut_target(
    workspace: Workspace,
    active_session_id: Option<&str>,
    selected_session_id: Option<&str>,
) -> Option<String> {
    if workspace.is_analytics() {
        return None;
    }

    active_session_id
        .or(selected_session_id)
        .map(str::to_string)
}

pub(super) fn retained_project_filter(
    selected: &ProjectFilter,
    projects: &[ProjectInfo],
    show_unassigned: bool,
) -> ProjectFilter {
    match selected {
        ProjectFilter::AllSessions => ProjectFilter::AllSessions,
        ProjectFilter::Pinned => ProjectFilter::Pinned,
        ProjectFilter::Unassigned => {
            if show_unassigned {
                ProjectFilter::Unassigned
            } else {
                ProjectFilter::AllSessions
            }
        }
        ProjectFilter::Project(project_id) => {
            if projects.iter().any(|project| project.id == *project_id) {
                ProjectFilter::Project(*project_id)
            } else {
                ProjectFilter::AllSessions
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AiAssistant, PerSourceResult, ProjectFilter, ProjectInfo, SourceStatus};

    fn make_result(assistant: AiAssistant, status: SourceStatus) -> PerSourceResult {
        PerSourceResult {
            assistant,
            display_path: "/tmp/test".into(),
            indexed: match status {
                SourceStatus::Indexed | SourceStatus::Degraded => 1,
                _ => 0,
            },
            skipped: 0,
            removed: 0,
            errors: match status {
                SourceStatus::Degraded | SourceStatus::Failed => 1,
                _ => 0,
            },
            status,
        }
    }

    #[test]
    fn indexing_diagnostics_banner_title_reflects_problem_count() {
        let degraded = make_result(AiAssistant::ClaudeCode, SourceStatus::Degraded);

        assert_eq!(
            banner_title(std::slice::from_ref(&degraded)).as_deref(),
            Some("1 session source has indexing issues")
        );
        assert_eq!(
            banner_title(&[degraded.clone(), degraded]).as_deref(),
            Some("2 session sources have indexing issues")
        );
    }

    #[test]
    fn indexing_diagnostics_banner_ignores_not_found_only_results() {
        let result = make_result(AiAssistant::Codex, SourceStatus::NotFound);
        assert_eq!(banner_title(&[result]), None);
    }

    #[test]
    fn indexing_status_banner_button_label_matches_issue_presence() {
        let degraded = make_result(AiAssistant::ClaudeCode, SourceStatus::Degraded);
        let ok = make_result(AiAssistant::ClaudeCode, SourceStatus::Indexed);

        assert_eq!(banner_button_label(&[degraded]), Some("Details"));
        assert_eq!(banner_button_label(&[ok]), None);
        assert_eq!(banner_button_label(&[]), None);
    }

    #[test]
    fn indexing_diagnostics_partial_toast_mentions_error_count() {
        assert_eq!(completion_toast_title(12, 0), "Index rebuilt — 12 sessions");
        assert_eq!(
            completion_toast_title(12, 3),
            "Indexed 12 sessions with 3 errors"
        );
    }

    #[test]
    fn project_sidebar_retained_project_filter_keeps_zero_count_selection() {
        let projects = vec![
            ProjectInfo {
                id: 1,
                name: "alpha".to_string(),
                path: "/tmp/alpha".to_string(),
                session_count: 4,
            },
            ProjectInfo {
                id: 2,
                name: "beta".to_string(),
                path: "/tmp/beta".to_string(),
                session_count: 0,
            },
        ];

        assert_eq!(
            retained_project_filter(&ProjectFilter::Project(2), &projects, false),
            ProjectFilter::Project(2)
        );
    }

    #[test]
    fn retained_project_filter_preserves_pinned_selection() {
        let projects = vec![ProjectInfo {
            id: 1,
            name: "alpha".to_string(),
            path: "/tmp/alpha".to_string(),
            session_count: 2,
        }];

        assert_eq!(
            retained_project_filter(&ProjectFilter::Pinned, &projects, false),
            ProjectFilter::Pinned
        );
    }

    #[test]
    fn resolve_pin_shortcut_target_prefers_active_detail_session() {
        let target = resolve_pin_shortcut_target(
            Workspace::Sessions,
            Some("detail-session"),
            Some("list-session"),
        );

        assert_eq!(target.as_deref(), Some("detail-session"));
    }

    #[test]
    fn resolve_pin_shortcut_target_is_disabled_in_analytics() {
        let target = resolve_pin_shortcut_target(
            Workspace::Analytics,
            Some("detail-session"),
            Some("list-session"),
        );

        assert!(target.is_none());
    }

    #[test]
    fn workspace_header_visibility_shows_date_filter_only_on_sessions_list() {
        let analytics = workspace_header_visibility(Workspace::Analytics, false, false);
        assert!(!analytics.date_filter_visible);

        let sessions_list = workspace_header_visibility(Workspace::Sessions, false, false);
        assert!(sessions_list.date_filter_visible);

        let sessions_detail = workspace_header_visibility(Workspace::Sessions, true, false);
        assert!(!sessions_detail.date_filter_visible);
    }
}
