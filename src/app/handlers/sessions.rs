use std::path::Path;

use adw::prelude::NavigationPageExt;
use gettextrs::gettext;
use relm4::ComponentController;
use relm4::gtk::prelude::WidgetExt;

use crate::database::load_session;
use crate::database::open_connection;
use crate::models::Session;
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::session_detail::build_source_details_page;
use crate::ui::session_detail::source_details_content;

use super::super::App;
use super::super::helpers::{active_search_query, parent_session_load_failure_message};
use super::super::types::{ActiveSessionRef, Workspace};

#[derive(Debug)]
enum ExternalOpenFailure {
    IndexMissing,
    Unavailable,
    Failed(anyhow::Error),
}

fn lookup_external_session(
    db_path: &Path,
    id: &str,
    index_available: bool,
) -> Result<Session, ExternalOpenFailure> {
    if id.is_empty() {
        return Err(ExternalOpenFailure::Unavailable);
    }
    if !index_available || !db_path.exists() {
        return Err(ExternalOpenFailure::IndexMissing);
    }

    match load_session(db_path, id) {
        Ok(Some(session)) if !session.is_subagent => Ok(session),
        Ok(Some(_)) | Ok(None) => Err(ExternalOpenFailure::Unavailable),
        Err(error) => Err(ExternalOpenFailure::Failed(error)),
    }
}

fn external_open_failure_title(failure: &ExternalOpenFailure) -> String {
    match failure {
        ExternalOpenFailure::Unavailable => gettext("Session not found"),
        ExternalOpenFailure::IndexMissing => gettext("Sessions are not indexed yet"),
        ExternalOpenFailure::Failed(_) => gettext("Could not open session"),
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
            source_missing: session.source.missing,
        });

        self.session_detail.emit(SessionDetailMsg::SetSession {
            session: Box::new(session),
            search_query,
        });
    }

    fn push_detail_page(&mut self) {
        if !self.detail_visible {
            self.filters_open_before_detail = self.filters_open;
            self.filters_open = false;
            self.nav_view.push(&self.detail_page);
            self.detail_visible = true;
            self.banner.set_revealed(false);
        }
    }

    /// Whether the retained session has any locally indexed transcript content.
    fn session_has_indexed_items(&self, session_id: &str) -> bool {
        match open_connection(&self.db_path) {
            Ok(conn) => conn
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM transcript_items WHERE session_id = ?1
                        UNION ALL
                        SELECT 1 FROM messages WHERE session_id = ?1
                        LIMIT 1)",
                    [session_id],
                    |row| row.get(0),
                )
                .unwrap_or(false),
            Err(err) => {
                tracing::warn!(
                    session_id,
                    error = %err,
                    "Failed to open index while checking retained content"
                );
                false
            }
        }
    }

    /// Opens the storage-aware Source details page for a retained session.
    /// The incoming snapshot is only a trigger: the row is reloaded by ID so
    /// the page never renders stale evidence. Page contents are queried from
    /// the local index rather than the currently rendered transcript.
    pub(crate) fn handle_show_source_details(&mut self, session: Session) {
        if self.source_details_visible {
            return;
        }
        let reloaded = match load_session(&self.db_path, &session.id) {
            Ok(Some(session)) => session,
            Ok(None) | Err(_) => return,
        };
        if !reloaded.source.missing {
            return;
        }

        let has_indexed_items = self.session_has_indexed_items(&reloaded.id);
        let content = source_details_content(&reloaded, has_indexed_items);

        match self.source_details_page.as_ref() {
            // First open: build the page, register it permanently with the
            // nav view (mirroring detail_page) so it can be re-pushed after
            // being popped, then push it.
            None => {
                let page = build_source_details_page(&reloaded, has_indexed_items);
                self.nav_view.add(&page);
                self.source_details_page = Some(page.clone());
                self.source_details_visible = true;
                self.nav_view.push(&page);
            }
            // Reuse the registered page, refreshing its content in place so a
            // reopened page never shows stale evidence.
            Some(page) => {
                page.set_child(Some(&content));
                self.source_details_visible = true;
                self.nav_view.push(page);
            }
        }
    }

    /// Collapses an open Source details overlay back to the transcript detail
    /// before a session change (selection, external open, or child open).
    pub(crate) fn pop_source_details_if_visible(&mut self) {
        if self.source_details_visible {
            self.handle_request_navigate_back();
        }
    }

    /// Refreshes an already-open Source details page after an indexing pass,
    /// without pushing another page or losing keyboard focus. A successful
    /// return updates the page's explanation/evidence; once the source is
    /// available again the page is popped back to the transcript detail.
    pub(crate) fn refresh_source_details_page(&mut self) {
        if !self.source_details_visible {
            return;
        }
        let Some(active) = self.active_session.as_ref() else {
            return;
        };
        let reloaded = match load_session(&self.db_path, &active.id) {
            Ok(Some(session)) => session,
            Ok(None) | Err(_) => return,
        };
        if !reloaded.source.missing {
            self.nav_view.pop();
            return;
        }
        let Some(page) = self.source_details_page.as_ref() else {
            return;
        };
        let has_indexed_items = self.session_has_indexed_items(&reloaded.id);
        let content = source_details_content(&reloaded, has_indexed_items);
        page.set_child(Some(&content));
    }

    /// Clears Source-details page state after the page was popped (native
    /// gesture or the app-owned back handler). The registered page is kept so
    /// it can be re-pushed with refreshed content later. Idempotent: safe to
    /// call again when a queued `SourceDetailsPopped` arrives after the
    /// programmatic back handler already cleaned up.
    pub(crate) fn handle_source_details_popped(&mut self) {
        if !self.source_details_visible {
            return;
        }
        self.source_details_visible = false;
        self.session_detail.widget().set_visible(true);
    }

    pub(crate) fn handle_session_selected(&mut self, id: String) {
        tracing::debug!("Session selected: {}", id);
        self.pop_source_details_if_visible();

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

        self.push_detail_page();
    }

    pub(crate) fn handle_open_child_session(&mut self, child_session_id: String) {
        tracing::debug!("Open child session: {}", child_session_id);
        self.pop_source_details_if_visible();
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
        self.pop_source_details_if_visible();
        if let Some(parent) = self.parent_session.take() {
            let search_query = active_search_query(&self.search_query);
            match load_session(&self.db_path, &parent.id) {
                Ok(Some(session)) => {
                    let mut parent = parent;
                    parent.pinned = session.pinned_at.is_some();
                    parent.can_resume = session.can_resume();
                    parent.source_missing = session.source.missing;
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

    /// Re-reads the active (and cached parent) session rows after an
    /// indexing pass so pin/resume/source-availability fields reflect the
    /// latest scan. Detection never rewrites the transcript itself, so this
    /// only refreshes cached metadata and pushes the updated source state
    /// into the open detail view — it never replaces the loaded session via
    /// `SetSession`, which would disturb transcript/search/scroll state.
    pub(crate) fn refresh_active_session_metadata(&mut self) {
        if let Some(active) = self.active_session.clone() {
            match load_session(&self.db_path, &active.id) {
                Ok(Some(session)) => {
                    if let Some(active_ref) = self.active_session.as_mut() {
                        active_ref.pinned = session.pinned_at.is_some();
                        active_ref.can_resume = session.can_resume();
                        active_ref.source_missing = session.source.missing;
                    }
                    self.session_detail
                        .emit(SessionDetailMsg::UpdateSourceState {
                            session_id: session.id.clone(),
                            source: session.source.clone(),
                        });
                }
                Ok(None) => {
                    tracing::warn!(
                        session_id = %active.id,
                        "Active session vanished from index during metadata refresh"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        session_id = %active.id,
                        error = %err,
                        "Failed to refresh active session metadata"
                    );
                }
            }
        }

        if let Some(parent) = self.parent_session.clone() {
            match load_session(&self.db_path, &parent.id) {
                Ok(Some(session)) => {
                    if let Some(parent_ref) = self.parent_session.as_mut() {
                        parent_ref.pinned = session.pinned_at.is_some();
                        parent_ref.can_resume = session.can_resume();
                        parent_ref.source_missing = session.source.missing;
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        session_id = %parent.id,
                        "Cached parent session vanished from index during metadata refresh"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        session_id = %parent.id,
                        error = %err,
                        "Failed to refresh parent session metadata"
                    );
                }
            }
        }
    }

    fn show_external_open_failure(&self, failure: &ExternalOpenFailure) {
        let toast = relm4::adw::Toast::builder()
            .title(external_open_failure_title(failure))
            .build();
        self.toast_overlay.add_toast(toast);
    }

    pub(crate) fn handle_external_session_open(&mut self, id: String) {
        tracing::debug!(session_id = %id, "External session open requested");
        self.pop_source_details_if_visible();

        let session = match lookup_external_session(&self.db_path, &id, self.index_available) {
            Ok(session) => session,
            Err(failure) => {
                if let ExternalOpenFailure::Failed(error) = &failure {
                    tracing::error!(session_id = %id, error = %error, "External session lookup failed");
                }
                self.show_external_open_failure(&failure);
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

        self.push_detail_page();
    }

    pub(crate) fn handle_external_search(&mut self, query: String) {
        tracing::debug!(%query, "External session search requested");
        if self.detail_visible {
            self.handle_request_navigate_back();
        }
        self.handle_workspace_changed(Workspace::Sessions);
        self.workspace_stack
            .set_visible_child_name(Workspace::Sessions.stack_name());
        self.handle_search_query_changed(query);
        self.search_visible = true;
        self.sync_search_entry.set(true);
        self.sync_search_bar.set(true);
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
            Ok(session) => assert_eq!(session.id, "top-level"),
            outcome => panic!("expected found, got {outcome:?}"),
        }
        assert!(matches!(
            lookup_external_session(&path, "subagent", true),
            Err(ExternalOpenFailure::Unavailable)
        ));
        assert!(matches!(
            lookup_external_session(&path, "missing", true),
            Err(ExternalOpenFailure::Unavailable)
        ));
        assert!(matches!(
            lookup_external_session(&path, "", true),
            Err(ExternalOpenFailure::Unavailable)
        ));
    }

    #[test]
    fn lookup_distinguishes_missing_index_from_missing_row() {
        let directory = tempfile::tempdir().unwrap();
        let missing_path = directory.path().join("not-created.db");

        assert!(matches!(
            lookup_external_session(&missing_path, "stale-id", true),
            Err(ExternalOpenFailure::IndexMissing)
        ));

        let (_seed_directory, seeded_path) = seeded_database();
        assert!(matches!(
            lookup_external_session(&seeded_path, "top-level", false),
            Err(ExternalOpenFailure::IndexMissing)
        ));
    }

    #[test]
    fn lookup_distinguishes_sqlite_failure() {
        let directory = tempfile::tempdir().unwrap();

        assert!(matches!(
            lookup_external_session(directory.path(), "any-id", true),
            Err(ExternalOpenFailure::Failed(_))
        ));
    }

    #[test]
    fn failure_titles_match_the_three_user_outcomes() {
        assert_eq!(
            external_open_failure_title(&ExternalOpenFailure::Unavailable),
            "Session not found"
        );
        assert_eq!(
            external_open_failure_title(&ExternalOpenFailure::IndexMissing),
            "Sessions are not indexed yet"
        );
        assert_eq!(
            external_open_failure_title(&ExternalOpenFailure::Failed(anyhow::anyhow!("database"))),
            "Could not open session"
        );
    }
}
