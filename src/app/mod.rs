use relm4::{
    ComponentController, ComponentParts, ComponentSender, Controller, SimpleComponent,
    WorkerController, adw, gtk, main_application,
};

use adw::prelude::*;
use anyhow::Context;
use gtk::prelude::{
    ActionableExt, ApplicationExt, ButtonExt, Cast, EditableExt, GtkApplicationExt, GtkWindowExt,
    OrientableExt, SettingsExt, ToggleButtonExt, WidgetExt,
};
use gtk::{gio, glib};
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
};

use crate::analytics_worker::AnalyticsWorker;
use crate::config::{APP_ID, PROFILE};
use crate::database::{
    SessionIndexer, count_all_sessions, count_unassigned_sessions, has_unassigned_sessions,
    load_projects,
};
use crate::indexing_worker::{IndexingWorker, IndexingWorkerInput};
use crate::models::{ProjectFilter, ProjectInfo, session::AiAssistant};
use crate::session_sources::{SessionSources, select_db_filename};
use crate::ui::modals::preferences::PreferencesDialog;
#[cfg(test)]
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::{
    analytics_view::AnalyticsView,
    session_detail::SessionDetail,
    session_list::{SessionList, SessionListMsg},
    sidebar::{Sidebar, SidebarMsg},
    tool_inspector_pane::{ToolInspectorPane, ToolInspectorPaneMsg},
};
use crate::utils::terminal;

mod handlers;
mod helpers;
mod init;
mod types;

#[cfg(test)]
use helpers::decide_reindex_action;
#[cfg(test)]
use helpers::workspace_allows_search;
#[cfg(test)]
use helpers::{
    active_search_query, analytics_indexing_completion_outcome, detail_pop_sync_decision,
    parent_session_load_failure_messages, resolve_escape_action, resolve_search_mode_change,
    search_query_update_messages, transition_to_detail, workspace_header_visibility,
};
use helpers::{retained_project_filter, transition_to_list};
#[cfg(test)]
use types::EscapeResolution;
#[cfg(test)]
use types::ReindexAction;
use types::{ActiveSessionRef, FilterState, UtilityPaneMode, Workspace};

/// Timeout in seconds for resume failure toast notifications
const RESUME_FAILURE_TOAST_TIMEOUT_SECS: u32 = 4;

struct SidebarProjectData {
    projects: Vec<ProjectInfo>,
    all_sessions_count: usize,
    unassigned_count: usize,
    show_unassigned: bool,
}

fn load_sidebar_project_data(
    db_path: &Path,
    tools: &[AiAssistant],
) -> anyhow::Result<SidebarProjectData> {
    let projects = load_projects(db_path, tools).context("load projects for sidebar")?;
    let all_sessions_count =
        count_all_sessions(db_path, tools).context("count all sessions for sidebar")?;
    let unassigned_count = count_unassigned_sessions(db_path, tools)
        .context("count unassigned sessions for sidebar")?;
    let show_unassigned =
        has_unassigned_sessions(db_path).context("determine unassigned sidebar visibility")?;

    Ok(SidebarProjectData {
        projects,
        all_sessions_count,
        unassigned_count,
        show_unassigned,
    })
}

pub(super) struct App {
    search_visible: bool,
    /// Set to `true` when model code changes `search_visible` and the GTK
    /// SearchBar needs to be updated in `post_view`.  Cleared after sync.
    /// This avoids unconditionally forcing widget state on every render cycle,
    /// which could oscillate when GTK signal callbacks enqueue intermediate
    /// messages (e.g. `SearchQueryChanged` from clearing the entry).
    /// Uses `Cell` because `post_view` takes `&self`.
    sync_search_bar: Cell<bool>,
    detail_visible: bool,
    pane_open: bool,
    pane_mode: UtilityPaneMode,
    active_session: Option<ActiveSessionRef>,
    /// When the user opens a child session from the inspector, this holds the
    /// originating parent session so a one-hop return is possible.
    parent_session: Option<ActiveSessionRef>,
    search_query: String,
    session_list: Controller<SessionList>,
    analytics_view: Controller<AnalyticsView>,
    session_detail: Controller<SessionDetail>,
    #[allow(dead_code)] // Controller must stay alive to keep the widget
    sidebar: Controller<Sidebar>,
    #[allow(dead_code)] // Controller must stay alive to keep the widget
    tool_inspector_pane: Controller<ToolInspectorPane>,
    preferences_dialog: Controller<PreferencesDialog>,
    indexing_worker: WorkerController<IndexingWorker>,
    analytics_worker: WorkerController<AnalyticsWorker>,
    workspace_stack: adw::ViewStack,
    nav_view: adw::NavigationView,
    detail_page: adw::NavigationPage,
    suppress_next_detail_pop_sync: bool,
    pane_stack: gtk::Stack,
    toast_overlay: adw::ToastOverlay,
    filter_state: FilterState,
    db_path: PathBuf,
    sources: SessionSources,
    indexing: bool,
    pending_reindex_feedback: bool,
    active_workspace: Workspace,
    banner: adw::Banner,
    banner_has_issues: bool,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    SearchModeChanged(bool),
    TogglePane,
    PaneVisibilityChanged(bool),
    SearchQueryChanged(String),
    WorkspaceChanged(Workspace),
    FiltersChanged {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
    SessionSelected(String),
    /// User-requested navigation back from detail to list.
    RequestNavigateBack,
    /// Detail page popped signal from `NavigationView`.
    NavigateBack,
    ResumeSession(String, AiAssistant),
    /// Resume the currently active session (triggered from the header bar button).
    ResumeActiveSession,
    InspectToolCall(String),
    InspectSubagent(String),
    /// Inspector pane requested opening a child session.
    OpenChildSession(String),
    /// Header-bar button: return to the one-hop parent session.
    ReturnToParentSession,
    /// Esc key: pop inspector drill-down (native) → close pane → navigate back.
    Escape,
    ShowPreferences,
    ReindexRequested,
    IndexingCompleted {
        indexed: usize,
        skipped: usize,
        per_source: Vec<crate::models::PerSourceResult>,
    },
    IndexingFailed,
    AnalyticsRefreshRequested,
    AnalyticsLoaded(crate::models::AnalyticsData),
    AnalyticsLoadFailed(String),
}

relm4::new_action_group!(pub(super) WindowActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(pub(super) ShortcutsAction, WindowActionGroup, "show-help-overlay");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");
relm4::new_stateless_action!(TogglePaneAction, WindowActionGroup, "toggle-pane");
relm4::new_stateless_action!(ShowSearchAction, WindowActionGroup, "show-search");
relm4::new_stateless_action!(EscapeAction, WindowActionGroup, "escape");

#[relm4::component(pub)]
impl SimpleComponent for App {
    type Init = Option<PathBuf>;
    type Input = AppMsg;
    type Output = ();
    type Widgets = AppWidgets;

    menu! {
        primary_menu: {
            section! {
                "_Preferences" => PreferencesAction,
                "_Keyboard" => ShortcutsAction,
                "_About Sessions Chronicle" => AboutAction,
            }
        }
    }

    view! {
        main_window = adw::ApplicationWindow::new(&main_application()) {
            set_visible: true,

            connect_close_request[sender] => move |_| {
                sender.input(AppMsg::Quit);
                glib::Propagation::Stop
            },

            add_css_class?: if PROFILE == "Devel" {
                    Some("devel")
                } else {
                    None
                },

            #[wrap(Some)]
            set_content = &adw::ToastOverlay {
                #[wrap(Some)]
                set_child = &adw::ToolbarView {
                    #[name = "header_bar"]
                    add_top_bar = &adw::HeaderBar {
                        #[name = "workspace_switcher"]
                        #[wrap(Some)]
                        set_title_widget = &adw::ViewSwitcher {
                            set_policy: adw::ViewSwitcherPolicy::Wide,
                        },

                        #[name = "back_button"]
                        pack_start = &gtk::Button {
                            set_icon_name: "go-previous-symbolic",
                            set_tooltip_text: Some("Go back"),
                            #[watch]
                            set_visible: model.detail_visible && model.are_detail_actions_visible(),
                            connect_clicked => AppMsg::RequestNavigateBack,
                        },

                        #[name = "search_toggle"]
                        pack_start = &gtk::ToggleButton {
                            set_icon_name: "system-search-symbolic",
                            set_tooltip_text: Some("Search sessions"),
                            #[watch]
                            set_visible: model.is_search_ui_visible(),
                        },

                        #[name = "parent_session_button"]
                        pack_end = &gtk::Button {
                            set_label: "Back to Parent",
                            set_tooltip_text: Some("Return to the parent session"),
                            add_css_class: "flat",
                            #[watch]
                            set_visible: model.parent_session.is_some() && model.detail_visible && model.are_detail_actions_visible(),
                            connect_clicked => AppMsg::ReturnToParentSession,
                        },

                        #[name = "resume_button"]
                        pack_end = &gtk::Button {
                            set_label: "Resume",
                            set_tooltip_text: Some("Resume session in terminal"),
                            add_css_class: "suggested-action",
                            #[watch]
                            set_visible: model.detail_visible && model.are_detail_actions_visible(),
                            connect_clicked => AppMsg::ResumeActiveSession,
                        },

                        #[name = "pane_toggle"]
                        pack_end = &gtk::ToggleButton {
                            set_icon_name: "sidebar-show-symbolic",
                            set_tooltip_text: Some("Toggle utility pane (F9)"),
                            set_action_name: Some("win.toggle-pane"),
                            #[watch]
                            set_active: model.pane_open,
                            #[watch]
                            set_visible: model.is_pane_controls_visible(),
                        },

                        pack_end = &gtk::Spinner {
                            set_tooltip_text: Some("Indexing sessions..."),
                            #[watch]
                            set_visible: model.indexing,
                            #[watch]
                            set_spinning: model.indexing,
                        },

                        pack_end = &gtk::MenuButton {
                            set_icon_name: "open-menu-symbolic",
                            set_menu_model: Some(&primary_menu),
                            set_primary: true,
                        },
                    },

                    #[wrap(Some)]
                    set_content = &gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,

                        #[name = "search_bar"]
                        gtk::SearchBar {
                            #[watch]
                            set_visible: model.is_search_ui_visible(),
                            #[name = "search_entry"]
                            #[wrap(Some)]
                            set_child = &gtk::SearchEntry {
                                set_placeholder_text: Some("Search sessions..."),
                                set_hexpand: true,
                                connect_search_changed[sender] => move |entry| {
                                    sender.input(AppMsg::SearchQueryChanged(entry.text().to_string()));
                                },
                            },
                        },

                        #[name = "overlay_split"]
                        adw::OverlaySplitView {
                            set_vexpand: true,
                            #[watch]
                            set_show_sidebar: model.pane_open,
                            #[watch]
                            set_sidebar_position: model.pane_mode.sidebar_position(),
                            #[watch]
                            set_min_sidebar_width: model.pane_mode.sidebar_min_width(),
                            #[watch]
                            set_sidebar_width_fraction: model.pane_mode.sidebar_width_fraction(),
                            set_enable_show_gesture: true,
                            set_enable_hide_gesture: true,
                        },

                        #[name = "workspace_switcher_bar"]
                        adw::ViewSwitcherBar {
                            set_reveal: false,
                        },
                    },
                },
            }
        }
    }

    fn init(
        sessions_dir: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Resolve session sources and database path
        let sources = SessionSources::resolve(sessions_dir.as_deref());
        let db_dir = glib::user_data_dir().join(APP_ID);
        let db_path = db_dir.join(select_db_filename(sources.override_mode));

        tracing::info!(
            "Session sources (override={}): claude={}, opencode={}, codex={}, vibe={}",
            sources.override_mode,
            sources.claude_dir.display(),
            sources.opencode_storage_root.display(),
            sources.codex_dir.display(),
            sources.vibe_dir.display(),
        );
        tracing::info!("Using database: {}", db_path.display());

        if let Err(err) = fs::create_dir_all(&db_dir) {
            tracing::error!("Failed to create data dir {}: {}", db_dir.display(), err);
        } else if let Err(err) = SessionIndexer::new(&db_path) {
            tracing::error!("Failed to initialize session indexer: {}", err);
        }

        // Build child components, navigation, and workspace stack
        let components = init::init_child_components(&db_path, &sender);
        let nav_setup = init::build_navigation(
            components.session_list.widget(),
            components.session_detail.widget(),
            components.sidebar.widget(),
            components.tool_inspector_pane.widget(),
            &sender,
        );

        let workspace_stack = adw::ViewStack::new();
        workspace_stack.set_vexpand(true);
        workspace_stack.set_hexpand(true);

        // Create model with a temporary toast_overlay (will be replaced after view_output!)
        let mut model = Self {
            search_visible: false,
            sync_search_bar: Cell::new(false),
            detail_visible: false,
            pane_open: true,
            pane_mode: UtilityPaneMode::Filters,
            active_session: None,
            parent_session: None,
            search_query: String::new(),
            session_list: components.session_list,
            analytics_view: components.analytics_view,
            session_detail: components.session_detail,
            sidebar: components.sidebar,
            tool_inspector_pane: components.tool_inspector_pane,
            preferences_dialog: components.preferences_dialog,
            indexing_worker: components.indexing_worker,
            analytics_worker: components.analytics_worker,
            workspace_stack: workspace_stack.clone(),
            nav_view: nav_setup.nav_view.clone(),
            detail_page: nav_setup.detail_page.clone(),
            suppress_next_detail_pop_sync: false,
            pane_stack: nav_setup.pane_stack,
            toast_overlay: adw::ToastOverlay::new(),
            filter_state: FilterState::default(),
            db_path,
            sources,
            indexing: true,
            pending_reindex_feedback: false,
            active_workspace: Workspace::Sessions,
            banner: adw::Banner::new(""),
            banner_has_issues: false,
        };

        // view_output!() must stay in the SimpleComponent impl (Relm4 macro requirement)
        let widgets = view_output!();

        // Get the actual ToastOverlay from the root window's content
        if let Some(toast_overlay) = root
            .content()
            .and_then(|w| w.downcast::<adw::ToastOverlay>().ok())
        {
            model.toast_overlay = toast_overlay;
        } else {
            tracing::warn!("Root content is not a ToastOverlay; toasts will be dropped");
        }

        // Wire up search bar, workspace stack, breakpoints, and actions
        init::wire_search_bar(
            &widgets.search_bar,
            &widgets.search_entry,
            &widgets.search_toggle,
            &widgets.main_window,
            &sender,
            model.session_list.sender(),
        );

        init::setup_workspace_stack(
            &mut model,
            &widgets.overlay_split,
            &widgets.search_bar,
            &widgets.workspace_switcher,
            &widgets.workspace_switcher_bar,
            &nav_setup.nav_view,
            &sender,
        );

        init::setup_breakpoints(
            &root,
            &widgets.overlay_split,
            &widgets.workspace_switcher,
            &widgets.workspace_switcher_bar,
        );

        init::register_actions(
            &root,
            &widgets.main_window,
            &sender,
            &widgets.search_bar,
            &widgets.search_entry,
            &workspace_stack,
        );

        // Startup: load window size, refresh sidebar, kick off indexing
        widgets.load_window_size();

        if model.refresh_sidebar_projects() {
            model.emit_session_list_filters();
        }

        model.session_list.emit(SessionListMsg::SetIndexing(true));
        model
            .indexing_worker
            .emit(IndexingWorkerInput::StartIncremental(model.sources.clone()));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::Quit => main_application().quit(),
            AppMsg::SearchModeChanged(enabled) => self.handle_search_mode_changed(enabled),
            AppMsg::TogglePane => self.handle_toggle_pane(),
            AppMsg::PaneVisibilityChanged(visible) => self.handle_pane_visibility_changed(visible),
            AppMsg::SearchQueryChanged(query) => self.handle_search_query_changed(query),
            AppMsg::WorkspaceChanged(workspace) => self.handle_workspace_changed(workspace),
            AppMsg::FiltersChanged {
                tools,
                project_filter,
            } => {
                self.filter_state.tools = tools;
                self.filter_state.project_filter = project_filter;
                self.refresh_sidebar_projects();
                self.emit_session_list_filters();
            }
            AppMsg::SessionSelected(id) => self.handle_session_selected(id),
            AppMsg::RequestNavigateBack => self.handle_request_navigate_back(),
            AppMsg::NavigateBack => self.handle_navigate_back(),
            AppMsg::ShowPreferences => {
                let dialog_widget = self.preferences_dialog.widget();
                dialog_widget.present(Some(&main_application().windows()[0]));
            }
            AppMsg::ReindexRequested => self.handle_reindex_requested(),
            AppMsg::IndexingCompleted {
                indexed,
                skipped,
                per_source,
            } => self.handle_indexing_completed(indexed, skipped, per_source),
            AppMsg::IndexingFailed => self.handle_indexing_failed(),
            AppMsg::AnalyticsRefreshRequested => self.handle_analytics_refresh_requested(),
            AppMsg::AnalyticsLoaded(data) => self.handle_analytics_loaded(data),
            AppMsg::AnalyticsLoadFailed(error) => self.handle_analytics_load_failed(error),
            AppMsg::ResumeSession(session_id, tool) => self.handle_resume_session(session_id, tool),
            AppMsg::ResumeActiveSession => self.handle_resume_active_session(&sender),
            AppMsg::InspectToolCall(tool_call_id) => self.handle_inspect_tool_call(tool_call_id),
            AppMsg::InspectSubagent(subagent_id) => self.handle_inspect_subagent(subagent_id),
            AppMsg::OpenChildSession(child_session_id) => {
                self.handle_open_child_session(child_session_id)
            }
            AppMsg::ReturnToParentSession => self.handle_return_to_parent_session(),
            AppMsg::Escape => self.handle_escape(&sender),
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        // Only sync the SearchBar when the model explicitly requests it
        // (e.g. Escape handler).  Unconditional sync would oscillate: closing
        // the bar clears the entry → SearchQueryChanged fires before
        // SearchModeChanged(false) → post_view sees the stale
        // search_visible=true and reopens the bar.
        if self.sync_search_bar.replace(false)
            && widgets.search_bar.is_search_mode() != self.search_visible
        {
            widgets.search_bar.set_search_mode(self.search_visible);
        }
    }

    fn shutdown(&mut self, widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        widgets.save_window_size().unwrap();
    }
}

impl App {
    /// Reset app state after leaving detail view.
    fn transition_to_session_list_mode(&mut self) {
        self.detail_visible = false;
        self.active_session = None;
        self.parent_session = None;
        self.tool_inspector_pane.emit(ToolInspectorPaneMsg::Clear);
        transition_to_list(&mut self.pane_mode, &mut self.pane_open);
        self.apply_pane_stack_switch();
        if self.banner_has_issues {
            self.banner.set_revealed(true);
        }
    }

    /// Apply the current `pane_mode` to the Stack widget, with verification.
    fn apply_pane_stack_switch(&self) {
        let target = self.pane_mode.stack_child_name();
        self.pane_stack.set_visible_child_name(target);

        let actual = self.pane_stack.visible_child_name();
        if actual.as_deref() != Some(target) {
            tracing::warn!(
                "Pane stack switch failed: requested '{}', got {:?}",
                target,
                actual
            );
        }
    }

    fn show_error_dialog(&self, title: &str, message: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading(title)
            .body(message)
            .build();

        dialog.add_response("ok", "OK");
        dialog.set_default_response(Some("ok"));

        dialog.present(Some(&relm4::main_application().windows()[0]));
    }

    fn show_resume_failure_toast(&self, error: &terminal::TerminalSpawnError) {
        let toast = adw::Toast::builder()
            .title(error.to_string())
            .timeout(RESUME_FAILURE_TOAST_TIMEOUT_SECS)
            .build();

        if error.should_show_preferences() {
            toast.set_button_label(Some("Preferences"));
            toast.set_action_name(Some("win.preferences"));
        }

        self.toast_overlay.add_toast(toast);
    }

    fn emit_session_list_filters(&self) {
        self.session_list.emit(SessionListMsg::SetFilters {
            tools: self.filter_state.tools.clone(),
            project_filter: self.filter_state.project_filter.clone(),
        });
    }

    fn refresh_sidebar_projects(&mut self) -> bool {
        let tools = self.filter_state.tools.clone();
        let sidebar_data = match load_sidebar_project_data(&self.db_path, &tools) {
            Ok(data) => data,
            Err(err) => {
                tracing::warn!("Failed to load sidebar project data: {err:#}");
                return false;
            }
        };

        let selected_filter = retained_project_filter(
            &self.filter_state.project_filter,
            &sidebar_data.projects,
            sidebar_data.show_unassigned,
        );
        let filter_changed = selected_filter != self.filter_state.project_filter;
        if filter_changed {
            self.filter_state.project_filter = selected_filter.clone();
        }

        self.sidebar.emit(SidebarMsg::ProjectsLoaded {
            projects: sidebar_data.projects,
            all_sessions_count: sidebar_data.all_sessions_count,
            unassigned_count: sidebar_data.unassigned_count,
            show_unassigned: sidebar_data.show_unassigned,
            selected_filter,
        });

        filter_changed
    }
}

impl AppWidgets {
    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = gio::Settings::new(APP_ID);
        let (width, height) = self.main_window.default_size();

        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;

        settings.set_boolean("is-maximized", self.main_window.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let settings = gio::Settings::new(APP_ID);

        let width = settings.int("window-width");
        let height = settings.int("window-height");
        let is_maximized = settings.boolean("is-maximized");

        self.main_window.set_default_size(width, height);

        if is_maximized {
            self.main_window.maximize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::WidgetExt;
    use relm4::Component;
    use relm4::ComponentController;
    use std::path::PathBuf;
    use std::time::Duration;

    fn find_indexing_spinner(widget: &gtk::Widget) -> Option<gtk::Spinner> {
        if let Ok(spinner) = widget.clone().downcast::<gtk::Spinner>()
            && spinner.tooltip_text().as_deref() == Some("Indexing sessions...")
        {
            return Some(spinner);
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            if let Some(found) = find_indexing_spinner(&child_widget) {
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
    fn startup_shows_indexing_spinner_during_incremental_indexing() {
        let schema_available = gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup(crate::config::APP_ID, true))
            .is_some();
        if !schema_available {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));

        {
            let parts = controller.state().get();
            assert!(
                parts.model.indexing,
                "app should start in indexing mode for background incremental scan"
            );
            assert!(
                parts.model.sources.opencode_db_path.is_some(),
                "fixtures should resolve an OpenCode SQLite source"
            );
        }

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let spinner = find_indexing_spinner(&root).expect("indexing spinner should exist");

        assert!(
            spinner.is_visible(),
            "header spinner should be visible while incremental indexing is active"
        );

        controller.emit(AppMsg::IndexingCompleted {
            indexed: 0,
            skipped: 0,
            per_source: vec![],
        });

        pump_main_context(|| !spinner.is_visible());
        assert!(
            !spinner.is_visible(),
            "header spinner should hide after indexing completes"
        );
    }

    #[test]
    fn search_query_update_messages_include_detail_update() {
        let query = "needle".to_string();

        let (list_msg, detail_msg) = search_query_update_messages(query);

        match list_msg {
            SessionListMsg::SetSearchQuery(list_query) => {
                assert_eq!(list_query, "needle");
            }
            _ => panic!("expected SessionListMsg::SetSearchQuery"),
        }

        match detail_msg {
            SessionDetailMsg::UpdateSearchQuery(Some(detail_query)) => {
                assert_eq!(detail_query, "needle");
            }
            _ => panic!("expected SessionDetailMsg::UpdateSearchQuery(Some(..))"),
        }
    }

    #[test]
    fn parent_session_load_failure_clears_detail_and_inspector() {
        let (detail_msg, inspector_msg) = parent_session_load_failure_messages();

        assert!(matches!(detail_msg, SessionDetailMsg::Clear));
        assert!(matches!(inspector_msg, ToolInspectorPaneMsg::Clear));
    }

    #[test]
    fn active_search_query_treats_blank_input_as_none() {
        assert_eq!(active_search_query(""), None);
        assert_eq!(active_search_query("   \n\t  "), None);
        assert_eq!(
            active_search_query("  needle  "),
            Some("needle".to_string())
        );
    }

    #[test]
    fn transition_to_detail_sets_tool_inspector_and_pane_closed() {
        let mut mode = UtilityPaneMode::Filters;
        let mut open = true;
        transition_to_detail(&mut mode, &mut open);
        assert_eq!(mode, UtilityPaneMode::ToolInspector);
        assert!(!open);
    }

    #[test]
    fn transition_to_list_sets_filters_and_reopens_pane() {
        let mut mode = UtilityPaneMode::ToolInspector;
        let mut open = false;
        transition_to_list(&mut mode, &mut open);
        assert_eq!(mode, UtilityPaneMode::Filters);
        assert!(open);
    }

    #[test]
    fn toggle_flips_pane_open_without_changing_mode() {
        let mut pane_open = false;
        let pane_mode = UtilityPaneMode::ToolInspector;

        pane_open = !pane_open;
        assert!(pane_open);
        assert_eq!(pane_mode, UtilityPaneMode::ToolInspector);

        pane_open = !pane_open;
        assert!(!pane_open);
        assert_eq!(pane_mode, UtilityPaneMode::ToolInspector);
    }

    #[test]
    fn pane_visibility_changed_mirrors_widget_state() {
        let mut pane_open = true;

        let visible = false;
        if pane_open != visible {
            pane_open = visible;
        }
        assert!(!pane_open);

        let visible = false;
        if pane_open != visible {
            pane_open = visible;
        }
        assert!(!pane_open);
    }

    #[test]
    fn utility_pane_mode_maps_to_correct_stack_child_name() {
        assert_eq!(UtilityPaneMode::Filters.stack_child_name(), "filters");
        assert_eq!(
            UtilityPaneMode::ToolInspector.stack_child_name(),
            "tool-inspector"
        );
    }

    #[test]
    fn utility_pane_mode_maps_to_correct_sidebar_position() {
        assert_eq!(
            UtilityPaneMode::Filters.sidebar_position(),
            gtk::PackType::Start
        );
        assert_eq!(
            UtilityPaneMode::ToolInspector.sidebar_position(),
            gtk::PackType::End
        );
    }

    #[gtk::test]
    fn pane_stack_sizes_to_visible_child_instead_of_widest_child() {
        let filters = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let inspector = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let pane_stack = init::build_pane_stack(&filters, &inspector);

        assert!(!pane_stack.is_hhomogeneous());
        assert_eq!(pane_stack.visible_child_name().as_deref(), Some("filters"));
    }

    #[test]
    fn suppressed_pop_signal_is_consumed_without_state_sync() {
        let (should_sync, suppress_next) = detail_pop_sync_decision(true, true);
        assert!(!should_sync);
        assert!(!suppress_next);
    }

    #[test]
    fn suppressed_pop_signal_is_consumed_even_when_detail_already_hidden() {
        // Covers the edge case where the suppress flag is set but detail_visible
        // has already been cleared by another path before the popped signal fires.
        let (should_sync, suppress_next) = detail_pop_sync_decision(true, false);
        assert!(!should_sync);
        assert!(!suppress_next);
    }

    #[test]
    fn unsuppressed_pop_signal_syncs_when_detail_visible() {
        let (should_sync, suppress_next) = detail_pop_sync_decision(false, true);
        assert!(should_sync);
        assert!(!suppress_next);
    }

    #[test]
    fn unsuppressed_pop_signal_is_ignored_when_detail_hidden() {
        let (should_sync, suppress_next) = detail_pop_sync_decision(false, false);
        assert!(!should_sync);
        assert!(!suppress_next);
    }

    #[test]
    fn reindex_request_is_ignored_when_indexing_already_running() {
        assert_eq!(decide_reindex_action(true), ReindexAction::AlreadyRunning);
    }

    #[test]
    fn reindex_request_starts_full_reindex_when_idle() {
        assert_eq!(decide_reindex_action(false), ReindexAction::StartFull);
    }

    #[test]
    fn escape_priority_chain_search_then_inspector_then_back() {
        let mut search_visible = true;
        let mut detail_visible = true;
        let mut pane_open = true;
        let pane_mode = UtilityPaneMode::ToolInspector;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, pane_open, pane_mode),
            EscapeResolution::CloseSearch
        );
        search_visible = false;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, pane_open, pane_mode),
            EscapeResolution::CloseInspector
        );
        pane_open = false;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, pane_open, pane_mode),
            EscapeResolution::NavigateBack
        );
        detail_visible = false;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, pane_open, pane_mode),
            EscapeResolution::Noop
        );
    }

    #[test]
    fn analytics_workspace_hides_session_specific_header_controls() {
        let analytics = workspace_header_visibility(Workspace::Analytics, true, true);
        assert!(!analytics.search_ui_visible);
        assert!(!analytics.pane_controls_visible);
        assert!(!analytics.detail_actions_visible);
        assert!(analytics.indexing_progress_visible);

        let sessions = workspace_header_visibility(Workspace::Sessions, true, true);
        assert!(sessions.search_ui_visible);
        assert!(sessions.pane_controls_visible);
        assert!(sessions.detail_actions_visible);
        assert!(sessions.indexing_progress_visible);
    }

    #[test]
    fn search_is_disabled_in_analytics_workspace() {
        assert!(!workspace_allows_search(Workspace::Analytics));
        assert!(!resolve_search_mode_change(Workspace::Analytics, true));
    }

    #[test]
    fn search_mode_change_preserves_sessions_workspace_behavior() {
        assert!(workspace_allows_search(Workspace::Sessions));
        assert!(resolve_search_mode_change(Workspace::Sessions, true));
        assert!(!resolve_search_mode_change(Workspace::Sessions, false));
    }

    #[test]
    fn indexing_completion_marks_analytics_stale_and_refreshes_when_visible() {
        let hidden = analytics_indexing_completion_outcome(Workspace::Sessions);
        assert!(hidden.mark_stale);
        assert!(!hidden.refresh_immediately);

        let visible = analytics_indexing_completion_outcome(Workspace::Analytics);
        assert!(visible.mark_stale);
        assert!(visible.refresh_immediately);
    }

    #[test]
    fn project_sidebar_refresh_data_returns_error_for_directory_path() {
        let db_path = std::env::temp_dir();

        let result = load_sidebar_project_data(&db_path, AiAssistant::ALL);

        assert!(
            result.is_err(),
            "expected loading sidebar project data to fail for a directory path"
        );
    }
}
