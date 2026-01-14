use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};
use std::path::PathBuf;

use adw::prelude::ActionRowExt;

use crate::database::{load_sessions, search_sessions};
use crate::models::{Session, Tool};

#[derive(Debug)]
pub struct SessionList {
    db_path: PathBuf,
    active_tools: Vec<Tool>,
    search_query: String,
    sessions: Vec<Session>,
    all_tools_selected: bool,
}

#[derive(Debug)]
pub enum SessionListMsg {
    SetTools(Vec<Tool>),
    SetSearchQuery(String),
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
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let active_tools = vec![Tool::ClaudeCode, Tool::OpenCode, Tool::Codex];
        let search_query = String::new();
        let sessions = Self::fetch_sessions(&db_path, &active_tools, &search_query);

        let model = Self {
            db_path,
            active_tools,
            search_query,
            sessions,
            all_tools_selected: true,
        };
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

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            SessionListMsg::SetTools(tools) => {
                self.active_tools = tools.clone();
                self.all_tools_selected = tools.len() == Tool::ALL.len();
                self.sessions =
                    Self::fetch_sessions(&self.db_path, &self.active_tools, &self.search_query);
            }
            SessionListMsg::SetSearchQuery(query) => {
                self.search_query = query;
                self.sessions =
                    Self::fetch_sessions(&self.db_path, &self.active_tools, &self.search_query);
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        while let Some(row) = widgets.session_list.first_child() {
            widgets.session_list.remove(&row);
        }

        if self.sessions.is_empty() {
            if !self.search_query.trim().is_empty() {
                widgets.empty_state.set_title("No sessions match search");
                widgets
                    .empty_state
                    .set_description(Some("Try a different query or adjust filters"));
            } else if self.all_tools_selected {
                widgets.empty_state.set_title("No Sessions Yet");
                widgets
                    .empty_state
                    .set_description(Some("Your AI coding sessions will appear here"));
            } else {
                widgets.empty_state.set_title("No sessions match filters");
                widgets
                    .empty_state
                    .set_description(Some("Try adjusting the tool filters in the sidebar"));
            }
            widgets
                .content_stack
                .set_visible_child(&widgets.empty_state);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.session_list_scroller);

            for session in &self.sessions {
                let row = Self::build_session_row(session);
                widgets.session_list.append(&row);
            }
        }
    }
}

impl SessionList {
    fn fetch_sessions(db_path: &PathBuf, tools: &[Tool], query: &str) -> Vec<Session> {
        let query = query.trim();
        let sessions = if query.is_empty() {
            load_sessions(db_path, tools)
        } else {
            search_sessions(db_path, tools, query)
        };

        match sessions {
            Ok(sessions) => sessions,
            Err(err) => {
                tracing::error!("Failed to load sessions: {}", err);
                Vec::new()
            }
        }
    }

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
