use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};
use std::path::{Path, PathBuf};

use crate::database::{load_sessions_for_filter, search_sessions_for_filter};
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
}

#[derive(Debug)]
pub enum SessionListMsg {
    SetFilters {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
    SetSearchQuery(String),
    SetIndexing(bool),
    SetSourceResults(Vec<PerSourceResult>),
    SessionActivated(i32),
    ResumeRequested(String, AiAssistant),
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
    ResumeRequested(String, AiAssistant),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmptyStateViewModel {
    title: &'static str,
    description: &'static str,
    show_source_results: bool,
}

fn compute_empty_state(
    sessions_empty: bool,
    search_query: &str,
    all_tools_selected: bool,
    indexing: bool,
    project_filter_active: bool,
    source_results_available: bool,
) -> EmptyStateViewModel {
    if sessions_empty && indexing {
        return EmptyStateViewModel {
            title: "Indexing sessions...",
            description: "This may take a moment on first launch.",
            show_source_results: false,
        };
    }

    if !search_query.trim().is_empty() {
        return EmptyStateViewModel {
            title: "No sessions match search",
            description: "Try a different query or adjust filters",
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
                self.active_tools = tools.clone();
                self.project_filter = project_filter;
                self.all_tools_selected = tools.len() == AiAssistant::ALL.len();
                self.reload_sessions();
            }
            SessionListMsg::SetSearchQuery(query) => {
                self.search_query = query;
                self.reload_sessions();
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
            SessionListMsg::ResumeRequested(id, tool) => {
                let _ = sender.output(SessionListOutput::ResumeRequested(id, tool));
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

    fn reload_sessions(&mut self) {
        let fetched = Self::fetch_sessions(
            &self.db_path,
            &self.active_tools,
            &self.project_filter,
            &self.search_query,
        );
        let mut guard = self.sessions.guard();
        guard.clear();
        for session in fetched {
            guard.push_back(SessionRowInit { session });
        }
        drop(guard);
        self.ensure_selection();
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
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn empty_state_prefers_indexing_placeholder_when_loading_and_empty() {
        let state = compute_empty_state(true, "", true, true, false, false);

        assert_eq!(state.title, "Indexing sessions...");
        assert_eq!(state.description, "This may take a moment on first launch.");
    }

    #[test]
    fn project_sidebar_empty_state_treats_project_selection_as_active_filter() {
        let state = compute_empty_state(true, "", true, false, true, false);

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
        let state = compute_empty_state(true, "", true, false, false, true);
        assert!(state.show_source_results);
    }

    #[test]
    fn indexing_diagnostics_empty_state_hides_source_results_for_search_results() {
        let state = compute_empty_state(true, "claude", true, false, false, true);
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
