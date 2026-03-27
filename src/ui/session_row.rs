use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::gtk::{gdk, gio};
use relm4::{adw, gtk};

use adw::prelude::ActionRowExt;

use crate::models::{AiAssistant, Session};
use gtk::glib;

/// Data passed to initialize each factory row.
pub struct SessionRowInit {
    pub session: Session,
}

/// A single session row inside the ListBox, managed by FactoryVecDeque.
#[derive(Debug)]
pub struct SessionRow {
    session: Session,
    context_menu: Option<gtk::PopoverMenu>,
}

#[derive(Debug)]
pub enum SessionRowOutput {
    ResumeRequested(String, AiAssistant),
}

fn emit_resume(sender: &relm4::Sender<SessionRowOutput>, id: &str, tool: AiAssistant) {
    let _ = sender.send(SessionRowOutput::ResumeRequested(id.to_string(), tool));
}

#[relm4::factory(pub)]
impl FactoryComponent for SessionRow {
    type Init = SessionRowInit;
    type Input = ();
    type Output = SessionRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        root = gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,

            append = &adw::ActionRow::builder()
                .title(Self::session_title(&self.session))
                .subtitle(Self::session_subtitle(&self.session))
                .activatable(true)
                .build() {
                set_hexpand: true,
                set_title_lines: 1,

                add_prefix = &gtk::Image::from_icon_name(self.session.tool.icon_name()) {
                    set_pixel_size: 16,
                },

                // Ending status label — hidden for "unknown"
                add_suffix = &gtk::Label::new(
                    crate::ui::format::ending_label(&self.session.ending_status)
                ) {
                    set_visible: crate::ui::format::ending_label(&self.session.ending_status).is_some(),
                    add_css_class: crate::ui::format::ending_css_class(&self.session.ending_status),
                    set_valign: gtk::Align::Center,
                },

                add_suffix = &gtk::Image::from_icon_name("go-next-symbolic") {
                    add_css_class: "dim-label",
                },
            },
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            session: init.session,
            context_menu: None,
        }
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        root: Self::Root,
        _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let root_for_actions = root.clone();
        let widgets = view_output!();

        let menu = gio::Menu::new();
        menu.append(Some("Resume in Terminal"), Some("row.resume"));

        let action_group = gio::SimpleActionGroup::new();
        let resume_action = gio::SimpleAction::new("resume", None);

        let output_sender = sender.output_sender().clone();
        let session_id = self.session.id.clone();
        let tool = self.session.tool;
        resume_action.connect_activate(move |_, _| {
            emit_resume(&output_sender, &session_id, tool);
        });

        action_group.add_action(&resume_action);
        root_for_actions.insert_action_group("row", Some(&action_group));

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(&root_for_actions);
        self.context_menu = Some(popover.clone());

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gdk::BUTTON_SECONDARY);
        gesture.connect_pressed(move |_, _, x, y| {
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        root_for_actions.add_controller(gesture);

        widgets
    }

    fn shutdown(&mut self, _widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        if let Some(popover) = self.context_menu.take() {
            popover.unparent();
        }
    }
}

impl SessionRow {
    pub fn session_id(&self) -> &str {
        &self.session.id
    }

    fn session_title(session: &Session) -> String {
        let raw = if let Some(prompt) = session
            .first_prompt
            .as_deref()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
        {
            prompt.to_string()
        } else {
            Self::project_name(session).unwrap_or_else(|| "Unknown project".to_string())
        };

        // ActionRow interprets title as Pango markup by default.
        // Escape special chars (<, >, &) to prevent parse failures.
        glib::markup_escape_text(&raw).to_string()
    }

    fn project_name(session: &Session) -> Option<String> {
        session
            .project_path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    }

    fn session_subtitle(session: &Session) -> String {
        let has_prompt = session
            .first_prompt
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty());

        let location = if has_prompt {
            Self::project_name(session).unwrap_or_else(|| "Unknown project".to_string())
        } else {
            session
                .project_path
                .clone()
                .unwrap_or_else(|| session.file_path.clone())
        };

        let duration_secs = session
            .last_updated
            .signed_duration_since(session.start_time)
            .num_seconds()
            .max(0);
        let duration = crate::ui::format::format_session_duration(duration_secs);

        let activity = crate::ui::format::format_dominant_activity(
            session.edit_count,
            session.command_count,
            session.read_count,
            session.message_count,
        );

        let relative_time = Self::format_relative_time(session.last_updated);
        let raw =
            format!("{location} \u{00b7} {duration} \u{00b7} {activity} \u{00b7} {relative_time}");

        // Escape for Pango markup (ActionRow subtitle also uses markup).
        glib::markup_escape_text(&raw).to_string()
    }

    fn format_relative_time(instant: DateTime<Utc>) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(instant);

        if duration < ChronoDuration::minutes(1) {
            "Just now".to_string()
        } else if duration < ChronoDuration::hours(1) {
            format!("{}m ago", duration.num_minutes())
        } else if duration < ChronoDuration::days(1) {
            format!("{}h ago", duration.num_hours())
        } else if duration < ChronoDuration::days(7) {
            format!("{}d ago", duration.num_days())
        } else {
            instant.format("%Y-%m-%d").to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_session(
        project_path: Option<&str>,
        first_prompt: Option<&str>,
        minutes_ago: i64,
    ) -> Session {
        let now = Utc::now();
        Session {
            id: "session-id".to_string(),
            tool: AiAssistant::ClaudeCode,
            project_path: project_path.map(str::to_string),
            project_id: None,
            start_time: now,
            message_count: 7,
            file_path: "/tmp/session.jsonl".to_string(),
            last_updated: now - ChronoDuration::minutes(minutes_ago),
            first_prompt: first_prompt.map(str::to_string),
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: "unknown".to_string(),
        }
    }

    #[test]
    fn session_activity_fields_default_to_zero_and_unknown() {
        let session = build_session(Some("/home/user/project"), Some("Fix bug"), 10);
        assert_eq!(session.edit_count, 0);
        assert_eq!(session.read_count, 0);
        assert_eq!(session.command_count, 0);
        assert_eq!(session.ending_status, "unknown");
    }

    #[test]
    fn session_title_uses_first_prompt_when_present() {
        let session = build_session(
            Some("/home/user/work/my-project"),
            Some("Investigate this failing parser test"),
            10,
        );

        assert_eq!(
            SessionRow::session_title(&session),
            "Investigate this failing parser test"
        );
    }

    #[test]
    fn session_title_falls_back_to_project_name_then_unknown_project() {
        let with_project = build_session(Some("/home/user/work/my-project"), None, 10);
        let without_project = build_session(None, None, 10);

        assert_eq!(SessionRow::session_title(&with_project), "my-project");
        assert_eq!(
            SessionRow::session_title(&without_project),
            "Unknown project"
        );
    }

    #[test]
    fn session_subtitle_shows_full_path_when_no_prompt() {
        let session = build_session(Some("/home/user/work/my-project"), None, 5);

        let subtitle = SessionRow::session_subtitle(&session);
        assert!(subtitle.starts_with("/home/user/work/my-project"));
        assert!(subtitle.contains("5m ago"));
    }

    #[test]
    fn session_subtitle_shows_project_name_when_prompt_present() {
        let session = build_session(Some("/home/user/work/my-project"), Some("Fix the build"), 5);

        let subtitle = SessionRow::session_subtitle(&session);
        assert!(subtitle.starts_with("my-project"));
        assert!(subtitle.contains("5m ago"));
    }

    #[test]
    fn session_subtitle_shows_duration_and_dominant_activity() {
        let mut session = build_session(Some("/home/user/work/my-project"), Some("Fix bug"), 5);
        session.edit_count = 8;
        session.command_count = 3;
        session.read_count = 12;

        let subtitle = SessionRow::session_subtitle(&session);
        assert!(subtitle.contains("my-project"));
        assert!(subtitle.contains("8 edits"));
        assert!(subtitle.contains("5m ago"));
    }

    #[test]
    fn session_subtitle_uses_command_when_no_edits() {
        let mut session = build_session(Some("/home/user/work/my-project"), Some("Run tests"), 5);
        session.command_count = 3;

        let subtitle = SessionRow::session_subtitle(&session);
        assert!(subtitle.contains("3 commands"));
    }

    #[test]
    fn session_subtitle_falls_back_to_messages() {
        let session = build_session(Some("/home/user/work/my-project"), Some("Chat"), 5);
        // All counts are 0, message_count is 7 (from build_session)

        let subtitle = SessionRow::session_subtitle(&session);
        assert!(subtitle.contains("7 messages"));
    }

    #[test]
    fn session_title_escapes_markup_special_chars() {
        let session = build_session(
            Some("/home/user/work/my-project"),
            Some("/review & fix"),
            10,
        );

        assert_eq!(SessionRow::session_title(&session), "/review &amp; fix");
    }

    #[test]
    fn emit_resume_sends_resume_requested_output() {
        let (sender, receiver) = relm4::channel();

        emit_resume(&sender, "session-123", AiAssistant::OpenCode);

        assert!(matches!(
            receiver.recv_sync(),
            Some(SessionRowOutput::ResumeRequested(id, tool)) if id == "session-123" && tool == AiAssistant::OpenCode
        ));
    }
}
