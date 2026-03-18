use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use gtk::glib;
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};

use crate::database::load_transcript_items;
use crate::models::Session;
use crate::ui::transcript_row::{
    TranscriptRow, TranscriptRowOutput, transcript_item_init_from_row,
};

#[derive(Debug)]
pub struct SessionDetail {
    db_path: Arc<PathBuf>,
    session: Option<Session>,
    messages: FactoryVecDeque<TranscriptRow>,
    page_size: usize,
    preview_len: usize,
    loaded_count: usize,
    has_more_messages: bool,
    search_query: Option<String>,
    /// Keyed by transcript item_index (== factory position). Only message rows contribute.
    match_counts: BTreeMap<usize, usize>,
    current_match: usize,
    total_matches: usize,
    scroll_to_item: Cell<Option<usize>>,
    pending_toast: Cell<bool>,
}

#[derive(Debug)]
pub enum SessionDetailOutput {
    InspectToolCall(String),
    InspectSubagent(String),
}

#[derive(Debug)]
pub enum SessionDetailMsg {
    SetSession {
        session: Box<Session>,
        search_query: Option<String>,
    },
    UpdateSearchQuery(Option<String>),
    LoadMore,
    PrevMatch,
    NextMatch,
    ClearSearch,
    MatchCount(usize, usize),
    ShowExpandLoadFailure,
    Clear,
    InspectToolCall(String),
    InspectSubagent(String),
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

                #[name = "toast_overlay"]
                adw::ToastOverlay {
                    set_vexpand: true,

                    #[name = "detail_overlay"]
                    gtk::Overlay {

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

                                // Token usage rows
                                #[name = "token_row"]
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 2,
                                    set_halign: gtk::Align::Start,
                                    #[watch]
                                    set_visible: model.session.as_ref()
                                        .and_then(|s| s.token_usage.as_ref())
                                        .is_some(),

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 6,

                                        gtk::Label {
                                            set_label: "Tokens:",
                                            add_css_class: "dim-label",
                                        },

                                        #[name = "token_details_label"]
                                        gtk::Label {
                                            add_css_class: "dim-label",
                                            set_has_tooltip: true,
                                        },
                                    },

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 6,
                                        #[watch]
                                        set_visible: model.session.as_ref()
                                            .and_then(|s| s.token_usage.as_ref())
                                            .and_then(crate::ui::format::format_token_cache)
                                            .is_some(),

                                        gtk::Label {
                                            set_label: "Cache:",
                                            add_css_class: "dim-label",
                                        },

                                        #[name = "token_cache_label"]
                                        gtk::Label {
                                            add_css_class: "dim-label",
                                            set_has_tooltip: true,
                                        },
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

                            // Transcript rows — managed by factory
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
                }, // close toast_overlay
            },
        }
    }

    fn init(
        db_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let messages: FactoryVecDeque<TranscriptRow> = FactoryVecDeque::builder()
            .launch_default()
            .forward(sender.input_sender(), |output| match output {
                TranscriptRowOutput::MatchCountChanged { item_index, count } => {
                    SessionDetailMsg::MatchCount(item_index, count)
                }
                TranscriptRowOutput::ExpandLoadFailed { .. } => {
                    SessionDetailMsg::ShowExpandLoadFailure
                }
                TranscriptRowOutput::InspectToolCall(id) => SessionDetailMsg::InspectToolCall(id),
                TranscriptRowOutput::InspectSubagent(id) => SessionDetailMsg::InspectSubagent(id),
            });

        let db_path = Arc::new(db_path);
        let model = Self {
            db_path,
            session: None,
            messages,
            page_size: 200,
            preview_len: 2000,
            loaded_count: 0,
            has_more_messages: false,
            search_query: None,
            match_counts: BTreeMap::new(),
            current_match: 0,
            total_matches: 0,
            scroll_to_item: Cell::new(None),
            pending_toast: Cell::new(false),
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
            SessionDetailMsg::SetSession {
                session,
                search_query,
            } => {
                self.search_query = search_query;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;

                let session = *session;
                let session_id = session.id.clone();
                self.session = Some(session);
                self.load_first_page(&session_id);
            }
            SessionDetailMsg::UpdateSearchQuery(query) => {
                self.search_query = query;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;

                if let Some(session) = &self.session {
                    let session_id = session.id.clone();
                    self.load_first_page(&session_id);
                }
            }
            SessionDetailMsg::LoadMore => {
                if let Some(session) = &self.session {
                    let session_id = session.id.clone();
                    let offset = self.loaded_count;
                    match load_transcript_items(
                        &self.db_path,
                        &session_id,
                        self.page_size as i64,
                        offset as i64,
                        self.preview_len as i64,
                    ) {
                        Ok(rows) => {
                            self.has_more_messages = rows.len() == self.page_size;
                            self.loaded_count += rows.len();
                            let highlight = self.search_query.clone();
                            let db_path = self.db_path.clone();
                            let mut guard = self.messages.guard();
                            for row in rows {
                                guard.push_back(transcript_item_init_from_row(
                                    &row,
                                    &session_id,
                                    highlight.clone(),
                                    db_path.clone(),
                                ));
                            }
                        }
                        Err(err) => {
                            tracing::error!("Failed to load more transcript items: {}", err);
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
                    let item_idx =
                        Self::find_item_for_match(&self.match_counts, self.current_match);
                    self.scroll_to_item.set(Some(item_idx));
                }
            }
            SessionDetailMsg::NextMatch => {
                if self.total_matches > 0 {
                    self.current_match = (self.current_match + 1) % self.total_matches;
                    let item_idx =
                        Self::find_item_for_match(&self.match_counts, self.current_match);
                    self.scroll_to_item.set(Some(item_idx));
                }
            }
            SessionDetailMsg::MatchCount(item_index, count) => {
                let was_empty = self.total_matches == 0;
                self.match_counts.insert(item_index, count);
                self.total_matches = self.match_counts.values().sum();
                if was_empty && self.total_matches > 0 && self.search_query.is_some() {
                    self.current_match = 0;
                    let item_idx = Self::find_item_for_match(&self.match_counts, 0);
                    self.scroll_to_item.set(Some(item_idx));
                }
                if self.total_matches > 0 {
                    if self.current_match >= self.total_matches {
                        self.current_match = self.total_matches - 1;
                    }
                } else {
                    self.current_match = 0;
                }
            }
            SessionDetailMsg::ShowExpandLoadFailure => {
                tracing::warn!("Could not load full message content");
                self.pending_toast.set(true);
            }
            SessionDetailMsg::ClearSearch => {
                self.search_query = None;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;

                if let Some(session) = &self.session {
                    let session_id = session.id.clone();
                    self.load_first_page(&session_id);
                }
            }
            SessionDetailMsg::Clear => {
                self.session = None;
                self.clear_messages_safely();
                self.loaded_count = 0;
                self.has_more_messages = false;
                self.search_query = None;
                self.match_counts.clear();
                self.current_match = 0;
                self.total_matches = 0;
            }
            SessionDetailMsg::InspectToolCall(id) => {
                sender.output(SessionDetailOutput::InspectToolCall(id)).ok();
            }
            SessionDetailMsg::InspectSubagent(id) => {
                sender.output(SessionDetailOutput::InspectSubagent(id)).ok();
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

            if let Some(usage) = &session.token_usage {
                widgets
                    .token_details_label
                    .set_label(&crate::ui::format::format_token_details(usage));
                widgets
                    .token_details_label
                    .set_tooltip_text(Some(&crate::ui::format::token_details_tooltip(usage)));
                if let Some(cache_text) = crate::ui::format::format_token_cache(usage) {
                    widgets.token_cache_label.set_label(&cache_text);
                    widgets
                        .token_cache_label
                        .set_tooltip_text(Some(crate::ui::format::token_cache_tooltip()));
                }
            } else {
                widgets.token_details_label.set_label("");
                widgets.token_details_label.set_tooltip_text(None);
                widgets.token_cache_label.set_label("");
                widgets.token_cache_label.set_tooltip_text(None);
            }

            widgets
                .content_stack
                .set_visible_child(&widgets.toast_overlay);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.loading_state);
        }

        if self.pending_toast.take() {
            widgets
                .toast_overlay
                .add_toast(adw::Toast::new("Could not load full message."));
        }

        if let Some(item_index) = self.scroll_to_item.take() {
            let messages_widget = self.messages.widget().clone();
            let scroll_child = widgets.scroll_child.clone();
            glib::idle_add_local_once(move || {
                let Some(target) = messages_widget
                    .observe_children()
                    .item(item_index as u32)
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
                let target_y = (point.y() as f64) - (vadj.page_size() / 3.0);
                vadj.set_value(target_y.max(0.0));
            });
        }
    }
}

impl SessionDetail {
    fn load_first_page(&mut self, session_id: &str) {
        match load_transcript_items(
            &self.db_path,
            session_id,
            self.page_size as i64,
            0,
            self.preview_len as i64,
        ) {
            Ok(rows) => {
                self.has_more_messages = rows.len() == self.page_size;
                self.loaded_count = rows.len();
                let highlight = self.search_query.clone();
                let db_path = self.db_path.clone();
                self.clear_messages_safely();
                let mut guard = self.messages.guard();
                for row in rows {
                    guard.push_back(transcript_item_init_from_row(
                        &row,
                        session_id,
                        highlight.clone(),
                        db_path.clone(),
                    ));
                }
            }
            Err(err) => {
                tracing::error!(
                    "Failed to load transcript items for {}: {}",
                    session_id,
                    err
                );
                self.clear_messages_safely();
                self.loaded_count = 0;
                self.has_more_messages = false;
            }
        }
    }

    /// Clear transcript rows after releasing focus from any currently-focused row widget.
    fn clear_messages_safely(&mut self) {
        self.release_focus_from_transcript_if_needed();
        self.messages.guard().clear();
    }

    /// Avoid GTK focus traversing a row subtree while it is being replaced.
    fn release_focus_from_transcript_if_needed(&self) {
        let messages_widget = self.messages.widget();
        let Some(window) = messages_widget
            .ancestor(gtk::Window::static_type())
            .and_then(|w| w.downcast::<gtk::Window>().ok())
        else {
            return;
        };

        let Some(focus_widget) = gtk::prelude::GtkWindowExt::focus(&window) else {
            return;
        };

        if focus_widget.is_ancestor(messages_widget) {
            tracing::debug!("Clearing window focus before replacing transcript rows");
            gtk::prelude::GtkWindowExt::set_focus(&window, Option::<&gtk::Widget>::None);
        }
    }

    /// Resolve a global match index to the transcript item_index of the matching row.
    /// Since item_index == factory position (items pushed in transcript order),
    /// this can be used directly as the scroll target.
    fn find_item_for_match(counts: &BTreeMap<usize, usize>, global_index: usize) -> usize {
        let mut remaining = global_index;
        for (&item_index, &count) in counts.iter() {
            if remaining < count {
                return item_index;
            }
            remaining -= count;
        }
        counts.keys().last().copied().unwrap_or(0)
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
