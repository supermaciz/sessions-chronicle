use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SimpleComponent,
    WorkerController,
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw, gtk, main_application,
};

use adw::prelude::*;
use gtk::prelude::{
    ActionableExt, ApplicationExt, BoxExt, ButtonExt, Cast, EditableExt, GtkApplicationExt,
    GtkWindowExt, ObjectExt, OrientableExt, SettingsExt, ToggleButtonExt, WidgetExt,
};
use gtk::{gdk, gio, glib};
use std::{cell::Cell, fs, path::PathBuf, sync::Arc};

use crate::analytics_worker::{AnalyticsWorker, AnalyticsWorkerOutput};
use crate::config::{APP_ID, PROFILE};
use crate::database::SessionIndexer;
use crate::indexing_worker::{IndexingWorker, IndexingWorkerInput, IndexingWorkerOutput};
use crate::models::session::Tool;
use crate::session_sources::{SessionSources, select_db_filename};
use crate::ui::modals::{
    about::AboutDialog,
    preferences::{PreferencesDialog, PreferencesOutput},
    shortcuts::ShortcutsDialog,
};
#[cfg(test)]
use crate::ui::session_detail::SessionDetailMsg;
use crate::ui::{
    analytics_view::{AnalyticsView, AnalyticsViewOutput},
    session_detail::{SessionDetail, SessionDetailOutput},
    session_list::{SessionList, SessionListMsg, SessionListOutput},
    sidebar::{Sidebar, SidebarOutput},
    tool_inspector_pane::{ToolInspectorPane, ToolInspectorPaneMsg, ToolInspectorPaneOutput},
};
use crate::utils::terminal;

mod handlers;
mod helpers;
mod types;

#[cfg(test)]
use helpers::decide_reindex_action;
use helpers::transition_to_list;
#[cfg(test)]
use helpers::{
    active_search_query, analytics_indexing_completion_outcome, detail_pop_sync_decision,
    parent_session_load_failure_messages, resolve_escape_action, search_query_update_messages,
    transition_to_detail, workspace_header_visibility,
};
#[cfg(test)]
use types::EscapeResolution;
#[cfg(test)]
use types::ReindexAction;
use types::{ActiveSessionRef, UtilityPaneMode, Workspace};

/// Timeout in seconds for resume failure toast notifications
const RESUME_FAILURE_TOAST_TIMEOUT_SECS: u32 = 4;

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
    #[allow(dead_code)]
    indexing_worker: WorkerController<IndexingWorker>,
    #[allow(dead_code)]
    analytics_worker: WorkerController<AnalyticsWorker>,
    workspace_stack: adw::ViewStack,
    nav_view: adw::NavigationView,
    detail_page: adw::NavigationPage,
    suppress_next_detail_pop_sync: bool,
    pane_stack: gtk::Stack,
    toast_overlay: adw::ToastOverlay,
    db_path: PathBuf,
    sources: SessionSources,
    indexing: bool,
    pending_reindex_feedback: bool,
    active_workspace: Workspace,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    SearchModeChanged(bool),
    TogglePane,
    PaneVisibilityChanged(bool),
    SearchQueryChanged(String),
    WorkspaceChanged(Workspace),
    FiltersChanged(Vec<Tool>),
    SessionSelected(String),
    /// User-requested navigation back from detail to list.
    RequestNavigateBack,
    /// Detail page popped signal from `NavigationView`.
    NavigateBack,
    ResumeSession(String, Tool),
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
                            set_enable_show_gesture: true,
                            set_enable_hide_gesture: true,
                        },

                        #[name = "workspace_switcher_bar"]
                        adw::ViewSwitcherBar {
                            set_reveal: true,
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
        // Initialize child components
        let session_list =
            SessionList::builder()
                .launch(db_path.clone())
                .forward(sender.input_sender(), |msg| match msg {
                    SessionListOutput::SessionSelected(id) => AppMsg::SessionSelected(id),
                    SessionListOutput::ResumeRequested(id, tool) => AppMsg::ResumeSession(id, tool),
                });
        let analytics_view =
            AnalyticsView::builder()
                .launch(None)
                .forward(sender.input_sender(), |output| match output {
                    AnalyticsViewOutput::RefreshRequested => AppMsg::AnalyticsRefreshRequested,
                });
        let session_detail = SessionDetail::builder().launch(db_path.clone()).forward(
            sender.input_sender(),
            |msg| match msg {
                SessionDetailOutput::InspectToolCall(id) => AppMsg::InspectToolCall(id),
                SessionDetailOutput::InspectSubagent(id) => AppMsg::InspectSubagent(id),
            },
        );
        let sidebar = Sidebar::builder()
            .launch(())
            .forward(sender.input_sender(), |output| match output {
                SidebarOutput::FiltersChanged(tools) => AppMsg::FiltersChanged(tools),
            });
        let tool_inspector_pane = ToolInspectorPane::builder()
            .launch(Arc::new(db_path.clone()))
            .forward(sender.input_sender(), |output| match output {
                ToolInspectorPaneOutput::OpenChildSession(id) => AppMsg::OpenChildSession(id),
            });

        // Create preferences dialog once, with forwarded outputs
        let preferences_dialog = PreferencesDialog::builder().launch(()).forward(
            sender.input_sender(),
            |msg| match msg {
                PreferencesOutput::ReindexRequested => AppMsg::ReindexRequested,
            },
        );

        let indexing_worker = IndexingWorker::builder()
            .detach_worker(db_path.clone())
            .forward(sender.input_sender(), |output| match output {
                IndexingWorkerOutput::Completed { indexed, skipped } => {
                    AppMsg::IndexingCompleted { indexed, skipped }
                }
                IndexingWorkerOutput::Failed => AppMsg::IndexingFailed,
            });

        let analytics_worker = AnalyticsWorker::builder()
            .detach_worker(db_path.clone())
            .forward(sender.input_sender(), |output| match output {
                AnalyticsWorkerOutput::Loaded(data) => AppMsg::AnalyticsLoaded(data),
                AnalyticsWorkerOutput::Failed(error) => AppMsg::AnalyticsLoadFailed(error),
            });

        // Create NavigationView and pages before model
        let nav_view = adw::NavigationView::new();
        // Esc is routed via EscapeAction; disable native pop to avoid conflicts.
        nav_view.set_pop_on_escape(false);

        let session_list_page = adw::NavigationPage::builder()
            .title("Sessions")
            .tag("sessions")
            .child(session_list.widget())
            .build();
        nav_view.add(&session_list_page);

        // Register the detail page with the nav_view permanently (add without pushing).
        // This keeps the page parented to nav_view across push/pop cycles so it can
        // be safely re-pushed after the user navigates back.  Transient push()-only
        // pages are unparented on pop(), causing a GTK assertion on the next push().
        let detail_page = adw::NavigationPage::builder()
            .title("Session")
            .tag("detail")
            .child(session_detail.widget())
            .build();
        nav_view.add(&detail_page);

        // Sync state when detail page is popped natively (e.g. gestures).
        let popped_sender = sender.input_sender().clone();
        nav_view.connect_popped(move |_, page| {
            if page.tag().as_deref() == Some("detail") {
                popped_sender.send(AppMsg::NavigateBack).ok();
            }
        });

        // Build the utility pane Stack (sidebar content switcher)
        let pane_stack = gtk::Stack::new();
        pane_stack.set_transition_type(gtk::StackTransitionType::None);
        pane_stack.add_named(sidebar.widget(), Some("filters"));
        pane_stack.add_named(tool_inspector_pane.widget(), Some("tool-inspector"));
        pane_stack.set_visible_child_name("filters");

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
            session_list,
            analytics_view,
            session_detail,
            sidebar,
            tool_inspector_pane,
            preferences_dialog,
            indexing_worker,
            analytics_worker,
            workspace_stack: workspace_stack.clone(),
            nav_view: nav_view.clone(),
            detail_page: detail_page.clone(),
            suppress_next_detail_pop_sync: false,
            pane_stack,
            toast_overlay: adw::ToastOverlay::new(),
            db_path,
            sources,
            indexing: true,
            pending_reindex_feedback: false,
            active_workspace: Workspace::Sessions,
        };

        let widgets = view_output!();

        // Get the actual ToastOverlay from the root window's content
        model.toast_overlay = root
            .content()
            .and_then(|w| w.downcast::<adw::ToastOverlay>().ok())
            .expect("Root content should be a ToastOverlay");

        // Enable type-to-search: keystrokes captured from main window open SearchBar
        widgets
            .search_bar
            .set_key_capture_widget(Some(&widgets.main_window));

        // Bidirectional binding: ToggleButton.active <-> SearchBar.search-mode-enabled
        widgets
            .search_bar
            .bind_property("search-mode-enabled", &widgets.search_toggle, "active")
            .bidirectional()
            .sync_create()
            .build();

        // Sync SearchBar state changes (Escape, type-to-search, ToggleButton) back to model
        {
            let search_mode_sender = sender.input_sender().clone();
            let search_entry = widgets.search_entry.clone();
            widgets
                .search_bar
                .connect_search_mode_enabled_notify(move |bar| {
                    let enabled = bar.is_search_mode();
                    if enabled {
                        search_entry.grab_focus();
                    } else {
                        search_entry.set_text("");
                    }
                    search_mode_sender
                        .send(AppMsg::SearchModeChanged(enabled))
                        .ok();
                });
        }

        // Intercept Up/Down in SearchEntry to move session list selection
        {
            let session_list_sender = model.session_list.sender().clone();
            let key_controller = gtk::EventControllerKey::new();
            key_controller.connect_key_pressed(move |_ctrl, key, _code, _mods| match key {
                gdk::Key::Up => {
                    session_list_sender
                        .send(SessionListMsg::MoveSelection(-1))
                        .ok();
                    glib::Propagation::Stop
                }
                gdk::Key::Down => {
                    session_list_sender
                        .send(SessionListMsg::MoveSelection(1))
                        .ok();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            });
            widgets.search_entry.add_controller(key_controller);
        }

        // Enter in SearchEntry activates the selected session directly
        {
            let session_list_sender = model.session_list.sender().clone();
            widgets.search_entry.connect_activate(move |_| {
                session_list_sender
                    .send(SessionListMsg::ActivateSelected)
                    .ok();
            });
        }

        // Set up OverlaySplitView: sidebar = pane Stack, content = NavigationView
        widgets.overlay_split.set_sidebar(Some(&model.pane_stack));
        widgets.overlay_split.set_content(Some(&nav_view));
        widgets.overlay_split.set_max_sidebar_width(720.0);

        // Build top-level workspace stack and switchers.
        model.workspace_stack.add_titled(
            &widgets.overlay_split,
            Some(Workspace::Sessions.stack_name()),
            "Sessions",
        );
        model.workspace_stack.add_titled(
            model.analytics_view.widget(),
            Some(Workspace::Analytics.stack_name()),
            "Analytics",
        );
        let content_box = widgets
            .overlay_split
            .parent()
            .and_then(|parent| parent.downcast::<gtk::Box>().ok())
            .expect("overlay split should be inside the main content box");
        content_box.remove(&widgets.overlay_split);
        content_box.insert_child_after(
            &model.workspace_stack,
            Some(&widgets.search_bar.clone().upcast::<gtk::Widget>()),
        );
        widgets
            .workspace_switcher
            .set_stack(Some(&model.workspace_stack));
        widgets
            .workspace_switcher_bar
            .set_stack(Some(&model.workspace_stack));
        model
            .workspace_stack
            .set_visible_child_name(Workspace::Sessions.stack_name());

        let workspace_sender = sender.input_sender().clone();
        model
            .workspace_stack
            .connect_visible_child_name_notify(move |stack| {
                if let Some(name) = stack.visible_child_name().as_deref()
                    && let Some(workspace) = Workspace::from_stack_name(name)
                {
                    workspace_sender
                        .send(AppMsg::WorkspaceChanged(workspace))
                        .ok();
                }
            });

        // Wire notify::show-sidebar for bidirectional sync (gestures, collapse)
        let visibility_sender = sender.input_sender().clone();
        widgets
            .overlay_split
            .connect_show_sidebar_notify(move |split| {
                visibility_sender
                    .send(AppMsg::PaneVisibilityChanged(split.shows_sidebar()))
                    .ok();
            });

        // Add responsive collapse breakpoint
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            400.0,
            adw::LengthUnit::Sp,
        ));
        breakpoint.add_setter(&widgets.overlay_split, "collapsed", Some(&true.into()));
        root.add_breakpoint(breakpoint);

        let app = root.application().unwrap();
        let mut actions = RelmActionGroup::<WindowActionGroup>::new();

        let preferences_action = {
            let sender = sender.clone();
            RelmAction::<PreferencesAction>::new_stateless(move |_| {
                sender.input(AppMsg::ShowPreferences);
            })
        };

        let shortcuts_action = {
            RelmAction::<ShortcutsAction>::new_stateless(move |_| {
                ShortcutsDialog::builder().launch(()).detach();
            })
        };

        let about_action = {
            RelmAction::<AboutAction>::new_stateless(move |_| {
                AboutDialog::builder().launch(()).detach();
            })
        };

        let show_search_action = {
            let search_bar = widgets.search_bar.clone();
            let search_entry = widgets.search_entry.clone();
            RelmAction::<ShowSearchAction>::new_stateless(move |_| {
                search_bar.set_search_mode(true);
                search_entry.grab_focus();
            })
        };

        let toggle_pane_action = {
            let sender = sender.clone();
            RelmAction::<TogglePaneAction>::new_stateless(move |_| {
                sender.input(AppMsg::TogglePane);
            })
        };

        let quit_action = {
            let sender = sender.clone();
            RelmAction::<QuitAction>::new_stateless(move |_| {
                sender.input(AppMsg::Quit);
            })
        };

        let escape_action = {
            let sender = sender.clone();
            RelmAction::<EscapeAction>::new_stateless(move |_| {
                sender.input(AppMsg::Escape);
            })
        };

        // Connect actions with hotkeys
        app.set_accelerators_for_action::<QuitAction>(&["<Control>q"]);
        app.set_accelerators_for_action::<TogglePaneAction>(&["F9"]);
        app.set_accelerators_for_action::<ShowSearchAction>(&["<Control>f"]);
        app.set_accelerators_for_action::<ShortcutsAction>(&["<Control>question"]);
        app.set_accelerators_for_action::<PreferencesAction>(&["<Control>comma"]);
        app.set_accelerators_for_action::<EscapeAction>(&["Escape"]);

        actions.add_action(preferences_action);
        actions.add_action(shortcuts_action);
        actions.add_action(about_action);
        actions.add_action(show_search_action);
        actions.add_action(toggle_pane_action);
        actions.add_action(quit_action);
        actions.add_action(escape_action);
        actions.register_for_widget(&widgets.main_window);

        widgets.load_window_size();

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
            AppMsg::FiltersChanged(tools) => {
                self.session_list.emit(SessionListMsg::SetTools(tools));
            }
            AppMsg::SessionSelected(id) => self.handle_session_selected(id),
            AppMsg::RequestNavigateBack => self.handle_request_navigate_back(),
            AppMsg::NavigateBack => self.handle_navigate_back(),
            AppMsg::ShowPreferences => {
                let dialog_widget = self.preferences_dialog.widget();
                dialog_widget.present(Some(&main_application().windows()[0]));
            }
            AppMsg::ReindexRequested => self.handle_reindex_requested(),
            AppMsg::IndexingCompleted { indexed, skipped } => {
                self.handle_indexing_completed(indexed, skipped)
            }
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

        // Apply sidebar position and width based on current pane mode
        widgets
            .overlay_split
            .set_sidebar_position(self.pane_mode.sidebar_position());
        widgets
            .overlay_split
            .set_min_sidebar_width(self.pane_mode.sidebar_min_width());
        widgets
            .overlay_split
            .set_sidebar_width_fraction(self.pane_mode.sidebar_width_fraction());
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
        transition_to_list(&mut self.pane_mode);
        self.apply_pane_stack_switch();
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
    fn transition_to_list_sets_filters_preserving_visibility() {
        let mut mode = UtilityPaneMode::ToolInspector;
        transition_to_list(&mut mode);
        assert_eq!(mode, UtilityPaneMode::Filters);
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
    fn indexing_completion_marks_analytics_stale_and_refreshes_when_visible() {
        let hidden = analytics_indexing_completion_outcome(Workspace::Sessions);
        assert!(hidden.mark_stale);
        assert!(!hidden.refresh_immediately);

        let visible = analytics_indexing_completion_outcome(Workspace::Analytics);
        assert!(visible.mark_stale);
        assert!(visible.refresh_immediately);
    }
}
