use std::path::Path;

use relm4::ComponentController;
use relm4::gtk::prelude::WidgetExt;

use crate::database::load_session;
use crate::models::Session;
use crate::ui::session_detail::SessionDetailMsg;

use super::super::App;
use super::super::helpers::{active_search_query, parent_session_load_failure_message};
use super::super::types::ActiveSessionRef;

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
        self.dismiss_summary_popover();
        let project_name = Self::project_name_from_session(&session);

        self.active_session = Some(ActiveSessionRef {
            id: session.id.clone(),
            tool: session.tool,
            project_name,
            pinned: session.pinned_at.is_some(),
        });

        self.session_detail.emit(SessionDetailMsg::SetSession {
            session: Box::new(session),
            search_query,
        });
    }

    pub(crate) fn handle_session_selected(&mut self, id: String) {
        tracing::debug!("Session selected: {}", id);

        self.session_detail.widget().set_visible(true);
        let search_query = active_search_query(&self.search_query);

        match load_session(&self.db_path, &id) {
            Ok(Some(session)) => {
                self.set_active_session_and_detail(session, search_query);
            }
            Ok(None) => {
                tracing::warn!("Session not found: {}", id);
                self.dismiss_summary_popover();
                self.active_session = None;
                self.session_detail.emit(SessionDetailMsg::Clear);
            }
            Err(err) => {
                tracing::error!("Failed to load session: {}", err);
                self.dismiss_summary_popover();
                self.active_session = None;
                self.session_detail.emit(SessionDetailMsg::Clear);
            }
        }

        if !self.detail_visible {
            self.filters_open_before_detail = self.filters_open;
            self.filters_open = false;
            self.nav_view.push(&self.detail_page);
            self.detail_visible = true;
            self.banner.set_revealed(false);
        }
    }

    pub(crate) fn handle_open_child_session(&mut self, child_session_id: String) {
        tracing::debug!("Open child session: {}", child_session_id);
        self.parent_session = self.active_session.clone();

        let search_query = active_search_query(&self.search_query);
        match load_session(&self.db_path, &child_session_id) {
            Ok(Some(session)) => {
                self.set_active_session_and_detail(session, search_query);
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
                    let mut parent = parent;
                    parent.pinned = session.pinned_at.is_some();
                    self.dismiss_summary_popover();
                    self.active_session = Some(parent);
                    self.session_detail.emit(SessionDetailMsg::SetSession {
                        session: Box::new(session),
                        search_query,
                    });
                }
                Ok(None) => {
                    tracing::warn!("Parent session no longer found; resetting");
                    self.active_session = None;
                    self.session_detail
                        .emit(parent_session_load_failure_message());
                }
                Err(err) => {
                    tracing::error!("Failed to load parent session: {}", err);
                    self.active_session = None;
                    self.session_detail
                        .emit(parent_session_load_failure_message());
                }
            }
        }
    }
}
