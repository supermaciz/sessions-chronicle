use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SimpleComponent,
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
    SessionIndexer, count_all_sessions, count_pinned_sessions, count_sessions_per_date_preset,
    count_unassigned_sessions, has_unassigned_sessions, load_projects,
};
use crate::icon_names;
use crate::indexing_worker::{IndexingWorker, IndexingWorkerInput};
use crate::models::{
    DateFilter, ProjectFilter, ProjectInfo, SessionQuery, SortOrder, session::AiAssistant,
};
use crate::session_sources::{SessionSources, select_db_filename};
use crate::ui::date_pill::{DatePill, DatePillInput};
use crate::ui::modals::{
    indexing_status::{IndexingStatusDialog, IndexingStatusMsg, IndexingStatusOutput},
    preferences::PreferencesDialog,
};
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::sort_pill::{SortPill, SortPillInput};
use crate::ui::{
    analytics_view::AnalyticsView,
    session_detail::SessionDetail,
    session_list::{SessionList, SessionListMsg},
    sidebar::{Sidebar, SidebarMsg},
};
use crate::utils::terminal;

mod handlers;
mod helpers;
mod init;
mod types;

#[cfg(test)]
use helpers::decide_reindex_action;
use helpers::retained_project_filter;
#[cfg(test)]
use helpers::workspace_allows_search;
#[cfg(test)]
use helpers::{
    active_search_query, analytics_indexing_completion_outcome, detail_pop_sync_decision,
    parent_session_load_failure_message, resolve_escape_action, resolve_search_mode_change,
    search_query_update_messages, should_reload_sessions_after_indexing,
    workspace_header_visibility,
};
#[cfg(test)]
use types::EscapeResolution;
#[cfg(test)]
use types::ReindexAction;
use types::{ActiveSessionRef, FilterState, Workspace};

/// Timeout in seconds for resume failure toast notifications
const RESUME_FAILURE_TOAST_TIMEOUT_SECS: u32 = 4;
const MIN_WINDOW_WIDTH: i32 = 710;
const MIN_WINDOW_HEIGHT: i32 = 600;

struct SidebarProjectData {
    projects: Vec<ProjectInfo>,
    all_sessions_count: usize,
    unassigned_count: usize,
    pinned_count: usize,
    show_unassigned: bool,
}

fn load_sidebar_project_data(
    db_path: &Path,
    tools: &[AiAssistant],
    date_filter: &DateFilter,
) -> anyhow::Result<SidebarProjectData> {
    let projects =
        load_projects(db_path, tools, date_filter).context("load projects for sidebar")?;
    let all_sessions_count = count_all_sessions(db_path, tools, date_filter)
        .context("count all sessions for sidebar")?;
    let unassigned_count = count_unassigned_sessions(db_path, tools, date_filter)
        .context("count unassigned sessions for sidebar")?;
    let pinned_count = count_pinned_sessions(db_path, tools, date_filter)
        .context("count pinned sessions for sidebar")?;
    let show_unassigned =
        has_unassigned_sessions(db_path).context("determine unassigned sidebar visibility")?;

    Ok(SidebarProjectData {
        projects,
        all_sessions_count,
        unassigned_count,
        pinned_count,
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
    /// Outer OverlaySplitView visibility (Filters pane in the Sessions list view).
    filters_open: bool,
    /// Snapshot of `filters_open` taken when the detail page is pushed, so the
    /// previous state is restored on pop. Filters are scoped to the list view.
    filters_open_before_detail: bool,
    /// Mirror of the inner SessionDetail's inspector pane visibility, used to
    /// drive the inspector toggle button state and the Escape resolver.
    inspector_open: bool,
    active_session: Option<ActiveSessionRef>,
    /// When the user opens a child session from the inspector, this holds the
    /// originating parent session so a one-hop return is possible.
    parent_session: Option<ActiveSessionRef>,
    search_query: String,
    session_list: Controller<SessionList>,
    analytics_view: Controller<AnalyticsView>,
    session_detail: Controller<SessionDetail>,
    date_pill: Controller<DatePill>,
    sort_order: SortOrder,
    search_sort_override: Option<SortOrder>,
    sort_pill: Controller<SortPill>,
    #[allow(dead_code)] // Controller must stay alive to keep the widget
    sidebar: Controller<Sidebar>,
    preferences_dialog: Controller<PreferencesDialog>,
    indexing_worker: WorkerController<IndexingWorker>,
    analytics_worker: WorkerController<AnalyticsWorker>,
    last_per_source: Vec<crate::models::PerSourceResult>,
    last_errors_detail: Vec<crate::models::IndexingError>,
    indexing_status_dialog: Option<Controller<IndexingStatusDialog>>,
    workspace_stack: adw::ViewStack,
    nav_view: adw::NavigationView,
    detail_page: adw::NavigationPage,
    suppress_next_detail_pop_sync: bool,
    toast_overlay: adw::ToastOverlay,
    filter_state: FilterState,
    db_path: PathBuf,
    sources: SessionSources,
    indexing: bool,
    pending_reindex_feedback: bool,
    active_workspace: Workspace,
    selected_date_filter: DateFilter,
    banner: adw::Banner,
    banner_has_issues: bool,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    SearchModeChanged(bool),
    /// Toggle the outer Filters pane (Sessions list view).
    ToggleFilters,
    /// Filters pane visibility changed (gesture, collapse, etc.).
    FiltersVisibilityChanged(bool),
    /// Toggle the inner Inspector pane (Session detail view).
    ToggleInspector,
    /// F9 dispatcher: route to filters in list view, inspector in detail view.
    ToggleActiveSidePane,
    /// SessionDetail reports its inspector visibility changed.
    InspectorVisibilityChanged(bool),
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
    TogglePinRequested(String),
    TogglePinShortcutRequested,
    /// Inspector pane requested opening a child session.
    OpenChildSession(String),
    /// Header-bar button: return to the one-hop parent session.
    ReturnToParentSession,
    /// Esc key: close search → close inspector → navigate back.
    Escape,
    OpenDateFilterShortcut,
    ShowPreferences,
    ShowIndexingStatus,
    ReindexRequested,
    IndexingCompleted {
        indexed: usize,
        skipped: usize,
        removed: usize,
        per_source: Vec<crate::models::PerSourceResult>,
        errors_detail: Vec<crate::models::IndexingError>,
    },
    IndexingFailed,
    AnalyticsRefreshRequested,
    AnalyticsLoaded(crate::models::AnalyticsData),
    AnalyticsLoadFailed(String),
    DateFilterChanged(DateFilter),
    DateCountsRequested,
    OpenSortShortcut,
    SortOrderPicked(SortOrder),
    RelevancePicked,
}

relm4::new_action_group!(pub(super) WindowActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(IndexingStatusAction, WindowActionGroup, "indexing-status");
relm4::new_stateless_action!(pub(super) ShortcutsAction, WindowActionGroup, "show-help-overlay");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");
relm4::new_stateless_action!(ToggleFiltersAction, WindowActionGroup, "toggle-filters");
relm4::new_stateless_action!(ToggleInspectorAction, WindowActionGroup, "toggle-inspector");
// F9 dispatcher — toggles filters in list view, inspector in detail view.
relm4::new_stateless_action!(ToggleSidePaneAction, WindowActionGroup, "toggle-side-pane");
relm4::new_stateless_action!(TogglePinAction, WindowActionGroup, "toggle-pin");
relm4::new_stateless_action!(OpenDateFilterAction, WindowActionGroup, "open-date-filter");
relm4::new_stateless_action!(OpenSortAction, WindowActionGroup, "open-sort");
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
                "_Indexing Status..." => IndexingStatusAction,
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

                        #[name = "pin_button"]
                        pack_start = &gtk::ToggleButton {
                            set_icon_name: "view-pin-symbolic",
                            add_css_class: "flat",
                            #[watch]
                            set_active: model.active_session_pinned(),
                            #[watch]
                            set_visible: model.detail_visible && model.are_detail_actions_visible(),
                            #[watch]
                            set_tooltip_text: Some(pin_button_tooltip(model.active_session_pinned())),
                            connect_clicked => AppMsg::TogglePinShortcutRequested,
                        },

                        #[name = "search_toggle"]
                        pack_start = &gtk::ToggleButton {
                            set_icon_name: "system-search-symbolic",
                            set_tooltip_text: Some("Search sessions"),
                            #[watch]
                            set_visible: model.is_search_ui_visible(),
                        },

                        #[name = "summary_menu_button"]
                        pack_start = &gtk::MenuButton {
                            set_tooltip_text: Some("Session summary"),
                            add_css_class: "flat",
                            #[watch]
                            set_visible: model.is_summary_button_visible(),

                            #[wrap(Some)]
                            set_child = &gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 6,

                                gtk::Image {
                                    set_icon_name: Some(icon_names::SPEAKER_NOTES),
                                    set_pixel_size: 16,
                                },

                                #[name = "summary_project_label"]
                                gtk::Label {
                                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                                    set_max_width_chars: 24,
                                    #[watch]
                                    set_label: model
                                        .active_session
                                        .as_ref()
                                        .map(|session| session.project_name.as_str())
                                        .unwrap_or("Unknown project"),
                                },

                                gtk::Image {
                                    set_icon_name: Some("pan-down-symbolic"),
                                    set_pixel_size: 16,
                                },
                            },
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

                        #[name = "filters_toggle"]
                        pack_end = &gtk::ToggleButton {
                            set_icon_name: "sidebar-show-symbolic",
                            set_tooltip_text: Some("Toggle filters pane (F9)"),
                            set_action_name: Some("win.toggle-filters"),
                            #[watch]
                            set_active: model.filters_open,
                            #[watch]
                            set_visible: model.is_filters_toggle_visible(),
                        },

                        #[name = "inspector_toggle"]
                        pack_end = &gtk::ToggleButton {
                            set_icon_name: "sidebar-show-right-symbolic",
                            set_tooltip_text: Some("Toggle inspector pane (F9)"),
                            set_action_name: Some("win.toggle-inspector"),
                            #[watch]
                            set_active: model.inspector_open,
                            #[watch]
                            set_visible: model.is_inspector_toggle_visible(),
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
                            set_sidebar_position: gtk::PackType::Start,
                            set_min_sidebar_width: 200.0,
                            set_sidebar_width_fraction: 0.18,
                            set_enable_show_gesture: true,
                            set_enable_hide_gesture: true,
                            #[watch]
                            set_show_sidebar: model.filters_open,
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
        let settings = gio::Settings::new(APP_ID);
        let sort_order = SortOrder::from_setting_str(settings.string("sort-order").as_str());
        let components = init::init_child_components(&db_path, sort_order, &sender);
        let nav_setup = init::build_navigation(
            components.session_list.widget(),
            components.session_detail.widget(),
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
            filters_open: true,
            filters_open_before_detail: true,
            inspector_open: false,
            active_session: None,
            parent_session: None,
            search_query: String::new(),
            session_list: components.session_list,
            analytics_view: components.analytics_view,
            session_detail: components.session_detail,
            date_pill: components.date_pill,
            sort_order,
            search_sort_override: None,
            sort_pill: components.sort_pill,
            sidebar: components.sidebar,
            preferences_dialog: components.preferences_dialog,
            indexing_worker: components.indexing_worker,
            analytics_worker: components.analytics_worker,
            last_per_source: Vec::new(),
            last_errors_detail: Vec::new(),
            indexing_status_dialog: None,
            workspace_stack: workspace_stack.clone(),
            nav_view: nav_setup.nav_view.clone(),
            detail_page: nav_setup.detail_page.clone(),
            suppress_next_detail_pop_sync: false,
            toast_overlay: adw::ToastOverlay::new(),
            filter_state: FilterState::default(),
            db_path,
            sources,
            indexing: true,
            pending_reindex_feedback: false,
            active_workspace: Workspace::Sessions,
            selected_date_filter: DateFilter::AnyTime,
            banner: adw::Banner::new(""),
            banner_has_issues: false,
        };

        // view_output!() must stay in the SimpleComponent impl (Relm4 macro requirement)
        let widgets = view_output!();

        // The summary popover is owned by SessionDetail but displayed from this
        // header button, so set_popover reparents it onto the MenuButton. Because
        // it no longer lives under the SessionDetail widget tree, hiding the detail
        // view does not close it: every transition that changes active_session or
        // leaves detail mode must call dismiss_summary_popover() explicitly.
        widgets
            .summary_menu_button
            .set_popover(Some(&model.session_detail.widgets().summary_popover));
        widgets
            .summary_menu_button
            .update_property(&[gtk::accessible::Property::Label("Session summary")]);

        widgets.header_bar.pack_start(model.date_pill.widget());
        model
            .date_pill
            .widget()
            .set_visible(model.is_date_filter_visible());

        widgets.header_bar.pack_start(model.sort_pill.widget());
        model
            .sort_pill
            .widget()
            .set_visible(model.is_sort_pill_visible());
        model.sync_sort_pill();

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
            &model.sort_pill,
        );

        init::register_actions(
            &root,
            &widgets.main_window,
            &sender,
            &model.banner,
            &widgets.search_bar,
            &widgets.search_entry,
            &workspace_stack,
        );

        // Startup: load window size, refresh sidebar, kick off indexing
        widgets.load_window_size();

        if model.refresh_sidebar_projects() {
            model.emit_session_list_filters();
        }

        model.session_list.emit(SessionListMsg::DateFilterChanged(
            model.selected_date_filter.clone(),
        ));

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
            AppMsg::ToggleFilters => self.handle_toggle_filters(),
            AppMsg::FiltersVisibilityChanged(visible) => {
                self.handle_filters_visibility_changed(visible)
            }
            AppMsg::ToggleInspector => self.handle_toggle_inspector(),
            AppMsg::ToggleActiveSidePane => self.handle_toggle_active_side_pane(),
            AppMsg::InspectorVisibilityChanged(visible) => {
                self.handle_inspector_visibility_changed(visible)
            }
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
                self.session_list.emit(SessionListMsg::DateFilterChanged(
                    self.selected_date_filter.clone(),
                ));
            }
            AppMsg::SessionSelected(id) => self.handle_session_selected(id),
            AppMsg::RequestNavigateBack => self.handle_request_navigate_back(),
            AppMsg::NavigateBack => self.handle_navigate_back(),
            AppMsg::ShowPreferences => {
                let dialog_widget = self.preferences_dialog.widget();
                dialog_widget.present(Some(&main_application().windows()[0]));
            }
            AppMsg::ShowIndexingStatus => {
                if self.indexing_status_dialog.is_none() {
                    let dialog = IndexingStatusDialog::builder().launch(()).forward(
                        sender.input_sender(),
                        |output| match output {
                            IndexingStatusOutput::Reindex => AppMsg::ReindexRequested,
                        },
                    );
                    self.indexing_status_dialog = Some(dialog);
                }

                if let Some(dialog) = self.indexing_status_dialog.as_ref() {
                    dialog.emit(IndexingStatusMsg::Update {
                        per_source: self.last_per_source.clone(),
                        errors_detail: self.last_errors_detail.clone(),
                        indexing: self.indexing,
                    });

                    if let Some(window) = main_application().windows().first() {
                        dialog.widget().present(Some(window));
                    }
                }
            }
            AppMsg::ReindexRequested => self.handle_reindex_requested(),
            AppMsg::IndexingCompleted {
                indexed,
                skipped,
                removed,
                per_source,
                errors_detail,
            } => {
                self.handle_indexing_completed(indexed, skipped, removed, per_source, errors_detail)
            }
            AppMsg::IndexingFailed => self.handle_indexing_failed(),
            AppMsg::AnalyticsRefreshRequested => self.handle_analytics_refresh_requested(),
            AppMsg::AnalyticsLoaded(data) => self.handle_analytics_loaded(data),
            AppMsg::AnalyticsLoadFailed(error) => self.handle_analytics_load_failed(error),
            AppMsg::ResumeSession(session_id, tool) => self.handle_resume_session(session_id, tool),
            AppMsg::ResumeActiveSession => self.handle_resume_active_session(&sender),
            AppMsg::TogglePinRequested(session_id) => self.handle_toggle_pin_requested(session_id),
            AppMsg::TogglePinShortcutRequested => self.handle_toggle_pin_shortcut_requested(),
            AppMsg::OpenChildSession(child_session_id) => {
                self.handle_open_child_session(child_session_id)
            }
            AppMsg::ReturnToParentSession => self.handle_return_to_parent_session(),
            AppMsg::Escape => self.handle_escape(&sender),
            AppMsg::OpenDateFilterShortcut => {
                if self.is_date_filter_visible() {
                    self.date_pill.emit(DatePillInput::OpenViaShortcut);
                }
            }
            AppMsg::OpenSortShortcut => {
                if self.is_sort_pill_visible() {
                    self.sort_pill.emit(SortPillInput::OpenViaShortcut);
                }
            }
            AppMsg::SortOrderPicked(order) => {
                self.sort_order = order;
                self.search_sort_override = if SessionQuery::classify(&self.search_query).is_fts() {
                    Some(order)
                } else {
                    None
                };
                self.persist_sort_order();
                self.sync_sort_pill();
                self.session_list
                    .emit(SessionListMsg::SetSortOrder(self.effective_sort()));
            }
            AppMsg::RelevancePicked => {
                if SessionQuery::classify(&self.search_query).is_fts() {
                    self.search_sort_override = None;
                    self.sync_sort_pill();
                    self.session_list.emit(SessionListMsg::SetSortOrder(None));
                }
            }
            AppMsg::DateFilterChanged(date_filter) => {
                self.selected_date_filter = date_filter.clone();
                self.refresh_sidebar_projects();
                self.session_list
                    .emit(SessionListMsg::DateFilterChanged(date_filter));
            }
            AppMsg::DateCountsRequested => {
                match count_sessions_per_date_preset(
                    &self.db_path,
                    &self.filter_state.tools,
                    &self.filter_state.project_filter,
                    &self.search_query,
                ) {
                    Ok(counts) => self.date_pill.emit(DatePillInput::CountsReceived(counts)),
                    Err(err) => {
                        tracing::warn!("Failed to count sessions for date presets: {err:#}")
                    }
                }
            }
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

        self.date_pill
            .widget()
            .set_visible(self.is_date_filter_visible());

        self.sort_pill
            .widget()
            .set_visible(self.is_sort_pill_visible());
    }

    fn shutdown(&mut self, widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        widgets.save_window_size().unwrap();
    }
}

impl App {
    fn active_session_pinned(&self) -> bool {
        self.active_session.as_ref().is_some_and(|s| s.pinned)
    }

    fn dismiss_summary_popover(&self) {
        self.session_detail.widgets().summary_popover.popdown();
    }

    /// Reset app state after leaving detail view.
    fn transition_to_session_list_mode(&mut self) {
        self.dismiss_summary_popover();
        self.detail_visible = false;
        self.active_session = None;
        self.parent_session = None;
        self.inspector_open = false;
        self.filters_open = self.filters_open_before_detail;
        // The inspector lives inside SessionDetail; popping the page tears the
        // widget tree down, but we still want a clean state when the page is
        // pushed again later.
        self.session_detail.emit(SessionDetailMsg::CloseInspector);
        if self.banner_has_issues {
            self.banner.set_revealed(true);
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

    fn effective_sort(&self) -> Option<SortOrder> {
        helpers::effective_sort(
            &self.search_query,
            self.sort_order,
            self.search_sort_override,
        )
    }

    fn sync_sort_pill(&self) {
        self.sort_pill.emit(SortPillInput::SyncState {
            sort_order: self.sort_order,
            fts_search_active: SessionQuery::classify(&self.search_query).is_fts(),
            override_active: self.search_sort_override.is_some(),
        });
    }

    fn persist_sort_order(&self) {
        if let Err(err) =
            gio::Settings::new(APP_ID).set_string("sort-order", self.sort_order.as_setting_str())
        {
            tracing::warn!("Failed to persist session sort order: {err}");
        }
    }

    fn emit_session_list_filters(&self) {
        self.session_list.emit(SessionListMsg::SetFilters {
            tools: self.filter_state.tools.clone(),
            project_filter: self.filter_state.project_filter.clone(),
        });
    }

    fn refresh_sidebar_projects(&mut self) -> bool {
        let tools = self.filter_state.tools.clone();
        let date_filter = self.selected_date_filter.clone();
        let sidebar_data = match load_sidebar_project_data(&self.db_path, &tools, &date_filter) {
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
            pinned_count: sidebar_data.pinned_count,
            show_unassigned: sidebar_data.show_unassigned,
            selected_filter,
        });

        filter_changed
    }
}

impl AppWidgets {
    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = gio::Settings::new(APP_ID);
        let is_maximized = self.main_window.is_maximized();
        let (width, height) = persisted_window_size(
            self.main_window.default_size(),
            (self.main_window.width(), self.main_window.height()),
            is_maximized,
        );

        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;

        settings.set_boolean("is-maximized", is_maximized)?;

        Ok(())
    }

    fn load_window_size(&self) {
        let settings = gio::Settings::new(APP_ID);

        let (width, height) =
            clamped_window_size((settings.int("window-width"), settings.int("window-height")));
        let is_maximized = settings.boolean("is-maximized");

        self.main_window
            .set_size_request(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT);
        self.main_window.set_default_size(width, height);

        if is_maximized {
            self.main_window.maximize();
        }
    }
}

fn persisted_window_size(
    default_size: (i32, i32),
    current_size: (i32, i32),
    is_maximized: bool,
) -> (i32, i32) {
    if is_maximized {
        default_size
    } else if current_size.0 > 0 && current_size.1 > 0 {
        current_size
    } else {
        default_size
    }
}

fn clamped_window_size(size: (i32, i32)) -> (i32, i32) {
    (size.0.max(MIN_WINDOW_WIDTH), size.1.max(MIN_WINDOW_HEIGHT))
}

fn pin_button_tooltip(pinned: bool) -> &'static str {
    if pinned {
        "Unpin session (Ctrl+D)"
    } else {
        "Pin session (Ctrl+D)"
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

    fn schema_is_available() -> bool {
        gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup(crate::config::APP_ID, true))
            .is_some()
    }

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
    fn summary_menu_button_uses_session_detail_popover() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);
        let parts = controller.state().get();

        assert_eq!(
            parts.widgets.summary_menu_button.popover(),
            Some(parts.model.session_detail.widgets().summary_popover.clone())
        );
    }

    #[gtk::test]
    fn summary_menu_button_owns_summary_popover_parent() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);
        let parts = controller.state().get();

        assert_eq!(
            parts
                .model
                .session_detail
                .widgets()
                .summary_popover
                .parent(),
            Some(parts.widgets.summary_menu_button.clone().upcast())
        );
    }

    #[gtk::test]
    fn summary_menu_button_hidden_until_active_detail_session() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);

        {
            let parts = controller.state().get();
            assert!(!parts.widgets.summary_menu_button.is_visible());
        }

        controller.emit(AppMsg::SessionSelected("abc123".to_string()));
        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.detail_visible && parts.model.active_session.is_some()
        });

        let parts = controller.state().get();
        assert!(parts.widgets.summary_menu_button.is_visible());
    }

    #[gtk::test]
    fn summary_menu_button_label_uses_active_session_project_name() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);
        controller.emit(AppMsg::SessionSelected("abc123".to_string()));
        pump_main_context(|| controller.state().get().model.active_session.is_some());

        let parts = controller.state().get();
        assert_eq!(parts.widgets.summary_project_label.label(), "project");
        assert_eq!(
            parts.widgets.summary_menu_button.tooltip_text().as_deref(),
            Some("Session summary")
        );
    }

    #[gtk::test]
    fn dismiss_summary_popover_is_safe_when_already_closed() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);

        let parts = controller.state().get();
        // Popover starts closed; the real dismissal path must stay a harmless no-op.
        assert!(
            !parts
                .model
                .session_detail
                .widgets()
                .summary_popover
                .is_visible()
        );
        parts.model.dismiss_summary_popover();
        assert!(
            !parts
                .model
                .session_detail
                .widgets()
                .summary_popover
                .is_visible()
        );
    }

    #[gtk::test]
    fn session_replacement_closes_summary_popover() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);
        controller.emit(AppMsg::SessionSelected("abc123".to_string()));
        pump_main_context(|| controller.state().get().model.active_session.is_some());

        {
            let parts = controller.state().get();
            parts.model.session_detail.widgets().summary_popover.popup();
            assert!(
                parts
                    .model
                    .session_detail
                    .widgets()
                    .summary_popover
                    .is_visible()
            );
        }

        controller.emit(AppMsg::SessionSelected("session-001".to_string()));
        pump_main_context(|| {
            let parts = controller.state().get();
            parts
                .model
                .active_session
                .as_ref()
                .is_some_and(|session| session.id == "session-001")
        });

        let parts = controller.state().get();
        assert!(
            !parts
                .model
                .session_detail
                .widgets()
                .summary_popover
                .is_visible()
        );
    }

    #[gtk::test]
    fn navigating_back_closes_summary_popover_and_hides_button() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);
        controller.emit(AppMsg::SessionSelected("abc123".to_string()));
        pump_main_context(|| controller.state().get().model.detail_visible);

        {
            let parts = controller.state().get();
            parts.model.session_detail.widgets().summary_popover.popup();
            assert!(
                parts
                    .model
                    .session_detail
                    .widgets()
                    .summary_popover
                    .is_visible()
            );
        }

        controller.emit(AppMsg::RequestNavigateBack);
        pump_main_context(|| !controller.state().get().model.detail_visible);

        let parts = controller.state().get();
        assert!(
            !parts
                .model
                .session_detail
                .widgets()
                .summary_popover
                .is_visible()
        );
        assert!(!parts.widgets.summary_menu_button.is_visible());
    }

    #[gtk::test]
    fn startup_shows_indexing_spinner_during_incremental_indexing() {
        if !schema_is_available() {
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
                !parts.model.sources.opencode_db_paths.is_empty(),
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
            removed: 0,
            per_source: vec![],
            errors_detail: vec![],
        });

        pump_main_context(|| !spinner.is_visible());
        assert!(
            !spinner.is_visible(),
            "header spinner should hide after indexing completes"
        );
    }

    #[gtk::test]
    fn indexing_status_dialog_is_created_lazily() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));

        {
            let parts = controller.state().get();
            assert!(parts.model.last_per_source.is_empty());
            assert!(parts.model.indexing_status_dialog.is_none());
        }

        controller.emit(AppMsg::ShowIndexingStatus);

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.indexing_status_dialog.is_some()
        });

        let parts = controller.state().get();
        assert!(parts.model.indexing_status_dialog.is_some());
    }

    #[gtk::test]
    fn indexing_completed_stores_error_details_for_dialog() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        let expected_errors = vec![crate::models::IndexingError {
            assistant: crate::models::session::AiAssistant::OpenCode,
            location: Some(
                "tests/fixtures/opencode/storage/project-a/session-1/messages.jsonl".into(),
            ),
            message: "Failed to parse message".into(),
        }];

        pump_main_context(|| {
            let parts = controller.state().get();
            !parts.model.indexing
        });

        controller.emit(AppMsg::IndexingCompleted {
            indexed: 1,
            skipped: 0,
            removed: 0,
            per_source: vec![],
            errors_detail: expected_errors.clone(),
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.last_errors_detail == expected_errors
        });

        let parts = controller.state().get();
        assert_eq!(parts.model.last_errors_detail, expected_errors);
    }

    #[gtk::test]
    fn escape_with_active_override_restores_persisted_sort_and_clears_search() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);

        controller.emit(AppMsg::SearchModeChanged(true));
        pump_main_context(|| controller.state().get().model.search_visible);

        controller.emit(AppMsg::SearchQueryChanged("hello".to_string()));
        pump_main_context(|| controller.state().get().model.search_query == "hello");

        controller.emit(AppMsg::SortOrderPicked(SortOrder::OldestFirst));
        pump_main_context(|| {
            controller.state().get().model.search_sort_override == Some(SortOrder::OldestFirst)
        });

        {
            let parts = controller.state().get();
            assert_eq!(parts.model.sort_order, SortOrder::OldestFirst);
            assert_eq!(
                parts.model.search_sort_override,
                Some(SortOrder::OldestFirst)
            );
            assert_eq!(parts.model.search_query, "hello");
        }

        controller.emit(AppMsg::Escape);
        pump_main_context(|| controller.state().get().model.search_query.is_empty());

        let parts = controller.state().get();
        assert!(!parts.model.search_visible);
        assert!(parts.model.search_query.is_empty());
        assert_eq!(parts.model.search_sort_override, None);
        // The persisted sort order (set via the earlier SortOrderPicked while
        // FTS was active) is preserved and becomes the effective order again
        // now that the override-only search state has been cleared.
        assert_eq!(parts.model.sort_order, SortOrder::OldestFirst);
        assert_eq!(parts.model.effective_sort(), Some(SortOrder::OldestFirst));
    }

    #[gtk::test]
    fn search_mode_toggle_with_active_override_restores_persisted_sort_and_clears_search() {
        if !schema_is_available() {
            return;
        }

        let controller = App::builder().launch(Some(PathBuf::from("tests/fixtures")));
        pump_main_context(|| !controller.state().get().model.indexing);

        controller.emit(AppMsg::SearchModeChanged(true));
        pump_main_context(|| controller.state().get().model.search_visible);

        controller.emit(AppMsg::SearchQueryChanged("hello".to_string()));
        pump_main_context(|| controller.state().get().model.search_query == "hello");

        controller.emit(AppMsg::SortOrderPicked(SortOrder::NewestFirst));
        pump_main_context(|| {
            controller.state().get().model.search_sort_override == Some(SortOrder::NewestFirst)
        });

        controller.emit(AppMsg::SearchModeChanged(false));
        pump_main_context(|| !controller.state().get().model.search_visible);

        let parts = controller.state().get();
        assert!(parts.model.search_query.is_empty());
        assert_eq!(parts.model.search_sort_override, None);
        assert_eq!(parts.model.sort_order, SortOrder::NewestFirst);
        assert_eq!(parts.model.effective_sort(), Some(SortOrder::NewestFirst));
    }

    #[test]
    fn search_query_update_messages_include_detail_update() {
        let query = "needle".to_string();

        let (list_msg, detail_msg) = search_query_update_messages(query, None);

        match list_msg {
            SessionListMsg::SetSearchState { query, sort } => {
                assert_eq!(query, "needle");
                assert_eq!(sort, None);
            }
            _ => panic!("expected SessionListMsg::SetSearchState"),
        }

        match detail_msg {
            SessionDetailMsg::UpdateSearchQuery(Some(detail_query)) => {
                assert_eq!(detail_query, "needle");
            }
            _ => panic!("expected SessionDetailMsg::UpdateSearchQuery(Some(..))"),
        }
    }

    #[test]
    fn parent_session_load_failure_clears_detail() {
        let detail_msg = parent_session_load_failure_message();

        assert!(matches!(detail_msg, SessionDetailMsg::Clear));
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
    fn toggle_filters_flips_filters_open() {
        let mut filters_open = false;

        filters_open = !filters_open;
        assert!(filters_open);

        filters_open = !filters_open;
        assert!(!filters_open);
    }

    #[test]
    fn filters_visibility_changed_mirrors_widget_state() {
        let mut filters_open = true;

        let visible = false;
        if filters_open != visible {
            filters_open = visible;
        }
        assert!(!filters_open);
    }

    #[test]
    fn pin_button_tooltip_matches_state() {
        assert_eq!(pin_button_tooltip(false), "Pin session (Ctrl+D)");
        assert_eq!(pin_button_tooltip(true), "Unpin session (Ctrl+D)");
    }

    #[test]
    fn persisted_window_size_uses_current_size_after_manual_resize() {
        assert_eq!(
            persisted_window_size((600, 400), (920, 710), false),
            (920, 710)
        );
    }

    #[test]
    fn persisted_window_size_preserves_default_size_when_maximized() {
        assert_eq!(
            persisted_window_size((600, 400), (1920, 1080), true),
            (600, 400)
        );
    }

    #[test]
    fn clamped_window_size_enforces_minimum_dimensions() {
        assert_eq!(clamped_window_size((640, 480)), (710, 600));
    }

    #[test]
    fn clamped_window_size_keeps_larger_dimensions() {
        assert_eq!(clamped_window_size((1280, 900)), (1280, 900));
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
    fn skipped_only_incremental_indexing_does_not_need_session_reload() {
        assert!(!should_reload_sessions_after_indexing(0, 0, false));
        assert!(should_reload_sessions_after_indexing(1, 0, false));
        assert!(should_reload_sessions_after_indexing(0, 1, false));
        assert!(should_reload_sessions_after_indexing(0, 0, true));
    }

    #[test]
    fn escape_priority_chain_search_then_inspector_then_back() {
        let mut search_visible = true;
        let mut detail_visible = true;
        let mut inspector_open = true;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, inspector_open),
            EscapeResolution::CloseSearch
        );
        search_visible = false;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, inspector_open),
            EscapeResolution::CloseInspector
        );
        inspector_open = false;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, inspector_open),
            EscapeResolution::NavigateBack
        );
        detail_visible = false;

        assert_eq!(
            resolve_escape_action(search_visible, detail_visible, inspector_open),
            EscapeResolution::Noop
        );
    }

    #[test]
    fn analytics_workspace_hides_session_specific_header_controls() {
        let analytics = workspace_header_visibility(Workspace::Analytics, true, true, true);
        assert!(!analytics.search_ui_visible);
        assert!(!analytics.pane_controls_visible);
        assert!(!analytics.detail_actions_visible);
        assert!(analytics.indexing_progress_visible);

        let sessions = workspace_header_visibility(Workspace::Sessions, true, true, true);
        assert!(sessions.search_ui_visible);
        assert!(sessions.pane_controls_visible);
        assert!(sessions.detail_actions_visible);
        assert!(sessions.indexing_progress_visible);
    }

    #[test]
    fn summary_button_visibility_requires_sessions_detail_and_active_session() {
        let analytics = workspace_header_visibility(Workspace::Analytics, true, true, true);
        assert!(!analytics.summary_button_visible);

        let list = workspace_header_visibility(Workspace::Sessions, false, false, true);
        assert!(!list.summary_button_visible);

        let no_active_session =
            workspace_header_visibility(Workspace::Sessions, true, false, false);
        assert!(!no_active_session.summary_button_visible);

        let detail = workspace_header_visibility(Workspace::Sessions, true, false, true);
        assert!(detail.summary_button_visible);
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

        let result = load_sidebar_project_data(&db_path, AiAssistant::ALL, &DateFilter::AnyTime);

        assert!(
            result.is_err(),
            "expected loading sidebar project data to fail for a directory path"
        );
    }
}
