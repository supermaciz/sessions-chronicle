use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adw::prelude::BreakpointBinExt;
use gtk::glib;
use gtk::prelude::*;
use relm4::binding::Binding;
use relm4::typed_view::list::TypedListView;
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, RelmWidgetExt,
    adw, gtk,
};

use crate::database::{
    MatchPosition, find_session_match_positions, load_all_transcript_items,
    load_message_full_content,
};
use crate::models::Session;
use crate::ui::activity_bar::SessionActivityBar;
use crate::ui::tool_inspector_pane::{
    ToolInspectorPane, ToolInspectorPaneMsg, ToolInspectorPaneOutput,
};
use crate::ui::transcript_display::{DisplayTranscriptItem, group_transcript_rows};
use crate::ui::transcript_item_data::TranscriptItemData;
use crate::ui::transcript_row::{
    TranscriptItemInit, TranscriptRowBuildKind, transcript_item_init_from_display_item,
};
use crate::ui::typed_transcript_row::TRANSCRIPT_ROW_WIDGET_NAME_PREFIX;

const PREVIEW_LEN: usize = 2000;
const DEFERRED_FIRST_PAGE_LOAD_DELAY_MS: u64 = 250;
const DEFERRED_CLEAR_DELAY_MS: u64 = 250;

/// Detail view for a single indexed session.
///
/// This component owns the session summary header, transcript
/// rendering, the inspector pane (right-hand split sidebar), and transcript
/// search navigation.
pub struct SessionDetail {
    db_path: Arc<PathBuf>,
    session: Option<Session>,
    messages: TypedListView<TranscriptItemData, gtk::NoSelection>,
    transcript_render_widget: gtk::Widget,
    preview_len: usize,
    transcript: TranscriptState,
    search: SearchState,
    pending_toast: Cell<bool>,
    inspector: Controller<ToolInspectorPane>,
    inspector_open: bool,
}

/// Transcript loading and paging state.
///
/// `request_id` is bumped to invalidate in-flight page loads; `loading` and
/// `loaded_count` track paging progress, and `display_targets_by_item_index`
/// maps transcript item indexes to their resolved [`ScrollTarget`] as pages
/// render (consumed by search-jump navigation).
#[derive(Default)]
struct TranscriptState {
    load_started_at: Option<Instant>,
    loaded_count: usize,
    loading: bool,
    request_id: u64,
    display_targets_by_item_index: BTreeMap<i64, ScrollTarget>,
}

/// Transcript search and match-navigation state.
///
/// These fields move together: `request_id` is bumped to invalidate in-flight
/// loads, and `match_positions`/`current_match`/`pending_jump`/`loading_jump`
/// are reset as a unit whenever the query changes (see
/// [`SessionDetail::reset_search_matches`] and `invalidate_search_requests`).
#[derive(Default)]
struct SearchState {
    query: Option<String>,
    match_positions: Vec<MatchPosition>,
    current_match: usize,
    pending_jump: Option<usize>,
    loading_jump: bool,
    request_id: u64,
    scroll_to_item: Cell<Option<ScrollTarget>>,
}

/// Resolved scroll destination for global search navigation.
///
/// `display_index` addresses a top-level transcript row in the factory. When
/// `child_index` is present, the target is a child entry inside an expanded
/// burst row rather than the row container itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScrollTarget {
    display_index: usize,
    child_index: Option<usize>,
}

struct PreparedTranscriptItems {
    items: Vec<TranscriptItemInit>,
    display_targets_by_item_index: BTreeMap<i64, ScrollTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClearMessagesMetrics {
    row_count_before: usize,
    duration_ms: u128,
}

/// Parent-facing actions emitted by [`SessionDetail`].
#[derive(Debug)]
pub enum SessionDetailOutput {
    /// Inspector visibility changed (user toggled, gesture, or programmatic).
    InspectorVisibilityChanged(bool),
    /// User asked to open a child session linked from a subagent inside the inspector.
    OpenChildSession(String),
}

/// Input messages accepted by [`SessionDetail`].
///
/// This enum mixes app-level commands sent by parent components with
/// child-row events and internal UI messages that are funneled back into the
/// detail view's `update` loop.
#[derive(Debug)]
pub enum SessionDetailMsg {
    /// Replaces the active session and reloads transcript content, optionally
    /// applying an active transcript search query.
    SetSession {
        session: Box<Session>,
        search_query: Option<String>,
    },
    /// Updates the active transcript search query and reloads the current
    /// session so match highlighting stays in sync with displayed rows.
    UpdateSearchQuery(Option<String>),
    SetMatchPositions {
        request_id: u64,
        session_id: String,
        positions: Vec<MatchPosition>,
    },
    PrevMatch,
    NextMatch,
    ClearSearch,
    StartDeferredFirstPageLoad {
        request_id: u64,
        session_id: String,
    },
    /// Stop active transcript work before the detail page is popped. The heavy
    /// widget teardown is deferred so it does not compete with the navigation
    /// animation.
    PrepareForNavigationBack,
    DeferredClear {
        request_id: u64,
    },
    /// Indicates that a transcript row failed to expand to its full content and
    /// should trigger the shared toast notification path.
    ShowExpandLoadFailure,
    ToggleMessageExpand {
        item_index: usize,
    },
    RowBuilt {
        item_index: usize,
        kind: TranscriptRowBuildKind,
        build_duration_ms: u128,
    },
    Clear,
    InspectToolCall(String),
    InspectSubagent(String),
    InspectReasoning(i64),
    /// Toggle the internal inspector pane visibility.
    ToggleInspector,
    /// Force the internal inspector pane to be hidden (e.g. Escape key from App).
    CloseInspector,
    /// Sync inspector state with widget gesture/collapse changes.
    InspectorWidgetVisibilityChanged(bool),
    /// Open the child session linked from the inspector (forwarded to App).
    OpenChildSession(String),
}

pub enum SessionDetailCmd {
    TranscriptLoaded {
        request_id: u64,
        session_id: String,
        load_duration_ms: u128,
        result: Result<Vec<crate::database::TranscriptItemRow>, String>,
    },
    SearchPositionsLoaded {
        request_id: u64,
        session_id: String,
        load_duration_ms: u128,
        result: Result<Vec<MatchPosition>, String>,
    },
    MessageFullContentReady {
        item_index: usize,
        session_id: String,
        message_index: usize,
        result: Result<String, String>,
    },
}

impl std::fmt::Debug for SessionDetailCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TranscriptLoaded {
                request_id,
                session_id,
                load_duration_ms,
                result,
            } => {
                let result_summary = match result {
                    Ok(rows) => format!("Ok({} rows)", rows.len()),
                    Err(err) => format!("Err({err})"),
                };

                f.debug_struct("TranscriptLoaded")
                    .field("request_id", request_id)
                    .field("session_id", session_id)
                    .field("load_duration_ms", load_duration_ms)
                    .field("result", &result_summary)
                    .finish()
            }
            Self::SearchPositionsLoaded {
                request_id,
                session_id,
                load_duration_ms,
                result,
            } => {
                let result_summary = match result {
                    Ok(positions) => format!("Ok({} positions)", positions.len()),
                    Err(err) => format!("Err({err})"),
                };
                f.debug_struct("SearchPositionsLoaded")
                    .field("request_id", request_id)
                    .field("session_id", session_id)
                    .field("load_duration_ms", load_duration_ms)
                    .field("result", &result_summary)
                    .finish()
            }
            Self::MessageFullContentReady {
                item_index,
                session_id,
                message_index,
                result,
            } => {
                let result_summary = match result {
                    Ok(content) => format!("Ok({} chars)", content.len()),
                    Err(err) => format!("Err({err})"),
                };
                f.debug_struct("MessageFullContentReady")
                    .field("item_index", item_index)
                    .field("session_id", session_id)
                    .field("message_index", message_index)
                    .field("result", &result_summary)
                    .finish()
            }
        }
    }
}

#[relm4::component(pub)]
impl Component for SessionDetail {
    type Init = PathBuf;
    type Input = SessionDetailMsg;
    type Output = SessionDetailOutput;
    type CommandOutput = SessionDetailCmd;
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
                    #[name = "inspector_breakpoint_bin"]
                    set_child = &adw::BreakpointBin {
                        set_vexpand: true,
                        // BreakpointBin only collapses the split once it can be
                        // allocated narrower than the split's natural width, so
                        // it needs a small minimum size of its own.
                        set_width_request: 360,

                    #[wrap(Some)]
                    #[name = "inspector_split"]
                    set_child = &adw::OverlaySplitView {
                        set_vexpand: true,
                        set_sidebar_position: gtk::PackType::End,
                        set_min_sidebar_width: 360.0,
                        set_max_sidebar_width: 720.0,
                        set_sidebar_width_fraction: 0.32,
                        set_enable_show_gesture: true,
                        set_enable_hide_gesture: true,
                        #[watch]
                        set_show_sidebar: model.inspector_open,

                    #[wrap(Some)]
                    set_content = &gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_vexpand: true,

                        #[name = "summary_box"]
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
                        },

                        #[name = "transcript_scroller"]
                        gtk::ScrolledWindow {
                            set_vexpand: true,
                            set_hscrollbar_policy: gtk::PolicyType::Never,

                            #[local_ref]
                            messages_box -> gtk::ListView {
                                add_css_class: "transcript-list",
                                set_margin_start: 16,
                                set_margin_end: 16,
                                set_margin_top: 12,
                                set_margin_bottom: 16,
                            },
                        },
                    },
                    }, // close inspector_split (adw::OverlaySplitView)
                    }, // close inspector_breakpoint_bin (adw::BreakpointBin)

                    // Floating search navigation bar
                    add_overlay = &gtk::Box {
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Start,
                        add_css_class: "search-nav-bar",
                        set_spacing: 8,
                        #[watch]
                        set_visible: model.search.query.is_some(),

                        #[name = "search_jump_spinner"]
                        gtk::Spinner {
                            add_css_class: "search-jump-spinner",
                            #[watch]
                            set_visible: model.search.loading_jump,
                            #[watch]
                            set_spinning: model.search.loading_jump,
                        },

                        #[name = "search_term_label"]
                        gtk::Label {
                            add_css_class: "dim-label",
                            #[watch]
                            set_label: &model.search.query.as_deref()
                                .map(|q| format!("\"{}\"", q))
                                .unwrap_or_default(),
                        },

                        #[name = "previous_match_button"]
                        gtk::Button {
                            set_icon_name: "go-up-symbolic",
                            set_tooltip_text: Some("Previous match"),
                            add_css_class: "flat",
                            #[watch]
                            set_sensitive: !model.search.loading_jump && !model.search.match_positions.is_empty(),
                            connect_clicked => SessionDetailMsg::PrevMatch,
                        },

                        #[name = "match_counter_label"]
                        gtk::Label {
                            add_css_class: "match-counter",
                            set_halign: gtk::Align::Center,
                            #[watch]
                            set_label: &if !model.search.match_positions.is_empty() {
                                format!("{} / {}", model.search.current_match + 1, model.search.match_positions.len())
                            } else {
                                "0 results".to_string()
                            },
                        },

                        #[name = "loaded_match_counter_label"]
                        gtk::Label {
                            add_css_class: "dim-label",
                            add_css_class: "loaded-match-counter",
                            #[watch]
                            set_visible: model.search.loading_jump
                                && model.loaded_match_count() < model.search.match_positions.len(),
                            #[watch]
                            set_label: &format!(
                                "({}/{} loaded)",
                                model.loaded_match_count(),
                                model.search.match_positions.len()
                            ),
                        },

                        #[name = "next_match_button"]
                        gtk::Button {
                            set_icon_name: "go-down-symbolic",
                            set_tooltip_text: Some("Next match"),
                            add_css_class: "flat",
                            #[watch]
                            set_sensitive: !model.search.loading_jump && !model.search.match_positions.is_empty(),
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
        let messages: TypedListView<TranscriptItemData, gtk::NoSelection> = TypedListView::new();
        let transcript_render_widget = messages.view.clone().upcast::<gtk::Widget>();
        let db_path = Arc::new(db_path);
        let inspector = ToolInspectorPane::builder()
            .launch(db_path.clone())
            .forward(sender.input_sender(), |output| match output {
                ToolInspectorPaneOutput::OpenChildSession(id) => {
                    SessionDetailMsg::OpenChildSession(id)
                }
            });

        let model = Self {
            db_path,
            session: None,
            messages,
            transcript_render_widget,
            preview_len: PREVIEW_LEN,
            transcript: TranscriptState::default(),
            search: SearchState::default(),
            pending_toast: Cell::new(false),
            inspector,
            inspector_open: false,
        };

        let messages_box = model.messages.view.clone();
        let widgets = view_output!();

        widgets
            .content_stack
            .set_visible_child(&widgets.loading_state);

        // Mount the ToolInspectorPane widget as the inner OverlaySplitView's sidebar
        // and wire its visibility back into the model so user gestures stay in sync.
        widgets
            .inspector_split
            .set_sidebar(Some(model.inspector.widget()));
        let visibility_sender = sender.input_sender().clone();
        widgets
            .inspector_split
            .connect_show_sidebar_notify(move |split| {
                visibility_sender
                    .send(SessionDetailMsg::InspectorWidgetVisibilityChanged(
                        split.shows_sidebar(),
                    ))
                    .ok();
            });

        // Collapse the inspector into an overlay when the detail area is too
        // narrow to host the 360px sidebar beside a readable transcript.
        // Without this the split keeps the sidebar inline and forces the
        // window past its minimum width, which in turn disrupts the header bar.
        //
        // The threshold sits well above the inline layout's natural minimum
        // (360px sidebar + a usable transcript): if it were close to that
        // minimum, there would be a band of widths where the split is still
        // inline but allocated below its minimum, rendering the pane wrong.
        let inspector_breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            860.0,
            adw::LengthUnit::Sp,
        ));
        inspector_breakpoint.add_setter(&widgets.inspector_split, "collapsed", Some(&true.into()));
        widgets
            .inspector_breakpoint_bin
            .add_breakpoint(inspector_breakpoint);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            SessionDetailMsg::SetSession {
                session,
                search_query,
            } => {
                self.set_session(*session, search_query, &sender);
            }
            SessionDetailMsg::UpdateSearchQuery(query) => {
                self.update_search_query(query, &sender);
            }
            SessionDetailMsg::SetMatchPositions {
                request_id,
                session_id,
                positions,
            } => {
                self.set_match_positions(request_id, session_id, positions, &sender);
            }
            SessionDetailMsg::PrevMatch => {
                self.jump_to_previous_match(&sender);
            }
            SessionDetailMsg::NextMatch => {
                self.jump_to_next_match(&sender);
            }
            SessionDetailMsg::StartDeferredFirstPageLoad {
                request_id,
                session_id,
            } => {
                self.handle_start_deferred_first_page_load(request_id, session_id, &sender);
            }
            SessionDetailMsg::PrepareForNavigationBack => {
                self.prepare_for_navigation_back(&sender);
            }
            SessionDetailMsg::DeferredClear { request_id } => {
                self.handle_deferred_clear(request_id);
            }
            SessionDetailMsg::ShowExpandLoadFailure => {
                self.show_expand_load_failure();
            }
            SessionDetailMsg::ToggleMessageExpand { item_index } => {
                self.toggle_message_expand(item_index, &sender);
            }
            SessionDetailMsg::RowBuilt {
                item_index: _,
                kind: _,
                build_duration_ms: _,
            } => {
                tracing::trace!("Transcript row built (no-op in typed path)");
            }
            SessionDetailMsg::ClearSearch => {
                self.clear_search();
            }
            SessionDetailMsg::Clear => {
                self.clear_session(&sender);
            }
            SessionDetailMsg::InspectToolCall(id) => {
                self.inspect_tool_call(id, &sender);
            }
            SessionDetailMsg::InspectSubagent(id) => {
                self.inspect_subagent(id, &sender);
            }
            SessionDetailMsg::InspectReasoning(transcript_item_index) => {
                self.inspect_reasoning(transcript_item_index, &sender);
            }
            SessionDetailMsg::ToggleInspector => {
                self.toggle_inspector(&sender);
            }
            SessionDetailMsg::CloseInspector => {
                self.close_inspector(&sender);
            }
            SessionDetailMsg::InspectorWidgetVisibilityChanged(visible) => {
                self.sync_inspector_widget_visibility(visible, &sender);
            }
            SessionDetailMsg::OpenChildSession(child_session_id) => {
                self.open_child_session(child_session_id, &sender);
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            SessionDetailCmd::TranscriptLoaded {
                request_id,
                session_id,
                load_duration_ms,
                result,
            } => {
                self.apply_transcript_page_result(
                    &sender,
                    request_id,
                    session_id,
                    load_duration_ms,
                    result,
                );
            }
            SessionDetailCmd::SearchPositionsLoaded {
                request_id,
                session_id,
                load_duration_ms,
                result,
            } => {
                self.handle_search_positions_loaded(
                    &sender,
                    request_id,
                    session_id,
                    load_duration_ms,
                    result,
                );
            }
            SessionDetailCmd::MessageFullContentReady {
                item_index,
                session_id,
                message_index,
                result,
            } => {
                self.handle_message_full_content_ready(
                    item_index,
                    session_id,
                    message_index,
                    result,
                );
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        if let Some(session) = &self.session {
            Self::update_session_header(widgets, session);
            Self::update_chip_row(widgets, session);
            Self::update_first_prompt(widgets, session);
            Self::update_activity_section(widgets, session);
            Self::update_tokens_section(widgets, session);

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

        self.apply_scroll_target();
    }
}

/// Search highlighting must stay aligned with match navigation: `Next` /
/// `Previous` and the match counter are produced from `find_session_match_positions`,
/// which only reports message-kind FTS matches. Highlighting tool calls, tool
/// bursts, or unmatched messages would show highlights and burst match badges
/// that navigation can never reach and the counter never includes.
fn highlight_query_for_navigable_row(
    is_message: bool,
    transcript_item_index: i64,
    matched_item_indexes: &BTreeSet<i64>,
    highlight_query: Option<&str>,
) -> Option<String> {
    if is_message && matched_item_indexes.contains(&transcript_item_index) {
        highlight_query.map(str::to_string)
    } else {
        None
    }
}

impl SessionDetail {
    fn normalize_search_query(query: Option<String>) -> Option<String> {
        query.and_then(|query| {
            let trimmed = query.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
    }

    fn update_session_header(widgets: &SessionDetailWidgets, session: &Session) {
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
    }

    fn update_chip_row(widgets: &SessionDetailWidgets, session: &Session) {
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
    }

    fn update_first_prompt(widgets: &SessionDetailWidgets, session: &Session) {
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
    }

    fn update_activity_section(widgets: &SessionDetailWidgets, session: &Session) {
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
    }

    fn update_tokens_section(widgets: &SessionDetailWidgets, session: &Session) {
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

            let has_cache = usage.cache_read_tokens.is_some() || usage.cache_write_tokens.is_some();
            widgets.cache_pair.set_visible(has_cache);
            if has_cache && let Some(cache_text) = crate::ui::format::format_token_cache(usage) {
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
    }

    /// Scrolls to the pending [`ScrollTarget`], if any.
    ///
    /// The target row may not be realized yet when this runs, so we first nudge
    /// the [`gtk::ListView`] toward it, then resolve and focus the row from an
    /// idle callback, falling back to polling the frame clock until it appears.
    fn apply_scroll_target(&self) {
        let Some(target) = self.search.scroll_to_item.take() else {
            return;
        };

        let list_view = self.messages.view.clone();
        list_view.scroll_to(
            target.display_index as u32,
            gtk::ListScrollFlags::NONE,
            None,
        );

        let scroll_child = list_view.clone().upcast::<gtk::Widget>();
        glib::idle_add_local_once(move || {
            if let Some(row_widget) =
                Self::observed_row_widget_for_display_index(&list_view, target.display_index)
            {
                Self::focus_scroll_target(&row_widget, target, &scroll_child);
                return;
            }

            // Row not realized yet; poll the frame clock until it appears.
            let tick_count = std::cell::Cell::new(0u32);
            list_view.add_tick_callback(move |list_view, _| {
                let ticks = tick_count.get() + 1;
                tick_count.set(ticks);
                if ticks > 60 {
                    return glib::ControlFlow::Break;
                }
                let Some(row_widget) =
                    Self::observed_row_widget_for_display_index(list_view, target.display_index)
                else {
                    return glib::ControlFlow::Continue;
                };
                Self::focus_scroll_target(&row_widget, target, &scroll_child);
                glib::ControlFlow::Break
            });
        });
    }

    /// Brings a resolved transcript row into view. When the target addresses a
    /// child entry inside a collapsed tool-burst row, the burst is expanded
    /// first and the child is scrolled to once the revealer settles.
    fn focus_scroll_target(
        row_widget: &gtk::Widget,
        target: ScrollTarget,
        scroll_child: &gtk::Widget,
    ) {
        if let Some(child_index) = target.child_index
            && let Some((header_button, revealer)) =
                Self::tool_burst_header_and_revealer(row_widget)
        {
            if !revealer.reveals_child() {
                header_button.emit_clicked();
            }
            Self::scroll_to_burst_child_when_revealed(&revealer, child_index, scroll_child);
        } else {
            Self::scroll_widget_into_view(row_widget, scroll_child);
        }
    }

    /// Scrolls to the `child_index`-th child of a tool-burst revealer once it
    /// has materialized its content, polling the frame clock until then.
    fn scroll_to_burst_child_when_revealed(
        revealer: &gtk::Revealer,
        child_index: usize,
        scroll_child: &gtk::Widget,
    ) {
        let scroll_child = scroll_child.clone();
        let tick_count = std::cell::Cell::new(0u32);
        revealer.add_tick_callback(move |revealer, _| {
            let ticks = tick_count.get() + 1;
            tick_count.set(ticks);
            if ticks > 60 {
                return glib::ControlFlow::Break;
            }

            let Some(child_box) = revealer.child().and_then(|w| w.downcast::<gtk::Box>().ok())
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

            Self::scroll_widget_into_view(&child_widget, &scroll_child);
            glib::ControlFlow::Break
        });
    }

    fn set_session(
        &mut self,
        session: Session,
        search_query: Option<String>,
        sender: &ComponentSender<Self>,
    ) {
        self.invalidate_search_requests();
        let normalized = Self::normalize_search_query(search_query);
        self.search.query = normalized.clone();
        self.reset_search_matches();

        let session_id = session.id.clone();
        let message_count = session.message_count;
        let has_search_query = normalized.is_some();
        let query_len = normalized.as_ref().map(|query| query.len()).unwrap_or(0);

        self.session = Some(session);
        self.start_transcript_load(sender, &session_id, true, "open");
        tracing::info!(
            request_id = self.transcript.request_id,
            session_id = session_id.as_str(),
            message_count,
            has_search_query,
            query_len,
            "Session detail open started"
        );

        if let Some(query) = normalized {
            let request_id = self.search.request_id;
            self.spawn_match_positions_load(sender, request_id, session_id.clone(), query);
        }

        self.inspector.emit(ToolInspectorPaneMsg::Clear);
        self.set_inspector_open(false, sender);
    }

    fn update_search_query(&mut self, query: Option<String>, sender: &ComponentSender<Self>) {
        let normalized = Self::normalize_search_query(query);
        let active_session_id = self.session.as_ref().map(|session| session.id.as_str());
        let previous_match_count = self.search.match_positions.len();
        let query_len = normalized.as_ref().map(|query| query.len()).unwrap_or(0);
        let will_load_match_positions = self.session.is_some() && normalized.is_some();
        tracing::info!(
            session_id = active_session_id,
            has_query = normalized.is_some(),
            query_len,
            previous_match_count,
            will_load_match_positions,
            "Session detail search update started"
        );

        self.search.query = normalized.clone();
        self.invalidate_search_requests();
        self.reset_search_matches();

        if !self.messages.is_empty() {
            self.apply_highlight_query_to_typed_items(normalized.clone());
            self.refresh_typed_rows_preserving_scroll();
        }

        if let (Some(session), Some(query)) = (&self.session, normalized) {
            let request_id = self.search.request_id;
            self.spawn_match_positions_load(sender, request_id, session.id.clone(), query);
        }
    }

    fn set_match_positions(
        &mut self,
        request_id: u64,
        session_id: String,
        positions: Vec<MatchPosition>,
        sender: &ComponentSender<Self>,
    ) {
        let active_session_matches = self
            .session
            .as_ref()
            .is_some_and(|session| session.id == session_id);
        if request_id != self.search.request_id || !active_session_matches {
            tracing::debug!(
                request_id,
                session_id,
                "Ignoring stale session detail search results"
            );
            return;
        }

        let match_count = positions.len();
        let will_schedule_initial_jump = match_count > 0;
        tracing::info!(
            request_id,
            session_id = session_id.as_str(),
            match_count,
            will_schedule_initial_jump,
            loaded_count = self.transcript.loaded_count,
            "Session detail search positions applied"
        );

        self.search.match_positions = positions;
        self.clamp_current_match();
        self.search.pending_jump = None;
        self.search.loading_jump = false;
        if self.messages.is_empty() {
            self.start_transcript_load(sender, &session_id, false, "search");
        } else {
            self.apply_highlight_query_to_typed_items(self.search.query.clone());
            self.refresh_typed_rows_preserving_scroll();
        }
        if !self.search.match_positions.is_empty() {
            self.jump_to(0, sender);
        }
    }

    fn jump_to_previous_match(&mut self, sender: &ComponentSender<Self>) {
        if self.search.match_positions.is_empty() || self.search.loading_jump {
            return;
        }

        let target = match self.search.current_match {
            0 => self.search.match_positions.len() - 1,
            n => n - 1,
        };
        self.jump_to(target, sender);
    }

    fn jump_to_next_match(&mut self, sender: &ComponentSender<Self>) {
        if self.search.match_positions.is_empty() || self.search.loading_jump {
            return;
        }

        let target = (self.search.current_match + 1) % self.search.match_positions.len();
        self.jump_to(target, sender);
    }

    fn show_expand_load_failure(&self) {
        tracing::warn!("Could not load full message content");
        self.pending_toast.set(true);
    }

    fn toggle_message_expand(&mut self, item_index: usize, sender: &ComponentSender<Self>) {
        let idx = item_index as u32;
        let Some(item) = self.messages.get(idx) else {
            tracing::debug!(item_index, "Typed message expand ignored: item not found");
            return;
        };

        let mut load_request = None;
        let (clone, will_expand) = {
            let ref_data = item.borrow();
            let will_expand = !ref_data.expanded.get();
            ref_data.expanded.set(will_expand);
            if will_expand
                && ref_data.full_content.is_none()
                && let crate::ui::transcript_item_data::TranscriptItemKind::Message(message) =
                    &ref_data.kind
            {
                load_request = Some((
                    message.db_path.clone(),
                    message.preview.session_id.clone(),
                    message.preview.message_index,
                ));
            }
            (ref_data.clone(), will_expand)
        };

        self.messages.remove(idx);
        self.messages.insert(idx, clone);
        if let Some((db_path, session_id, message_index)) = load_request {
            sender.spawn_oneshot_command(move || SessionDetailCmd::MessageFullContentReady {
                item_index,
                session_id: session_id.clone(),
                message_index,
                result: load_message_full_content(&db_path, &session_id, message_index)
                    .map_err(|err| format!("{err:#}")),
            });
        }

        tracing::debug!(item_index, will_expand, "Typed message expand requested");
    }

    fn clear_search(&mut self) {
        self.search.query = None;
        self.invalidate_search_requests();
        self.reset_search_matches();
        if !self.messages.is_empty() {
            self.apply_highlight_query_to_typed_items(None);
            self.refresh_typed_rows_preserving_scroll();
        }
    }

    fn clear_session(&mut self, sender: &ComponentSender<Self>) {
        self.invalidate_transcript_requests();
        self.invalidate_search_requests();
        self.session = None;
        self.transcript.load_started_at = None;
        self.clear_messages_safely_with_metrics("component_clear");
        self.transcript.loaded_count = 0;
        self.transcript.loading = false;
        self.search.query = None;
        self.reset_search_matches();
        self.inspector.emit(ToolInspectorPaneMsg::Clear);
        self.set_inspector_open(false, sender);
    }

    /// Shared body for the inspector selection handlers: resolve the active
    /// session, emit the pane message built from its id, and open the inspector.
    /// No-op when no session is active.
    ///
    /// `kind`/`target` are recorded for tracing only; `build_message` receives
    /// the resolved `session_id` so each caller can assemble its own variant.
    fn select_in_inspector(
        &mut self,
        kind: &'static str,
        target: &str,
        build_message: impl FnOnce(String) -> ToolInspectorPaneMsg,
        sender: &ComponentSender<Self>,
    ) {
        let Some(session_id) = self.session.as_ref().map(|s| s.id.clone()) else {
            return;
        };

        tracing::info!(
            session_id = session_id.as_str(),
            kind,
            target,
            previous_open = self.inspector_open,
            new_open = true,
            "Session detail inspector selection"
        );
        self.inspector.emit(build_message(session_id));
        self.set_inspector_open(true, sender);
    }

    fn inspect_tool_call(&mut self, id: String, sender: &ComponentSender<Self>) {
        self.select_in_inspector(
            "tool_call",
            &id,
            |session_id| ToolInspectorPaneMsg::SelectToolCall {
                session_id,
                tool_call_id: id.clone(),
            },
            sender,
        );
    }

    fn inspect_subagent(&mut self, id: String, sender: &ComponentSender<Self>) {
        self.select_in_inspector(
            "subagent",
            &id,
            |session_id| ToolInspectorPaneMsg::SelectSubagent {
                session_id,
                subagent_id: id.clone(),
            },
            sender,
        );
    }

    fn inspect_reasoning(&mut self, transcript_item_index: i64, sender: &ComponentSender<Self>) {
        self.select_in_inspector(
            "reasoning",
            &transcript_item_index.to_string(),
            |session_id| ToolInspectorPaneMsg::SelectReasoning {
                session_id,
                transcript_item_index,
            },
            sender,
        );
    }

    fn toggle_inspector(&mut self, sender: &ComponentSender<Self>) {
        let new_open = !self.inspector_open;
        tracing::info!(
            previous_open = self.inspector_open,
            new_open,
            "Session detail inspector toggled"
        );
        self.set_inspector_open(new_open, sender);
    }

    fn close_inspector(&mut self, sender: &ComponentSender<Self>) {
        tracing::info!(
            previous_open = self.inspector_open,
            new_open = false,
            "Session detail inspector close requested"
        );
        self.set_inspector_open(false, sender);
    }

    fn sync_inspector_widget_visibility(&mut self, visible: bool, sender: &ComponentSender<Self>) {
        if self.inspector_open == visible {
            return;
        }

        tracing::info!(
            previous_open = self.inspector_open,
            new_open = visible,
            "Session detail inspector widget visibility changed"
        );
        self.inspector_open = visible;
        sender
            .output(SessionDetailOutput::InspectorVisibilityChanged(visible))
            .ok();
    }

    fn open_child_session(&self, child_session_id: String, sender: &ComponentSender<Self>) {
        sender
            .output(SessionDetailOutput::OpenChildSession(child_session_id))
            .ok();
    }

    fn handle_start_deferred_first_page_load(
        &mut self,
        request_id: u64,
        session_id: String,
        sender: &ComponentSender<Self>,
    ) {
        let active_session_matches = self
            .session
            .as_ref()
            .is_some_and(|session| session.id == session_id);
        if request_id == self.transcript.request_id && active_session_matches {
            tracing::debug!(
                request_id,
                session_id = session_id.as_str(),
                configured_delay_ms = DEFERRED_FIRST_PAGE_LOAD_DELAY_MS,
                "Session detail deferred first page load started"
            );
            self.spawn_transcript_page_load(sender, request_id, session_id);
        }
    }

    fn handle_deferred_clear(&mut self, request_id: u64) {
        if request_id == self.transcript.request_id {
            self.clear_for_navigation_back();
        }
    }

    fn current_match_item_indexes(&self) -> BTreeSet<i64> {
        self.search
            .match_positions
            .iter()
            .map(|position| position.item_index)
            .collect()
    }

    fn display_targets_for_item(
        display_index: usize,
        item: &DisplayTranscriptItem,
    ) -> BTreeMap<i64, ScrollTarget> {
        let mut targets = BTreeMap::new();
        match item {
            DisplayTranscriptItem::Single(row) => {
                targets.insert(
                    row.item_index,
                    ScrollTarget {
                        display_index,
                        child_index: None,
                    },
                );
            }
            DisplayTranscriptItem::ToolBurst(burst) => {
                for (child_index, row) in burst.rows.iter().enumerate() {
                    targets.insert(
                        row.item_index,
                        ScrollTarget {
                            display_index,
                            child_index: Some(child_index),
                        },
                    );
                }
            }
        }
        targets
    }

    fn extend_display_targets(&mut self, targets: BTreeMap<i64, ScrollTarget>) {
        self.transcript
            .display_targets_by_item_index
            .extend(targets);
    }

    fn build_display_items(
        rows: Vec<crate::database::TranscriptItemRow>,
        session_id: &str,
        highlight_query: Option<String>,
        matched_item_indexes: &BTreeSet<i64>,
        db_path: Arc<PathBuf>,
        base_display_index: usize,
    ) -> PreparedTranscriptItems {
        let mut items = Vec::new();
        let mut display_targets_by_item_index = BTreeMap::new();

        for (offset, item) in group_transcript_rows(rows).into_iter().enumerate() {
            let display_index = base_display_index + offset;
            display_targets_by_item_index
                .extend(Self::display_targets_for_item(display_index, &item));

            let item_highlight = match &item {
                DisplayTranscriptItem::Single(row) => highlight_query_for_navigable_row(
                    row.kind == crate::models::TranscriptItemKind::Message,
                    row.item_index,
                    matched_item_indexes,
                    highlight_query.as_deref(),
                ),
                DisplayTranscriptItem::ToolBurst(_) => None,
            };

            items.push(transcript_item_init_from_display_item(
                display_index,
                &item,
                session_id,
                item_highlight,
                db_path.clone(),
            ));
        }

        PreparedTranscriptItems {
            items,
            display_targets_by_item_index,
        }
    }

    fn invalidate_transcript_requests(&mut self) {
        self.transcript.request_id = self.transcript.request_id.wrapping_add(1);
    }

    fn invalidate_search_requests(&mut self) {
        self.search.request_id = self.search.request_id.wrapping_add(1);
    }

    /// Returns `true` while transcript content is in flight and the display
    /// index is therefore unstable.
    ///
    /// Search-jump bookkeeping (`continue_pending_jump`) must wait for this to
    /// be `false` before resolving an `item_index` to a `ScrollTarget`,
    /// because `display_targets_by_item_index` is only populated as pages
    /// finish rendering.
    fn is_transcript_loading(&self) -> bool {
        self.transcript.loading
    }

    fn spawn_match_positions_load(
        &self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        session_id: String,
        query: String,
    ) {
        let db_path = self.db_path.clone();
        sender.spawn_oneshot_command(move || {
            let started_at = Instant::now();
            let result = crate::database::open_connection(&db_path)
                .and_then(|db| find_session_match_positions(&db, &session_id, &query))
                .map_err(|err| format!("{err:#}"));
            let load_duration_ms = started_at.elapsed().as_millis();

            SessionDetailCmd::SearchPositionsLoaded {
                request_id,
                session_id,
                load_duration_ms,
                result,
            }
        });
    }

    /// Update internal inspector visibility and notify the parent.  Idempotent:
    /// no-op when the requested state matches the current one.
    fn set_inspector_open(&mut self, open: bool, sender: &ComponentSender<Self>) {
        if self.inspector_open == open {
            return;
        }
        self.inspector_open = open;
        sender
            .output(SessionDetailOutput::InspectorVisibilityChanged(open))
            .ok();
    }

    /// Reset transcript state and trigger the first-page load.
    ///
    /// Pass `defer = true` only when a fresh session is being opened: the load
    /// is delayed by `DEFERRED_FIRST_PAGE_LOAD_DELAY_MS` so the navigation
    /// animation can complete before transcript widgets start rendering.
    ///
    /// Pass `defer = false` for in-place reloads (search query updates,
    /// `ClearSearch`). Deferring those would leave the transcript blank for
    /// 250 ms after every keystroke, since this method clears the existing
    /// rows synchronously before scheduling the load.
    fn start_transcript_load(
        &mut self,
        sender: &ComponentSender<Self>,
        session_id: &str,
        defer: bool,
        clear_reason: &'static str,
    ) {
        self.invalidate_transcript_requests();
        self.transcript.loading = true;
        self.transcript.loaded_count = 0;
        self.clear_messages_safely_with_metrics(clear_reason);
        self.transcript.load_started_at = Some(Instant::now());

        let request_id = self.transcript.request_id;
        let session_id = session_id.to_string();

        if defer {
            let input_sender = sender.input_sender().clone();
            glib::timeout_add_local_once(
                Duration::from_millis(DEFERRED_FIRST_PAGE_LOAD_DELAY_MS),
                move || {
                    let _ = input_sender.send(SessionDetailMsg::StartDeferredFirstPageLoad {
                        request_id,
                        session_id,
                    });
                },
            );
        } else {
            self.spawn_transcript_page_load(sender, request_id, session_id);
        }
    }

    fn spawn_transcript_page_load(
        &self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        session_id: String,
    ) {
        let db_path = self.db_path.clone();
        let preview_len = self.preview_len as i64;

        sender.spawn_oneshot_command(move || {
            let started_at = Instant::now();
            let result = load_all_transcript_items(&db_path, &session_id, preview_len)
                .map_err(|err| format!("{err:#}"));
            let load_duration_ms = started_at.elapsed().as_millis();

            SessionDetailCmd::TranscriptLoaded {
                request_id,
                session_id,
                load_duration_ms,
                result,
            }
        });
    }

    fn apply_transcript_rows(
        &mut self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        session_id: &str,
        rows: Vec<crate::database::TranscriptItemRow>,
    ) {
        self.transcript.loading = false;
        self.transcript.loaded_count = rows.len();
        let highlight = self.search.query.clone();
        let db_path = self.db_path.clone();
        self.clear_messages_safely_with_metrics("first_page_apply");
        let build_started_at = Instant::now();
        let source_row_count = rows.len();
        let matched_item_indexes = self.current_match_item_indexes();
        let prepared = Self::build_display_items(
            rows,
            session_id,
            highlight,
            &matched_item_indexes,
            db_path,
            0,
        );
        let display_item_count = prepared.items.len();
        self.extend_display_targets(prepared.display_targets_by_item_index);
        tracing::info!(
            request_id,
            session_id,
            source_row_count,
            display_item_count,
            build_duration_ms = build_started_at.elapsed().as_millis(),
            "Prepared transcript"
        );

        let input_sender = sender.input_sender().clone();
        let items: Vec<TranscriptItemData> = prepared
            .items
            .into_iter()
            .map(|init| TranscriptItemData::from_init(init, input_sender.clone()))
            .collect();
        self.messages.extend_from_iter(items);
        self.continue_pending_jump(sender);
    }

    fn handle_transcript_page_error(&mut self, session_id: &str, err: String) {
        tracing::error!(
            "Failed to load transcript items for {}: {}",
            session_id,
            err
        );
        self.clear_messages_safely_with_metrics("transcript_error");
        self.transcript.loaded_count = 0;
        self.transcript.loading = false;
        self.search.pending_jump = None;
        self.search.loading_jump = false;
    }

    fn handle_search_positions_loaded(
        &self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        session_id: String,
        load_duration_ms: u128,
        result: Result<Vec<MatchPosition>, String>,
    ) {
        let success = result.is_ok();
        let match_count = result
            .as_ref()
            .map(|positions| positions.len())
            .unwrap_or(0);
        tracing::info!(
            request_id,
            session_id = session_id.as_str(),
            success,
            match_count,
            load_duration_ms,
            "Session detail search positions loaded"
        );
        let positions = match result {
            Ok(positions) => positions,
            Err(err) => {
                tracing::error!(
                    request_id,
                    session_id,
                    "Failed to load session detail search positions: {}",
                    err
                );
                Vec::new()
            }
        };
        let _ = sender
            .input_sender()
            .send(SessionDetailMsg::SetMatchPositions {
                request_id,
                session_id,
                positions,
            });
    }

    fn handle_message_full_content_ready(
        &mut self,
        item_index: usize,
        session_id: String,
        message_index: usize,
        result: Result<String, String>,
    ) {
        if !self.typed_message_full_content_target_matches(item_index, &session_id, message_index) {
            tracing::debug!(
                item_index,
                session_id = session_id.as_str(),
                message_index,
                "Ignoring stale full message content result"
            );
            return;
        }

        match result {
            Ok(content) => self.set_typed_message_full_content(item_index, content),
            Err(err) => {
                tracing::error!(item_index, "Failed to load full message content: {err}");
                self.reset_typed_message_expansion(item_index);
                self.pending_toast.set(true);
            }
        }
    }

    fn apply_transcript_page_result(
        &mut self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        session_id: String,
        load_duration_ms: u128,
        result: Result<Vec<crate::database::TranscriptItemRow>, String>,
    ) {
        if request_id != self.transcript.request_id {
            tracing::debug!("Ignoring stale transcript page for session {}", session_id,);
            return;
        }

        let active_session_matches = self
            .session
            .as_ref()
            .is_some_and(|session| session.id == session_id);
        if !active_session_matches {
            tracing::debug!(
                "Ignoring transcript page for inactive session {}",
                session_id,
            );
            return;
        }

        match result {
            Ok(rows) => {
                tracing::info!(
                    request_id,
                    session_id = session_id.as_str(),
                    source_row_count = rows.len(),
                    load_duration_ms,
                    "Loaded transcript"
                );
                self.apply_transcript_rows(sender, request_id, &session_id, rows)
            }
            Err(err) => self.handle_transcript_page_error(&session_id, err),
        }
    }

    fn reset_search_matches(&mut self) {
        self.search.match_positions.clear();
        self.search.current_match = 0;
        self.search.pending_jump = None;
        self.search.loading_jump = false;
    }

    fn loaded_match_count(&self) -> usize {
        self.search
            .match_positions
            .iter()
            .filter(|position| {
                self.transcript
                    .display_targets_by_item_index
                    .contains_key(&position.item_index)
            })
            .count()
    }

    fn clamp_current_match(&mut self) {
        self.search.current_match = match self.search.match_positions.len() {
            0 => 0,
            len if self.search.current_match >= len => len - 1,
            _ => self.search.current_match,
        };
    }

    fn jump_to(&mut self, target: usize, sender: &ComponentSender<Self>) {
        if self.search.match_positions.get(target).is_none() {
            return;
        }

        self.search.current_match = target;
        self.search.pending_jump = Some(target);
        self.search.loading_jump = true;
        self.continue_pending_jump(sender);
    }

    fn continue_pending_jump(&mut self, _sender: &ComponentSender<Self>) {
        let Some(target) = self.search.pending_jump else {
            return;
        };
        let Some(position) = self.search.match_positions.get(target) else {
            self.search.pending_jump = None;
            self.search.loading_jump = false;
            return;
        };

        if self.is_transcript_loading() && self.messages.is_empty() {
            return;
        }

        if let Some(scroll_target) = self
            .transcript
            .display_targets_by_item_index
            .get(&position.item_index)
            .copied()
            && (self.messages.len() as usize) > scroll_target.display_index
        {
            self.search.pending_jump = None;
            self.search.loading_jump = false;
            self.search.scroll_to_item.set(Some(scroll_target));
        } else if (position.item_index as usize) >= self.transcript.loaded_count {
            tracing::warn!(
                item_index = position.item_index,
                loaded_count = self.transcript.loaded_count,
                "search match position is outside loaded transcript range"
            );
            self.search.pending_jump = None;
            self.search.loading_jump = false;
        } else {
            debug_assert!(
                false,
                "match item_index {} is within loaded range ({}) but absent from display_targets_by_item_index",
                position.item_index, self.transcript.loaded_count
            );
            tracing::warn!(
                item_index = position.item_index,
                loaded_count = self.transcript.loaded_count,
                "search match position is loaded but missing from display index"
            );
            self.search.pending_jump = None;
            self.search.loading_jump = false;
        }
    }

    fn set_typed_message_full_content(&mut self, item_index: usize, content: String) {
        let idx = item_index as u32;
        let Some(item) = self.messages.get(idx) else {
            return;
        };

        let mut clone = item.borrow().clone();
        clone.full_content = Some(content);
        self.messages.remove(idx);
        self.messages.insert(idx, clone);
    }

    fn reset_typed_message_expansion(&mut self, item_index: usize) {
        let idx = item_index as u32;
        let Some(item) = self.messages.get(idx) else {
            return;
        };

        let clone = {
            let item = item.borrow();
            Self::reset_message_expansion_after_full_content_failure(&item);
            item.clone()
        };
        self.messages.remove(idx);
        self.messages.insert(idx, clone);
    }

    fn reset_message_expansion_after_full_content_failure(item: &TranscriptItemData) {
        item.expanded.set(false);
    }

    fn typed_message_full_content_target_matches(
        &self,
        item_index: usize,
        session_id: &str,
        message_index: usize,
    ) -> bool {
        let active_session_id = self.session.as_ref().map(|session| session.id.as_str());
        let Some(item) = self.messages.get(item_index as u32) else {
            return false;
        };

        Self::message_full_content_target_matches(
            &item.borrow(),
            active_session_id,
            session_id,
            message_index,
        )
    }

    fn message_full_content_target_matches(
        item: &TranscriptItemData,
        active_session_id: Option<&str>,
        session_id: &str,
        message_index: usize,
    ) -> bool {
        let Some(active_session_id) = active_session_id else {
            return false;
        };
        if active_session_id != session_id {
            return false;
        }

        let crate::ui::transcript_item_data::TranscriptItemKind::Message(message) = &item.kind
        else {
            return false;
        };

        message.preview.session_id == session_id && message.preview.message_index == message_index
    }

    fn apply_highlight_query_to_typed_items(&self, query: Option<String>) {
        let matched_item_indexes = self.current_match_item_indexes();
        for item in self.messages.iter() {
            let mut data = item.borrow_mut();
            let is_message = matches!(
                data.kind,
                crate::ui::transcript_item_data::TranscriptItemKind::Message(_)
            );
            let item_query = match data.transcript_item_index {
                Some(transcript_item_index) => highlight_query_for_navigable_row(
                    is_message,
                    transcript_item_index,
                    &matched_item_indexes,
                    query.as_deref(),
                ),
                None => None,
            };
            data.apply_highlight_query(item_query);
        }
    }

    fn prepare_for_navigation_back(&mut self, sender: &ComponentSender<Self>) {
        self.invalidate_transcript_requests();
        self.transcript.loading = false;

        let request_id = self.transcript.request_id;
        let input_sender = sender.input_sender().clone();
        glib::timeout_add_local_once(Duration::from_millis(DEFERRED_CLEAR_DELAY_MS), move || {
            let _ = input_sender.send(SessionDetailMsg::DeferredClear { request_id });
        });
    }

    fn clear_for_navigation_back(&mut self) {
        self.invalidate_search_requests();
        self.session = None;
        self.transcript.load_started_at = None;
        self.clear_messages_safely_with_metrics("navigation_back");
        self.transcript.loaded_count = 0;
        self.search.query = None;
        self.reset_search_matches();
    }

    /// Clear transcript rows after releasing focus from any currently-focused row widget.
    fn clear_messages_safely(&mut self) {
        self.release_focus_from_transcript_if_needed();
        self.messages.clear();
        self.transcript.display_targets_by_item_index.clear();
    }

    fn clear_messages_safely_with_metrics(&mut self, reason: &'static str) -> ClearMessagesMetrics {
        let row_count_before = self.messages.len() as usize;
        let started_at = Instant::now();
        self.clear_messages_safely();
        let duration_ms = started_at.elapsed().as_millis();
        tracing::info!(
            reason,
            row_count_before,
            duration_ms,
            "Cleared session detail transcript rows"
        );
        ClearMessagesMetrics {
            row_count_before,
            duration_ms,
        }
    }

    /// Avoid GTK focus traversing a row subtree while it is being replaced.
    fn release_focus_from_transcript_if_needed(&self) {
        let messages_widget: gtk::Widget = self.messages.view.clone().upcast();
        let Some(window) = messages_widget
            .ancestor(gtk::Window::static_type())
            .and_then(|w| w.downcast::<gtk::Window>().ok())
        else {
            return;
        };

        let Some(focus_widget) = gtk::prelude::GtkWindowExt::focus(&window) else {
            return;
        };

        if focus_widget.is_ancestor(&messages_widget) {
            tracing::debug!("Clearing window focus before replacing transcript rows");
            gtk::prelude::GtkWindowExt::set_focus(&window, Option::<&gtk::Widget>::None);
        }
    }

    /// Looks up the `(header_button, revealer)` pair for a tool-burst transcript
    /// row. Typed rows wrap the page contents in a `gtk::Stack`; this traverses
    /// the Stack's "tool-burst" named child to find the header button and
    /// revealer.
    fn tool_burst_header_and_revealer(
        row_widget: &gtk::Widget,
    ) -> Option<(gtk::Button, gtk::Revealer)> {
        let stack = row_widget
            .first_child()
            .and_then(|w| w.downcast::<gtk::Stack>().ok())?;
        let tool_burst_page = stack
            .child_by_name("tool-burst")
            .and_then(|w| w.downcast::<gtk::Box>().ok())?;
        let header_button = tool_burst_page
            .first_child()
            .and_then(|w| w.downcast::<gtk::Button>().ok())?;
        let revealer = header_button
            .next_sibling()
            .and_then(|w| w.downcast::<gtk::Revealer>().ok())?;
        Some((header_button, revealer))
    }

    /// Resolves the named transcript row root from a [`gtk::ListView`] child.
    ///
    /// `GtkListView` wraps every factory-produced row in an internal
    /// `GtkListItemWidget`; the row root we name `transcript-row-{index}` is
    /// that wrapper's child, so the wrapper itself never carries the name.
    /// Descend one level when a child exists, otherwise return the widget as-is.
    fn list_view_row_widget_from_child(obj: gtk::glib::Object) -> Option<gtk::Widget> {
        let widget = obj.downcast::<gtk::Widget>().ok()?;
        Some(widget.first_child().unwrap_or(widget))
    }

    fn observed_row_widget_for_display_index(
        list_view: &gtk::ListView,
        display_index: usize,
    ) -> Option<gtk::Widget> {
        let children = list_view.observe_children();
        for index in 0..children.n_items() {
            let Some(row_widget) = children
                .item(index)
                .and_then(Self::list_view_row_widget_from_child)
            else {
                continue;
            };

            if Self::row_widget_matches_display_index(&row_widget, display_index) {
                return Some(row_widget);
            }
        }

        None
    }

    fn row_widget_matches_display_index(row_widget: &gtk::Widget, display_index: usize) -> bool {
        row_widget.widget_name().as_str()
            == format!("{TRANSCRIPT_ROW_WIDGET_NAME_PREFIX}{display_index}")
    }

    /// Snapshot scroll position, clone all items, clear and re-extend to force
    /// GTK re-bind (propagating in-place `highlight_query` mutations), then
    /// restore scroll position via idle callback.
    fn refresh_typed_rows_preserving_scroll(&mut self) {
        let saved_vadj = self
            .messages
            .view
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok())
            .map(|sw| sw.vadjustment().value());

        let items: Vec<TranscriptItemData> = self
            .messages
            .iter()
            .map(|item| item.borrow().clone())
            .collect();

        self.messages.clear();
        self.messages.extend_from_iter(items);

        if let Some(saved_value) = saved_vadj {
            let view = self.messages.view.clone();
            glib::idle_add_local_once(move || {
                if let Some(sw) = view
                    .ancestor(gtk::ScrolledWindow::static_type())
                    .and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok())
                {
                    sw.vadjustment().set_value(saved_value);
                }
            });
        }
    }

    fn scroll_widget_into_view(widget: &gtk::Widget, scroll_child: &gtk::Widget) {
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
}

#[cfg(test)]
mod tests;
