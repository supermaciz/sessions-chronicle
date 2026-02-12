use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::gtk::{gdk, gio};
use relm4::{adw, gtk};

use adw::prelude::ActionRowExt;

use crate::models::{Session, Tool};
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
    ResumeRequested(String, Tool),
}

fn emit_resume(sender: &relm4::Sender<SessionRowOutput>, id: &str, tool: Tool) {
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

        let relative_time = Self::format_relative_time(session.last_updated);
        let raw = format!(
            "{} · {} messages · {}",
            location, session.message_count, relative_time
        );

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
            tool: Tool::ClaudeCode,
            project_path: project_path.map(str::to_string),
            start_time: now,
            message_count: 7,
            file_path: "/tmp/session.jsonl".to_string(),
            last_updated: now - ChronoDuration::minutes(minutes_ago),
            first_prompt: first_prompt.map(str::to_string),
        }
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

        assert_eq!(
            SessionRow::session_subtitle(&session),
            "/home/user/work/my-project · 7 messages · 5m ago"
        );
    }

    #[test]
    fn session_subtitle_shows_project_name_when_prompt_present() {
        let session = build_session(Some("/home/user/work/my-project"), Some("Fix the build"), 5);

        assert_eq!(
            SessionRow::session_subtitle(&session),
            "my-project · 7 messages · 5m ago"
        );
    }

    #[test]
    fn session_title_escapes_markup_special_chars() {
        let session = build_session(
            Some("/home/user/work/my-project"),
            Some("<command-message>review</command-message> & fix"),
            10,
        );

        assert_eq!(
            SessionRow::session_title(&session),
            "&lt;command-message&gt;review&lt;/command-message&gt; &amp; fix"
        );
    }

    #[test]
    fn emit_resume_sends_resume_requested_output() {
        let (sender, receiver) = relm4::channel();

        emit_resume(&sender, "session-123", Tool::OpenCode);

        assert!(matches!(
            receiver.recv_sync(),
            Some(SessionRowOutput::ResumeRequested(id, tool)) if id == "session-123" && tool == Tool::OpenCode
        ));
    }
}
