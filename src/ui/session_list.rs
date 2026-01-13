use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};
use std::path::PathBuf;

use adw::prelude::ActionRowExt;

use crate::database::load_sessions;
use crate::models::Session;

#[derive(Debug)]
pub struct SessionList {
    sessions: Vec<Session>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum SessionListMsg {
    SelectSession(String),
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum SessionListOutput {
    SessionSelected(String),
}

#[relm4::component(pub)]
impl SimpleComponent for SessionList {
    type Init = PathBuf;
    type Input = SessionListMsg;
    type Output = SessionListOutput;
    type Widgets = SessionListWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 0,
            set_vexpand: true,

            #[name = "content_stack"]
            gtk::Stack {
                set_vexpand: true,
                set_hexpand: true,

                #[name = "empty_state"]
                adw::StatusPage {
                    set_vexpand: true,
                    set_icon_name: Some("document-open-recent-symbolic"),
                    set_title: "No Sessions Yet",
                    set_description: Some("Your AI coding sessions will appear here"),
                },

                #[name = "session_list_scroller"]
                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    #[name = "session_list"]
                    gtk::ListBox {
                        add_css_class: "boxed-list",
                        set_selection_mode: gtk::SelectionMode::None,
                    }
                }
            }
        }
    }

    fn init(
        db_path: Self::Init,
        _root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let sessions = match load_sessions(&db_path) {
            Ok(sessions) => sessions,
            Err(err) => {
                tracing::error!("Failed to load sessions: {}", err);
                Vec::new()
            }
        };

        let model = Self { sessions };
        let widgets = view_output!();

        if model.sessions.is_empty() {
            widgets
                .content_stack
                .set_visible_child(&widgets.empty_state);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.session_list_scroller);

            for session in &model.sessions {
                let row = Self::build_session_row(session);
                widgets.session_list.append(&row);
            }
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SessionListMsg::SelectSession(id) => {
                let _ = sender.output(SessionListOutput::SessionSelected(id));
            }
        }
    }
}

impl SessionList {
    fn build_session_row(session: &Session) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(Self::session_title(session))
            .subtitle(Self::session_subtitle(session))
            .build();

        let icon = gtk::Image::from_icon_name(session.tool.icon_name());
        icon.set_pixel_size(16);
        row.add_prefix(&icon);

        let time_label = gtk::Label::new(Some(&Self::format_relative_time(session.last_updated)));
        time_label.add_css_class("dim-label");
        time_label.set_halign(gtk::Align::End);
        row.add_suffix(&time_label);

        row
    }

    fn session_title(session: &Session) -> String {
        session
            .project_path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| "Unknown project".to_string())
    }

    fn session_subtitle(session: &Session) -> String {
        let detail = session
            .project_path
            .as_deref()
            .unwrap_or(&session.file_path);
        format!("{} • {} messages", detail, session.message_count)
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
