use adw::prelude::NavigationPageExt;
use relm4::gtk::prelude::WidgetExt;
use relm4::{ComponentController, ComponentSender};

use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::session_list::SessionListMsg;

use super::super::helpers::{
    detail_pop_sync_decision, resolve_escape_action, resolve_search_mode_change,
    search_query_update_messages, workspace_allows_search,
};
use super::super::types::EscapeResolution;
use super::super::{App, AppMsg};

impl App {
    pub(crate) fn handle_search_mode_changed(&mut self, enabled: bool) {
        let resolved_enabled = resolve_search_mode_change(self.active_workspace, enabled);

        if enabled && !workspace_allows_search(self.active_workspace) {
            self.sync_search_bar.set(true);
        }

        if self.search_visible != resolved_enabled {
            self.search_visible = resolved_enabled;
            if !resolved_enabled {
                self.search_query.clear();
                let (list_msg, detail_msg) = search_query_update_messages(String::new());
                self.session_list.emit(list_msg);
                self.session_detail.emit(detail_msg);
                if !self.detail_visible {
                    self.session_list.emit(SessionListMsg::RestoreFocus);
                }
            }
        }
    }

    pub(crate) fn handle_toggle_filters(&mut self) {
        if self.active_workspace.is_analytics() || self.detail_visible {
            return;
        }
        self.filters_open = !self.filters_open;
    }

    pub(crate) fn handle_filters_visibility_changed(&mut self, visible: bool) {
        if self.active_workspace.is_analytics() || self.detail_visible {
            return;
        }
        if self.filters_open != visible {
            self.filters_open = visible;
        }
    }

    pub(crate) fn handle_toggle_inspector(&mut self) {
        if !self.detail_visible {
            return;
        }
        self.session_detail.emit(SessionDetailMsg::ToggleInspector);
    }

    /// Single-shortcut dispatcher (F9): toggle the side pane that belongs to
    /// the currently visible view. Skipped in Analytics, where neither pane
    /// applies.
    pub(crate) fn handle_toggle_active_side_pane(&mut self) {
        if self.active_workspace.is_analytics() {
            return;
        }
        if self.detail_visible {
            self.handle_toggle_inspector();
        } else {
            self.handle_toggle_filters();
        }
    }

    pub(crate) fn handle_inspector_visibility_changed(&mut self, visible: bool) {
        self.inspector_open = visible;
    }

    pub(crate) fn handle_search_query_changed(&mut self, query: String) {
        self.search_query = query.clone();
        let (list_msg, detail_msg) = search_query_update_messages(query);
        self.session_list.emit(list_msg);
        self.session_detail.emit(detail_msg);
    }

    pub(crate) fn handle_request_navigate_back(&mut self) {
        if self.detail_visible {
            self.session_detail.widget().set_visible(false);
            self.session_detail
                .emit(SessionDetailMsg::PrepareForNavigationBack);
            let visible_page_tag = self.nav_view.visible_page().and_then(|p| p.tag());
            if visible_page_tag.as_deref() == Some("detail") {
                self.suppress_next_detail_pop_sync = true;
                self.nav_view.pop();
            }
            self.transition_to_session_list_mode();
            self.session_list.emit(SessionListMsg::RestoreFocus);
        }
    }

    pub(crate) fn handle_navigate_back(&mut self) {
        let (should_sync, suppress_next) =
            detail_pop_sync_decision(self.suppress_next_detail_pop_sync, self.detail_visible);
        self.suppress_next_detail_pop_sync = suppress_next;
        if should_sync {
            self.session_detail.widget().set_visible(false);
            self.session_detail
                .emit(SessionDetailMsg::PrepareForNavigationBack);
            self.transition_to_session_list_mode();
            self.session_list.emit(SessionListMsg::RestoreFocus);
        }
    }

    pub(crate) fn handle_escape(&mut self, sender: &ComponentSender<App>) {
        match resolve_escape_action(
            self.search_visible,
            self.detail_visible,
            self.inspector_open,
        ) {
            EscapeResolution::CloseSearch => {
                self.search_visible = false;
                self.sync_search_bar.set(true);
                self.search_query.clear();
                let (list_msg, detail_msg) = search_query_update_messages(String::new());
                self.session_list.emit(list_msg);
                self.session_detail.emit(detail_msg);
                if !self.detail_visible {
                    self.session_list.emit(SessionListMsg::RestoreFocus);
                }
            }
            EscapeResolution::CloseInspector => {
                self.session_detail.emit(SessionDetailMsg::CloseInspector);
            }
            EscapeResolution::NavigateBack => {
                sender.input(AppMsg::RequestNavigateBack);
            }
            EscapeResolution::Noop => {}
        }
    }
}
