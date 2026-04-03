use std::path::Path;

use relm4::ComponentController;

use crate::database::load_session;
use crate::models::Session;
use crate::ui::{session_detail::SessionDetailMsg, tool_inspector_pane::ToolInspectorPaneMsg};

use super::super::App;
use super::super::helpers::{
    active_search_query, parent_session_load_failure_messages, transition_to_detail,
};
use super::super::types::{ActiveSessionRef, UtilityPaneMode};

impl App {
    fn project_name_from_session(session: &Session) -> String {
        session
            .project_path
            .as_deref()
            .and_then(|p| Path::new(p).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown project")
            .to_string()
    }

    fn set_active_session_and_detail(&mut self, session: Session, search_query: Option<String>) {
        let project_name = Self::project_name_from_session(&session);
        self.active_session_pinned = session.pinned_at.is_some();

        self.active_session = Some(ActiveSessionRef {
            id: session.id.clone(),
            tool: session.tool,
            project_name,
        });

        self.session_detail.emit(SessionDetailMsg::SetSession {
            session: Box::new(session),
            search_query,
        });
    }

    pub(crate) fn handle_session_selected(&mut self, id: String) {
        tracing::debug!("Session selected: {}", id);

        let search_query = active_search_query(&self.search_query);

        match load_session(&self.db_path, &id) {
            Ok(Some(session)) => {
                self.set_active_session_and_detail(session, search_query);
            }
            Ok(None) => {
                tracing::warn!("Session not found: {}", id);
                self.active_session = None;
                self.active_session_pinned = false;
                self.session_detail.emit(SessionDetailMsg::Clear);
            }
            Err(err) => {
                tracing::error!("Failed to load session: {}", err);
                self.active_session = None;
                self.active_session_pinned = false;
                self.session_detail.emit(SessionDetailMsg::Clear);
            }
        }

        if !self.detail_visible {
            self.nav_view.push(&self.detail_page);
            self.detail_visible = true;
            self.banner.set_revealed(false);
        }

        transition_to_detail(&mut self.pane_mode, &mut self.pane_open);
        self.apply_pane_stack_switch();
    }

    fn open_inspector_for_active_session(&mut self) -> Option<String> {
        let session_id = self
            .active_session
            .as_ref()
            .map(|session| session.id.clone())?;
        self.pane_mode = UtilityPaneMode::ToolInspector;
        self.pane_open = true;
        self.apply_pane_stack_switch();
        Some(session_id)
    }

    pub(crate) fn handle_inspect_tool_call(&mut self, tool_call_id: String) {
        tracing::debug!("Inspect tool call: {}", tool_call_id);
        if let Some(session_id) = self.open_inspector_for_active_session() {
            self.tool_inspector_pane
                .emit(ToolInspectorPaneMsg::SelectToolCall {
                    session_id,
                    tool_call_id,
                });
        }
    }

    pub(crate) fn handle_inspect_subagent(&mut self, subagent_id: String) {
        tracing::debug!("Inspect subagent: {}", subagent_id);
        if let Some(session_id) = self.open_inspector_for_active_session() {
            self.tool_inspector_pane
                .emit(ToolInspectorPaneMsg::SelectSubagent {
                    session_id,
                    subagent_id,
                });
        }
    }

    pub(crate) fn handle_open_child_session(&mut self, child_session_id: String) {
        tracing::debug!("Open child session: {}", child_session_id);
        self.parent_session = self.active_session.clone();

        let search_query = active_search_query(&self.search_query);
        match load_session(&self.db_path, &child_session_id) {
            Ok(Some(session)) => {
                self.set_active_session_and_detail(session, search_query);
                self.tool_inspector_pane.emit(ToolInspectorPaneMsg::Clear);
            }
            Ok(None) => {
                tracing::warn!("Child session not found: {}", child_session_id);
                self.parent_session = None;
            }
            Err(err) => {
                tracing::error!("Failed to load child session {}: {}", child_session_id, err);
                self.parent_session = None;
            }
        }
    }

    pub(crate) fn handle_return_to_parent_session(&mut self) {
        tracing::debug!("Return to parent session");
        if let Some(parent) = self.parent_session.take() {
            let search_query = active_search_query(&self.search_query);
            match load_session(&self.db_path, &parent.id) {
                Ok(Some(session)) => {
                    self.active_session = Some(parent);
                    self.active_session_pinned = session.pinned_at.is_some();
                    self.session_detail.emit(SessionDetailMsg::SetSession {
                        session: Box::new(session),
                        search_query,
                    });
                    self.tool_inspector_pane.emit(ToolInspectorPaneMsg::Clear);
                }
                Ok(None) => {
                    tracing::warn!("Parent session no longer found; resetting");
                    self.active_session = None;
                    self.active_session_pinned = false;
                    let (detail_msg, inspector_msg) = parent_session_load_failure_messages();
                    self.session_detail.emit(detail_msg);
                    self.tool_inspector_pane.emit(inspector_msg);
                }
                Err(err) => {
                    tracing::error!("Failed to load parent session: {}", err);
                    self.active_session = None;
                    self.active_session_pinned = false;
                    let (detail_msg, inspector_msg) = parent_session_load_failure_messages();
                    self.session_detail.emit(detail_msg);
                    self.tool_inspector_pane.emit(inspector_msg);
                }
            }
        }
    }
}
