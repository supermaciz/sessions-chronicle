use relm4::{ComponentController, adw};

use crate::database::toggle_pin;
use crate::ui::session_list::SessionListMsg;

use super::super::App;
use super::super::helpers::resolve_pin_shortcut_target;

impl App {
    pub(crate) fn handle_toggle_pin_requested(&mut self, session_id: String) {
        match toggle_pin(&self.db_path, &session_id) {
            Ok(is_pinned) => {
                if let Some(ref mut s) = self.active_session
                    && s.id == session_id
                {
                    s.pinned = is_pinned;
                }

                let title = if is_pinned {
                    "Session pinned"
                } else {
                    "Session unpinned"
                };
                self.toast_overlay
                    .add_toast(adw::Toast::builder().title(title).timeout(2).build());

                if self.refresh_sidebar_projects() {
                    self.emit_session_list_filters();
                } else {
                    self.session_list.emit(SessionListMsg::Reload);
                }
            }
            Err(err) => {
                tracing::warn!("Failed to toggle pin for '{}': {}", session_id, err);
                self.toast_overlay.add_toast(
                    adw::Toast::builder()
                        .title("Could not update pin state.")
                        .timeout(2)
                        .build(),
                );
            }
        }
    }

    pub(crate) fn handle_toggle_pin_shortcut_requested(&mut self) {
        if self.active_workspace.is_analytics() {
            return;
        }

        let detail_session_id = self.active_session.as_ref().map(|s| s.id.as_str());
        let target = resolve_pin_shortcut_target(self.active_workspace, detail_session_id, None);

        if let Some(session_id) = target {
            self.handle_toggle_pin_requested(session_id);
        } else {
            self.session_list
                .emit(SessionListMsg::RequestSelectedSessionForPin);
        }
    }
}
