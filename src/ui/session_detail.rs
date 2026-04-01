use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use gtk::glib;
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};

use crate::database::load_transcript_items;
use crate::models::Session;
use crate::ui::activity_bar::SessionActivityBar;
use crate::ui::transcript_display::{
    group_transcript_rows, regroup_boundary, trailing_tool_call_rows,
    trailing_tool_rows_from_display,
};
use crate::ui::transcript_row::{
    TranscriptItemInit, TranscriptRow, TranscriptRowOutput, transcript_item_init_from_display_item,
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
    pending_boundary_tool_rows: Vec<crate::database::TranscriptItemRow>,
    has_pending_boundary_burst: bool,
    search_query: Option<String>,
    /// Keyed by transcript item_index (== factory position).
    match_segments: BTreeMap<usize, Vec<usize>>,
    current_match: usize,
    total_matches: usize,
    scroll_to_item: Cell<Option<ScrollTarget>>,
    pending_toast: Cell<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollTarget {
    display_index: usize,
    child_index: Option<usize>,
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
    MatchSegments(usize, Vec<usize>),
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
                            set_margin_all: 16,

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

                            #[name = "session_id_row"]
                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 6,

                                gtk::Label {
                                    set_label: "Session ID:",
                                    add_css_class: "dim-label",
                                },

                                #[name = "session_id_label"]
                                gtk::Label {
                                    add_css_class: "monospace",
                                    set_selectable: true,
                                    set_wrap: true,
                                    set_wrap_mode: gtk::pango::WrapMode::WordChar,
                                },
                            },

                            #[name = "chip_row"]
                            gtk::FlowBox {
                                set_selection_mode: gtk::SelectionMode::None,
                                set_row_spacing: 8,
                                set_column_spacing: 8,
                                set_max_children_per_line: 4,
                                set_min_children_per_line: 1,

                                append = &gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 6,
                                    add_css_class: "pill",

                                    #[name = "tool_icon"]
                                    gtk::Image {
                                        set_pixel_size: 16,
                                    },

                                    #[name = "tool_label"]
                                    gtk::Label {},
                                },

                                append = &gtk::Box {
                                    add_css_class: "pill",

                                    #[name = "duration_chip"]
                                    gtk::Label {},
                                },

                                append = &gtk::Box {
                                    add_css_class: "pill",

                                    #[name = "message_count_chip"]
                                    gtk::Label {},
                                },

                                append = &gtk::Box {
                                    add_css_class: "pill",

                                    #[name = "ending_status_chip"]
                                    gtk::Label {},
                                },
                            },

                            #[name = "first_prompt_separator"]
                            gtk::Separator {},

                            #[name = "first_prompt_section"]
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,

                                gtk::Label {
                                    set_label: "FIRST PROMPT",
                                    add_css_class: "section-heading",
                                    set_halign: gtk::Align::Start,
                                },

                                #[name = "first_prompt_label"]
                                gtk::Label {
                                    set_halign: gtk::Align::Start,
                                    set_xalign: 0.0,
                                    set_wrap: true,
                                    set_wrap_mode: gtk::pango::WrapMode::WordChar,
                                    set_lines: 3,
                                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                                    set_max_width_chars: 80,
                                },
                            },

                            #[name = "activity_separator"]
                            gtk::Separator {},

                            #[name = "activity_section"]
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,

                                gtk::Label {
                                    set_label: "ACTIVITY",
                                    add_css_class: "section-heading",
                                    set_halign: gtk::Align::Start,
                                },

                                #[name = "activity_bar"]
                                SessionActivityBar {
                                    add_css_class: "activity-bar",
                                },

                                #[name = "legend_row"]
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,

                                    #[name = "edit_legend"]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 4,

                                        gtk::Box {
                                            add_css_class: "activity-edits",
                                            set_size_request: (8, 8),
                                            set_valign: gtk::Align::Center,
                                        },

                                        #[name = "edit_count_label"]
                                        gtk::Label {
                                            add_css_class: "dim-label",
                                        },
                                    },

                                    #[name = "command_legend"]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 4,

                                        gtk::Box {
                                            add_css_class: "activity-commands",
                                            set_size_request: (8, 8),
                                            set_valign: gtk::Align::Center,
                                        },

                                        #[name = "command_count_label"]
                                        gtk::Label {
                                            add_css_class: "dim-label",
                                        },
                                    },

                                    #[name = "read_legend"]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 4,

                                        gtk::Box {
                                            add_css_class: "activity-reads",
                                            set_size_request: (8, 8),
                                            set_valign: gtk::Align::Center,
                                        },

                                        #[name = "read_count_label"]
                                        gtk::Label {
                                            add_css_class: "dim-label",
                                        },
                                    },
                                },

                                #[name = "conversation_only_label"]
                                gtk::Label {
                                    set_label: "Conversation only",
                                    add_css_class: "dim-label",
                                    set_halign: gtk::Align::Start,
                                },
                            },

                            #[name = "tokens_separator"]
                            gtk::Separator {},

                            #[name = "tokens_section"]
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,

                                gtk::Label {
                                    set_label: "TOKENS",
                                    add_css_class: "section-heading",
                                    set_halign: gtk::Align::Start,
                                },

                                #[name = "tokens_grid"]
                                gtk::FlowBox {
                                    set_selection_mode: gtk::SelectionMode::None,
                                    set_row_spacing: 8,
                                    set_column_spacing: 16,
                                    set_homogeneous: true,
                                    set_max_children_per_line: 4,
                                    set_min_children_per_line: 2,

                                    #[name = "input_pair"]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,

                                        #[name = "input_value_label"]
                                        gtk::Label {
                                            add_css_class: "token-value",
                                            set_halign: gtk::Align::Start,
                                        },

                                        gtk::Label {
                                            set_label: "Input",
                                            add_css_class: "dim-label",
                                            set_halign: gtk::Align::Start,
                                        },
                                    },

                                    #[name = "output_pair"]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,

                                        #[name = "output_value_label"]
                                        gtk::Label {
                                            add_css_class: "token-value",
                                            set_halign: gtk::Align::Start,
                                        },

                                        gtk::Label {
                                            set_label: "Output",
                                            add_css_class: "dim-label",
                                            set_halign: gtk::Align::Start,
                                        },
                                    },

                                    #[name = "cache_pair"]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,

                                        #[name = "cache_value_label"]
                                        gtk::Label {
                                            add_css_class: "token-value",
                                            set_halign: gtk::Align::Start,
                                        },

                                        gtk::Label {
                                            set_label: "Cache",
                                            add_css_class: "dim-label",
                                            set_halign: gtk::Align::Start,
                                        },
                                    },

                                    #[name = "reasoning_pair"]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,

                                        #[name = "reasoning_value_label"]
                                        gtk::Label {
                                            add_css_class: "token-value",
                                            set_halign: gtk::Align::Start,
                                        },

                                        gtk::Label {
                                            set_label: "Reasoning",
                                            add_css_class: "dim-label",
                                            set_halign: gtk::Align::Start,
                                        },
                                    },
                                },
                            },

                            #[name = "transcript_separator"]
                            gtk::Separator {},

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
                TranscriptRowOutput::MatchSegmentsChanged {
                    item_index,
                    segments,
                } => SessionDetailMsg::MatchSegments(item_index, segments),
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
            pending_boundary_tool_rows: Vec::new(),
            has_pending_boundary_burst: false,
            search_query: None,
            match_segments: BTreeMap::new(),
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
                self.reset_search_matches();
                let session = *session;
                let session_id = session.id.clone();
                self.session = Some(session);
                self.load_first_page(&session_id);
            }
            SessionDetailMsg::UpdateSearchQuery(query) => {
                self.search_query = query;
                self.reset_search_matches();
                self.reload_current_session();
            }
            SessionDetailMsg::LoadMore => {
                self.load_next_page();
            }
            SessionDetailMsg::PrevMatch => {
                if self.total_matches > 0 {
                    self.current_match = match self.current_match {
                        0 => self.total_matches - 1,
                        n => n - 1,
                    };
                    self.scroll_to_current_match();
                }
            }
            SessionDetailMsg::NextMatch => {
                if self.total_matches > 0 {
                    self.current_match = (self.current_match + 1) % self.total_matches;
                    self.scroll_to_current_match();
                }
            }
            SessionDetailMsg::MatchSegments(item_index, segments) => {
                self.update_match_segments(item_index, segments);
            }
            SessionDetailMsg::ShowExpandLoadFailure => {
                tracing::warn!("Could not load full message content");
                self.pending_toast.set(true);
            }
            SessionDetailMsg::ClearSearch => {
                self.search_query = None;
                self.reset_search_matches();
                self.reload_current_session();
            }
            SessionDetailMsg::Clear => {
                self.session = None;
                self.clear_messages_safely();
                self.loaded_count = 0;
                self.has_more_messages = false;
                self.search_query = None;
                self.reset_search_matches();
                self.pending_boundary_tool_rows.clear();
                self.has_pending_boundary_burst = false;
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

            widgets.session_id_label.set_label(&session.id);

            // Populate chip row
            widgets
                .duration_chip
                .set_label(&crate::ui::format::format_session_duration(
                    session.start_time,
                    session.last_updated,
                ));
            widgets
                .message_count_chip
                .set_label(&crate::ui::format::format_count(
                    session.message_count,
                    "message",
                    "messages",
                ));
            widgets
                .ending_status_chip
                .set_label(crate::ui::format::format_ending_label(
                    &session.ending_status,
                ));
            widgets.ending_status_chip.set_css_classes(&[
                "pill",
                crate::ui::format::ending_css_class(&session.ending_status),
            ]);
            widgets
                .ending_status_chip
                .update_property(&[gtk::accessible::Property::Label(
                    crate::ui::format::format_ending_accessible_label(&session.ending_status),
                )]);

            // First prompt section visibility
            let has_first_prompt = session
                .first_prompt
                .as_ref()
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false);
            widgets.first_prompt_section.set_visible(has_first_prompt);
            widgets.first_prompt_separator.set_visible(has_first_prompt);
            if has_first_prompt {
                widgets
                    .first_prompt_label
                    .set_label(session.first_prompt.as_ref().unwrap());
            }

            // Activity section
            let has_activity =
                session.edit_count > 0 || session.command_count > 0 || session.read_count > 0;
            widgets.activity_section.set_visible(true);
            widgets.activity_bar.set_visible(has_activity);
            widgets.legend_row.set_visible(has_activity);
            widgets.conversation_only_label.set_visible(!has_activity);

            if has_activity {
                widgets.activity_bar.set_counts(
                    session.edit_count,
                    session.command_count,
                    session.read_count,
                );

                widgets
                    .edit_count_label
                    .set_label(&crate::ui::format::format_count(
                        session.edit_count,
                        "edit",
                        "edits",
                    ));
                widgets
                    .command_count_label
                    .set_label(&crate::ui::format::format_count(
                        session.command_count,
                        "command",
                        "commands",
                    ));
                widgets
                    .read_count_label
                    .set_label(&crate::ui::format::format_count(
                        session.read_count,
                        "read",
                        "reads",
                    ));

                widgets.edit_legend.set_visible(session.edit_count > 0);
                widgets
                    .command_legend
                    .set_visible(session.command_count > 0);
                widgets.read_legend.set_visible(session.read_count > 0);
            } else {
                widgets.activity_bar.set_counts(0, 0, 0);
            }

            // Tokens section
            let has_tokens = session.token_usage.is_some();
            widgets.tokens_section.set_visible(has_tokens);
            widgets.tokens_separator.set_visible(has_tokens);

            if let Some(usage) = &session.token_usage {
                widgets
                    .input_value_label
                    .set_label(&crate::ui::format::format_token_count(usage.input_tokens));
                widgets
                    .output_value_label
                    .set_label(&crate::ui::format::format_token_count(usage.output_tokens));

                let has_cache =
                    usage.cache_read_tokens.is_some() || usage.cache_write_tokens.is_some();
                widgets.cache_pair.set_visible(has_cache);
                if has_cache && let Some(cache_text) = crate::ui::format::format_token_cache(usage)
                {
                    widgets.cache_value_label.set_label(&cache_text);
                }

                let has_reasoning = usage.reasoning_tokens.is_some();
                widgets.reasoning_pair.set_visible(has_reasoning);
                if let Some(reasoning) = usage.reasoning_tokens {
                    widgets
                        .reasoning_value_label
                        .set_label(&crate::ui::format::format_token_count(reasoning));
                }

                widgets
                    .tokens_section
                    .set_tooltip_text(Some(crate::ui::format::token_semantics_help_tooltip()));

                widgets
                    .input_pair
                    .update_property(&[gtk::accessible::Property::Label(&format!(
                        "Input tokens: {}",
                        crate::ui::format::format_token_count(usage.input_tokens)
                    ))]);
                widgets
                    .output_pair
                    .update_property(&[gtk::accessible::Property::Label(&format!(
                        "Output tokens: {}",
                        crate::ui::format::format_token_count(usage.output_tokens)
                    ))]);
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

        if let Some(target) = self.scroll_to_item.take() {
            let messages_widget = self.messages.widget().clone();
            let scroll_child = widgets.scroll_child.clone();
            glib::idle_add_local_once(move || {
                let Some(row_widget) = messages_widget
                    .observe_children()
                    .item(target.display_index as u32)
                    .and_then(|obj| obj.downcast::<gtk::Widget>().ok())
                else {
                    return;
                };

                if let Some(child_index) = target.child_index
                    && let Some(expander) = row_widget
                        .first_child()
                        .and_then(|w| w.downcast::<gtk::Expander>().ok())
                {
                    expander.set_expanded(true);
                    let scroll_child_for_tick = scroll_child.clone();
                    let expander_for_tick = expander.clone();
                    let tick_count = std::cell::Cell::new(0u32);
                    expander.add_tick_callback(move |_, _| {
                        let ticks = tick_count.get() + 1;
                        tick_count.set(ticks);
                        if ticks > 60 {
                            return glib::ControlFlow::Break;
                        }

                        let Some(child_box) = expander_for_tick
                            .child()
                            .and_then(|w| w.downcast::<gtk::Box>().ok())
                        else {
                            return glib::ControlFlow::Break;
                        };

                        let Some(child_widget) = child_box
                            .observe_children()
                            .item(child_index as u32)
                            .and_then(|obj| obj.downcast::<gtk::Widget>().ok())
                        else {
                            return glib::ControlFlow::Continue;
                        };

                        Self::scroll_widget_into_view(&child_widget, &scroll_child_for_tick);
                        glib::ControlFlow::Break
                    });
                    return;
                }

                Self::scroll_widget_into_view(&row_widget, &scroll_child);
            });
        }
    }
}

impl SessionDetail {
    fn build_display_items(
        rows: Vec<crate::database::TranscriptItemRow>,
        session_id: &str,
        highlight_query: Option<String>,
        db_path: Arc<PathBuf>,
        base_display_index: usize,
    ) -> Vec<TranscriptItemInit> {
        group_transcript_rows(rows)
            .into_iter()
            .enumerate()
            .map(|(offset, item)| {
                transcript_item_init_from_display_item(
                    base_display_index + offset,
                    &item,
                    session_id,
                    highlight_query.clone(),
                    db_path.clone(),
                )
            })
            .collect()
    }

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
                self.pending_boundary_tool_rows = if self.has_more_messages {
                    trailing_tool_call_rows(&rows)
                } else {
                    Vec::new()
                };
                self.has_pending_boundary_burst = !self.pending_boundary_tool_rows.is_empty();
                self.clear_messages_safely();
                let mut guard = self.messages.guard();
                for item in Self::build_display_items(rows, session_id, highlight, db_path, 0) {
                    guard.push_back(item);
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
                self.pending_boundary_tool_rows.clear();
                self.has_pending_boundary_burst = false;
            }
        }
    }

    fn reset_search_matches(&mut self) {
        self.match_segments.clear();
        self.current_match = 0;
        self.total_matches = 0;
    }

    fn reload_current_session(&mut self) {
        if let Some(session) = &self.session {
            let session_id = session.id.clone();
            self.load_first_page(&session_id);
        }
    }

    fn scroll_to_current_match(&self) {
        let target = Self::find_match_target(&self.match_segments, self.current_match);
        self.scroll_to_item.set(Some(target));
    }

    fn load_next_page(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let session_id = session.id.clone();
        let offset = self.loaded_count;
        let rows = match load_transcript_items(
            &self.db_path,
            &session_id,
            self.page_size as i64,
            offset as i64,
            self.preview_len as i64,
        ) {
            Ok(rows) => rows,
            Err(err) => {
                tracing::error!("Failed to load more transcript items: {}", err);
                self.has_more_messages = false;
                return;
            }
        };

        let source_len = rows.len();
        self.has_more_messages = source_len == self.page_size;
        self.loaded_count += source_len;

        let mut rows = rows;
        let highlight = self.search_query.clone();
        let db_path = self.db_path.clone();

        // Boundary regrouping must run before borrowing `self.messages` since it
        // mutates `self.pending_boundary_tool_rows`.
        let boundary_replacements = if !self.pending_boundary_tool_rows.is_empty() {
            let regrouped = regroup_boundary(self.pending_boundary_tool_rows.clone(), rows);
            let trailing_from_merge = trailing_tool_rows_from_display(&regrouped.replacement_items);
            let pop_count = usize::from(self.has_pending_boundary_burst);

            rows = regrouped.remaining_rows;
            self.pending_boundary_tool_rows = if rows.is_empty() {
                trailing_from_merge
            } else {
                trailing_tool_call_rows(&rows)
            };
            self.has_pending_boundary_burst = !self.pending_boundary_tool_rows.is_empty();

            Some((regrouped.replacement_items, pop_count))
        } else {
            None
        };

        if self.pending_boundary_tool_rows.is_empty() && self.has_more_messages {
            self.pending_boundary_tool_rows = trailing_tool_call_rows(&rows);
            self.has_pending_boundary_burst = !self.pending_boundary_tool_rows.is_empty();
        }

        let mut guard = self.messages.guard();

        if let Some((replacement_items, pop_count)) = boundary_replacements
            && !replacement_items.is_empty()
        {
            for _ in 0..pop_count {
                let _ = guard.pop_back();
            }
            let start_index = guard.len();
            for item in replacement_items
                .into_iter()
                .enumerate()
                .map(|(offset, item)| {
                    transcript_item_init_from_display_item(
                        start_index + offset,
                        &item,
                        &session_id,
                        highlight.clone(),
                        db_path.clone(),
                    )
                })
            {
                guard.push_back(item);
            }
        }

        let start_index = guard.len();
        for item in Self::build_display_items(rows, &session_id, highlight, db_path, start_index) {
            guard.push_back(item);
        }

        if !self.has_more_messages {
            self.pending_boundary_tool_rows.clear();
            self.has_pending_boundary_burst = false;
        }
    }

    fn update_match_segments(&mut self, item_index: usize, segments: Vec<usize>) {
        let was_empty = self.total_matches == 0;
        self.match_segments.insert(item_index, segments);
        self.total_matches = self
            .match_segments
            .values()
            .map(|parts| parts.iter().sum::<usize>())
            .sum();
        if was_empty && self.total_matches > 0 && self.search_query.is_some() {
            self.current_match = 0;
            let target = Self::find_match_target(&self.match_segments, 0);
            self.scroll_to_item.set(Some(target));
        }
        self.current_match = match self.total_matches {
            0 => 0,
            n if self.current_match >= n => n - 1,
            _ => self.current_match,
        };
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

    fn scroll_widget_into_view(widget: &gtk::Widget, scroll_child: &gtk::Box) {
        let Some(point) = widget.compute_point(scroll_child, &gtk::graphene::Point::zero()) else {
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
    }

    fn find_match_target(
        segments_by_display_index: &BTreeMap<usize, Vec<usize>>,
        global_index: usize,
    ) -> ScrollTarget {
        let mut remaining = global_index;

        for (&display_index, segments) in segments_by_display_index {
            for (child_index, count) in segments.iter().copied().enumerate() {
                if remaining < count {
                    return ScrollTarget {
                        display_index,
                        child_index: (segments.len() > 1).then_some(child_index),
                    };
                }
                remaining = remaining.saturating_sub(count);
            }
        }

        ScrollTarget {
            display_index: segments_by_display_index
                .keys()
                .last()
                .copied()
                .unwrap_or(0),
            child_index: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relm4::{Component, ComponentController};

    fn build_test_session(
        first_prompt: Option<&str>,
        token_usage: Option<crate::models::TokenUsage>,
        edit_count: usize,
        command_count: usize,
        read_count: usize,
    ) -> Session {
        use chrono::{TimeZone, Utc};

        Session {
            id: "test-session-123".to_string(),
            tool: crate::models::AiAssistant::ClaudeCode,
            project_path: Some("/tmp/project".to_string()),
            project_id: None,
            start_time: Utc.with_ymd_and_hms(2026, 3, 30, 10, 0, 0).unwrap(),
            message_count: 42,
            file_path: "/tmp/test.json".to_string(),
            last_updated: Utc.with_ymd_and_hms(2026, 3, 30, 12, 14, 0).unwrap(),
            first_prompt: first_prompt.map(|s| s.to_string()),
            parent_session_id: None,
            is_subagent: false,
            token_usage,
            edit_count,
            read_count,
            command_count,
            ending_status: crate::models::SessionEndingStatus::Clean,
        }
    }

    #[test]
    fn build_display_items_groups_two_tool_calls_into_one_tool_burst() {
        let rows = vec![
            crate::database::TranscriptItemRow {
                item_index: 0,
                kind: crate::models::TranscriptItemKind::Message,
                message_index: Some(0),
                role: Some(crate::models::Role::Assistant),
                content_preview: Some("hello".to_string()),
                content_len: Some(5),
                timestamp: Some(0),
                model: None,
                tool_call_id: None,
                tool_name: None,
                tool_status: None,
                tool_summary: None,
                tool_input_json: None,
                tool_output_text: None,
                duration_ms: None,
                subagent_id: None,
                subagent_title: None,
                subagent_prompt: None,
            },
            crate::database::TranscriptItemRow {
                item_index: 1,
                kind: crate::models::TranscriptItemKind::ToolCall,
                message_index: None,
                role: None,
                content_preview: None,
                content_len: None,
                timestamp: None,
                model: None,
                tool_call_id: Some("call-1".to_string()),
                tool_name: Some("Read".to_string()),
                tool_status: Some(crate::models::ToolCallStatus::Completed),
                tool_summary: Some("read a file".to_string()),
                tool_input_json: Some("{}".to_string()),
                tool_output_text: None,
                duration_ms: Some(5),
                subagent_id: None,
                subagent_title: None,
                subagent_prompt: None,
            },
            crate::database::TranscriptItemRow {
                item_index: 2,
                kind: crate::models::TranscriptItemKind::ToolCall,
                message_index: None,
                role: None,
                content_preview: None,
                content_len: None,
                timestamp: None,
                model: None,
                tool_call_id: Some("call-2".to_string()),
                tool_name: Some("Edit".to_string()),
                tool_status: Some(crate::models::ToolCallStatus::Completed),
                tool_summary: Some("edit a file".to_string()),
                tool_input_json: Some("{}".to_string()),
                tool_output_text: None,
                duration_ms: Some(7),
                subagent_id: None,
                subagent_title: None,
                subagent_prompt: None,
            },
        ];

        let items = SessionDetail::build_display_items(
            rows,
            "session-1",
            None,
            Arc::new(PathBuf::from("/tmp/test.db")),
            0,
        );

        assert_eq!(items.len(), 2);
        assert!(matches!(items[1], TranscriptItemInit::ToolBurst(_)));
    }

    #[test]
    fn find_match_target_returns_child_index_for_burst_matches() {
        let mut segments = BTreeMap::new();
        segments.insert(0, vec![2]);
        segments.insert(1, vec![0, 3, 1]);

        assert_eq!(
            SessionDetail::find_match_target(&segments, 3),
            ScrollTarget {
                display_index: 1,
                child_index: Some(1),
            }
        );
        assert_eq!(
            SessionDetail::find_match_target(&segments, 5),
            ScrollTarget {
                display_index: 1,
                child_index: Some(2),
            }
        );
    }

    #[gtk::test]
    fn session_detail_header_hides_optional_sections_when_data_is_missing() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(None, None, 0, 0, 0)),
            search_query: None,
        });

        while gtk::glib::MainContext::default().iteration(false) {}

        let parts = controller.state().get();
        assert!(!parts.widgets.first_prompt_section.is_visible());
        assert!(!parts.widgets.tokens_section.is_visible());
        assert!(parts.widgets.activity_section.is_visible());
    }

    #[gtk::test]
    fn session_detail_header_populates_identity_prompt_and_tokens() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(
                Some("Ship the summary header"),
                Some(crate::models::TokenUsage {
                    input_tokens: 1200,
                    output_tokens: 300,
                    cache_read_tokens: Some(400),
                    cache_write_tokens: None,
                    reasoning_tokens: Some(50),
                }),
                4,
                2,
                1,
            )),
            search_query: None,
        });

        while gtk::glib::MainContext::default().iteration(false) {}

        let parts = controller.state().get();
        assert_eq!(parts.widgets.project_label.label(), "project");
        assert_eq!(parts.widgets.duration_chip.label(), "2h 14m");
        assert_eq!(parts.widgets.message_count_chip.label(), "42 messages");
        assert_eq!(parts.widgets.ending_status_chip.label(), "Ended cleanly");
        assert!(parts.widgets.first_prompt_section.is_visible());
        assert!(parts.widgets.tokens_section.is_visible());
    }

    #[gtk::test]
    fn session_detail_header_uses_conversation_only_when_activity_counts_are_zero() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(Some("Only chat"), None, 0, 0, 0)),
            search_query: None,
        });

        while gtk::glib::MainContext::default().iteration(false) {}

        let parts = controller.state().get();
        assert!(parts.widgets.conversation_only_label.is_visible());
        assert!(!parts.widgets.activity_bar.is_visible());
    }

    #[gtk::test]
    fn session_detail_activity_bar_does_not_lock_a_minimum_width_request() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(Some("Ship it"), None, 4, 2, 1)),
            search_query: None,
        });

        while gtk::glib::MainContext::default().iteration(false) {}

        let parts = controller.state().get();
        assert_eq!(parts.widgets.activity_bar.width_request(), -1);
    }

    fn build_error_session_for_css_test() -> Session {
        let mut session = build_test_session(None, None, 0, 0, 0);
        session.ending_status = crate::models::SessionEndingStatus::Error;
        session
    }

    #[gtk::test]
    fn session_detail_header_applies_status_and_activity_css_classes() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_error_session_for_css_test()),
            search_query: None,
        });

        while gtk::glib::MainContext::default().iteration(false) {}

        let parts = controller.state().get();
        assert!(parts.widgets.ending_status_chip.has_css_class("pill"));
        assert!(
            parts
                .widgets
                .ending_status_chip
                .has_css_class("ending-failed")
        );
        assert!(parts.widgets.activity_bar.has_css_class("activity-bar"));
    }
}
