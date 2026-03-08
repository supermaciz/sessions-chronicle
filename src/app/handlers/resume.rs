use std::{path::PathBuf, str::FromStr};

use relm4::{
    ComponentSender,
    gtk::{gio, prelude::SettingsExt},
};

use crate::config::APP_ID;
use crate::database::load_session;
use crate::models::session::AiAssistant;
use crate::utils::terminal::{self, Terminal};

use super::super::{App, AppMsg};

impl App {
    fn resolve_workdir_for_resume(file_path: &str, project_path: Option<&str>) -> Option<PathBuf> {
        if let Some(project_path) = project_path {
            return Some(PathBuf::from(project_path));
        }

        PathBuf::from(file_path)
            .parent()
            .map(|dir| dir.to_path_buf())
    }

    pub(crate) fn handle_resume_session(&self, session_id: String, tool: AiAssistant) {
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

        match terminal::build_resume_command(tool, &session_id, &workdir) {
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
            sender.input(AppMsg::ResumeSession(session.id.clone(), session.tool));
        } else {
            tracing::warn!("ResumeActiveSession ignored — no active session");
        }
    }
}
