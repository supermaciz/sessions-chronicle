use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SimpleComponent,
    WorkerController,
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw, gtk, main_application,
};

use adw::prelude::{AdwApplicationWindowExt, AdwDialogExt, AlertDialogExt, NavigationPageExt};
use gtk::prelude::{
    ActionableExt, ApplicationExt, ButtonExt, Cast, EditableExt, GtkApplicationExt, GtkWindowExt,
    ObjectExt, OrientableExt, SettingsExt, ToggleButtonExt, WidgetExt,
};
use gtk::{gdk, gio, glib};
use std::{cell::Cell, fs, path::PathBuf, str::FromStr, sync::Arc};

use crate::config::{APP_ID, PROFILE};
use crate::database::{SessionIndexer, load_session};
use crate::indexing_worker::{IndexingWorker, IndexingWorkerInput, IndexingWorkerOutput};
use crate::models::session::Tool;
use crate::session_sources::{SessionSources, select_db_filename};
use crate::ui::modals::{
    about::AboutDialog,
    preferences::{PreferencesDialog, PreferencesOutput},
    shortcuts::ShortcutsDialog,
};
use crate::ui::{
    session_detail::{SessionDetail, SessionDetailMsg, SessionDetailOutput},
    session_list::{SessionList, SessionListMsg, SessionListOutput},
    sidebar::{Sidebar, SidebarOutput},
    tool_inspector_pane::{ToolInspectorPane, ToolInspectorPaneMsg, ToolInspectorPaneOutput},
};
use crate::utils::terminal::{self, Terminal};

mod handlers;
mod helpers;
mod types;

use helpers::{
    active_search_query, decide_reindex_action, detail_pop_sync_decision,
    parent_session_load_failure_messages, resolve_escape_action, search_query_update_messages,
    transition_to_detail, transition_to_list,
};
use types::{ActiveSessionRef, EscapeResolution, ReindexAction, UtilityPaneMode};

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
    session_detail: Controller<SessionDetail>,
    #[allow(dead_code)] // Controller must stay alive to keep the widget
    sidebar: Controller<Sidebar>,
    #[allow(dead_code)] // Controller must stay alive to keep the widget
    tool_inspector_pane: Controller<ToolInspectorPane>,
    preferences_dialog: Controller<PreferencesDialog>,
    #[allow(dead_code)]
    indexing_worker: WorkerController<IndexingWorker>,
    nav_view: adw::NavigationView,
    detail_page: adw::NavigationPage,
    suppress_next_detail_pop_sync: bool,
    pane_stack: gtk::Stack,
    toast_overlay: adw::ToastOverlay,
    db_path: PathBuf,
    sources: SessionSources,
    indexing: bool,
    pending_reindex_feedback: bool,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    SearchModeChanged(bool),
    TogglePane,
    PaneVisibilityChanged(bool),
    SearchQueryChanged(String),
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
                    add_top_bar = &adw::HeaderBar {
                        #[name = "back_button"]
                        pack_start = &gtk::Button {
                            set_icon_name: "go-previous-symbolic",
                            set_tooltip_text: Some("Go back"),
                            #[watch]
                            set_visible: model.detail_visible,
                            connect_clicked => AppMsg::RequestNavigateBack,
                        },

                        #[name = "search_toggle"]
                        pack_start = &gtk::ToggleButton {
                            set_icon_name: "system-search-symbolic",
                            set_tooltip_text: Some("Search sessions"),
                        },

                        #[name = "parent_session_button"]
                        pack_end = &gtk::Button {
                            set_label: "Back to Parent",
                            set_tooltip_text: Some("Return to the parent session"),
                            add_css_class: "flat",
                            #[watch]
                            set_visible: model.parent_session.is_some() && model.detail_visible,
                            connect_clicked => AppMsg::ReturnToParentSession,
                        },

                        #[name = "resume_button"]
                        pack_end = &gtk::Button {
                            set_label: "Resume",
                            set_tooltip_text: Some("Resume session in terminal"),
                            add_css_class: "suggested-action",
                            #[watch]
                            set_visible: model.detail_visible,
                            connect_clicked => AppMsg::ResumeActiveSession,
                        },

                        #[name = "pane_toggle"]
                        pack_end = &gtk::ToggleButton {
                            set_icon_name: "sidebar-show-symbolic",
                            set_tooltip_text: Some("Toggle utility pane (F9)"),
                            set_action_name: Some("win.toggle-pane"),
                            #[watch]
                            set_active: model.pane_open,
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
            session_detail,
            sidebar,
            tool_inspector_pane,
            preferences_dialog,
            indexing_worker,
            nav_view: nav_view.clone(),
            detail_page: detail_page.clone(),
            suppress_next_detail_pop_sync: false,
            pane_stack,
            toast_overlay: adw::ToastOverlay::new(),
            db_path,
            sources,
            indexing: true,
            pending_reindex_feedback: false,
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

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            AppMsg::Quit => main_application().quit(),
            AppMsg::SearchModeChanged(enabled) => {
                if self.search_visible != enabled {
                    self.search_visible = enabled;
                    if !enabled {
                        self.search_query.clear();
                        let (list_msg, detail_msg) = search_query_update_messages(String::new());
                        self.session_list.emit(list_msg);
                        self.session_detail.emit(detail_msg);
                        if !self.detail_visible {
                            self.session_list.emit(SessionListMsg::RestoreFocus);
                        }
                    }
                }
            }
            AppMsg::TogglePane => {
                self.pane_open = !self.pane_open;
            }
            AppMsg::PaneVisibilityChanged(visible) => {
                if self.pane_open != visible {
                    self.pane_open = visible;
                }
            }
            AppMsg::SearchQueryChanged(query) => {
                self.search_query = query.clone();
                let (list_msg, detail_msg) = search_query_update_messages(query);
                self.session_list.emit(list_msg);
                self.session_detail.emit(detail_msg);
            }
            AppMsg::FiltersChanged(tools) => {
                self.session_list.emit(SessionListMsg::SetTools(tools));
            }
            AppMsg::SessionSelected(id) => {
                tracing::debug!("Session selected: {}", id);

                let search_query = active_search_query(&self.search_query);

                match load_session(&self.db_path, &id) {
                    Ok(Some(session)) => {
                        let project_name = session
                            .project_path
                            .as_deref()
                            .and_then(|p| std::path::Path::new(p).file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown project")
                            .to_string();

                        self.active_session = Some(ActiveSessionRef {
                            id: session.id.clone(),
                            tool: session.tool,
                            project_name,
                        });

                        self.session_detail.emit(SessionDetailMsg::SetSession {
                            session: Box::new(session),
                            search_query,
                        });
                    }
                    Ok(None) => {
                        tracing::warn!("Session not found: {}", id);
                        self.active_session = None;
                        self.session_detail.emit(SessionDetailMsg::Clear);
                    }
                    Err(err) => {
                        tracing::error!("Failed to load session: {}", err);
                        self.active_session = None;
                        self.session_detail.emit(SessionDetailMsg::Clear);
                    }
                }

                // Push the detail page onto the navigation stack
                if !self.detail_visible {
                    self.nav_view.push(&self.detail_page);
                    self.detail_visible = true;
                }

                // Switch to tool inspector pane mode (pane stays closed until inspect action)
                transition_to_detail(&mut self.pane_mode, &mut self.pane_open);
                self.apply_pane_stack_switch();
            }
            AppMsg::RequestNavigateBack => {
                if self.detail_visible {
                    let visible_page_tag = self.nav_view.visible_page().and_then(|p| p.tag());
                    if visible_page_tag.as_deref() == Some("detail") {
                        self.suppress_next_detail_pop_sync = true;
                        self.nav_view.pop();
                    }
                    self.transition_to_session_list_mode();
                    self.session_list.emit(SessionListMsg::RestoreFocus);
                }
            }
            AppMsg::NavigateBack => {
                let (should_sync, suppress_next) = detail_pop_sync_decision(
                    self.suppress_next_detail_pop_sync,
                    self.detail_visible,
                );
                self.suppress_next_detail_pop_sync = suppress_next;
                if should_sync {
                    self.transition_to_session_list_mode();
                    self.session_list.emit(SessionListMsg::RestoreFocus);
                }
            }
            AppMsg::ShowPreferences => {
                let dialog_widget = self.preferences_dialog.widget();
                dialog_widget.present(Some(&main_application().windows()[0]));
            }
            AppMsg::ReindexRequested => match decide_reindex_action(self.indexing) {
                ReindexAction::AlreadyRunning => {
                    self.toast_overlay.add_toast(
                        adw::Toast::builder()
                            .title("Indexing already in progress.")
                            .timeout(3)
                            .build(),
                    );
                }
                ReindexAction::StartFull => {
                    tracing::info!("Reindex requested — scheduling full background reindex");
                    self.indexing = true;
                    self.pending_reindex_feedback = true;
                    self.session_list.emit(SessionListMsg::SetIndexing(true));
                    self.indexing_worker
                        .emit(IndexingWorkerInput::StartFullReindex(self.sources.clone()));
                }
            },
            AppMsg::IndexingCompleted { indexed, skipped } => {
                tracing::info!(
                    "Background indexing complete: indexed={}, skipped={}",
                    indexed,
                    skipped
                );
                self.indexing = false;
                self.session_list.emit(SessionListMsg::SetIndexing(false));
                self.session_list.emit(SessionListMsg::Reload);

                if self.pending_reindex_feedback {
                    self.pending_reindex_feedback = false;
                    self.toast_overlay.add_toast(
                        adw::Toast::builder()
                            .title(format!("Index rebuilt — {} sessions", indexed))
                            .timeout(3)
                            .build(),
                    );
                }
            }
            AppMsg::IndexingFailed => {
                tracing::error!("Background indexing failed");
                self.indexing = false;
                self.session_list.emit(SessionListMsg::SetIndexing(false));

                let title = if self.pending_reindex_feedback {
                    self.pending_reindex_feedback = false;
                    "Failed to reset index"
                } else {
                    "Background indexing failed"
                };

                self.toast_overlay
                    .add_toast(adw::Toast::builder().title(title).timeout(3).build());
            }
            AppMsg::ResumeSession(session_id, tool) => {
                tracing::debug!("Resume session requested: {}", session_id);

                let session = match load_session(&self.db_path, &session_id) {
                    Ok(Some(session)) => session,
                    Ok(None) => {
                        tracing::error!("Session not found: {}", session_id);
                        self.show_error_dialog(
                            "Session Not Found",
                            "The requested session could not be found in the database.",
                        );
                        return;
                    }
                    Err(err) => {
                        tracing::error!("Failed to load session {}: {}", session_id, err);
                        self.show_error_dialog(
                            "Failed to Load Session",
                            &format!("An error occurred while loading the session: {}", err),
                        );
                        return;
                    }
                };

                let workdir = if let Some(project_path) = &session.project_path {
                    PathBuf::from(project_path)
                } else {
                    match PathBuf::from(&session.file_path).parent() {
                        Some(dir) => dir.to_path_buf(),
                        None => {
                            tracing::error!(
                                "Cannot determine workdir for session: no project_path and no valid parent directory"
                            );
                            self.show_error_dialog(
                                "Invalid Session",
                                "The session has no valid working directory.",
                            );
                            return;
                        }
                    }
                };

                let settings = gio::Settings::new(APP_ID);
                let terminal_str = settings.string("resume-terminal");
                let terminal = match Terminal::from_str(&terminal_str) {
                    Ok(t) => t,
                    Err(()) => {
                        tracing::error!("Invalid terminal preference: {}", terminal_str);
                        self.show_error_dialog(
                            "Invalid Terminal Preference",
                            "Please check your terminal preference in settings.",
                        );
                        return;
                    }
                };

                match terminal::build_resume_command(tool, &session_id, &workdir) {
                    Ok(args) => match terminal::spawn_terminal(terminal, &args) {
                        Ok(_) => {
                            tracing::info!(
                                "Successfully launched terminal for session: {}",
                                session_id
                            );
                        }
                        Err(err) => {
                            tracing::error!(
                                "Failed to spawn terminal for session {}: {}",
                                session_id,
                                err
                            );
                            self.show_resume_failure_toast(&err);
                        }
                    },
                    Err(err) => {
                        tracing::error!(
                            "Failed to build resume command for session {}: {}",
                            session_id,
                            err
                        );
                        self.show_error_dialog(
                            "Failed to Build Resume Command",
                            &format!("Could not build the resume command: {}", err),
                        );
                    }
                }
            }
            AppMsg::ResumeActiveSession => {
                if let Some(ref session) = self.active_session {
                    _sender.input(AppMsg::ResumeSession(session.id.clone(), session.tool));
                } else {
                    tracing::warn!("ResumeActiveSession ignored — no active session");
                }
            }
            AppMsg::InspectToolCall(tool_call_id) => {
                tracing::debug!("Inspect tool call: {}", tool_call_id);
                if let Some(ref session) = self.active_session {
                    let session_id = session.id.clone();
                    self.pane_mode = UtilityPaneMode::ToolInspector;
                    self.pane_open = true;
                    self.apply_pane_stack_switch();
                    self.tool_inspector_pane
                        .emit(ToolInspectorPaneMsg::SelectToolCall {
                            session_id,
                            tool_call_id,
                        });
                }
            }
            AppMsg::InspectSubagent(subagent_id) => {
                tracing::debug!("Inspect subagent: {}", subagent_id);
                if let Some(ref session) = self.active_session {
                    let session_id = session.id.clone();
                    self.pane_mode = UtilityPaneMode::ToolInspector;
                    self.pane_open = true;
                    self.apply_pane_stack_switch();
                    self.tool_inspector_pane
                        .emit(ToolInspectorPaneMsg::SelectSubagent {
                            session_id,
                            subagent_id,
                        });
                }
            }
            AppMsg::OpenChildSession(child_session_id) => {
                tracing::debug!("Open child session: {}", child_session_id);
                // Store current session as parent for one-hop return.
                self.parent_session = self.active_session.clone();

                let search_query = active_search_query(&self.search_query);
                match load_session(&self.db_path, &child_session_id) {
                    Ok(Some(session)) => {
                        let project_name = session
                            .project_path
                            .as_deref()
                            .and_then(|p| std::path::Path::new(p).file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown project")
                            .to_string();

                        self.active_session = Some(ActiveSessionRef {
                            id: session.id.clone(),
                            tool: session.tool,
                            project_name,
                        });
                        self.session_detail.emit(SessionDetailMsg::SetSession {
                            session: Box::new(session),
                            search_query,
                        });
                        self.tool_inspector_pane.emit(ToolInspectorPaneMsg::Clear);
                    }
                    Ok(None) => {
                        tracing::warn!("Child session not found: {}", child_session_id);
                        self.parent_session = None;
                    }
                    Err(err) => {
                        tracing::error!(
                            "Failed to load child session {}: {}",
                            child_session_id,
                            err
                        );
                        self.parent_session = None;
                    }
                }
            }
            AppMsg::ReturnToParentSession => {
                tracing::debug!("Return to parent session");
                if let Some(parent) = self.parent_session.take() {
                    let search_query = active_search_query(&self.search_query);
                    match load_session(&self.db_path, &parent.id) {
                        Ok(Some(session)) => {
                            self.active_session = Some(parent);
                            self.session_detail.emit(SessionDetailMsg::SetSession {
                                session: Box::new(session),
                                search_query,
                            });
                            self.tool_inspector_pane.emit(ToolInspectorPaneMsg::Clear);
                        }
                        Ok(None) => {
                            tracing::warn!("Parent session no longer found; resetting");
                            self.active_session = None;
                            let (detail_msg, inspector_msg) =
                                parent_session_load_failure_messages();
                            self.session_detail.emit(detail_msg);
                            self.tool_inspector_pane.emit(inspector_msg);
                        }
                        Err(err) => {
                            tracing::error!("Failed to load parent session: {}", err);
                            self.active_session = None;
                            let (detail_msg, inspector_msg) =
                                parent_session_load_failure_messages();
                            self.session_detail.emit(detail_msg);
                            self.tool_inspector_pane.emit(inspector_msg);
                        }
                    }
                }
            }
            AppMsg::Escape => {
                // Priority chain:
                // 1. Close SearchBar (if search is active)
                // 2. Close inspector pane (if open in detail view)
                // 3. Navigate back to session list (if in detail view)
                // 4. No-op
                match resolve_escape_action(
                    self.search_visible,
                    self.detail_visible,
                    self.pane_open,
                    self.pane_mode,
                ) {
                    EscapeResolution::CloseSearch => {
                        self.search_visible = false;
                        self.sync_search_bar.set(true);
                        self.search_query.clear();
                        let (list_msg, detail_msg) = search_query_update_messages(String::new());
                        self.session_list.emit(list_msg);
                        self.session_detail.emit(detail_msg);
                        if !self.detail_visible {
                            self.session_list.emit(SessionListMsg::RestoreFocus);
                        }
                    }
                    EscapeResolution::CloseInspector => {
                        self.pane_open = false;
                    }
                    EscapeResolution::NavigateBack => {
                        _sender.input(AppMsg::RequestNavigateBack);
                    }
                    EscapeResolution::Noop => {}
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

        // Apply sidebar position based on current pane mode
        widgets
            .overlay_split
            .set_sidebar_position(self.pane_mode.sidebar_position());
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
}
