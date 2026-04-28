use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, RelmWidgetExt,
    adw, gtk,
};

use crate::database::load_transcript_items;
use crate::models::Session;
use crate::ui::activity_bar::SessionActivityBar;
use crate::ui::tool_inspector_pane::{
    ToolInspectorPane, ToolInspectorPaneMsg, ToolInspectorPaneOutput,
};
use crate::ui::transcript_display::{
    DisplayTranscriptItem, group_transcript_rows, regroup_boundary, trailing_tool_call_rows,
    trailing_tool_rows_from_display,
};
use crate::ui::transcript_row::{
    TranscriptItemInit, TranscriptRow, TranscriptRowOutput, transcript_item_init_from_display_item,
};

const INITIAL_PAGE_SIZE: usize = 75;
const NEXT_PAGE_SIZE: usize = 100;
const PREVIEW_LEN: usize = 2000;
const RENDER_BATCH_SIZE: usize = 3;
const RENDER_BATCH_DELAY_MS: u64 = 16;
const DEFERRED_FIRST_PAGE_LOAD_DELAY_MS: u64 = 250;
const DEFERRED_CLEAR_DELAY_MS: u64 = 250;

/// Detail view for a single indexed session.
///
/// This component owns the session summary header, paginated transcript
/// rendering, the inspector pane (right-hand split sidebar), and transcript
/// search navigation.
pub struct SessionDetail {
    db_path: Arc<PathBuf>,
    session: Option<Session>,
    messages: FactoryVecDeque<TranscriptRow>,
    initial_page_size: usize,
    page_size: usize,
    preview_len: usize,
    loaded_count: usize,
    has_more_messages: bool,
    loading_first_page: bool,
    loading_next_page: bool,
    transcript_request_id: u64,
    pending_render_batch: Option<PendingRenderBatch>,
    last_render_metrics: Option<RenderMetrics>,
    pending_boundary_tool_rows: Vec<crate::database::TranscriptItemRow>,
    search_query: Option<String>,
    /// Keyed by top-level display index in the transcript factory.
    ///
    /// This is intentionally a UI index, not the original database
    /// `transcript_items.item_index`, because grouped tool bursts collapse
    /// multiple source rows into one displayed row.
    match_segments: BTreeMap<usize, Vec<usize>>,
    current_match: usize,
    total_matches: usize,
    scroll_to_item: Cell<Option<ScrollTarget>>,
    pending_toast: Cell<bool>,
    inspector: Controller<ToolInspectorPane>,
    inspector_open: bool,
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

struct BoundaryAppendPlan {
    replacement_items: Vec<DisplayTranscriptItem>,
    rows: Vec<crate::database::TranscriptItemRow>,
}

struct PendingRenderBatch {
    request_id: u64,
    offset: usize,
    source_row_count: usize,
    total_items: usize,
    rendered_items: usize,
    batch_count: usize,
    queued_at: Instant,
    last_batch_completed_at: Option<Instant>,
    total_push_duration: Duration,
    max_push_duration: Duration,
    max_schedule_gap: Duration,
    row_kind_counts: RenderRowKindCounts,
    items: VecDeque<TranscriptItemInit>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RenderRowKindCounts {
    message_count: usize,
    tool_call_count: usize,
    tool_burst_count: usize,
    subagent_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderMetrics {
    offset: usize,
    source_row_count: usize,
    display_item_count: usize,
    batch_count: usize,
    wall_duration_ms: u128,
    total_duration_ms: u128,
    total_push_duration_ms: u128,
    max_push_duration_ms: u128,
    max_schedule_gap_ms: u128,
    message_count: usize,
    tool_call_count: usize,
    tool_burst_count: usize,
    subagent_count: usize,
}

impl std::fmt::Debug for PendingRenderBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingRenderBatch")
            .field("request_id", &self.request_id)
            .field("offset", &self.offset)
            .field("source_row_count", &self.source_row_count)
            .field("total_items", &self.total_items)
            .field("rendered_items", &self.rendered_items)
            .field("batch_count", &self.batch_count)
            .field("remaining_items", &self.items.len())
            .finish()
    }
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
    LoadMore,
    PrevMatch,
    NextMatch,
    ClearSearch,
    /// Receives per-row match counts from [`TranscriptRow`] children; one count
    /// per segment (burst child or single row).
    MatchSegments(usize, Vec<usize>),
    RenderNextTranscriptBatch {
        request_id: u64,
    },
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
    TranscriptPageLoaded {
        request_id: u64,
        session_id: String,
        offset: usize,
        limit: usize,
        load_duration_ms: u128,
        result: Result<Vec<crate::database::TranscriptItemRow>, String>,
    },
}

impl std::fmt::Debug for SessionDetailCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TranscriptPageLoaded {
                request_id,
                session_id,
                offset,
                limit,
                load_duration_ms,
                result,
            } => {
                let result_summary = match result {
                    Ok(rows) => format!("Ok({} rows)", rows.len()),
                    Err(err) => format!("Err({err})"),
                };

                f.debug_struct("TranscriptPageLoaded")
                    .field("request_id", request_id)
                    .field("session_id", session_id)
                    .field("offset", offset)
                    .field("limit", limit)
                    .field("load_duration_ms", load_duration_ms)
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
                    set_content = &gtk::ScrolledWindow {
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
                                set_halign: gtk::Align::Center,
                                set_margin_top: 12,
                                set_margin_bottom: 12,
                                #[watch]
                                set_visible: model.has_more_messages,
                                #[watch]
                                set_sensitive: !model.loading_next_page
                                    && !model.loading_first_page
                                    && model.pending_render_batch.is_none(),
                                #[watch]
                                set_label: if model.loading_next_page { "Loading..." } else { "Load more" },
                                connect_clicked => SessionDetailMsg::LoadMore,
                            },
                        },
                    },
                    }, // close inspector_split (adw::OverlaySplitView)

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
                    display_index,
                    segments,
                } => SessionDetailMsg::MatchSegments(display_index, segments),
                TranscriptRowOutput::ExpandLoadFailed { .. } => {
                    SessionDetailMsg::ShowExpandLoadFailure
                }
                TranscriptRowOutput::InspectToolCall(id) => SessionDetailMsg::InspectToolCall(id),
                TranscriptRowOutput::InspectSubagent(id) => SessionDetailMsg::InspectSubagent(id),
                TranscriptRowOutput::InspectReasoning {
                    transcript_item_index,
                    ..
                } => SessionDetailMsg::InspectReasoning(transcript_item_index),
            });

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
            initial_page_size: INITIAL_PAGE_SIZE,
            page_size: NEXT_PAGE_SIZE,
            preview_len: PREVIEW_LEN,
            loaded_count: 0,
            has_more_messages: false,
            loading_first_page: false,
            loading_next_page: false,
            transcript_request_id: 0,
            pending_render_batch: None,
            last_render_metrics: None,
            pending_boundary_tool_rows: Vec::new(),
            search_query: None,
            match_segments: BTreeMap::new(),
            current_match: 0,
            total_matches: 0,
            scroll_to_item: Cell::new(None),
            pending_toast: Cell::new(false),
            inspector,
            inspector_open: false,
        };

        let messages_box = model.messages.widget();
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

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
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
                self.start_first_page_load(&sender, &session_id);
                self.inspector.emit(ToolInspectorPaneMsg::Clear);
                self.set_inspector_open(false, &sender);
            }
            SessionDetailMsg::UpdateSearchQuery(query) => {
                self.search_query = query;
                self.reset_search_matches();
                self.reload_current_session(&sender);
            }
            SessionDetailMsg::LoadMore => {
                self.load_next_page(&sender);
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
            SessionDetailMsg::MatchSegments(display_index, segments) => {
                self.update_match_segments(display_index, segments);
            }
            SessionDetailMsg::RenderNextTranscriptBatch { request_id } => {
                self.render_next_transcript_batch(&sender, request_id);
            }
            SessionDetailMsg::StartDeferredFirstPageLoad {
                request_id,
                session_id,
            } => {
                let active_session_matches = self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.id == session_id);
                if request_id == self.transcript_request_id && active_session_matches {
                    self.spawn_transcript_page_load(
                        &sender,
                        request_id,
                        session_id,
                        0,
                        self.initial_page_size,
                    );
                }
            }
            SessionDetailMsg::PrepareForNavigationBack => {
                self.prepare_for_navigation_back(&sender);
            }
            SessionDetailMsg::DeferredClear { request_id } => {
                if request_id == self.transcript_request_id {
                    self.clear_for_navigation_back();
                }
            }
            SessionDetailMsg::ShowExpandLoadFailure => {
                tracing::warn!("Could not load full message content");
                self.pending_toast.set(true);
            }
            SessionDetailMsg::ClearSearch => {
                self.search_query = None;
                self.reset_search_matches();
                self.reload_current_session(&sender);
            }
            SessionDetailMsg::Clear => {
                self.invalidate_transcript_requests();
                self.session = None;
                self.clear_messages_safely();
                self.loaded_count = 0;
                self.has_more_messages = false;
                self.loading_first_page = false;
                self.loading_next_page = false;
                self.pending_render_batch = None;
                self.search_query = None;
                self.reset_search_matches();
                self.clear_pending_boundary_tool_rows();
                self.inspector.emit(ToolInspectorPaneMsg::Clear);
                self.set_inspector_open(false, &sender);
            }
            SessionDetailMsg::InspectToolCall(id) => {
                if let Some(session_id) = self.session.as_ref().map(|s| s.id.clone()) {
                    self.inspector.emit(ToolInspectorPaneMsg::SelectToolCall {
                        session_id,
                        tool_call_id: id,
                    });
                    self.set_inspector_open(true, &sender);
                }
            }
            SessionDetailMsg::InspectSubagent(id) => {
                if let Some(session_id) = self.session.as_ref().map(|s| s.id.clone()) {
                    self.inspector.emit(ToolInspectorPaneMsg::SelectSubagent {
                        session_id,
                        subagent_id: id,
                    });
                    self.set_inspector_open(true, &sender);
                }
            }
            SessionDetailMsg::InspectReasoning(transcript_item_index) => {
                if let Some(session_id) = self.session.as_ref().map(|s| s.id.clone()) {
                    self.inspector.emit(ToolInspectorPaneMsg::SelectReasoning {
                        session_id,
                        transcript_item_index,
                    });
                    self.set_inspector_open(true, &sender);
                }
            }
            SessionDetailMsg::ToggleInspector => {
                self.set_inspector_open(!self.inspector_open, &sender);
            }
            SessionDetailMsg::CloseInspector => {
                self.set_inspector_open(false, &sender);
            }
            SessionDetailMsg::InspectorWidgetVisibilityChanged(visible) => {
                if self.inspector_open != visible {
                    self.inspector_open = visible;
                    sender
                        .output(SessionDetailOutput::InspectorVisibilityChanged(visible))
                        .ok();
                }
            }
            SessionDetailMsg::OpenChildSession(child_session_id) => {
                sender
                    .output(SessionDetailOutput::OpenChildSession(child_session_id))
                    .ok();
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        self.apply_transcript_page_result(&sender, message);
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
                    && let Some((header_button, revealer)) =
                        Self::tool_burst_header_and_revealer(&row_widget)
                {
                    if !revealer.reveals_child() {
                        header_button.emit_clicked();
                    }
                    let scroll_child_for_tick = scroll_child.clone();
                    let revealer_for_tick = revealer.clone();
                    let tick_count = std::cell::Cell::new(0u32);
                    revealer.add_tick_callback(move |_, _| {
                        let ticks = tick_count.get() + 1;
                        tick_count.set(ticks);
                        if ticks > 60 {
                            return glib::ControlFlow::Break;
                        }

                        let Some(child_box) = revealer_for_tick
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
    /// Converts database transcript rows into display items, assigning
    /// factory display indexes (starting from `base_display_index`) for
    /// search-match bookkeeping.
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

    fn invalidate_transcript_requests(&mut self) {
        self.transcript_request_id = self.transcript_request_id.wrapping_add(1);
        self.pending_render_batch = None;
        self.last_render_metrics = None;
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

    fn start_first_page_load(&mut self, sender: &ComponentSender<Self>, session_id: &str) {
        self.invalidate_transcript_requests();
        self.loading_first_page = true;
        self.loading_next_page = false;
        self.loaded_count = 0;
        self.has_more_messages = false;
        self.clear_pending_boundary_tool_rows();
        self.clear_messages_safely();

        let request_id = self.transcript_request_id;
        let session_id = session_id.to_string();
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
    }

    fn spawn_transcript_page_load(
        &self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        session_id: String,
        offset: usize,
        limit: usize,
    ) {
        let db_path = self.db_path.clone();
        let preview_len = self.preview_len as i64;

        sender.spawn_oneshot_command(move || {
            let started_at = Instant::now();
            let result = load_transcript_items(
                &db_path,
                &session_id,
                limit as i64,
                offset as i64,
                preview_len,
            )
            .map_err(|err| format!("{err:#}"));
            let load_duration_ms = started_at.elapsed().as_millis();

            SessionDetailCmd::TranscriptPageLoaded {
                request_id,
                session_id,
                offset,
                limit,
                load_duration_ms,
                result,
            }
        });
    }

    fn schedule_transcript_render_batch(&self, sender: &ComponentSender<Self>, request_id: u64) {
        let input_sender = sender.input_sender().clone();
        glib::timeout_add_local_once(Duration::from_millis(RENDER_BATCH_DELAY_MS), move || {
            let _ = input_sender.send(SessionDetailMsg::RenderNextTranscriptBatch { request_id });
        });
    }

    fn prepare_for_navigation_back(&mut self, sender: &ComponentSender<Self>) {
        self.invalidate_transcript_requests();
        self.loading_first_page = false;
        self.loading_next_page = false;
        self.clear_pending_boundary_tool_rows();

        let request_id = self.transcript_request_id;
        let input_sender = sender.input_sender().clone();
        glib::timeout_add_local_once(Duration::from_millis(DEFERRED_CLEAR_DELAY_MS), move || {
            let _ = input_sender.send(SessionDetailMsg::DeferredClear { request_id });
        });
    }

    fn clear_for_navigation_back(&mut self) {
        self.session = None;
        self.clear_messages_safely();
        self.loaded_count = 0;
        self.has_more_messages = false;
        self.search_query = None;
        self.reset_search_matches();
    }

    fn queue_transcript_items_for_render(
        &mut self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        offset: usize,
        source_row_count: usize,
        items: Vec<TranscriptItemInit>,
    ) {
        let total_items = items.len();
        let row_kind_counts = Self::count_render_item_kinds(&items);
        tracing::info!(
            request_id,
            offset,
            source_row_count,
            display_item_count = total_items,
            message_count = row_kind_counts.message_count,
            tool_call_count = row_kind_counts.tool_call_count,
            tool_burst_count = row_kind_counts.tool_burst_count,
            subagent_count = row_kind_counts.subagent_count,
            "Queued transcript render batch"
        );
        self.last_render_metrics = None;
        self.pending_render_batch = Some(PendingRenderBatch {
            request_id,
            offset,
            source_row_count,
            total_items,
            rendered_items: 0,
            batch_count: 0,
            queued_at: Instant::now(),
            last_batch_completed_at: None,
            total_push_duration: Duration::ZERO,
            max_push_duration: Duration::ZERO,
            max_schedule_gap: Duration::ZERO,
            row_kind_counts,
            items: items.into(),
        });
        self.schedule_transcript_render_batch(sender, request_id);
    }

    fn count_render_item_kinds(items: &[TranscriptItemInit]) -> RenderRowKindCounts {
        let mut counts = RenderRowKindCounts::default();
        for item in items {
            match item {
                TranscriptItemInit::Message(_) => counts.message_count += 1,
                TranscriptItemInit::ToolCall(_) => counts.tool_call_count += 1,
                TranscriptItemInit::ToolBurst(_) => counts.tool_burst_count += 1,
                TranscriptItemInit::Subagent(_) => counts.subagent_count += 1,
            }
        }
        counts
    }

    fn render_next_transcript_batch(&mut self, sender: &ComponentSender<Self>, request_id: u64) {
        if request_id != self.transcript_request_id {
            return;
        }

        let Some(batch) = &mut self.pending_render_batch else {
            return;
        };
        if batch.request_id != request_id {
            return;
        }

        let schedule_gap = batch
            .last_batch_completed_at
            .map(|completed_at| completed_at.elapsed())
            .unwrap_or(Duration::ZERO);
        batch.max_schedule_gap = batch.max_schedule_gap.max(schedule_gap);

        let mut guard = self.messages.guard();
        let push_started_at = Instant::now();
        let mut rendered_this_batch = 0usize;
        for _ in 0..RENDER_BATCH_SIZE {
            let Some(item) = batch.items.pop_front() else {
                break;
            };
            guard.push_back(item);
            rendered_this_batch += 1;
        }
        let push_duration = push_started_at.elapsed();
        let has_more_items = !batch.items.is_empty();
        batch.rendered_items += rendered_this_batch;
        batch.batch_count += 1;
        batch.total_push_duration += push_duration;
        batch.max_push_duration = batch.max_push_duration.max(push_duration);
        let remaining_items = batch.items.len();
        let rendered_items = batch.rendered_items;
        let total_items = batch.total_items;
        let batch_count = batch.batch_count;
        let offset = batch.offset;
        let source_row_count = batch.source_row_count;
        let total_push_duration_ms = batch.total_push_duration.as_millis();
        let max_push_duration_ms = batch.max_push_duration.as_millis();
        let max_schedule_gap_ms = batch.max_schedule_gap.as_millis();
        let total_duration_ms = batch.queued_at.elapsed().as_millis();
        let row_kind_counts = batch.row_kind_counts.clone();
        batch.last_batch_completed_at = Some(Instant::now());
        drop(guard);

        if has_more_items {
            tracing::debug!(
                request_id,
                offset,
                rendered_this_batch,
                rendered_items,
                total_items,
                remaining_items,
                push_duration_ms = push_duration.as_millis(),
                schedule_gap_ms = schedule_gap.as_millis(),
                max_schedule_gap_ms,
                "Rendered transcript batch"
            );
            self.schedule_transcript_render_batch(sender, request_id);
        } else {
            tracing::info!(
                request_id,
                offset,
                source_row_count,
                display_item_count = total_items,
                rendered_items,
                batch_count,
                total_push_duration_ms,
                max_push_duration_ms,
                total_duration_ms,
                message_count = row_kind_counts.message_count,
                tool_call_count = row_kind_counts.tool_call_count,
                tool_burst_count = row_kind_counts.tool_burst_count,
                subagent_count = row_kind_counts.subagent_count,
                max_schedule_gap_ms,
                "Finished rendering transcript page"
            );
            self.last_render_metrics = Some(RenderMetrics {
                offset,
                source_row_count,
                display_item_count: total_items,
                batch_count,
                wall_duration_ms: 0,
                total_duration_ms,
                total_push_duration_ms,
                max_push_duration_ms,
                max_schedule_gap_ms,
                message_count: row_kind_counts.message_count,
                tool_call_count: row_kind_counts.tool_call_count,
                tool_burst_count: row_kind_counts.tool_burst_count,
                subagent_count: row_kind_counts.subagent_count,
            });
            self.pending_render_batch = None;
        }
    }

    fn apply_first_page_rows(
        &mut self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        session_id: &str,
        limit: usize,
        rows: Vec<crate::database::TranscriptItemRow>,
    ) {
        self.loading_first_page = false;
        self.loading_next_page = false;
        self.has_more_messages = rows.len() == limit;
        self.loaded_count = rows.len();
        let highlight = self.search_query.clone();
        let db_path = self.db_path.clone();
        self.track_pending_boundary_tool_rows(&rows);
        self.clear_messages_safely();
        let build_started_at = Instant::now();
        let source_row_count = rows.len();
        let items = Self::build_display_items(rows, session_id, highlight, db_path, 0);
        tracing::info!(
            request_id,
            session_id,
            offset = 0usize,
            source_row_count,
            display_item_count = items.len(),
            build_duration_ms = build_started_at.elapsed().as_millis(),
            "Prepared first transcript page"
        );
        self.queue_transcript_items_for_render(sender, request_id, 0, source_row_count, items);
    }

    fn handle_transcript_page_error(&mut self, session_id: &str, offset: usize, err: String) {
        tracing::error!(
            "Failed to load transcript items for {} at offset {}: {}",
            session_id,
            offset,
            err
        );

        if offset == 0 {
            self.clear_messages_safely();
            self.loaded_count = 0;
            self.has_more_messages = false;
            self.clear_pending_boundary_tool_rows();
        }

        self.loading_first_page = false;
        self.loading_next_page = false;
    }

    fn apply_transcript_page_result(
        &mut self,
        sender: &ComponentSender<Self>,
        message: SessionDetailCmd,
    ) {
        let SessionDetailCmd::TranscriptPageLoaded {
            request_id,
            session_id,
            offset,
            limit,
            load_duration_ms,
            result,
        } = message;

        if request_id != self.transcript_request_id {
            tracing::debug!(
                "Ignoring stale transcript page for session {} at offset {}",
                session_id,
                offset
            );
            return;
        }

        let active_session_matches = self
            .session
            .as_ref()
            .is_some_and(|session| session.id == session_id);
        if !active_session_matches {
            tracing::debug!(
                "Ignoring transcript page for inactive session {} at offset {}",
                session_id,
                offset
            );
            return;
        }

        match result {
            Ok(rows) if offset == 0 => {
                tracing::info!(
                    request_id,
                    session_id = session_id.as_str(),
                    offset,
                    limit,
                    source_row_count = rows.len(),
                    load_duration_ms,
                    "Loaded first transcript page"
                );
                self.apply_first_page_rows(sender, request_id, &session_id, limit, rows)
            }
            Ok(rows) => {
                tracing::info!(
                    request_id,
                    session_id = session_id.as_str(),
                    offset,
                    limit,
                    source_row_count = rows.len(),
                    load_duration_ms,
                    "Loaded next transcript page"
                );
                self.apply_next_page_rows(sender, request_id, offset, &session_id, rows)
            }
            Err(err) => self.handle_transcript_page_error(&session_id, offset, err),
        }
    }

    fn reset_search_matches(&mut self) {
        self.match_segments.clear();
        self.current_match = 0;
        self.total_matches = 0;
    }

    fn reload_current_session(&mut self, sender: &ComponentSender<Self>) {
        if let Some(session) = &self.session {
            let session_id = session.id.clone();
            self.start_first_page_load(sender, &session_id);
        }
    }

    fn scroll_to_current_match(&self) {
        let target = Self::find_match_target(&self.match_segments, self.current_match);
        self.scroll_to_item.set(Some(target));
    }

    fn clear_pending_boundary_tool_rows(&mut self) {
        self.pending_boundary_tool_rows.clear();
    }

    fn track_pending_boundary_tool_rows(&mut self, rows: &[crate::database::TranscriptItemRow]) {
        self.pending_boundary_tool_rows = if self.has_more_messages {
            trailing_tool_call_rows(rows)
        } else {
            Vec::new()
        };
    }

    fn regroup_next_page_boundary(
        &mut self,
        rows: Vec<crate::database::TranscriptItemRow>,
    ) -> BoundaryAppendPlan {
        if self.pending_boundary_tool_rows.is_empty() {
            self.track_pending_boundary_tool_rows(&rows);
            return BoundaryAppendPlan {
                replacement_items: Vec::new(),
                rows,
            };
        }

        let regrouped =
            regroup_boundary(std::mem::take(&mut self.pending_boundary_tool_rows), rows);
        let replacement_items = regrouped.replacement_items;
        let rows = regrouped.remaining_rows;

        self.pending_boundary_tool_rows = if !self.has_more_messages {
            Vec::new()
        } else if rows.is_empty() {
            trailing_tool_rows_from_display(&replacement_items)
        } else {
            trailing_tool_call_rows(&rows)
        };

        BoundaryAppendPlan {
            replacement_items,
            rows,
        }
    }

    /// Loads the next transcript page and repairs any tool-burst grouping that
    /// straddles the previous page boundary.
    ///
    /// The last rows of a loaded page may be trailing tool calls that cannot be
    /// grouped correctly until the next page is available. When that happens,
    /// the component replaces the affected tail items before appending the new
    /// page so display rows and their search indexes stay stable.
    fn load_next_page(&mut self, sender: &ComponentSender<Self>) {
        let Some(session) = &self.session else {
            return;
        };

        if self.loading_first_page
            || self.loading_next_page
            || self.pending_render_batch.is_some()
            || !self.has_more_messages
        {
            return;
        }

        let session_id = session.id.clone();
        let offset = self.loaded_count;
        self.loading_next_page = true;
        self.spawn_transcript_page_load(
            sender,
            self.transcript_request_id,
            session_id,
            offset,
            self.page_size,
        );
    }

    fn apply_next_page_rows(
        &mut self,
        sender: &ComponentSender<Self>,
        request_id: u64,
        offset: usize,
        session_id: &str,
        rows: Vec<crate::database::TranscriptItemRow>,
    ) {
        let apply_started_at = Instant::now();
        self.loading_next_page = false;
        let source_len = rows.len();
        self.has_more_messages = source_len == self.page_size;
        self.loaded_count += source_len;

        let highlight = self.search_query.clone();
        let db_path = self.db_path.clone();
        // Boundary regrouping must run before borrowing `self.messages` since it
        // mutates `self.pending_boundary_tool_rows`.
        let BoundaryAppendPlan {
            replacement_items,
            rows,
        } = self.regroup_next_page_boundary(rows);
        let replacement_item_count = replacement_items.len();

        {
            let mut guard = self.messages.guard();

            if !replacement_items.is_empty() {
                let _ = guard.pop_back();
                let start_index = guard.len();
                for item in replacement_items
                    .into_iter()
                    .enumerate()
                    .map(|(offset, item)| {
                        transcript_item_init_from_display_item(
                            start_index + offset,
                            &item,
                            session_id,
                            highlight.clone(),
                            db_path.clone(),
                        )
                    })
                {
                    guard.push_back(item);
                }
            }

            let start_index = guard.len();
            let items =
                Self::build_display_items(rows, session_id, highlight, db_path, start_index);
            tracing::info!(
                request_id,
                session_id,
                offset,
                source_row_count = source_len,
                replacement_item_count,
                display_item_count = items.len(),
                prepare_duration_ms = apply_started_at.elapsed().as_millis(),
                "Prepared next transcript page"
            );
            drop(guard);
            self.queue_transcript_items_for_render(sender, request_id, offset, source_len, items);
        }

        if !self.has_more_messages {
            self.clear_pending_boundary_tool_rows();
        }
    }

    /// Merges per-row match counts reported by child rows into global search
    /// navigation state.
    ///
    /// `display_index` is the current top-level factory position for the row,
    /// not the original database `item_index`.
    fn update_match_segments(&mut self, display_index: usize, segments: Vec<usize>) {
        let was_empty = self.total_matches == 0;
        self.match_segments.insert(display_index, segments);
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

    /// Looks up the `(header_button, revealer)` pair for a tool-burst transcript
    /// row. The row widget is a vertical `gtk::Box` whose first child is the
    /// header toggle button and whose second child is the `gtk::Revealer`
    /// holding the grouped tool call rows.
    fn tool_burst_header_and_revealer(
        row_widget: &gtk::Widget,
    ) -> Option<(gtk::Button, gtk::Revealer)> {
        let header_button = row_widget
            .first_child()
            .and_then(|w| w.downcast::<gtk::Button>().ok())?;
        let revealer = header_button
            .next_sibling()
            .and_then(|w| w.downcast::<gtk::Revealer>().ok())?;
        Some((header_button, revealer))
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

    /// Maps a global match ordinal to the row, and optional burst child,
    /// containing that match.
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
    use std::time::Duration;

    use relm4::{Component, ComponentController};
    use rusqlite::{Connection, params};

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
            pinned_at: None,
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

    fn pump_main_context(condition: impl Fn() -> bool) {
        let context = gtk::glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(1000);
        while std::time::Instant::now() < deadline {
            if condition() {
                return;
            }

            if !context.iteration(false) {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    fn seed_message_transcript(db_path: &std::path::Path, session_id: &str, count: usize) {
        let conn = Connection::open(db_path).expect("open temp db");
        crate::database::schema::initialize_database(&conn).expect("initialize db");

        for index in 0..count {
            conn.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, 'user', ?3, ?4, NULL)",
                params![
                    session_id,
                    index as i64,
                    format!("message {index}"),
                    index as i64,
                ],
            )
            .expect("insert message");
            conn.execute(
                "INSERT INTO transcript_items (session_id, item_index, kind, message_index)
                 VALUES (?1, ?2, 'message', ?2)",
                params![session_id, index as i64],
            )
            .expect("insert transcript item");
        }
    }

    fn seed_tool_burst_transcript(db_path: &std::path::Path, session_id: &str, count: usize) {
        let conn = Connection::open(db_path).expect("open temp db");
        crate::database::schema::initialize_database(&conn).expect("initialize db");

        for index in 0..count {
            let tool_call_id = format!("call-{index}");
            conn.execute(
                "INSERT INTO tool_calls (
                    id, session_id, tool_name, status, summary, input_json, output_text, duration_ms
                 ) VALUES (?1, ?2, ?3, 'completed', ?4, ?5, ?6, ?7)",
                params![
                    tool_call_id,
                    session_id,
                    if index % 2 == 0 { "Read" } else { "Edit" },
                    format!("tool summary {index}"),
                    format!(r#"{{"path":"/tmp/file-{index}.rs"}}"#),
                    format!("tool output line {index}"),
                    index as i64,
                ],
            )
            .expect("insert tool call");
            conn.execute(
                "INSERT INTO transcript_items (session_id, item_index, kind, tool_call_id)
                 VALUES (?1, ?2, 'tool_call', ?3)",
                params![session_id, index as i64, tool_call_id],
            )
            .expect("insert transcript item");
        }
    }

    fn seed_markdown_transcript(db_path: &std::path::Path, session_id: &str, count: usize) {
        let conn = Connection::open(db_path).expect("open temp db");
        crate::database::schema::initialize_database(&conn).expect("initialize db");

        let content = "# Synthetic assistant response\n\n\
This paragraph contains a needle used for search-highlight measurements.\n\n\
```rust\n\
fn synthetic_measurement(input: &str) -> String {\n\
    format!(\"needle {input}\")\n\
}\n\
```\n\n\
| file | status |\n\
| --- | --- |\n\
| src/ui/session_detail.rs | measured |\n\n"
            .repeat(12);

        for index in 0..count {
            conn.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, 'assistant', ?3, ?4, 'synthetic-model')",
                params![session_id, index as i64, content, index as i64],
            )
            .expect("insert message");
            conn.execute(
                "INSERT INTO transcript_items (session_id, item_index, kind, message_index)
                 VALUES (?1, ?2, 'message', ?2)",
                params![session_id, index as i64],
            )
            .expect("insert transcript item");
        }
    }

    fn measure_session_detail_perf_scenario(
        name: &str,
        seed: impl FnOnce(&std::path::Path),
    ) -> RenderMetrics {
        measure_session_detail_perf_scenario_with_query(name, None, seed)
    }

    fn measure_session_detail_perf_scenario_with_query(
        _name: &str,
        search_query: Option<&str>,
        seed: impl FnOnce(&std::path::Path),
    ) -> RenderMetrics {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        seed(temp_db.path());

        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
        let started_at = Instant::now();
        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(None, None, 0, 0, 0)),
            search_query: search_query.map(str::to_string),
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_render_batch.is_none() && parts.model.last_render_metrics.is_some()
        });

        let parts = controller.state().get();
        let mut metrics = parts
            .model
            .last_render_metrics
            .clone()
            .expect("scenario should record render metrics");
        metrics.wall_duration_ms = started_at.elapsed().as_millis();
        metrics
    }

    fn transcript_message_row(
        item_index: i64,
        role: crate::models::Role,
        content: &str,
    ) -> crate::database::TranscriptItemRow {
        crate::database::TranscriptItemRow {
            item_index,
            kind: crate::models::TranscriptItemKind::Message,
            reasoning_preview: crate::models::ReasoningPreview::default(),
            message_index: Some(item_index),
            role: Some(role),
            content_preview: Some(content.to_string()),
            content_len: Some(content.len() as i64),
            timestamp: Some(item_index),
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
        }
    }

    fn transcript_tool_row(item_index: i64, tool_name: &str) -> crate::database::TranscriptItemRow {
        crate::database::TranscriptItemRow {
            item_index,
            kind: crate::models::TranscriptItemKind::ToolCall,
            reasoning_preview: crate::models::ReasoningPreview::default(),
            message_index: None,
            role: None,
            content_preview: None,
            content_len: None,
            timestamp: None,
            model: None,
            tool_call_id: Some(format!("call-{item_index}")),
            tool_name: Some(tool_name.to_string()),
            tool_status: Some(crate::models::ToolCallStatus::Completed),
            tool_summary: Some(format!("{tool_name} summary")),
            tool_input_json: Some("{}".to_string()),
            tool_output_text: None,
            duration_ms: Some(1),
            subagent_id: None,
            subagent_title: None,
            subagent_prompt: None,
        }
    }

    fn transcript_subagent_row(item_index: i64, title: &str) -> crate::database::TranscriptItemRow {
        crate::database::TranscriptItemRow {
            item_index,
            kind: crate::models::TranscriptItemKind::Subagent,
            reasoning_preview: crate::models::ReasoningPreview::default(),
            message_index: None,
            role: None,
            content_preview: None,
            content_len: None,
            timestamp: None,
            model: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
            tool_summary: None,
            tool_input_json: None,
            tool_output_text: None,
            duration_ms: None,
            subagent_id: Some(format!("subagent-{item_index}")),
            subagent_title: Some(title.to_string()),
            subagent_prompt: Some("investigate".to_string()),
        }
    }

    #[test]
    fn build_display_items_groups_two_tool_calls_into_one_tool_burst() {
        let rows = vec![
            crate::database::TranscriptItemRow {
                item_index: 0,
                kind: crate::models::TranscriptItemKind::Message,
                reasoning_preview: crate::models::ReasoningPreview::default(),
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
                reasoning_preview: crate::models::ReasoningPreview::default(),
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
                reasoning_preview: crate::models::ReasoningPreview::default(),
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

    #[gtk::test]
    fn session_detail_defers_initial_transcript_load_after_session_change() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        seed_message_transcript(temp_db.path(), "test-session-123", INITIAL_PAGE_SIZE);

        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(None, None, 0, 0, 0)),
            search_query: None,
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.session.is_some() && parts.model.loading_first_page
        });

        let context = gtk::glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(50);
        while std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(2));
        }

        let parts = controller.state().get();
        assert_eq!(parts.model.loaded_count, 0);
        assert_eq!(parts.model.messages.len(), 0);
        assert!(parts.model.pending_render_batch.is_none());
    }

    #[gtk::test]
    fn session_detail_loads_transcript_pages_incrementally() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        seed_message_transcript(temp_db.path(), "test-session-123", INITIAL_PAGE_SIZE + 5);

        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(None, None, 0, 0, 0)),
            search_query: None,
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            !parts.model.loading_first_page && parts.model.loaded_count == INITIAL_PAGE_SIZE
        });

        {
            let parts = controller.state().get();
            assert_eq!(parts.model.loaded_count, INITIAL_PAGE_SIZE);
            assert!(parts.model.messages.len() < INITIAL_PAGE_SIZE);
            assert!(parts.model.pending_render_batch.is_some());
            assert!(parts.model.has_more_messages);
        }

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_render_batch.is_none()
                && parts.model.messages.len() == INITIAL_PAGE_SIZE
        });

        controller.emit(SessionDetailMsg::LoadMore);

        pump_main_context(|| {
            let parts = controller.state().get();
            !parts.model.loading_next_page && parts.model.loaded_count == INITIAL_PAGE_SIZE + 5
        });

        {
            let parts = controller.state().get();
            assert_eq!(parts.model.loaded_count, INITIAL_PAGE_SIZE + 5);
            assert!(parts.model.messages.len() < INITIAL_PAGE_SIZE + 5);
        }

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_render_batch.is_none()
                && parts.model.messages.len() == INITIAL_PAGE_SIZE + 5
        });

        let parts = controller.state().get();
        assert!(!parts.model.has_more_messages);
    }

    #[gtk::test]
    fn session_detail_records_render_batch_measurements() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        seed_message_transcript(temp_db.path(), "test-session-123", INITIAL_PAGE_SIZE + 5);

        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());
        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(None, None, 0, 0, 0)),
            search_query: None,
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_render_batch.is_none()
                && parts.model.messages.len() == INITIAL_PAGE_SIZE
        });

        let parts = controller.state().get();
        let metrics = parts
            .model
            .last_render_metrics
            .as_ref()
            .expect("first page should record render metrics");
        assert_eq!(metrics.offset, 0);
        assert_eq!(metrics.source_row_count, INITIAL_PAGE_SIZE);
        assert_eq!(metrics.display_item_count, INITIAL_PAGE_SIZE);
        assert_eq!(
            metrics.batch_count,
            INITIAL_PAGE_SIZE.div_ceil(RENDER_BATCH_SIZE)
        );
        assert_eq!(metrics.message_count, INITIAL_PAGE_SIZE);
        assert_eq!(metrics.tool_call_count, 0);
        assert_eq!(metrics.tool_burst_count, 0);
        assert_eq!(metrics.subagent_count, 0);
    }

    #[test]
    fn render_item_kind_counts_capture_heterogeneous_rows() {
        let rows = vec![
            transcript_message_row(0, crate::models::Role::Assistant, "hello"),
            transcript_tool_row(1, "Read"),
            transcript_tool_row(2, "Edit"),
            transcript_subagent_row(3, "Explore"),
        ];

        let items = SessionDetail::build_display_items(
            rows,
            "session-1",
            None,
            Arc::new(PathBuf::from("/tmp/test.db")),
            0,
        );
        let counts = SessionDetail::count_render_item_kinds(&items);

        assert_eq!(counts.message_count, 1);
        assert_eq!(counts.tool_call_count, 0);
        assert_eq!(counts.tool_burst_count, 1);
        assert_eq!(counts.subagent_count, 1);
    }

    #[gtk::test]
    #[ignore = "manual performance smoke test for issue #127"]
    fn session_detail_perf_smoke_measures_synthetic_scenarios() {
        let message_metrics = measure_session_detail_perf_scenario("messages", |path| {
            seed_message_transcript(path, "test-session-123", INITIAL_PAGE_SIZE);
        });
        let tool_burst_metrics = measure_session_detail_perf_scenario("tool-burst", |path| {
            seed_tool_burst_transcript(path, "test-session-123", INITIAL_PAGE_SIZE);
        });
        let markdown_metrics = measure_session_detail_perf_scenario("markdown", |path| {
            seed_markdown_transcript(path, "test-session-123", INITIAL_PAGE_SIZE);
        });
        let markdown_search_metrics = measure_session_detail_perf_scenario_with_query(
            "markdown-search",
            Some("needle"),
            |path| {
                seed_markdown_transcript(path, "test-session-123", INITIAL_PAGE_SIZE);
            },
        );

        println!("messages: {message_metrics:?}");
        println!("tool-burst: {tool_burst_metrics:?}");
        println!("markdown: {markdown_metrics:?}");
        println!("markdown-search: {markdown_search_metrics:?}");

        assert_eq!(message_metrics.source_row_count, INITIAL_PAGE_SIZE);
        assert_eq!(message_metrics.message_count, INITIAL_PAGE_SIZE);
        assert_eq!(tool_burst_metrics.source_row_count, INITIAL_PAGE_SIZE);
        assert_eq!(tool_burst_metrics.display_item_count, 1);
        assert_eq!(tool_burst_metrics.tool_burst_count, 1);
        assert_eq!(markdown_metrics.source_row_count, INITIAL_PAGE_SIZE);
        assert_eq!(markdown_metrics.message_count, INITIAL_PAGE_SIZE);
        assert_eq!(markdown_search_metrics.source_row_count, INITIAL_PAGE_SIZE);
        assert_eq!(markdown_search_metrics.message_count, INITIAL_PAGE_SIZE);
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

    #[test]
    fn inspector_visibility_output_carries_state() {
        let output = SessionDetailOutput::InspectorVisibilityChanged(true);
        assert!(matches!(
            output,
            SessionDetailOutput::InspectorVisibilityChanged(true)
        ));
    }

    #[gtk::test]
    fn inspect_tool_call_opens_inspector_when_session_active() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(None, None, 0, 0, 0)),
            search_query: None,
        });
        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.session.is_some()
        });

        controller.emit(SessionDetailMsg::InspectToolCall("call-123".to_string()));
        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.inspector_open
        });

        let parts = controller.state().get();
        assert!(parts.model.inspector_open);
    }

    #[gtk::test]
    fn close_inspector_resets_inspector_open() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionDetail::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionDetailMsg::SetSession {
            session: Box::new(build_test_session(None, None, 0, 0, 0)),
            search_query: None,
        });
        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.session.is_some()
        });

        controller.emit(SessionDetailMsg::InspectToolCall("call-1".to_string()));
        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.inspector_open
        });

        controller.emit(SessionDetailMsg::CloseInspector);
        pump_main_context(|| {
            let parts = controller.state().get();
            !parts.model.inspector_open
        });

        let parts = controller.state().get();
        assert!(!parts.model.inspector_open);
    }

    #[test]
    fn next_match_after_top_level_row_targets_burst_child() {
        let mut segments = BTreeMap::new();
        segments.insert(0, vec![1]);
        segments.insert(1, vec![0, 2]);

        assert_eq!(
            SessionDetail::find_match_target(&segments, 1),
            ScrollTarget {
                display_index: 1,
                child_index: Some(1),
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
