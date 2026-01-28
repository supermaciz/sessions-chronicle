use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::database::{load_message_previews_for_session, load_session};
use crate::models::{MessagePreview, Session, Tool};

#[derive(Debug)]
pub struct SessionDetail {
    db_path: PathBuf,
    session: Option<Session>,
    message_previews: Vec<MessagePreview>,
    page_size: usize,
    preview_len: usize,
    has_more_messages: bool,
    sender: ComponentSender<Self>,
    output_sender: relm4::Sender<SessionDetailOutput>,
    current_session_id: Rc<RefCell<Option<String>>>,
    current_tool: Rc<RefCell<Option<Tool>>>,
}

#[derive(Debug)]
pub enum SessionDetailMsg {
    SetSession(String),
    LoadMore,
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

                    #[name = "main_box"]
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 12,
                        set_margin_all: 12,

                        // Metadata header
                        #[name = "metadata_box"]
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

                            #[name = "info_box"]
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

                            #[name = "session_id_box"]
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
                            },
                        },

                        // Messages container
                        #[name = "messages_box"]
                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,
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
        let output_sender = sender.output_sender().clone();

        let current_session_id = Rc::new(RefCell::new(None));
        let current_tool = Rc::new(RefCell::new(None));

        let model = Self {
            db_path,
            session: None,
            message_previews: Vec::new(),
            page_size: 200,
            preview_len: 2000,
            has_more_messages: false,
            sender: sender.clone(),
            output_sender,
            current_session_id: current_session_id.clone(),
            current_tool: current_tool.clone(),
        };
        let widgets = view_output!();

        // Show loading state initially
        widgets
            .content_stack
            .set_visible_child(&widgets.loading_state);

        // Connect resume button once
        let sender = model.output_sender.clone();
        widgets.resume_button.connect_clicked(move |_| {
            if let Some(session_id) = current_session_id.borrow().as_ref()
                && let Some(tool) = current_tool.borrow().as_ref()
            {
                let _ = sender.send(SessionDetailOutput::ResumeRequested(
                    session_id.clone(),
                    *tool,
                ));
            }
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            SessionDetailMsg::SetSession(session_id) => {
                // Load session metadata
                match load_session(&self.db_path, &session_id) {
                    Ok(Some(session)) => {
                        self.current_session_id
                            .borrow_mut()
                            .replace(session.id.clone());
                        self.current_tool.borrow_mut().replace(session.tool);
                        self.session = Some(session);
                    }
                    Ok(None) => {
                        tracing::warn!("Session not found: {}", session_id);
                        self.session = None;
                        self.message_previews = Vec::new();
                        self.current_session_id.borrow_mut().take();
                        self.current_tool.borrow_mut().take();
                        return;
                    }
                    Err(err) => {
                        tracing::error!("Failed to load session {}: {}", session_id, err);
                        self.session = None;
                        self.message_previews = Vec::new();
                        self.current_session_id.borrow_mut().take();
                        self.current_tool.borrow_mut().take();
                        return;
                    }
                }

                // Load first page of message previews
                match load_message_previews_for_session(
                    &self.db_path,
                    &session_id,
                    self.page_size,
                    0,
                    self.preview_len,
                ) {
                    Ok(previews) => {
                        self.has_more_messages = previews.len() == self.page_size;
                        self.message_previews = previews;
                    }
                    Err(err) => {
                        tracing::error!(
                            "Failed to load message previews for {}: {}",
                            session_id,
                            err
                        );
                        self.message_previews = Vec::new();
                        self.has_more_messages = false;
                    }
                }
            }
            SessionDetailMsg::LoadMore => {
                if let Some(session_id) = self.current_session_id.borrow().as_ref() {
                    let offset = self.message_previews.len();
                    match load_message_previews_for_session(
                        &self.db_path,
                        session_id,
                        self.page_size,
                        offset,
                        self.preview_len,
                    ) {
                        Ok(mut previews) => {
                            self.has_more_messages = previews.len() == self.page_size;
                            self.message_previews.append(&mut previews);
                        }
                        Err(err) => {
                            tracing::error!("Failed to load more previews: {}", err);
                            self.has_more_messages = false;
                        }
                    }
                }
            }
            SessionDetailMsg::Clear => {
                self.session = None;
                self.message_previews = Vec::new();
                self.has_more_messages = false;
                self.current_session_id.borrow_mut().take();
                self.current_tool.borrow_mut().take();
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        let start = std::time::Instant::now();
        // Clear existing message widgets
        while let Some(child) = widgets.messages_box.first_child() {
            widgets.messages_box.remove(&child);
        }

        if let Some(session) = &self.session {
            // Update metadata
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
            widgets.resume_button.set_label("Resume in Terminal");

            // Build message widgets
            for preview in &self.message_previews {
                let message_widget = Self::build_message_widget(preview);
                widgets.messages_box.append(&message_widget);
            }

            // Add "Load more" button if there are more messages
            if self.has_more_messages {
                let sender = self.sender.clone();
                let load_more_button = gtk::Button::builder()
                    .label("Load more")
                    .halign(gtk::Align::Center)
                    .margin_top(12)
                    .margin_bottom(12)
                    .build();
                load_more_button.connect_clicked(move |_| {
                    sender.input(SessionDetailMsg::LoadMore);
                });
                widgets.messages_box.append(&load_more_button);
            }

            tracing::debug!(
                "post_view UI rendering took {:?} - {} rows rendered",
                start.elapsed(),
                self.message_previews.len()
            );

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
    fn build_message_widget(preview: &MessagePreview) -> gtk::Box {
        let container = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .css_classes(["message-row", preview.role.css_class()])
            .build();

        // Header with role and time
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let role_label = gtk::Label::builder()
            .label(preview.role.label())
            .css_classes(["caption", "heading", preview.role.css_class()])
            .halign(gtk::Align::Start)
            .build();
        header.append(&role_label);

        let time_label = gtk::Label::builder()
            .label(preview.timestamp.format("%H:%M:%S").to_string())
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Start)
            .build();
        header.append(&time_label);

        container.append(&header);

        // Content label
        let content_label = gtk::Label::builder()
            .label(&preview.content_preview)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .selectable(true)
            .build();
        container.append(&content_label);

        container
    }

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
