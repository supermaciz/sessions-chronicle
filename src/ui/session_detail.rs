use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};
use std::path::PathBuf;

use crate::database::{load_message_previews_for_session, load_session};
use crate::models::{Session, Tool};
use crate::ui::message_row::{MessageRow, MessageRowInit};

#[derive(Debug)]
pub struct SessionDetail {
    db_path: PathBuf,
    session: Option<Session>,
    messages: FactoryVecDeque<MessageRow>,
    page_size: usize,
    preview_len: usize,
    loaded_count: usize,
    has_more_messages: bool,
}

#[derive(Debug)]
pub enum SessionDetailMsg {
    SetSession(String),
    LoadMore,
    ResumeClicked,
    #[allow(dead_code)]
    Clear,
}

#[derive(Debug)]
pub enum SessionDetailOutput {
    ResumeRequested(String, Tool),
}

#[relm4::component(pub)]
impl SimpleComponent for SessionDetail {
    type Init = PathBuf;
    type Input = SessionDetailMsg;
    type Output = SessionDetailOutput;
    type Widgets = SessionDetailWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 0,
            set_vexpand: true,

            #[name = "content_stack"]
            gtk::Stack {
                set_vexpand: true,
                set_hexpand: true,

                #[name = "loading_state"]
                adw::StatusPage {
                    set_vexpand: true,
                    set_icon_name: Some("content-loading-symbolic"),
                    set_title: "Loading Session",
                    set_description: Some("Please wait..."),
                },

                #[name = "detail_content"]
                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 12,
                        set_margin_all: 12,

                        // Metadata header
                        gtk::Box {
                            add_css_class: "card",
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,
                            set_margin_bottom: 12,

                            #[name = "project_label"]
                            gtk::Label {
                                add_css_class: "title-2",
                                set_halign: gtk::Align::Start,
                                set_wrap: true,
                                set_wrap_mode: gtk::pango::WrapMode::WordChar,
                            },

                            #[name = "path_label"]
                            gtk::Label {
                                add_css_class: "dim-label",
                                set_halign: gtk::Align::Start,
                                set_wrap: true,
                                set_wrap_mode: gtk::pango::WrapMode::WordChar,
                                set_selectable: true,
                            },

                            gtk::Separator {},

                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 24,
                                set_halign: gtk::Align::Start,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 6,

                                    #[name = "tool_icon"]
                                    gtk::Image {
                                        set_pixel_size: 16,
                                    },

                                    #[name = "tool_label"]
                                    gtk::Label {
                                        add_css_class: "dim-label",
                                    },
                                },

                                #[name = "message_count_label"]
                                gtk::Label {
                                    add_css_class: "dim-label",
                                },

                                #[name = "time_label"]
                                gtk::Label {
                                    add_css_class: "dim-label",
                                },
                            },

                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 6,
                                set_halign: gtk::Align::Start,

                                gtk::Label {
                                    set_label: "Session ID:",
                                    add_css_class: "dim-label",
                                },

                                #[name = "session_id_label"]
                                gtk::Label {
                                    add_css_class: "monospace",
                                    set_selectable: true,
                                },
                            },

                            #[name = "resume_button"]
                            gtk::Button {
                                set_label: "Resume in Terminal",
                                add_css_class: "suggested-action",
                                set_halign: gtk::Align::Start,
                                connect_clicked => SessionDetailMsg::ResumeClicked,
                            },
                        },

                        // Messages container — managed by factory
                        #[local_ref]
                        messages_box -> gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,
                        },

                        #[name = "load_more_button"]
                        gtk::Button {
                            set_label: "Load more",
                            set_halign: gtk::Align::Center,
                            set_margin_top: 12,
                            set_margin_bottom: 12,
                            #[watch]
                            set_visible: model.has_more_messages,
                            connect_clicked => SessionDetailMsg::LoadMore,
                        },
                    },
                },
            },
        }
    }

    fn init(
        db_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let messages: FactoryVecDeque<MessageRow> =
            FactoryVecDeque::builder().launch_default().detach();

        let model = Self {
            db_path,
            session: None,
            messages,
            page_size: 200,
            preview_len: 2000,
            loaded_count: 0,
            has_more_messages: false,
        };

        let messages_box = model.messages.widget();
        let widgets = view_output!();

        widgets
            .content_stack
            .set_visible_child(&widgets.loading_state);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SessionDetailMsg::SetSession(session_id) => {
                match load_session(&self.db_path, &session_id) {
                    Ok(Some(session)) => {
                        self.session = Some(session);
                    }
                    Ok(None) => {
                        tracing::warn!("Session not found: {}", session_id);
                        self.session = None;
                        self.messages.guard().clear();
                        self.loaded_count = 0;
                        self.has_more_messages = false;
                        return;
                    }
                    Err(err) => {
                        tracing::error!("Failed to load session {}: {}", session_id, err);
                        self.session = None;
                        self.messages.guard().clear();
                        self.loaded_count = 0;
                        self.has_more_messages = false;
                        return;
                    }
                }

                // Load first page
                match load_message_previews_for_session(
                    &self.db_path,
                    &session_id,
                    self.page_size,
                    0,
                    self.preview_len,
                ) {
                    Ok(previews) => {
                        self.has_more_messages = previews.len() == self.page_size;
                        self.loaded_count = previews.len();
                        let mut guard = self.messages.guard();
                        guard.clear();
                        for preview in previews {
                            guard.push_back(MessageRowInit { preview });
                        }
                    }
                    Err(err) => {
                        tracing::error!(
                            "Failed to load message previews for {}: {}",
                            session_id,
                            err
                        );
                        self.messages.guard().clear();
                        self.loaded_count = 0;
                        self.has_more_messages = false;
                    }
                }
            }
            SessionDetailMsg::LoadMore => {
                if let Some(session) = &self.session {
                    let session_id = session.id.clone();
                    let offset = self.loaded_count;
                    match load_message_previews_for_session(
                        &self.db_path,
                        &session_id,
                        self.page_size,
                        offset,
                        self.preview_len,
                    ) {
                        Ok(previews) => {
                            self.has_more_messages = previews.len() == self.page_size;
                            self.loaded_count += previews.len();
                            let mut guard = self.messages.guard();
                            for preview in previews {
                                guard.push_back(MessageRowInit { preview });
                            }
                        }
                        Err(err) => {
                            tracing::error!("Failed to load more previews: {}", err);
                            self.has_more_messages = false;
                        }
                    }
                }
            }
            SessionDetailMsg::ResumeClicked => {
                if let Some(session) = &self.session {
                    let _ = sender.output(SessionDetailOutput::ResumeRequested(
                        session.id.clone(),
                        session.tool,
                    ));
                }
            }
            SessionDetailMsg::Clear => {
                self.session = None;
                self.messages.guard().clear();
                self.loaded_count = 0;
                self.has_more_messages = false;
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        if let Some(session) = &self.session {
            let project_name = session
                .project_path
                .as_deref()
                .and_then(|path| std::path::Path::new(path).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("Unknown project");
            widgets.project_label.set_label(project_name);

            let path = session
                .project_path
                .as_deref()
                .unwrap_or(&session.file_path);
            widgets.path_label.set_label(path);

            widgets
                .tool_icon
                .set_icon_name(Some(session.tool.icon_name()));
            widgets.tool_label.set_label(session.tool.display_name());

            widgets
                .message_count_label
                .set_label(&format!("{} messages", session.message_count));

            let time_str = format!(
                "Started {} • Updated {}",
                Self::format_relative_time(session.start_time),
                Self::format_relative_time(session.last_updated)
            );
            widgets.time_label.set_label(&time_str);

            widgets.session_id_label.set_label(&session.id);
            widgets.resume_button.set_sensitive(true);

            widgets
                .content_stack
                .set_visible_child(&widgets.detail_content);
        } else {
            widgets.resume_button.set_sensitive(false);
            widgets
                .content_stack
                .set_visible_child(&widgets.loading_state);
        }
    }
}

impl SessionDetail {
    fn format_relative_time(instant: DateTime<Utc>) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(instant);

        if duration < ChronoDuration::minutes(1) {
            "just now".to_string()
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
