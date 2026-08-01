use std::path::Path;

use gettextrs::gettext;
use relm4::ComponentController;
use relm4::gtk::prelude::WidgetExt;

use crate::database::load_session;
use crate::models::Session;
use crate::ui::session_detail::SessionDetailMsg;

use super::super::App;
use super::super::helpers::{active_search_query, parent_session_load_failure_message};
use super::super::types::{ActiveSessionRef, Workspace};

#[derive(Debug)]
enum ExternalSessionLookup {
    Found(Box<Session>),
    IndexMissing,
    Unavailable,
    Failed(anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalOpenFailure {
    IndexMissing,
    Unavailable,
    Failed,
}

fn lookup_external_session(db_path: &Path, id: &str, index_ready: bool) -> ExternalSessionLookup {
    if id.is_empty() {
        return ExternalSessionLookup::Unavailable;
    }
    if !index_ready || !db_path.exists() {
        return ExternalSessionLookup::IndexMissing;
    }

    match load_session(db_path, id) {
        Ok(Some(session)) if !session.is_subagent => {
            ExternalSessionLookup::Found(Box::new(session))
        }
        Ok(Some(_)) | Ok(None) => ExternalSessionLookup::Unavailable,
        Err(error) => ExternalSessionLookup::Failed(error),
    }
}

fn external_open_failure_title(failure: ExternalOpenFailure) -> String {
    match failure {
        ExternalOpenFailure::Unavailable => gettext("Session not found"),
        ExternalOpenFailure::IndexMissing => gettext("Sessions are not indexed yet"),
        ExternalOpenFailure::Failed => gettext("Could not open session"),
    }
}

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
            project_name,
            pinned: session.pinned_at.is_some(),
            can_resume: session.can_resume(),
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
                    parent.can_resume = session.can_resume();
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

    fn show_external_open_failure(&self, failure: ExternalOpenFailure) {
        let toast = relm4::adw::Toast::builder()
            .title(external_open_failure_title(failure))
            .build();
        self.toast_overlay.add_toast(toast);
    }

    pub(crate) fn handle_external_session_open(&mut self, id: String) {
        tracing::debug!(session_id = %id, "External session open requested");

        let session = match lookup_external_session(&self.db_path, &id, self.index_ready) {
            ExternalSessionLookup::Found(session) => *session,
            ExternalSessionLookup::IndexMissing => {
                self.show_external_open_failure(ExternalOpenFailure::IndexMissing);
                return;
            }
            ExternalSessionLookup::Unavailable => {
                self.show_external_open_failure(ExternalOpenFailure::Unavailable);
                return;
            }
            ExternalSessionLookup::Failed(error) => {
                tracing::error!(session_id = %id, error = %error, "External session lookup failed");
                self.show_external_open_failure(ExternalOpenFailure::Failed);
                return;
            }
        };

        self.dismiss_summary_popover();
        self.search_visible = false;
        self.sync_search_bar.set(true);
        let (list_msg, detail_msg) = self.clear_search_state();
        self.session_list.emit(list_msg);
        self.session_detail.emit(detail_msg);

        self.handle_workspace_changed(Workspace::Sessions);
        self.workspace_stack
            .set_visible_child_name(Workspace::Sessions.stack_name());
        self.parent_session = None;
        self.session_detail.widget().set_visible(true);
        self.set_active_session_and_detail(session, None);

        if !self.detail_visible {
            self.filters_open_before_detail = self.filters_open;
            self.filters_open = false;
            self.nav_view.push(&self.detail_page);
            self.detail_visible = true;
            self.banner.set_revealed(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::schema::initialize_database;
    use rusqlite::Connection;

    fn seeded_database() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sessions.db");
        let connection = Connection::open(&path).unwrap();
        initialize_database(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO sessions
                 (id, tool, project_path, start_time, message_count, file_path,
                  last_updated, is_subagent)
                 VALUES (?1, 'claude_code', '/projects/demo', 1, 1, '/tmp/demo.jsonl', 1, ?2)",
                rusqlite::params!["top-level", false],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sessions
                 (id, tool, project_path, start_time, message_count, file_path,
                  last_updated, is_subagent)
                 VALUES (?1, 'claude_code', '/projects/demo', 1, 1, '/tmp/child.jsonl', 1, ?2)",
                rusqlite::params!["subagent", true],
            )
            .unwrap();
        drop(connection);
        (directory, path)
    }

    #[test]
    fn lookup_accepts_only_existing_top_level_session() {
        let (_directory, path) = seeded_database();

        match lookup_external_session(&path, "top-level", true) {
            ExternalSessionLookup::Found(session) => assert_eq!(session.id, "top-level"),
            outcome => panic!("expected found, got {outcome:?}"),
        }
        assert!(matches!(
            lookup_external_session(&path, "subagent", true),
            ExternalSessionLookup::Unavailable
        ));
        assert!(matches!(
            lookup_external_session(&path, "missing", true),
            ExternalSessionLookup::Unavailable
        ));
        assert!(matches!(
            lookup_external_session(&path, "", true),
            ExternalSessionLookup::Unavailable
        ));
    }

    #[test]
    fn lookup_distinguishes_missing_index_from_missing_row() {
        let directory = tempfile::tempdir().unwrap();
        let missing_path = directory.path().join("not-created.db");

        assert!(matches!(
            lookup_external_session(&missing_path, "stale-id", true),
            ExternalSessionLookup::IndexMissing
        ));

        let (_seed_directory, seeded_path) = seeded_database();
        assert!(matches!(
            lookup_external_session(&seeded_path, "top-level", false),
            ExternalSessionLookup::IndexMissing
        ));
    }

    #[test]
    fn lookup_distinguishes_sqlite_failure() {
        let directory = tempfile::tempdir().unwrap();

        assert!(matches!(
            lookup_external_session(directory.path(), "any-id", true),
            ExternalSessionLookup::Failed(_)
        ));
    }

    #[test]
    fn failure_titles_match_the_three_user_outcomes() {
        assert_eq!(
            external_open_failure_title(ExternalOpenFailure::Unavailable),
            "Session not found"
        );
        assert_eq!(
            external_open_failure_title(ExternalOpenFailure::IndexMissing),
            "Sessions are not indexed yet"
        );
        assert_eq!(
            external_open_failure_title(ExternalOpenFailure::Failed),
            "Could not open session"
        );
    }
}
