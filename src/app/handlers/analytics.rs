use relm4::ComponentController;

use crate::analytics_worker::AnalyticsWorkerInput;
use crate::models::AnalyticsData;
use crate::ui::analytics_view::AnalyticsViewMsg;

use super::super::App;
use super::super::helpers::workspace_header_visibility;
use super::super::types::Workspace;

impl App {
    pub(crate) fn handle_workspace_changed(&mut self, workspace: Workspace) {
        if self.active_workspace == workspace {
            return;
        }

        self.active_workspace = workspace;

        if self.active_workspace.is_analytics() {
            self.search_visible = false;
            self.sync_search_bar.set(true);
            self.analytics_view.emit(AnalyticsViewMsg::Entered);
        }
    }

    pub(crate) fn handle_analytics_refresh_requested(&mut self) {
        self.analytics_view.emit(AnalyticsViewMsg::LoadingStarted);
        self.analytics_worker.emit(AnalyticsWorkerInput::Load);
    }

    pub(crate) fn handle_analytics_loaded(&mut self, data: AnalyticsData) {
        self.analytics_view.emit(AnalyticsViewMsg::Loaded(data));
    }

    pub(crate) fn handle_analytics_load_failed(&mut self, error: String) {
        self.analytics_view
            .emit(AnalyticsViewMsg::LoadFailed(error));
    }

    pub(crate) fn is_search_ui_visible(&self) -> bool {
        workspace_header_visibility(
            self.active_workspace,
            self.detail_visible,
            self.parent_session.is_some(),
        )
        .search_ui_visible
    }

    pub(crate) fn is_pane_controls_visible(&self) -> bool {
        workspace_header_visibility(
            self.active_workspace,
            self.detail_visible,
            self.parent_session.is_some(),
        )
        .pane_controls_visible
    }

    pub(crate) fn is_filters_toggle_visible(&self) -> bool {
        self.is_pane_controls_visible() && !self.detail_visible
    }

    pub(crate) fn is_inspector_toggle_visible(&self) -> bool {
        self.is_pane_controls_visible() && self.detail_visible
    }

    pub(crate) fn are_detail_actions_visible(&self) -> bool {
        workspace_header_visibility(
            self.active_workspace,
            self.detail_visible,
            self.parent_session.is_some(),
        )
        .detail_actions_visible
    }
}
