use std::{path::PathBuf, str::FromStr};

use relm4::{
    ComponentSender,
    gtk::{gio, prelude::SettingsExt},
};

use crate::config::APP_ID;
use crate::database::load_session;
use crate::utils::terminal::{self, Terminal};

use crate::models::Session;

use super::super::{App, AppMsg};

/// Why resume is unavailable for this session, checked immediately after a
/// fresh load and before any workdir/settings/terminal access — a missing
/// source and a non-resumable Kimi child are both explained here, in that
/// order, so the freshest reason wins.
fn resume_unavailable_reason(session: &Session) -> Option<&'static str> {
    if session.source.missing {
        Some("The session source is missing. Only indexed content is available.")
    } else if !session.can_resume() {
        Some("Kimi Code child sessions cannot be resumed directly.")
    } else {
        None
    }
}

impl App {
    fn resolve_workdir_for_resume(file_path: &str, project_path: Option<&str>) -> Option<PathBuf> {
        if let Some(project_path) = project_path {
            return Some(PathBuf::from(project_path));
        }

        PathBuf::from(file_path)
            .parent()
            .map(|dir| dir.to_path_buf())
    }

    pub(crate) fn handle_resume_session(&self, session_id: String) {
        tracing::debug!("Resume session requested: {}", session_id);

        let session = match load_session(&self.db_path, &session_id) {
            Ok(Some(session)) => session,
            Ok(None) => {
                tracing::error!("Session not found: {}", session_id);
                self.show_error_dialog(
                    "Session Not Found",
                    "The requested session could not be found in the database.",
                );
                return;
            }
            Err(err) => {
                tracing::error!("Failed to load session {}: {}", session_id, err);
                self.show_error_dialog(
                    "Failed to Load Session",
                    &format!("An error occurred while loading the session: {}", err),
                );
                return;
            }
        };

        if let Some(reason) = resume_unavailable_reason(&session) {
            tracing::warn!(session_id = %session.id, reason, "resume is unavailable for this session");
            self.show_error_dialog("Resume Unavailable", reason);
            return;
        }

        let workdir = match Self::resolve_workdir_for_resume(
            &session.file_path,
            session.project_path.as_deref(),
        ) {
            Some(dir) => dir,
            None => {
                tracing::error!(
                    "Cannot determine workdir for session: no project_path and no valid parent directory"
                );
                self.show_error_dialog(
                    "Invalid Session",
                    "The session has no valid working directory.",
                );
                return;
            }
        };

        let settings = gio::Settings::new(APP_ID);
        let terminal_str = settings.string("resume-terminal");
        let terminal = match Terminal::from_str(&terminal_str) {
            Ok(t) => t,
            Err(()) => {
                tracing::error!("Invalid terminal preference: {}", terminal_str);
                self.show_error_dialog(
                    "Invalid Terminal Preference",
                    "Please check your terminal preference in settings.",
                );
                return;
            }
        };

        match terminal::build_resume_command(session.tool, &session.id, &workdir) {
            Ok(args) => match terminal::spawn_terminal(terminal, &args) {
                Ok(_) => {
                    tracing::info!("Successfully launched terminal for session: {}", session_id);
                }
                Err(err) => {
                    tracing::error!(
                        "Failed to spawn terminal for session {}: {}",
                        session_id,
                        err
                    );
                    self.show_resume_failure_toast(&err);
                }
            },
            Err(err) => {
                tracing::error!(
                    "Failed to build resume command for session {}: {}",
                    session_id,
                    err
                );
                self.show_error_dialog(
                    "Failed to Build Resume Command",
                    &format!("Could not build the resume command: {}", err),
                );
            }
        }
    }

    pub(crate) fn handle_resume_active_session(&self, sender: &ComponentSender<App>) {
        if let Some(ref session) = self.active_session {
            sender.input(AppMsg::ResumeSession(session.id.clone()));
        } else {
            tracing::warn!("ResumeActiveSession ignored — no active session");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resume_unavailable_reason;
    use crate::models::Session;
    use crate::models::session::{AiAssistant, SessionEndingStatus};
    use chrono::Utc;

    fn test_session() -> Session {
        Session {
            id: "session_test".to_string(),
            tool: AiAssistant::ClaudeCode,
            project_path: Some("/tmp/project".to_string()),
            project_id: None,
            start_time: Utc::now(),
            message_count: 1,
            file_path: "/tmp/session.jsonl".to_string(),
            last_updated: Utc::now(),
            pinned_at: None,
            first_prompt: Some("Prompt".to_string()),
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: SessionEndingStatus::Unknown,
            source: Default::default(),
        }
    }

    #[test]
    fn resumable_session_has_no_unavailable_reason() {
        assert_eq!(resume_unavailable_reason(&test_session()), None);
    }

    #[test]
    fn missing_source_reason_takes_priority_over_kimi_child_check() {
        let mut session = test_session();
        session.tool = AiAssistant::KimiCode;
        session.is_subagent = true;
        session.source.missing = true;

        assert_eq!(
            resume_unavailable_reason(&session),
            Some("The session source is missing. Only indexed content is available.")
        );
    }

    #[test]
    fn kimi_child_reason_is_reported_when_source_is_present() {
        let mut session = test_session();
        session.tool = AiAssistant::KimiCode;
        session.is_subagent = true;

        assert_eq!(
            resume_unavailable_reason(&session),
            Some("Kimi Code child sessions cannot be resumed directly.")
        );
    }
}
