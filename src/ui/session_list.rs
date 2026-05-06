use adw::prelude::*;
use gtk::glib;
use relm4::factory::FactoryVecDeque;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};
use std::{
    collections::VecDeque,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::database::{
    load_session_by_id_for_filter, load_sessions_for_filter, search_sessions_for_filter,
};
use crate::models::{AiAssistant, PerSourceResult, ProjectFilter, Session, SourceStatus};
use crate::ui::session_row::{SessionRow, SessionRowInit, SessionRowOutput};

#[derive(Debug)]
pub struct SessionList {
    db_path: PathBuf,
    active_tools: Vec<AiAssistant>,
    project_filter: ProjectFilter,
    search_query: String,
    all_tools_selected: bool,
    indexing: bool,
    sessions: FactoryVecDeque<SessionRow>,
    source_results: Vec<PerSourceResult>,
    source_results_available: bool,
    active_post_indexing_measurement: Option<ActivePostIndexingMeasurement>,
    pending_post_indexing_batch: Option<PendingPostIndexingBatch>,
    selection_signal_handler: Option<glib::SignalHandlerId>,
}

#[derive(Debug)]
pub enum SessionListMsg {
    SetFilters {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
    SetSearchQuery(String),
    Reload,
    ReloadAfterIndexing {
        assistants: Vec<AiAssistant>,
        project_filter: ProjectFilter,
        context: IndexingReloadContext,
    },
    PostIndexingReloadIdleMeasured {
        token: MeasurementToken,
        delay_ms: u128,
    },
    PostIndexingReloadFrameMeasured {
        token: MeasurementToken,
        delay_ms: u128,
    },
    PostIndexingReloadBatch {
        token: MeasurementToken,
    },
    PostIndexingSelectionChanged,
    SetIndexing(bool),
    SetSourceResults(Vec<PerSourceResult>),
    SessionActivated(i32),
    TogglePinRequested(String),
    ResumeRequested(String, AiAssistant),
    RequestSelectedSessionForPin,
    /// Ensure a row is selected (defaults to first) and grab keyboard focus.
    RestoreFocus,
    /// Move selection by delta rows (−1 = up, +1 = down) without changing focus.
    MoveSelection(i32),
    /// Activate the currently selected session (Enter during search).
    ActivateSelected,
}

#[derive(Debug)]
pub enum SessionListOutput {
    SessionSelected(String),
    TogglePinRequested(String),
    SelectedSessionForPin(String),
    ResumeRequested(String, AiAssistant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexingReloadContext {
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
    pub pending_reindex_feedback: bool,
    pub errors_present: bool,
}

#[derive(Clone)]
pub struct MeasurementToken {
    valid: Arc<AtomicBool>,
}

impl MeasurementToken {
    fn new() -> Self {
        Self {
            valid: Arc::new(AtomicBool::new(true)),
        }
    }

    fn invalidate(&self) {
        self.valid.store(false, Ordering::Relaxed);
    }

    fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Relaxed)
    }

    fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.valid, &other.valid)
    }
}

impl fmt::Debug for MeasurementToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeasurementToken")
            .field("valid", &self.valid.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallbackTiming {
    Pending,
    Available(u128),
    Unavailable,
}

impl CallbackTiming {
    fn is_pending(self) -> bool {
        matches!(self, CallbackTiming::Pending)
    }

    fn as_option(self) -> Option<u128> {
        match self {
            CallbackTiming::Available(ms) => Some(ms),
            CallbackTiming::Pending | CallbackTiming::Unavailable => None,
        }
    }

    fn is_unavailable(self) -> bool {
        matches!(self, CallbackTiming::Unavailable)
    }
}

#[derive(Debug)]
struct ActivePostIndexingMeasurement {
    token: MeasurementToken,
    started_at: Instant,
    context: IndexingReloadContext,
    assistant_filters: Vec<AiAssistant>,
    project_filter: ProjectFilter,
    search_query_present: bool,
    search_query_len: usize,
    previously_selected_id_present: bool,
    fetch_duration: Duration,
    clear_duration: Duration,
    push_duration: Duration,
    row_count: usize,
    batch_count: usize,
    batch_size: usize,
    max_batch_push_duration: Duration,
    selection_restore_attempted: bool,
    selection_restore_succeeded: bool,
    ensure_selection_fallback_ran: bool,
    user_selection_changed_during_batch: bool,
    next_idle_delay: CallbackTiming,
    next_frame_delay: CallbackTiming,
}

impl ActivePostIndexingMeasurement {
    fn new(
        context: IndexingReloadContext,
        assistant_filters: &[AiAssistant],
        project_filter: &ProjectFilter,
        search_query: &str,
    ) -> Self {
        Self {
            token: MeasurementToken::new(),
            started_at: Instant::now(),
            context,
            assistant_filters: assistant_filters.to_vec(),
            project_filter: project_filter.clone(),
            search_query_present: !search_query.trim().is_empty(),
            search_query_len: search_query.len(),
            previously_selected_id_present: false,
            fetch_duration: Duration::ZERO,
            clear_duration: Duration::ZERO,
            push_duration: Duration::ZERO,
            row_count: 0,
            batch_count: 0,
            batch_size: POST_INDEXING_RELOAD_BATCH_SIZE,
            max_batch_push_duration: Duration::ZERO,
            selection_restore_attempted: false,
            selection_restore_succeeded: false,
            ensure_selection_fallback_ran: false,
            user_selection_changed_during_batch: false,
            next_idle_delay: CallbackTiming::Pending,
            next_frame_delay: CallbackTiming::Pending,
        }
    }

    fn record_sync_phases(
        &mut self,
        previously_selected_id_present: bool,
        fetch_duration: Duration,
        clear_duration: Duration,
        push_duration: Duration,
        row_count: usize,
    ) {
        self.previously_selected_id_present = previously_selected_id_present;
        self.fetch_duration = fetch_duration;
        self.clear_duration = clear_duration;
        self.push_duration = push_duration;
        self.row_count = row_count;
    }

    fn record_selection(&mut self, attempted: bool, succeeded: bool, fallback_ran: bool) {
        self.selection_restore_attempted = attempted;
        self.selection_restore_succeeded = succeeded;
        self.ensure_selection_fallback_ran = fallback_ran;
    }

    fn record_batch_push(&mut self, duration: Duration) {
        self.batch_count += 1;
        self.push_duration += duration;
        self.max_batch_push_duration = self.max_batch_push_duration.max(duration);
    }
}

const POST_INDEXING_RELOAD_BATCH_SIZE: usize = 64;

#[derive(Debug)]
struct PendingPostIndexingBatch {
    token: MeasurementToken,
    remaining_sessions: VecDeque<Session>,
    previously_selected_id: Option<String>,
    user_selection_changed: bool,
    had_focus_before_reload: bool,
}

impl PendingPostIndexingBatch {
    fn new(
        token: MeasurementToken,
        sessions: Vec<Session>,
        previously_selected_id: Option<String>,
    ) -> Self {
        Self {
            token,
            remaining_sessions: sessions.into(),
            previously_selected_id,
            user_selection_changed: false,
            had_focus_before_reload: false,
        }
    }

    fn with_focus_state(mut self, had_focus_before_reload: bool) -> Self {
        self.had_focus_before_reload = had_focus_before_reload;
        self
    }

    fn had_focus_before_reload(&self) -> bool {
        self.had_focus_before_reload
    }

    fn token_matches(&self, token: &MeasurementToken) -> bool {
        self.token.is_valid() && self.token.same_identity(token)
    }

    fn invalidate(&self) {
        self.token.invalidate();
    }

    fn take_next_rows(&mut self, batch_size: usize) -> Vec<Session> {
        let take_count = batch_size.min(self.remaining_sessions.len());
        self.remaining_sessions.drain(..take_count).collect()
    }

    fn is_exhausted(&self) -> bool {
        self.remaining_sessions.is_empty()
    }

    #[cfg(test)]
    fn remaining_row_count(&self) -> usize {
        self.remaining_sessions.len()
    }

    fn previously_selected_id(&self) -> Option<&str> {
        self.previously_selected_id.as_deref()
    }

    fn user_selection_changed(&self) -> bool {
        self.user_selection_changed
    }

    fn mark_user_selection_changed(&mut self) {
        self.user_selection_changed = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmptyStateViewModel {
    title: &'static str,
    description: &'static str,
    show_source_results: bool,
}

fn parse_session_id_query(query: &str) -> Option<&str> {
    query.trim().strip_prefix("id:").map(str::trim)
}

fn focus_is_within(widget: &impl IsA<gtk::Widget>) -> bool {
    let widget_ref = widget.upcast_ref::<gtk::Widget>();
    let Some(root) = widget_ref.root() else {
        return false;
    };
    let Some(focused) = root.focus() else {
        return false;
    };
    focused.eq(widget_ref) || focused.is_ancestor(widget_ref)
}

fn compute_empty_state(
    sessions_empty: bool,
    search_query: &str,
    all_tools_selected: bool,
    indexing: bool,
    project_filter_active: bool,
    source_results_available: bool,
    project_filter: &ProjectFilter,
) -> EmptyStateViewModel {
    if sessions_empty && indexing {
        return EmptyStateViewModel {
            title: "Indexing sessions...",
            description: "This may take a moment on first launch.",
            show_source_results: false,
        };
    }

    let session_id_lookup = parse_session_id_query(search_query).is_some();
    let has_search = !search_query.trim().is_empty();
    let pinned_selected = *project_filter == ProjectFilter::Pinned;

    if session_id_lookup {
        return EmptyStateViewModel {
            title: "No session found with this ID",
            description: "Try a different session ID or adjust filters",
            show_source_results: false,
        };
    }

    if has_search && pinned_selected {
        return EmptyStateViewModel {
            title: "No pinned sessions match search",
            description: "Try a different query or clear the pinned filter",
            show_source_results: false,
        };
    }

    if has_search {
        return EmptyStateViewModel {
            title: "No sessions match search",
            description: "Try a different query or adjust filters",
            show_source_results: false,
        };
    }

    if pinned_selected {
        return EmptyStateViewModel {
            title: "No pinned sessions",
            description: "Pin sessions from the list to keep them easy to revisit",
            show_source_results: false,
        };
    }

    if all_tools_selected && !project_filter_active {
        let description = if source_results_available {
            "No sessions found in checked session sources"
        } else {
            "Your AI coding sessions will appear here"
        };
        EmptyStateViewModel {
            title: "No Sessions Yet",
            description,
            show_source_results: source_results_available,
        }
    } else {
        EmptyStateViewModel {
            title: "No sessions match filters",
            description: "Try adjusting the tool filters in the sidebar",
            show_source_results: false,
        }
    }
}

fn build_source_results_list(results: &[PerSourceResult]) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");

    for result in results {
        let title = match result.assistant {
            AiAssistant::ClaudeCode => "Claude Code",
            AiAssistant::OpenCode => "OpenCode",
            AiAssistant::Codex => "Codex",
            AiAssistant::MistralVibe => "Mistral Vibe",
        };

        let subtitle = if result.status == SourceStatus::NotFound {
            format!("{} (not found)", result.display_path)
        } else {
            result.display_path.clone()
        };

        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(&subtitle)
            .build();
        row.set_subtitle_selectable(true);

        list.append(&row);
    }

    list
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

                    #[local_ref]
                    session_list_box -> gtk::ListBox {
                        add_css_class: "boxed-list",
                        set_selection_mode: gtk::SelectionMode::Single,
                    }
                }
            }
        }
    }

    fn init(
        db_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let active_tools = vec![
            AiAssistant::ClaudeCode,
            AiAssistant::OpenCode,
            AiAssistant::Codex,
            AiAssistant::MistralVibe,
        ];
        let search_query = String::new();
        let project_filter = ProjectFilter::AllSessions;
        let fetched = Self::fetch_sessions(&db_path, &active_tools, &project_filter, &search_query);

        let sessions: FactoryVecDeque<SessionRow> = FactoryVecDeque::builder()
            .launch_default()
            .forward(sender.input_sender(), |msg| match msg {
                SessionRowOutput::TogglePinRequested(id) => SessionListMsg::TogglePinRequested(id),
                SessionRowOutput::ResumeRequested(id, tool) => {
                    SessionListMsg::ResumeRequested(id, tool)
                }
            });

        let mut model = Self {
            db_path,
            active_tools,
            project_filter,
            search_query,
            all_tools_selected: true,
            indexing: false,
            sessions,
            source_results: vec![],
            source_results_available: false,
            active_post_indexing_measurement: None,
            pending_post_indexing_batch: None,
            selection_signal_handler: None,
        };

        // Populate initial data
        {
            let mut guard = model.sessions.guard();
            for session in fetched {
                guard.push_back(SessionRowInit { session });
            }
        }

        let session_list_box = model.sessions.widget();
        let widgets = view_output!();

        let input_sender = sender.input_sender().clone();
        session_list_box.connect_row_activated(move |_, row| {
            let _ = input_sender.send(SessionListMsg::SessionActivated(row.index()));
        });

        let selection_sender = sender.input_sender().clone();
        let selection_signal_handler = session_list_box.connect_selected_rows_changed(move |_| {
            let _ = selection_sender.send(SessionListMsg::PostIndexingSelectionChanged);
        });
        model.selection_signal_handler = Some(selection_signal_handler);

        if model.sessions.is_empty() {
            widgets
                .content_stack
                .set_visible_child(&widgets.empty_state);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.session_list_scroller);
        }

        sender.input(SessionListMsg::RestoreFocus);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SessionListMsg::SetFilters {
                tools,
                project_filter,
            } => {
                if !Self::filters_changed(
                    &self.active_tools,
                    &self.project_filter,
                    &tools,
                    &project_filter,
                ) {
                    return;
                }

                self.active_tools = tools.clone();
                self.project_filter = project_filter;
                self.all_tools_selected = tools.len() == AiAssistant::ALL.len();
                self.reload_sessions(&sender);
            }
            SessionListMsg::SetSearchQuery(query) => {
                if !Self::search_query_changed(&self.search_query, &query) {
                    return;
                }

                self.search_query = query;
                self.reload_sessions(&sender);
            }
            SessionListMsg::Reload => {
                self.reload_sessions(&sender);
            }
            SessionListMsg::ReloadAfterIndexing {
                assistants,
                project_filter,
                context,
            } => {
                self.active_tools = assistants;
                self.all_tools_selected = self.active_tools.len() == AiAssistant::ALL.len();
                self.project_filter = project_filter;
                self.start_post_indexing_measurement(context);
                self.reload_sessions_after_indexing(&sender);
            }
            SessionListMsg::PostIndexingReloadIdleMeasured { token, delay_ms } => {
                self.record_post_indexing_idle(token, delay_ms);
            }
            SessionListMsg::PostIndexingReloadFrameMeasured { token, delay_ms } => {
                self.record_post_indexing_frame(token, delay_ms);
            }
            SessionListMsg::PostIndexingReloadBatch { token } => {
                self.run_post_indexing_batch(&sender, token);
            }
            SessionListMsg::PostIndexingSelectionChanged => {
                self.mark_post_indexing_selection_changed();
            }
            SessionListMsg::SetIndexing(indexing) => {
                self.indexing = indexing;
            }
            SessionListMsg::SetSourceResults(results) => {
                self.source_results_available = !results.is_empty();
                self.source_results = results;
            }
            SessionListMsg::SessionActivated(index) => {
                if let Some(row) = self.sessions.get(index as usize) {
                    let _ = sender.output(SessionListOutput::SessionSelected(
                        row.session_id().to_owned(),
                    ));
                }
            }
            SessionListMsg::TogglePinRequested(id) => {
                let _ = sender.output(SessionListOutput::TogglePinRequested(id));
            }
            SessionListMsg::ResumeRequested(id, tool) => {
                let _ = sender.output(SessionListOutput::ResumeRequested(id, tool));
            }
            SessionListMsg::RequestSelectedSessionForPin => {
                let list_box = self.sessions.widget();
                if let Some(row) = list_box.selected_row()
                    && let Some(session_row) = self.sessions.get(row.index() as usize)
                {
                    let _ = sender.output(SessionListOutput::SelectedSessionForPin(
                        session_row.session_id().to_owned(),
                    ));
                }
            }
            SessionListMsg::RestoreFocus => {
                self.ensure_selection();
                let list_box = self.sessions.widget();
                if let Some(row) = list_box.selected_row() {
                    row.grab_focus();
                }
            }
            SessionListMsg::MoveSelection(delta) => {
                let list_box = self.sessions.widget();
                let row_count = self.sessions.len() as i32;
                if row_count == 0 {
                    return;
                }
                let current_index = list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
                let next_index = if current_index < 0 {
                    if delta < 0 { row_count - 1 } else { 0 }
                } else {
                    (current_index + delta).clamp(0, row_count - 1)
                };
                if let Some(row) = list_box.row_at_index(next_index) {
                    list_box.select_row(Some(&row));
                    Self::scroll_row_into_view(&row, list_box);
                }
            }
            SessionListMsg::ActivateSelected => {
                let list_box = self.sessions.widget();
                if let Some(row) = list_box.selected_row()
                    && let Some(session_row) = self.sessions.get(row.index() as usize)
                {
                    let _ = sender.output(SessionListOutput::SessionSelected(
                        session_row.session_id().to_owned(),
                    ));
                }
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        if self.sessions.is_empty() {
            let empty = compute_empty_state(
                true,
                &self.search_query,
                self.all_tools_selected,
                self.indexing,
                self.project_filter != ProjectFilter::AllSessions,
                self.source_results_available,
                &self.project_filter,
            );
            widgets.empty_state.set_title(empty.title);
            widgets.empty_state.set_description(Some(empty.description));
            if empty.show_source_results {
                let list = build_source_results_list(&self.source_results);
                widgets.empty_state.set_child(Some(&list));
            } else {
                widgets.empty_state.set_child(gtk::Widget::NONE);
            }
            widgets
                .content_stack
                .set_visible_child(&widgets.empty_state);
        } else {
            widgets
                .content_stack
                .set_visible_child(&widgets.session_list_scroller);
        }
    }
}

impl SessionList {
    fn filters_changed(
        current_tools: &[AiAssistant],
        current_project_filter: &ProjectFilter,
        next_tools: &[AiAssistant],
        next_project_filter: &ProjectFilter,
    ) -> bool {
        current_tools != next_tools || current_project_filter != next_project_filter
    }

    fn search_query_changed(current_query: &str, next_query: &str) -> bool {
        current_query != next_query
    }

    /// Scroll the ancestor `ScrolledWindow` so that `row` is fully visible.
    fn scroll_row_into_view(row: &gtk::ListBoxRow, list_box: &gtk::ListBox) {
        let Some(sw) = list_box
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_then(|w| w.downcast::<gtk::ScrolledWindow>().ok())
        else {
            return;
        };
        let adj = sw.vadjustment();
        let src = gtk::graphene::Point::new(0.0, 0.0);
        let Some(dst) = row.compute_point(list_box, &src) else {
            return;
        };
        let y = dst.y() as f64;
        let row_height = row.height() as f64;
        let visible_start = adj.value();
        let visible_end = visible_start + adj.page_size();
        if y < visible_start {
            adj.set_value(y);
        } else if y + row_height > visible_end {
            adj.set_value(y + row_height - adj.page_size());
        }
    }

    fn fetch_sessions(
        db_path: &Path,
        tools: &[AiAssistant],
        project_filter: &ProjectFilter,
        query: &str,
    ) -> Vec<Session> {
        let query = query.trim();
        let sessions = if query.is_empty() {
            load_sessions_for_filter(db_path, tools, project_filter)
        } else if let Some(session_id) = parse_session_id_query(query) {
            load_session_by_id_for_filter(db_path, tools, project_filter, session_id)
        } else {
            search_sessions_for_filter(db_path, tools, project_filter, query)
        };

        match sessions {
            Ok(sessions) => sessions,
            Err(err) => {
                tracing::error!("Failed to load sessions: {}", err);
                Vec::new()
            }
        }
    }

    fn ensure_selection(&self) {
        let list_box = self.sessions.widget();
        if list_box.selected_row().is_none() {
            list_box.select_row(list_box.row_at_index(0).as_ref());
        }
    }

    fn selected_session_id(&self) -> Option<String> {
        let list_box = self.sessions.widget();
        let row = list_box.selected_row()?;
        let session_row = self.sessions.get(row.index() as usize)?;
        Some(session_row.session_id().to_string())
    }

    fn select_session_by_id(&self, session_id: &str) -> bool {
        let list_box = self.sessions.widget();
        for index in 0..self.sessions.len() {
            if let Some(session_row) = self.sessions.get(index)
                && session_row.session_id() == session_id
                && let Some(row) = list_box.row_at_index(index as i32)
            {
                list_box.select_row(Some(&row));
                return true;
            }
        }

        false
    }

    fn start_post_indexing_measurement(&mut self, context: IndexingReloadContext) {
        self.cancel_post_indexing_batch();

        self.active_post_indexing_measurement = Some(ActivePostIndexingMeasurement::new(
            context,
            &self.active_tools,
            &self.project_filter,
            &self.search_query,
        ));
    }

    fn cancel_post_indexing_batch(&mut self) {
        if let Some(batch) = self.pending_post_indexing_batch.take() {
            batch.invalidate();
        }

        if let Some(active) = self.active_post_indexing_measurement.take() {
            active.token.invalidate();
        }
    }

    fn mark_post_indexing_selection_changed(&mut self) {
        if let Some(batch) = self.pending_post_indexing_batch.as_mut() {
            batch.mark_user_selection_changed();
            if let Some(active) = self.active_post_indexing_measurement.as_mut() {
                active.user_selection_changed_during_batch = true;
            }
        }
    }

    fn finalize_post_indexing_batch(&mut self, batch: PendingPostIndexingBatch) {
        let mut selection_restore_attempted = false;
        let mut selection_restore_succeeded = false;
        let mut ensure_selection_fallback_ran = false;

        if !batch.user_selection_changed() {
            if let Some(session_id) = batch.previously_selected_id() {
                selection_restore_attempted = true;
                selection_restore_succeeded = self.select_session_by_id(session_id);
                if !selection_restore_succeeded {
                    ensure_selection_fallback_ran = true;
                    self.ensure_selection();
                }
            } else {
                ensure_selection_fallback_ran = true;
                self.ensure_selection();
            }

            if batch.had_focus_before_reload()
                && let Some(row) = self.sessions.widget().selected_row()
            {
                row.grab_focus();
            }
        }

        if let Some(active) = self.active_post_indexing_measurement.as_mut() {
            active.record_selection(
                selection_restore_attempted,
                selection_restore_succeeded,
                ensure_selection_fallback_ran,
            );
            active.user_selection_changed_during_batch = batch.user_selection_changed();
        }

        self.maybe_emit_post_indexing_measurement();
    }

    fn run_post_indexing_batch(&mut self, sender: &ComponentSender<Self>, token: MeasurementToken) {
        let Some(batch) = self.pending_post_indexing_batch.as_mut() else {
            return;
        };

        if !batch.token_matches(&token) {
            return;
        }

        let rows = batch.take_next_rows(POST_INDEXING_RELOAD_BATCH_SIZE);
        let batch_push_started_at = Instant::now();
        {
            let mut guard = self.sessions.guard();
            for session in rows {
                guard.push_back(SessionRowInit { session });
            }
        }
        let batch_push_duration = batch_push_started_at.elapsed();

        if let Some(active) = self.current_post_indexing_measurement_mut(&token) {
            active.record_batch_push(batch_push_duration);
        }

        let exhausted = self
            .pending_post_indexing_batch
            .as_ref()
            .is_some_and(PendingPostIndexingBatch::is_exhausted);

        if exhausted {
            if let Some(batch) = self.pending_post_indexing_batch.take() {
                self.finalize_post_indexing_batch(batch);
            }
        } else {
            self.schedule_post_indexing_batch(sender, token);
        }
    }

    fn current_post_indexing_measurement_mut(
        &mut self,
        token: &MeasurementToken,
    ) -> Option<&mut ActivePostIndexingMeasurement> {
        self.active_post_indexing_measurement
            .as_mut()
            .filter(|active| active.token.is_valid() && active.token.same_identity(token))
    }

    fn schedule_post_indexing_callbacks(
        &mut self,
        sender: &ComponentSender<Self>,
        token: MeasurementToken,
        after_drop_at: Instant,
    ) {
        let idle_sender = sender.input_sender().clone();
        let idle_token = token.clone();
        glib::idle_add_local_once(move || {
            if idle_token.is_valid() {
                let _ = idle_sender.send(SessionListMsg::PostIndexingReloadIdleMeasured {
                    token: idle_token,
                    delay_ms: after_drop_at.elapsed().as_millis(),
                });
            }
        });

        let list_widget = self.sessions.widget().clone();
        if list_widget.root().is_none() {
            self.mark_post_indexing_frame_unavailable(&token);
            return;
        }

        let frame_sender = sender.input_sender().clone();
        let frame_token = token;
        list_widget.add_tick_callback(move |_, _| {
            if frame_token.is_valid() {
                let _ = frame_sender.send(SessionListMsg::PostIndexingReloadFrameMeasured {
                    token: frame_token.clone(),
                    delay_ms: after_drop_at.elapsed().as_millis(),
                });
            }
            glib::ControlFlow::Break
        });
    }

    fn schedule_post_indexing_batch(
        &self,
        sender: &ComponentSender<Self>,
        token: MeasurementToken,
    ) {
        let batch_sender = sender.input_sender().clone();
        glib::idle_add_local_once(move || {
            if token.is_valid() {
                let _ = batch_sender.send(SessionListMsg::PostIndexingReloadBatch { token });
            }
        });
    }

    fn record_post_indexing_idle(&mut self, token: MeasurementToken, delay_ms: u128) {
        if let Some(active) = self.current_post_indexing_measurement_mut(&token) {
            active.next_idle_delay = CallbackTiming::Available(delay_ms);
        }
        self.maybe_emit_post_indexing_measurement();
    }

    fn record_post_indexing_frame(&mut self, token: MeasurementToken, delay_ms: u128) {
        if let Some(active) = self.current_post_indexing_measurement_mut(&token) {
            active.next_frame_delay = CallbackTiming::Available(delay_ms);
        }
        self.maybe_emit_post_indexing_measurement();
    }

    fn mark_post_indexing_frame_unavailable(&mut self, token: &MeasurementToken) {
        if let Some(active) = self.current_post_indexing_measurement_mut(token) {
            active.next_frame_delay = CallbackTiming::Unavailable;
        }
    }

    fn maybe_emit_post_indexing_measurement(&mut self) {
        let Some(active) = &self.active_post_indexing_measurement else {
            return;
        };

        if active.next_idle_delay.is_pending()
            || active.next_frame_delay.is_pending()
            || self.pending_post_indexing_batch.is_some()
        {
            return;
        }

        tracing::info!(
            reason = "post_indexing_completion",
            indexed = active.context.indexed,
            skipped = active.context.skipped,
            removed = active.context.removed,
            pending_reindex_feedback = active.context.pending_reindex_feedback,
            errors_present = active.context.errors_present,
            assistant_filter_count = active.assistant_filters.len(),
            assistant_filters = ?active.assistant_filters,
            all_assistants_selected = active.assistant_filters.len() == AiAssistant::ALL.len(),
            project_filter = ?active.project_filter,
            project_filter_active = active.project_filter != ProjectFilter::AllSessions,
            search_query_present = active.search_query_present,
            search_query_len = active.search_query_len,
            previously_selected_id_present = active.previously_selected_id_present,
            fetch_ms = active.fetch_duration.as_millis(),
            clear_ms = active.clear_duration.as_millis(),
            push_ms = active.push_duration.as_millis(),
            row_count = active.row_count,
            batch_count = active.batch_count,
            batch_size = active.batch_size,
            max_batch_push_ms = active.max_batch_push_duration.as_millis(),
            total_batch_push_ms = active.push_duration.as_millis(),
            selection_restore_attempted = active.selection_restore_attempted,
            selection_restore_succeeded = active.selection_restore_succeeded,
            ensure_selection_fallback_ran = active.ensure_selection_fallback_ran,
            user_selection_changed_during_batch = active.user_selection_changed_during_batch,
            next_idle_delay_ms = active.next_idle_delay.as_option(),
            next_idle_delay_unavailable = active.next_idle_delay.is_unavailable(),
            next_frame_delay_ms = active.next_frame_delay.as_option(),
            next_frame_delay_unavailable = active.next_frame_delay.is_unavailable(),
            total_reload_ms = active.started_at.elapsed().as_millis(),
            "sessionlist.post_indexing_reload.measured"
        );

        active.token.invalidate();
        self.active_post_indexing_measurement = None;
    }

    fn reload_sessions_after_indexing(&mut self, sender: &ComponentSender<Self>) {
        let previously_selected_id = self.selected_session_id();

        let fetch_started_at = Instant::now();
        let fetched = Self::fetch_sessions(
            &self.db_path,
            &self.active_tools,
            &self.project_filter,
            &self.search_query,
        );
        let fetch_duration = fetch_started_at.elapsed();
        let row_count = fetched.len();

        let list_box = self.sessions.widget().clone();
        let had_focus_before_reload = focus_is_within(&list_box);
        let blocked_handler = self.selection_signal_handler.as_ref();
        if let Some(handler) = blocked_handler {
            list_box.block_signal(handler);
        }
        let mut guard = self.sessions.guard();
        let clear_started_at = Instant::now();
        guard.clear();
        let clear_duration = clear_started_at.elapsed();
        drop(guard);
        if let Some(handler) = blocked_handler {
            list_box.unblock_signal(handler);
        }

        let Some(token) = self
            .active_post_indexing_measurement
            .as_ref()
            .map(|active| active.token.clone())
        else {
            return;
        };

        if let Some(active) = self.active_post_indexing_measurement.as_mut() {
            active.record_sync_phases(
                previously_selected_id.is_some(),
                fetch_duration,
                clear_duration,
                Duration::ZERO,
                row_count,
            );
        }

        self.pending_post_indexing_batch = Some(
            PendingPostIndexingBatch::new(token.clone(), fetched, previously_selected_id)
                .with_focus_state(had_focus_before_reload),
        );

        let after_setup_at = Instant::now();
        self.schedule_post_indexing_callbacks(sender, token.clone(), after_setup_at);
        self.schedule_post_indexing_batch(sender, token);
    }

    fn reload_sessions(&mut self, sender: &ComponentSender<Self>) {
        self.cancel_post_indexing_batch();

        let previously_selected_id = self.selected_session_id();

        let fetch_started_at = Instant::now();
        let fetched = Self::fetch_sessions(
            &self.db_path,
            &self.active_tools,
            &self.project_filter,
            &self.search_query,
        );
        let fetch_duration = fetch_started_at.elapsed();
        let row_count = fetched.len();

        let mut guard = self.sessions.guard();

        let clear_started_at = Instant::now();
        guard.clear();
        let clear_duration = clear_started_at.elapsed();

        let push_started_at = Instant::now();
        for session in fetched {
            guard.push_back(SessionRowInit { session });
        }
        let push_duration = push_started_at.elapsed();
        drop(guard);

        let maybe_token = self
            .active_post_indexing_measurement
            .as_ref()
            .map(|active| active.token.clone());

        if let Some(active) = self.active_post_indexing_measurement.as_mut() {
            active.record_sync_phases(
                previously_selected_id.is_some(),
                fetch_duration,
                clear_duration,
                push_duration,
                row_count,
            );
        }

        if let Some(token) = maybe_token {
            self.schedule_post_indexing_callbacks(sender, token, Instant::now());
        }

        let mut selection_restore_attempted = false;
        let mut selection_restore_succeeded = false;
        let mut ensure_selection_fallback_ran = false;

        if let Some(session_id) = previously_selected_id {
            selection_restore_attempted = true;
            selection_restore_succeeded = self.select_session_by_id(&session_id);
            if !selection_restore_succeeded {
                ensure_selection_fallback_ran = true;
                self.ensure_selection();
            }
        } else {
            ensure_selection_fallback_ran = true;
            self.ensure_selection();
        }

        if let Some(active) = self.active_post_indexing_measurement.as_mut() {
            active.record_selection(
                selection_restore_attempted,
                selection_restore_succeeded,
                ensure_selection_fallback_ran,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::schema::initialize_database;
    use crate::models::ProjectFilter;
    use gtk::glib::prelude::ObjectExt;
    use relm4::Component;
    use relm4::ComponentController;
    use rusqlite::Connection;
    use std::{
        cell::RefCell,
        path::PathBuf,
        rc::Rc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct TempDatabase {
        path: PathBuf,
        connection: Connection,
    }

    impl TempDatabase {
        fn new() -> Self {
            let mut path = std::env::temp_dir();
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            path.push(format!(
                "sessions-chronicle-session-list-test-{}-{}.db",
                std::process::id(),
                nanos
            ));

            let connection = Connection::open(&path).expect("Failed to open temp database");
            initialize_database(&connection).expect("Failed to initialize database");

            Self { path, connection }
        }

        fn seed_project_sidebar_fixture(&self) {
            self.connection
                .execute(
                    "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                    rusqlite::params![1_i64, "/projects/alpha", "alpha"],
                )
                .expect("Failed to insert project alpha");

            self.connection
                .execute(
                    "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                    rusqlite::params![2_i64, "/projects/beta", "beta"],
                )
                .expect("Failed to insert project beta");

            self.connection
                .execute(
                    "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        "alpha-claude-old",
                        "claude_code",
                        Some("/projects/alpha"),
                        Some(1_i64),
                        10_i64,
                        2_i64,
                        "/tmp/alpha-claude-old.jsonl",
                        100_i64,
                    ],
                )
                .expect("Failed to insert alpha old claude session");

            self.connection
                .execute(
                    "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        "alpha-claude-new",
                        "claude_code",
                        Some("/projects/alpha"),
                        Some(1_i64),
                        20_i64,
                        3_i64,
                        "/tmp/alpha-claude-new.jsonl",
                        200_i64,
                    ],
                )
                .expect("Failed to insert alpha new claude session");

            self.connection
                .execute(
                    "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        "alpha-opencode",
                        "opencode",
                        Some("/projects/alpha"),
                        Some(1_i64),
                        30_i64,
                        2_i64,
                        "/tmp/alpha-opencode.jsonl",
                        300_i64,
                    ],
                )
                .expect("Failed to insert alpha opencode session");

            self.connection
                .execute(
                    "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        "unassigned-claude",
                        "claude_code",
                        Option::<String>::None,
                        Option::<i64>::None,
                        40_i64,
                        1_i64,
                        "/tmp/unassigned-claude.jsonl",
                        400_i64,
                    ],
                )
                .expect("Failed to insert unassigned claude session");

            self.connection
                .execute(
                    "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        "beta-claude",
                        "claude_code",
                        Some("/projects/beta"),
                        Some(2_i64),
                        50_i64,
                        1_i64,
                        "/tmp/beta-claude.jsonl",
                        500_i64,
                    ],
                )
                .expect("Failed to insert beta claude session");
        }

        fn seed_many_claude_sessions(&self, count: usize) {
            for index in 0..count {
                let id = format!("bulk-claude-{index:03}");
                let file_path = format!("/tmp/{id}.jsonl");
                self.connection
                    .execute(
                        "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        rusqlite::params![
                            id,
                            "claude_code",
                            Some("/projects/alpha"),
                            Some(1_i64),
                            index as i64,
                            1_i64,
                            file_path,
                            index as i64,
                        ],
                    )
                    .expect("Failed to insert bulk claude session");
            }
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn empty_state_prefers_indexing_placeholder_when_loading_and_empty() {
        let state = compute_empty_state(
            true,
            "",
            true,
            true,
            false,
            false,
            &ProjectFilter::AllSessions,
        );

        assert_eq!(state.title, "Indexing sessions...");
        assert_eq!(state.description, "This may take a moment on first launch.");
    }

    #[test]
    fn project_sidebar_empty_state_treats_project_selection_as_active_filter() {
        let state = compute_empty_state(
            true,
            "",
            true,
            false,
            true,
            false,
            &ProjectFilter::Project(1),
        );

        assert_eq!(state.title, "No sessions match filters");
        assert_eq!(
            state.description,
            "Try adjusting the tool filters in the sidebar"
        );
    }

    fn find_list_box(widget: &gtk::Widget) -> Option<gtk::ListBox> {
        if let Ok(list_box) = widget.clone().downcast::<gtk::ListBox>() {
            return Some(list_box);
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            if let Some(found) = find_list_box(&child_widget) {
                return Some(found);
            }
            child = child_widget.next_sibling();
        }

        None
    }

    fn pump_main_context(condition: impl Fn() -> bool) {
        let context = gtk::glib::MainContext::default();
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            if condition() {
                return;
            }

            if !context.iteration(false) {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    fn drain_main_context() {
        let context = gtk::glib::MainContext::default();
        for _ in 0..20 {
            if !context.iteration(false) {
                break;
            }
        }
    }

    #[gtk::test]
    fn pump_main_context_waits_for_timeout_callbacks() {
        let done = Rc::new(RefCell::new(false));
        let done_ref = done.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(10), move || {
            *done_ref.borrow_mut() = true;
        });

        pump_main_context(|| *done.borrow());

        assert!(
            *done.borrow(),
            "main context pump should wait for timeout work"
        );
    }

    #[gtk::test]
    fn session_list_activates_on_single_click() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        assert!(list_box.activates_on_single_click());
    }

    #[gtk::test]
    fn session_list_emits_selection_on_row_activation() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let outputs: Rc<RefCell<Vec<SessionListOutput>>> = Rc::new(RefCell::new(Vec::new()));
        let outputs_ref = outputs.clone();

        let controller = SessionList::builder()
            .launch(temp_db.path().to_path_buf())
            .connect_receiver(move |_, output| {
                outputs_ref.borrow_mut().push(output);
            });

        let session = Session {
            id: "test-session".to_string(),
            tool: AiAssistant::ClaudeCode,
            project_path: Some("/tmp/project".to_string()),
            project_id: None,
            start_time: chrono::Utc::now(),
            message_count: 1,
            file_path: "/tmp/session.jsonl".to_string(),
            last_updated: chrono::Utc::now(),
            pinned_at: None,
            first_prompt: None,
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: crate::models::SessionEndingStatus::Unknown,
        };

        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: session.clone(),
            });
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");
        let row = list_box.row_at_index(0).expect("row");

        list_box.emit_by_name::<()>("row-activated", &[&row]);

        pump_main_context(|| !outputs.borrow().is_empty());

        let outputs = outputs.borrow();
        assert!(matches!(
            outputs.as_slice(),
            [SessionListOutput::SessionSelected(id)] if id == "test-session"
        ));
    }

    #[gtk::test]
    fn session_list_uses_single_selection_mode() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        assert_eq!(list_box.selection_mode(), gtk::SelectionMode::Single);
    }

    #[gtk::test]
    fn session_list_selects_first_row_on_init() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());

        // Add a row
        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: Session {
                    id: "sel-test".to_string(),
                    tool: AiAssistant::ClaudeCode,
                    project_path: Some("/tmp/p".to_string()),
                    project_id: None,
                    start_time: chrono::Utc::now(),
                    message_count: 1,
                    file_path: "/tmp/s.jsonl".to_string(),
                    last_updated: chrono::Utc::now(),
                    pinned_at: None,
                    first_prompt: None,
                    parent_session_id: None,
                    is_subagent: false,
                    token_usage: None,
                    edit_count: 0,
                    read_count: 0,
                    command_count: 0,
                    ending_status: crate::models::SessionEndingStatus::Unknown,
                },
            });
        }

        // Simulate RestoreFocus (which reload would send)
        controller.emit(SessionListMsg::RestoreFocus);

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");
        pump_main_context(|| list_box.selected_row().is_some());

        assert!(
            list_box.selected_row().is_some(),
            "first row should be selected"
        );
    }

    #[gtk::test]
    fn project_sidebar_session_list_set_filters_reloads_project_intersection() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        controller.emit(SessionListMsg::SetFilters {
            tools: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 2
        });

        let ids: Vec<String> = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .filter_map(|index| parts.model.sessions.get(index))
                .map(|row| row.session_id().to_string())
                .collect()
        };

        assert_eq!(ids, vec!["alpha-claude-new", "alpha-claude-old"]);
    }

    #[test]
    fn unchanged_filters_do_not_need_reload() {
        let tools = vec![AiAssistant::ClaudeCode, AiAssistant::OpenCode];

        assert!(!SessionList::filters_changed(
            &tools,
            &ProjectFilter::Project(1),
            &tools,
            &ProjectFilter::Project(1),
        ));
        assert!(SessionList::filters_changed(
            &tools,
            &ProjectFilter::Project(1),
            &[AiAssistant::ClaudeCode],
            &ProjectFilter::Project(1),
        ));
        assert!(SessionList::filters_changed(
            &tools,
            &ProjectFilter::Project(1),
            &tools,
            &ProjectFilter::AllSessions,
        ));
    }

    #[test]
    fn unchanged_search_query_does_not_need_reload() {
        assert!(!SessionList::search_query_changed("needle", "needle"));
        assert!(SessionList::search_query_changed("needle", "other"));
    }

    #[test]
    fn parse_session_id_query_recognizes_lowercase_prefix_and_trims_suffix() {
        assert_eq!(parse_session_id_query("id:abc"), Some("abc"));
        assert_eq!(parse_session_id_query(" id: abc "), Some("abc"));
        assert_eq!(parse_session_id_query("id:   "), Some(""));
        assert_eq!(parse_session_id_query("ID:abc"), None);
        assert_eq!(parse_session_id_query("abc"), None);
    }

    #[test]
    fn session_id_search_empty_state_has_specific_copy() {
        let state = compute_empty_state(
            true,
            "id:missing-session",
            true,
            false,
            false,
            true,
            &ProjectFilter::AllSessions,
        );

        assert_eq!(state.title, "No session found with this ID");
        assert_eq!(
            state.description,
            "Try a different session ID or adjust filters"
        );
        assert!(!state.show_source_results);
    }

    #[test]
    fn session_id_search_empty_state_overrides_pinned_search_copy() {
        let state = compute_empty_state(
            true,
            "id:missing-session",
            true,
            false,
            false,
            false,
            &ProjectFilter::Pinned,
        );

        assert_eq!(state.title, "No session found with this ID");
        assert_eq!(
            state.description,
            "Try a different session ID or adjust filters"
        );
        assert!(!state.show_source_results);
    }

    #[test]
    fn blank_session_id_search_empty_state_has_specific_copy() {
        let state = compute_empty_state(
            true,
            "id:   ",
            true,
            false,
            false,
            false,
            &ProjectFilter::AllSessions,
        );

        assert_eq!(state.title, "No session found with this ID");
        assert_eq!(
            state.description,
            "Try a different session ID or adjust filters"
        );
        assert!(!state.show_source_results);
    }

    #[test]
    fn post_indexing_measurement_token_invalidates_stale_callbacks() {
        let token = MeasurementToken::new();
        let stale_callback_token = token.clone();

        assert!(token.is_valid());
        assert!(stale_callback_token.is_valid());
        assert!(token.same_identity(&stale_callback_token));

        token.invalidate();

        assert!(!token.is_valid());
        assert!(!stale_callback_token.is_valid());
    }

    #[test]
    fn pending_post_indexing_batch_takes_bounded_rows_and_tracks_progress() {
        let token = MeasurementToken::new();
        let sessions = vec![
            make_test_session("batch-1"),
            make_test_session("batch-2"),
            make_test_session("batch-3"),
        ];
        let mut batch =
            PendingPostIndexingBatch::new(token.clone(), sessions, Some("batch-2".to_string()));

        assert_eq!(batch.remaining_row_count(), 3);
        assert_eq!(batch.previously_selected_id(), Some("batch-2"));
        assert!(!batch.is_exhausted());

        let first = batch.take_next_rows(2);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].id, "batch-1");
        assert_eq!(first[1].id, "batch-2");
        assert_eq!(batch.remaining_row_count(), 1);
        assert!(!batch.is_exhausted());

        let second = batch.take_next_rows(2);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].id, "batch-3");
        assert_eq!(batch.remaining_row_count(), 0);
        assert!(batch.is_exhausted());
    }

    #[test]
    fn pending_post_indexing_batch_invalidates_token_and_records_user_selection_change() {
        let token = MeasurementToken::new();
        let callback_token = token.clone();
        let mut batch = PendingPostIndexingBatch::new(
            token,
            vec![make_test_session("batch-1")],
            Some("batch-1".to_string()),
        );

        assert!(callback_token.is_valid());
        assert!(batch.token_matches(&callback_token));
        assert!(!batch.user_selection_changed());

        batch.mark_user_selection_changed();
        assert!(batch.user_selection_changed());

        batch.invalidate();
        assert!(!callback_token.is_valid());
        assert!(!batch.token_matches(&callback_token));
    }

    #[test]
    fn post_indexing_measurement_accumulates_batch_push_metrics() {
        let context = IndexingReloadContext {
            indexed: 3,
            skipped: 0,
            removed: 0,
            pending_reindex_feedback: false,
            errors_present: false,
        };
        let mut measurement = ActivePostIndexingMeasurement::new(
            context,
            &[AiAssistant::ClaudeCode],
            &ProjectFilter::AllSessions,
            "",
        );

        measurement.record_batch_push(Duration::from_millis(2));
        measurement.record_batch_push(Duration::from_millis(5));
        measurement.record_batch_push(Duration::from_millis(3));

        assert_eq!(measurement.batch_count, 3);
        assert_eq!(measurement.batch_size, POST_INDEXING_RELOAD_BATCH_SIZE);
        assert_eq!(measurement.push_duration, Duration::from_millis(10));
        assert_eq!(
            measurement.max_batch_push_duration,
            Duration::from_millis(5)
        );
    }

    #[gtk::test]
    fn cancel_post_indexing_batch_invalidates_pending_batch_and_measurement() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());
        let context = IndexingReloadContext {
            indexed: 1,
            skipped: 0,
            removed: 0,
            pending_reindex_feedback: false,
            errors_present: false,
        };

        let callback_token = {
            let mut parts = controller.state().get_mut();
            parts.model.start_post_indexing_measurement(context);
            let token = parts
                .model
                .active_post_indexing_measurement
                .as_ref()
                .expect("active measurement")
                .token
                .clone();
            parts.model.pending_post_indexing_batch = Some(PendingPostIndexingBatch::new(
                token.clone(),
                vec![make_test_session("pending")],
                None,
            ));
            token
        };

        {
            let mut parts = controller.state().get_mut();
            parts.model.cancel_post_indexing_batch();
            assert!(parts.model.pending_post_indexing_batch.is_none());
            assert!(parts.model.active_post_indexing_measurement.is_none());
        }

        assert!(!callback_token.is_valid());
    }

    #[gtk::test]
    fn start_post_indexing_measurement_cancels_previous_pending_batch() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());
        let first_context = IndexingReloadContext {
            indexed: 1,
            skipped: 0,
            removed: 0,
            pending_reindex_feedback: false,
            errors_present: false,
        };
        let second_context = IndexingReloadContext {
            indexed: 2,
            skipped: 0,
            removed: 0,
            pending_reindex_feedback: false,
            errors_present: false,
        };

        let first_token = {
            let mut parts = controller.state().get_mut();
            parts.model.start_post_indexing_measurement(first_context);
            let token = parts
                .model
                .active_post_indexing_measurement
                .as_ref()
                .expect("first measurement")
                .token
                .clone();
            parts.model.pending_post_indexing_batch = Some(PendingPostIndexingBatch::new(
                token.clone(),
                vec![make_test_session("first")],
                None,
            ));
            parts.model.start_post_indexing_measurement(second_context);
            token
        };

        let parts = controller.state().get();
        assert!(parts.model.pending_post_indexing_batch.is_none());
        assert!(!first_token.is_valid());
        assert_eq!(
            parts
                .model
                .active_post_indexing_measurement
                .as_ref()
                .expect("second measurement")
                .context
                .indexed,
            2
        );
    }

    #[gtk::test]
    fn explicit_reload_refreshes_even_when_filters_are_unchanged() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 5
        });

        temp_db
            .connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "fresh-claude",
                    "claude_code",
                    Some("/projects/alpha"),
                    Some(1_i64),
                    60_i64,
                    1_i64,
                    "/tmp/fresh-claude.jsonl",
                    600_i64,
                ],
            )
            .expect("Failed to insert fresh session");

        controller.emit(SessionListMsg::Reload);

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 6
        });

        let first_session_id = {
            let parts = controller.state().get();
            parts
                .model
                .sessions
                .get(0)
                .map(|row| row.session_id().to_string())
        };

        assert_eq!(first_session_id.as_deref(), Some("fresh-claude"));
    }

    #[gtk::test]
    fn reload_after_indexing_applies_filters_and_refreshes_sessions() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 5
        });

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::OpenCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 1,
                skipped: 2,
                removed: 3,
                pending_reindex_feedback: true,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 1
        });

        let ids: Vec<String> = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .filter_map(|index| parts.model.sessions.get(index))
                .map(|row| row.session_id().to_string())
                .collect()
        };

        assert_eq!(ids, vec!["alpha-opencode"]);
    }

    #[gtk::test]
    fn reload_after_indexing_finishes_batched_reload_with_expected_rows() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();
        temp_db.seed_many_claude_sessions(70);

        let controller = SessionList::builder().launch(temp_db.path.clone());

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 75
        });

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 70,
                skipped: 0,
                removed: 0,
                pending_reindex_feedback: false,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 0 && parts.model.pending_post_indexing_batch.is_some()
        });

        {
            let parts = controller.state().get();
            assert_eq!(parts.model.sessions.len(), 0);
            assert!(parts.model.pending_post_indexing_batch.is_some());
        }

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == POST_INDEXING_RELOAD_BATCH_SIZE
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_post_indexing_batch.is_none() && parts.model.sessions.len() == 72
        });

        let ids: Vec<String> = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .filter_map(|index| parts.model.sessions.get(index))
                .map(|row| row.session_id().to_string())
                .collect()
        };

        assert_eq!(ids[0], "alpha-claude-new");
        assert_eq!(ids[1], "alpha-claude-old");
        assert_eq!(ids.len(), 72);
    }

    #[gtk::test]
    fn second_reload_after_indexing_invalidates_first_pending_batch() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();
        temp_db.seed_many_claude_sessions(130);

        let controller = SessionList::builder().launch(temp_db.path.clone());

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 135
        });

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 130,
                skipped: 0,
                removed: 0,
                pending_reindex_feedback: false,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == POST_INDEXING_RELOAD_BATCH_SIZE
        });

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::OpenCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 1,
                skipped: 0,
                removed: 0,
                pending_reindex_feedback: false,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_post_indexing_batch.is_none() && parts.model.sessions.len() == 1
        });

        drain_main_context();

        let ids: Vec<String> = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .filter_map(|index| parts.model.sessions.get(index))
                .map(|row| row.session_id().to_string())
                .collect()
        };

        assert_eq!(ids, vec!["alpha-opencode"]);
    }

    #[gtk::test]
    fn ordinary_reload_cancels_active_post_indexing_batch() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();
        temp_db.seed_many_claude_sessions(130);

        let controller = SessionList::builder().launch(temp_db.path.clone());

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 135
        });

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 130,
                skipped: 0,
                removed: 0,
                pending_reindex_feedback: false,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == POST_INDEXING_RELOAD_BATCH_SIZE
        });

        controller.emit(SessionListMsg::SetSearchQuery(
            "id:alpha-claude-new".to_string(),
        ));

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_post_indexing_batch.is_none() && parts.model.sessions.len() == 1
        });

        drain_main_context();

        let ids: Vec<String> = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .filter_map(|index| parts.model.sessions.get(index))
                .map(|row| row.session_id().to_string())
                .collect()
        };

        assert_eq!(ids, vec!["alpha-claude-new"]);
    }

    #[gtk::test]
    fn user_selection_during_batch_is_not_overwritten_by_final_restore() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();
        temp_db.seed_many_claude_sessions(130);

        let controller = SessionList::builder().launch(temp_db.path.clone());

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 135
        });

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");
        let old_row = list_box.row_at_index(1).expect("alpha old row");
        list_box.select_row(Some(&old_row));
        pump_main_context(|| list_box.selected_row().map(|row| row.index()) == Some(1));

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 130,
                skipped: 0,
                removed: 0,
                pending_reindex_feedback: false,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == POST_INDEXING_RELOAD_BATCH_SIZE
        });

        let new_row = list_box.row_at_index(0).expect("alpha new row");
        list_box.select_row(Some(&new_row));
        pump_main_context(|| list_box.selected_row().map(|row| row.index()) == Some(0));

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_post_indexing_batch.is_none() && parts.model.sessions.len() == 132
        });

        let selected_session_id = {
            let parts = controller.state().get();
            let selected_index = list_box
                .selected_row()
                .map(|row| row.index() as usize)
                .expect("selected row");
            parts
                .model
                .sessions
                .get(selected_index)
                .map(|row| row.session_id().to_string())
                .expect("selected session")
        };

        assert_eq!(selected_session_id, "alpha-claude-new");
    }

    #[gtk::test]
    fn previously_selected_session_survives_post_indexing_reload_with_no_user_interaction() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 5
        });

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        let target_index = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .find(|index| {
                    parts
                        .model
                        .sessions
                        .get(*index)
                        .map(|row| row.session_id() == "alpha-claude-old")
                        .unwrap_or(false)
                })
                .expect("alpha-claude-old in initial dataset")
        };

        let target_row = list_box
            .row_at_index(target_index as i32)
            .expect("alpha-claude-old row");
        list_box.select_row(Some(&target_row));
        pump_main_context(|| {
            list_box.selected_row().map(|row| row.index()) == Some(target_index as i32)
        });

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 1,
                skipped: 0,
                removed: 0,
                pending_reindex_feedback: false,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_post_indexing_batch.is_none() && parts.model.sessions.len() == 2
        });

        drain_main_context();

        let final_selected_id = {
            let parts = controller.state().get();
            let selected_index = list_box
                .selected_row()
                .map(|row| row.index() as usize)
                .expect("a row should remain selected after the post-indexing reload");
            parts
                .model
                .sessions
                .get(selected_index)
                .map(|row| row.session_id().to_string())
                .expect("selected session present")
        };

        assert_eq!(final_selected_id, "alpha-claude-old");
    }

    #[gtk::test]
    fn post_indexing_reload_restores_focus_when_listbox_was_focused() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        let window = gtk::Window::new();
        window.set_child(Some(controller.widget()));
        window.present();

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 5
        });

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        let target_index = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .find(|index| {
                    parts
                        .model
                        .sessions
                        .get(*index)
                        .map(|row| row.session_id() == "alpha-claude-old")
                        .unwrap_or(false)
                })
                .expect("alpha-claude-old in initial dataset")
        };

        let target_row = list_box
            .row_at_index(target_index as i32)
            .expect("alpha-claude-old row");
        list_box.select_row(Some(&target_row));
        target_row.grab_focus();
        pump_main_context(|| target_row.has_focus());
        assert!(
            target_row.has_focus(),
            "row should have focus before reload"
        );

        controller.emit(SessionListMsg::ReloadAfterIndexing {
            assistants: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
            context: IndexingReloadContext {
                indexed: 1,
                skipped: 0,
                removed: 0,
                pending_reindex_feedback: false,
                errors_present: false,
            },
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.pending_post_indexing_batch.is_none() && parts.model.sessions.len() == 2
        });

        drain_main_context();

        let restored_row = list_box.selected_row().expect("selected row after reload");
        assert!(
            restored_row.has_focus(),
            "selected row should regain focus after the post-indexing reload"
        );

        window.set_child(None::<&gtk::Widget>);
    }

    #[gtk::test]
    fn project_sidebar_session_list_set_filters_supports_pinned_destination() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();
        temp_db
            .connection
            .execute(
                "UPDATE sessions SET pinned_at = ?1 WHERE id IN (?2, ?3)",
                rusqlite::params![999_i64, "alpha-claude-new", "beta-claude"],
            )
            .expect("Failed to pin fixture sessions");

        let controller = SessionList::builder().launch(temp_db.path.clone());

        controller.emit(SessionListMsg::SetFilters {
            tools: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Pinned,
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 2
        });

        let ids: Vec<String> = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .filter_map(|index| parts.model.sessions.get(index))
                .map(|row| row.session_id().to_string())
                .collect()
        };

        assert_eq!(ids, vec!["beta-claude", "alpha-claude-new"]);
    }

    #[gtk::test]
    fn session_list_id_search_filters_to_exact_session_id() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        controller.emit(SessionListMsg::SetSearchQuery(
            " id: alpha-claude-old ".to_string(),
        ));

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 1
        });

        let ids: Vec<String> = {
            let parts = controller.state().get();
            (0..parts.model.sessions.len())
                .filter_map(|index| parts.model.sessions.get(index))
                .map(|row| row.session_id().to_string())
                .collect()
        };

        assert_eq!(ids, vec!["alpha-claude-old"]);
    }

    #[gtk::test]
    fn session_list_id_search_respects_active_filters() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        controller.emit(SessionListMsg::SetFilters {
            tools: vec![AiAssistant::OpenCode],
            project_filter: ProjectFilter::AllSessions,
        });
        controller.emit(SessionListMsg::SetSearchQuery(
            "id:alpha-claude-old".to_string(),
        ));

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.is_empty()
        });

        let title = {
            let parts = controller.state().get();
            parts.widgets.empty_state.title().to_string()
        };

        assert_eq!(title, "No session found with this ID");
    }

    #[gtk::test]
    fn session_list_reload_preserves_selected_session_when_order_changes() {
        let temp_db = TempDatabase::new();
        temp_db.seed_project_sidebar_fixture();

        let controller = SessionList::builder().launch(temp_db.path.clone());

        controller.emit(SessionListMsg::SetFilters {
            tools: vec![AiAssistant::ClaudeCode],
            project_filter: ProjectFilter::Project(1),
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.sessions.len() == 2
        });

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");
        let second_row = list_box.row_at_index(1).expect("second row");
        list_box.select_row(Some(&second_row));
        pump_main_context(|| list_box.selected_row().map(|r| r.index()) == Some(1));

        temp_db
            .connection
            .execute(
                "UPDATE sessions SET last_updated = ?1 WHERE id = ?2",
                rusqlite::params![999_i64, "alpha-claude-old"],
            )
            .expect("Failed to reorder selected session");

        controller.emit(SessionListMsg::SetSearchQuery("".to_string()));
        pump_main_context(|| list_box.selected_row().is_some());

        let selected_session_id = {
            let parts = controller.state().get();
            let selected_index = list_box
                .selected_row()
                .map(|row| row.index() as usize)
                .expect("selected row");
            parts
                .model
                .sessions
                .get(selected_index)
                .map(|row| row.session_id().to_string())
                .expect("selected session")
        };

        assert_eq!(selected_session_id, "alpha-claude-old");
    }

    #[gtk::test]
    fn session_list_forwards_row_resume_action_without_selection() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let outputs: Rc<RefCell<Vec<SessionListOutput>>> = Rc::new(RefCell::new(Vec::new()));
        let outputs_ref = outputs.clone();

        let controller = SessionList::builder()
            .launch(temp_db.path().to_path_buf())
            .connect_receiver(move |_, output| {
                outputs_ref.borrow_mut().push(output);
            });

        let session = Session {
            id: "resume-session".to_string(),
            tool: AiAssistant::OpenCode,
            project_path: Some("/tmp/project".to_string()),
            project_id: None,
            start_time: chrono::Utc::now(),
            message_count: 1,
            file_path: "/tmp/session.jsonl".to_string(),
            last_updated: chrono::Utc::now(),
            pinned_at: None,
            first_prompt: None,
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: crate::models::SessionEndingStatus::Unknown,
        };

        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: session.clone(),
            });
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");
        let row = list_box.row_at_index(0).expect("row");
        let row_child = row.child().expect("row child");

        row_child
            .activate_action("row.resume", None)
            .expect("activate row.resume action");

        pump_main_context(|| !outputs.borrow().is_empty());

        let outputs = outputs.borrow();
        assert_eq!(outputs.len(), 1);
        assert!(matches!(
            outputs.as_slice(),
            [SessionListOutput::ResumeRequested(id, tool)] if id == "resume-session" && *tool == AiAssistant::OpenCode
        ));
        assert!(
            !outputs
                .iter()
                .any(|output| matches!(output, SessionListOutput::SessionSelected(_)))
        );
    }

    fn make_test_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            tool: AiAssistant::ClaudeCode,
            project_path: Some("/tmp/project".to_string()),
            project_id: None,
            start_time: chrono::Utc::now(),
            message_count: 1,
            file_path: "/tmp/session.jsonl".to_string(),
            last_updated: chrono::Utc::now(),
            pinned_at: None,
            first_prompt: None,
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: crate::models::SessionEndingStatus::Unknown,
        }
    }

    #[gtk::test]
    fn move_selection_down_advances_index() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());

        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: make_test_session("s1"),
            });
            guard.push_back(SessionRowInit {
                session: make_test_session("s2"),
            });
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        // Select first row
        list_box.select_row(list_box.row_at_index(0).as_ref());
        pump_main_context(|| list_box.selected_row().is_some());

        controller.emit(SessionListMsg::MoveSelection(1));
        pump_main_context(|| list_box.selected_row().map(|r| r.index()).unwrap_or(-1) == 1);

        assert_eq!(list_box.selected_row().unwrap().index(), 1);
    }

    #[gtk::test]
    fn move_selection_clamps_at_boundaries() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());

        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: make_test_session("s1"),
            });
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        list_box.select_row(list_box.row_at_index(0).as_ref());
        pump_main_context(|| list_box.selected_row().is_some());

        controller.emit(SessionListMsg::MoveSelection(-1));
        pump_main_context(|| list_box.selected_row().is_some());

        assert_eq!(list_box.selected_row().unwrap().index(), 0);
    }

    #[gtk::test]
    fn move_selection_on_empty_list_is_noop() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        // Should not panic
        controller.emit(SessionListMsg::MoveSelection(1));
        pump_main_context(|| true);

        assert!(list_box.selected_row().is_none());
    }

    #[test]
    fn indexing_diagnostics_empty_state_shows_source_results_only_for_global_empty_state() {
        let state = compute_empty_state(
            true,
            "",
            true,
            false,
            false,
            true,
            &ProjectFilter::AllSessions,
        );
        assert!(state.show_source_results);
    }

    #[test]
    fn indexing_diagnostics_empty_state_hides_source_results_for_search_results() {
        let state = compute_empty_state(
            true,
            "claude",
            true,
            false,
            false,
            true,
            &ProjectFilter::AllSessions,
        );

        assert!(!state.show_source_results);
    }

    #[test]
    fn pinned_filter_empty_state_has_specific_copy() {
        let state =
            compute_empty_state(true, "", true, false, false, false, &ProjectFilter::Pinned);

        assert_eq!(state.title, "No pinned sessions");
        assert_eq!(
            state.description,
            "Pin sessions from the list to keep them easy to revisit"
        );
        assert!(!state.show_source_results);
    }

    #[test]
    fn pinned_filter_with_search_empty_state_mentions_both() {
        let state = compute_empty_state(
            true,
            "query",
            true,
            false,
            false,
            false,
            &ProjectFilter::Pinned,
        );

        assert_eq!(state.title, "No pinned sessions match search");
        assert_eq!(
            state.description,
            "Try a different query or clear the pinned filter"
        );
        assert!(!state.show_source_results);
    }

    #[gtk::test]
    fn indexing_diagnostics_status_page_gets_source_results_child() {
        use crate::models::{AiAssistant, PerSourceResult, SourceStatus};

        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let controller = SessionList::builder().launch(temp_db.path().to_path_buf());

        controller.emit(SessionListMsg::SetSourceResults(vec![PerSourceResult {
            assistant: AiAssistant::ClaudeCode,
            display_path: "/tmp/claude".into(),
            indexed: 0,
            skipped: 0,
            removed: 0,
            errors: 0,
            status: SourceStatus::Empty,
        }]));

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.source_results_available
        });

        let parts = controller.state().get();
        assert!(parts.widgets.empty_state.child().is_some());
    }

    #[gtk::test]
    fn request_selected_session_for_pin_emits_selected_id() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let outputs: Rc<RefCell<Vec<SessionListOutput>>> = Rc::new(RefCell::new(Vec::new()));
        let outputs_ref = outputs.clone();

        let controller = SessionList::builder()
            .launch(temp_db.path().to_path_buf())
            .connect_receiver(move |_, output| {
                outputs_ref.borrow_mut().push(output);
            });

        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: make_test_session("pin-target"),
            });
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        list_box.select_row(list_box.row_at_index(0).as_ref());
        pump_main_context(|| list_box.selected_row().is_some());

        controller.emit(SessionListMsg::RequestSelectedSessionForPin);
        pump_main_context(|| !outputs.borrow().is_empty());

        let outputs = outputs.borrow();
        assert!(matches!(
            outputs.as_slice(),
            [SessionListOutput::SelectedSessionForPin(id)] if id == "pin-target"
        ));
    }

    #[gtk::test]
    fn activate_selected_emits_session_selected() {
        let temp_db = tempfile::NamedTempFile::new().expect("temp db");
        let outputs: Rc<RefCell<Vec<SessionListOutput>>> = Rc::new(RefCell::new(Vec::new()));
        let outputs_ref = outputs.clone();

        let controller = SessionList::builder()
            .launch(temp_db.path().to_path_buf())
            .connect_receiver(move |_, output| {
                outputs_ref.borrow_mut().push(output);
            });

        {
            let mut parts = controller.state().get_mut();
            let mut guard = parts.model.sessions.guard();
            guard.push_back(SessionRowInit {
                session: make_test_session("activate-test"),
            });
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("list box");

        list_box.select_row(list_box.row_at_index(0).as_ref());
        pump_main_context(|| list_box.selected_row().is_some());

        controller.emit(SessionListMsg::ActivateSelected);
        pump_main_context(|| !outputs.borrow().is_empty());

        let outputs = outputs.borrow();
        assert!(matches!(
            outputs.as_slice(),
            [SessionListOutput::SessionSelected(id)] if id == "activate-test"
        ));
    }
}
