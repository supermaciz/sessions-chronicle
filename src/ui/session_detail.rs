use std::cell::Cell;
use std::path::PathBuf;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::glib;
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};

use crate::database::{load_message_previews_for_session, load_session};
use crate::models::Session;
use crate::ui::message_row::{MessageRow, MessageRowInit, MessageRowOutput};

#[derive(Debug)]
pub struct SessionDetail {
    db_path: PathBuf,
    session: Option<Session>,
    messages: FactoryVecDeque<MessageRow>,
    page_size: usize,
    preview_len: usize,
    loaded_count: usize,
    has_more_messages: bool,
    search_query: Option<String>,
    match_counts: Vec<usize>,
    current_match: usize,
    total_matches: usize,
    scroll_to_message: Cell<Option<usize>>,
}

#[derive(Debug)]
pub enum SessionDetailMsg {
    SetSession {
        id: String,
        search_query: Option<String>,
    },
    #[allow(dead_code)]
    UpdateSearchQuery(Option<String>),
    LoadMore,
    PrevMatch,
    NextMatch,
    ClearSearch,
    MatchCount(usize),
    #[allow(dead_code)]
    Clear,
}

#[derive(Debug)]
pub enum SessionDetailOutput {}

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

                #[name = "detail_overlay"]
                gtk::Overlay {
                    set_vexpand: true,

                    #[wrap(Some)]
                    set_child = &gtk::ScrolledWindow {
                        set_vexpand: true,
                        set_hscrollbar_policy: gtk::PolicyType::Never,

                        #[name = "scroll_child"]
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

                    // Floating search navigation bar
                    add_overlay = &gtk::Box {
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Start,
                        add_css_class: "search-nav-bar",
                        set_spacing: 8,
                        #[watch]
                        set_visible: model.search_query.is_some(),

                        #[name = "search_term_label"]
                        gtk::Label {
                            add_css_class: "dim-label",
                            #[watch]
                            set_label: &model.search_query.as_deref()
                                .map(|q| format!("\"{}\"", q))
                                .unwrap_or_default(),
                        },

                        gtk::Button {
                            set_icon_name: "go-up-symbolic",
                            set_tooltip_text: Some("Previous match"),
                            add_css_class: "flat",
                            #[watch]
                            set_sensitive: model.total_matches > 0,
                            connect_clicked => SessionDetailMsg::PrevMatch,
                        },

                        #[name = "match_counter_label"]
                        gtk::Label {
                            add_css_class: "match-counter",
                            set_halign: gtk::Align::Center,
                            #[watch]
                            set_label: &if model.total_matches > 0 {
                                format!("{} / {}", model.current_match + 1, model.total_matches)
                            } else {
                                "0 matches".to_string()
                            },
                        },

                        gtk::Button {
                            set_icon_name: "go-down-symbolic",
                            set_tooltip_text: Some("Next match"),
                            add_css_class: "flat",
                            #[watch]
                            set_sensitive: model.total_matches > 0,
                            connect_clicked => SessionDetailMsg::NextMatch,
                        },

                        gtk::Button {
                            set_icon_name: "window-close-symbolic",
                            set_tooltip_text: Some("Close search highlights"),
                            add_css_class: "flat",
                            connect_clicked => SessionDetailMsg::ClearSearch,
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
        let messages: FactoryVecDeque<MessageRow> = FactoryVecDeque::builder()
            .launch_default()
            .forward(sender.input_sender(), |output| match output {
                MessageRowOutput::MatchCount { count } => SessionDetailMsg::MatchCount(count),
            });

        let model = Self {
            db_path,
            session: None,
            messages,
            page_size: 200,
            preview_len: 2000,
            loaded_count: 0,
            has_more_messages: false,
            search_query: None,
            match_counts: Vec::new(),
            current_match: 0,
            total_matches: 0,
            scroll_to_message: Cell::new(None),
        };

        let messages_box = model.messages.widget();
        let widgets = view_output!();

        widgets
            .content_stack
            .set_visible_child(&widgets.loading_state);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            SessionDetailMsg::SetSession {
                id: session_id,
                search_query,
            } => {
                self.search_query = search_query;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;

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

                self.load_first_page(&session_id);
            }
            SessionDetailMsg::UpdateSearchQuery(query) => {
                self.search_query = query;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;

                // Rebuild messages with new highlighting
                if let Some(session) = &self.session {
                    let session_id = session.id.clone();
                    self.load_first_page(&session_id);
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
                            let highlight = self.search_query.clone();
                            let mut guard = self.messages.guard();
                            for preview in previews {
                                guard.push_back(MessageRowInit {
                                    preview,
                                    highlight_query: highlight.clone(),
                                });
                            }
                        }
                        Err(err) => {
                            tracing::error!("Failed to load more previews: {}", err);
                            self.has_more_messages = false;
                        }
                    }
                }
            }
            SessionDetailMsg::PrevMatch => {
                if self.total_matches > 0 {
                    if self.current_match == 0 {
                        self.current_match = self.total_matches - 1;
                    } else {
                        self.current_match -= 1;
                    }
                    let (msg_idx, _) =
                        Self::find_message_for_match(&self.match_counts, self.current_match);
                    self.scroll_to_message.set(Some(msg_idx));
                }
            }
            SessionDetailMsg::NextMatch => {
                if self.total_matches > 0 {
                    self.current_match = (self.current_match + 1) % self.total_matches;
                    let (msg_idx, _) =
                        Self::find_message_for_match(&self.match_counts, self.current_match);
                    self.scroll_to_message.set(Some(msg_idx));
                }
            }
            SessionDetailMsg::MatchCount(count) => {
                let was_empty = self.total_matches == 0;
                self.match_counts.push(count);
                self.total_matches = self.match_counts.iter().sum();
                // Auto-scroll to first match when results arrive
                if was_empty && self.total_matches > 0 && self.search_query.is_some() {
                    self.current_match = 0;
                    let (msg_idx, _) = Self::find_message_for_match(&self.match_counts, 0);
                    self.scroll_to_message.set(Some(msg_idx));
                }
            }
            SessionDetailMsg::ClearSearch => {
                self.search_query = None;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;

                // Rebuild messages without highlighting
                if let Some(session) = &self.session {
                    let session_id = session.id.clone();
                    self.load_first_page(&session_id);
                }
            }
            SessionDetailMsg::Clear => {
                self.session = None;
                self.messages.guard().clear();
                self.loaded_count = 0;
                self.has_more_messages = false;
                self.search_query = None;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;
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

            widgets
                .content_stack
                .set_visible_child(&widgets.detail_overlay);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.loading_state);
        }

        // Scroll to match if requested
        if let Some(msg_index) = self.scroll_to_message.take() {
            let messages_widget = self.messages.widget().clone();
            let scroll_child = widgets.scroll_child.clone();
            glib::idle_add_local_once(move || {
                let Some(target) = messages_widget
                    .observe_children()
                    .item(msg_index as u32)
                    .and_then(|obj| obj.downcast::<gtk::Widget>().ok())
                else {
                    return;
                };

                let Some(point) =
                    target.compute_point(&scroll_child, &gtk::graphene::Point::zero())
                else {
                    return;
                };

                let Some(scrolled_window) = scroll_child
                    .ancestor(gtk::ScrolledWindow::static_type())
                    .and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok())
                else {
                    return;
                };

                let vadj = scrolled_window.vadjustment();
                // Position match roughly 1/3 from the top
                let target_y = (point.y() as f64) - (vadj.page_size() / 3.0);
                vadj.set_value(target_y.max(0.0));
            });
        }
    }
}

impl SessionDetail {
    fn load_first_page(&mut self, session_id: &str) {
        match load_message_previews_for_session(
            &self.db_path,
            session_id,
            self.page_size,
            0,
            self.preview_len,
        ) {
            Ok(previews) => {
                self.has_more_messages = previews.len() == self.page_size;
                self.loaded_count = previews.len();
                let highlight = self.search_query.clone();
                let mut guard = self.messages.guard();
                guard.clear();
                for preview in previews {
                    guard.push_back(MessageRowInit {
                        preview,
                        highlight_query: highlight.clone(),
                    });
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

    /// Resolve a global match index to a (message_index, local_match_index) pair.
    fn find_message_for_match(counts: &[usize], global_index: usize) -> (usize, usize) {
        let mut remaining = global_index;
        for (i, &count) in counts.iter().enumerate() {
            if remaining < count {
                return (i, remaining);
            }
            remaining -= count;
        }
        (counts.len().saturating_sub(1), 0)
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
