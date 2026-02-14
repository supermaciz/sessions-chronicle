use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SimpleComponent,
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw, gtk, main_application,
};

use adw::prelude::{AdwApplicationWindowExt, AdwDialogExt, AlertDialogExt, NavigationPageExt};
use gtk::prelude::{
    ActionableExt, ApplicationExt, ButtonExt, Cast, EditableExt, GtkApplicationExt, GtkWindowExt,
    ObjectExt, OrientableExt, SettingsExt, ToggleButtonExt, WidgetExt,
};
use gtk::{gio, glib};
use std::{fs, path::PathBuf, str::FromStr};

use crate::config::{APP_ID, PROFILE};
use crate::database::{SessionIndexer, load_session};
use crate::models::session::Tool;
use crate::session_sources::{SessionSources, select_db_filename};
use crate::ui::modals::{
    about::AboutDialog,
    preferences::{PreferencesDialog, PreferencesOutput},
    shortcuts::ShortcutsDialog,
};
use crate::ui::{
    detail_context_pane::{DetailContextPane, DetailContextPaneMsg, DetailContextPaneOutput},
    session_detail::{SessionDetail, SessionDetailMsg},
    session_list::{SessionList, SessionListMsg, SessionListOutput},
    sidebar::{Sidebar, SidebarOutput},
};
use crate::utils::terminal::{self, Terminal};

/// Timeout in seconds for resume failure toast notifications
const RESUME_FAILURE_TOAST_TIMEOUT_SECS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UtilityPaneMode {
    Filters,
    SessionContext,
}

impl UtilityPaneMode {
    fn stack_child_name(self) -> &'static str {
        match self {
            UtilityPaneMode::Filters => "filters",
            UtilityPaneMode::SessionContext => "session-context",
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveSessionRef {
    id: String,
    tool: Tool,
    #[allow(dead_code)] // Retained for future pane enrichment
    project_name: String,
}

pub(super) struct App {
    search_visible: bool,
    detail_visible: bool,
    pane_open: bool,
    pane_mode: UtilityPaneMode,
    active_session: Option<ActiveSessionRef>,
    search_query: String,
    session_list: Controller<SessionList>,
    session_detail: Controller<SessionDetail>,
    #[allow(dead_code)] // Controller must stay alive to keep the widget
    sidebar: Controller<Sidebar>,
    detail_context_pane: Controller<DetailContextPane>,
    preferences_dialog: Controller<PreferencesDialog>,
    nav_view: adw::NavigationView,
    detail_page: adw::NavigationPage,
    pane_stack: gtk::Stack,
    toast_overlay: adw::ToastOverlay,
    db_path: PathBuf,
    sources: SessionSources,
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
    NavigateBack,
    ResumeSession(String, Tool),
    ResumeFromPane,
    ShowPreferences,
    ReindexRequested,
}

relm4::new_action_group!(pub(super) WindowActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(pub(super) ShortcutsAction, WindowActionGroup, "show-help-overlay");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");
relm4::new_stateless_action!(TogglePaneAction, WindowActionGroup, "toggle-pane");
relm4::new_stateless_action!(ShowSearchAction, WindowActionGroup, "show-search");

fn active_search_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn search_query_update_messages(query: String) -> (SessionListMsg, SessionDetailMsg) {
    let detail_query = active_search_query(&query);

    (
        SessionListMsg::SetSearchQuery(query),
        SessionDetailMsg::UpdateSearchQuery(detail_query),
    )
}

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
                            connect_clicked => AppMsg::NavigateBack,
                        },

                        #[name = "search_toggle"]
                        pack_start = &gtk::ToggleButton {
                            set_icon_name: "system-search-symbolic",
                            set_tooltip_text: Some("Search sessions"),
                        },

                        #[name = "pane_toggle"]
                        pack_end = &gtk::ToggleButton {
                            set_icon_name: "sidebar-show-symbolic",
                            set_tooltip_text: Some("Toggle utility pane (F9)"),
                            set_action_name: Some("win.toggle-pane"),
                            #[watch]
                            set_active: model.pane_open,
                        },

                        pack_end = &gtk::MenuButton {
                            set_icon_name: "open-menu-symbolic",
                            set_menu_model: Some(&primary_menu),
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
        } else {
            let mut indexer: Option<SessionIndexer> = match SessionIndexer::new(&db_path) {
                Ok(i) => Some(i),
                Err(err) => {
                    tracing::error!("Failed to initialize session indexer: {}", err);
                    None
                }
            };

            if let Some(ref mut idx) = indexer {
                match idx.index_claude_sessions(&sources.claude_dir) {
                    Ok(count) => {
                        tracing::info!(
                            "Indexed {} Claude sessions from {}",
                            count,
                            sources.claude_dir.display()
                        );
                    }
                    Err(err) => {
                        tracing::error!("Failed to index Claude sessions: {}", err);
                    }
                }

                match idx.index_opencode_sessions(&sources.opencode_storage_root) {
                    Ok(count) => {
                        tracing::info!(
                            "Indexed {} OpenCode sessions from {}",
                            count,
                            sources.opencode_storage_root.display()
                        );
                    }
                    Err(err) => {
                        tracing::error!("Failed to index OpenCode sessions: {}", err);
                    }
                }

                match idx.index_codex_sessions(&sources.codex_dir) {
                    Ok(count) => {
                        tracing::info!(
                            "Indexed {} Codex sessions from {}",
                            count,
                            sources.codex_dir.display()
                        );
                    }
                    Err(err) => {
                        tracing::error!("Failed to index Codex sessions: {}", err);
                    }
                }

                match idx.index_vibe_sessions(&sources.vibe_dir) {
                    Ok(count) => {
                        tracing::info!(
                            "Indexed {} Mistral Vibe sessions from {}",
                            count,
                            sources.vibe_dir.display()
                        );
                    }
                    Err(err) => {
                        tracing::error!("Failed to index Mistral Vibe sessions: {}", err);
                    }
                }
            }
        }
        // Initialize child components
        let session_list =
            SessionList::builder()
                .launch(db_path.clone())
                .forward(sender.input_sender(), |msg| match msg {
                    SessionListOutput::SessionSelected(id) => AppMsg::SessionSelected(id),
                    SessionListOutput::ResumeRequested(id, tool) => AppMsg::ResumeSession(id, tool),
                });
        let session_detail = SessionDetail::builder().launch(db_path.clone()).detach();
        let sidebar = Sidebar::builder()
            .launch(())
            .forward(sender.input_sender(), |output| match output {
                SidebarOutput::FiltersChanged(tools) => AppMsg::FiltersChanged(tools),
            });
        let detail_context_pane =
            DetailContextPane::builder()
                .launch(())
                .forward(sender.input_sender(), |output| match output {
                    DetailContextPaneOutput::ResumeClicked => AppMsg::ResumeFromPane,
                });

        // Create preferences dialog once, with forwarded outputs
        let preferences_dialog = PreferencesDialog::builder().launch(()).forward(
            sender.input_sender(),
            |msg| match msg {
                PreferencesOutput::ReindexRequested => AppMsg::ReindexRequested,
            },
        );

        // Create NavigationView and pages before model
        let nav_view = adw::NavigationView::new();

        let session_list_page = adw::NavigationPage::builder()
            .title("Sessions")
            .tag("sessions")
            .child(session_list.widget())
            .build();
        nav_view.add(&session_list_page);

        // Create detail page (but don't push yet)
        let detail_page = adw::NavigationPage::builder()
            .title("Session")
            .tag("detail")
            .child(session_detail.widget())
            .build();

        // Connect popped signal to reset detail_visible when user navigates back
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
        pane_stack.add_named(detail_context_pane.widget(), Some("session-context"));
        pane_stack.set_visible_child_name("filters");

        // Create model with a temporary toast_overlay (will be replaced after view_output!)
        let mut model = Self {
            search_visible: false,
            detail_visible: false,
            pane_open: true,
            pane_mode: UtilityPaneMode::Filters,
            active_session: None,
            search_query: String::new(),
            session_list,
            session_detail,
            sidebar,
            detail_context_pane,
            preferences_dialog,
            nav_view: nav_view.clone(),
            detail_page: detail_page.clone(),
            pane_stack,
            toast_overlay: adw::ToastOverlay::new(),
            db_path,
            sources,
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
            RelmAction::<QuitAction>::new_stateless(move |_| {
                sender.input(AppMsg::Quit);
            })
        };

        // Connect actions with hotkeys
        app.set_accelerators_for_action::<QuitAction>(&["<Control>q"]);
        app.set_accelerators_for_action::<TogglePaneAction>(&["F9"]);
        app.set_accelerators_for_action::<ShowSearchAction>(&["<Control>f"]);
        app.set_accelerators_for_action::<ShortcutsAction>(&["<Control>question"]);
        app.set_accelerators_for_action::<PreferencesAction>(&["<Control>comma"]);

        actions.add_action(preferences_action);
        actions.add_action(shortcuts_action);
        actions.add_action(about_action);
        actions.add_action(show_search_action);
        actions.add_action(toggle_pane_action);
        actions.add_action(quit_action);
        actions.register_for_widget(&widgets.main_window);

        widgets.load_window_size();

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

                // Load session once, shared by context pane and detail view
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
                            project_name: project_name.clone(),
                        });

                        self.detail_context_pane
                            .emit(DetailContextPaneMsg::SetSession {
                                project_name,
                                tool: session.tool,
                            });

                        self.session_detail.emit(SessionDetailMsg::SetSession {
                            session,
                            search_query,
                        });
                    }
                    Ok(None) => {
                        tracing::warn!("Session not found: {}", id);
                        self.active_session = None;
                        self.detail_context_pane
                            .emit(DetailContextPaneMsg::ClearSession);
                        self.session_detail.emit(SessionDetailMsg::Clear);
                    }
                    Err(err) => {
                        tracing::error!("Failed to load session: {}", err);
                        self.active_session = None;
                        self.detail_context_pane
                            .emit(DetailContextPaneMsg::ClearSession);
                        self.session_detail.emit(SessionDetailMsg::Clear);
                    }
                }

                // Push the detail page onto the navigation stack
                if !self.detail_visible {
                    self.nav_view.push(&self.detail_page);
                    self.detail_visible = true;
                }

                // Switch to session context pane mode (open by default)
                transition_to_detail(&mut self.pane_mode, &mut self.pane_open);
                self.apply_pane_stack_switch();
            }
            AppMsg::NavigateBack => {
                if self.detail_visible {
                    self.detail_visible = false;
                    // Only pop if we're currently showing detail (avoid double-pop from signal)
                    if self
                        .nav_view
                        .visible_page()
                        .and_then(|p| p.tag())
                        .as_deref()
                        == Some("detail")
                    {
                        self.nav_view.pop();
                    }

                    // Return to filter pane mode
                    self.active_session = None;
                    transition_to_list(&mut self.pane_mode);
                    self.apply_pane_stack_switch();
                }
            }
            AppMsg::ShowPreferences => {
                let dialog_widget = self.preferences_dialog.widget();
                dialog_widget.present(Some(&main_application().windows()[0]));
            }
            AppMsg::ReindexRequested => {
                tracing::info!("Reindex requested — clearing and rebuilding index");
                match SessionIndexer::new(&self.db_path) {
                    Ok(mut indexer) => {
                        if let Err(err) = indexer.clear_all_sessions() {
                            tracing::error!("Failed to clear sessions: {}", err);
                            self.toast_overlay.add_toast(
                                adw::Toast::builder()
                                    .title("Failed to reset index")
                                    .timeout(3)
                                    .build(),
                            );
                            return;
                        }

                        let mut total = 0usize;
                        match indexer.index_claude_sessions(&self.sources.claude_dir) {
                            Ok(n) => total += n,
                            Err(err) => tracing::warn!("Claude sessions: {}", err),
                        }
                        match indexer.index_opencode_sessions(&self.sources.opencode_storage_root) {
                            Ok(n) => total += n,
                            Err(err) => tracing::warn!("OpenCode sessions: {}", err),
                        }
                        match indexer.index_codex_sessions(&self.sources.codex_dir) {
                            Ok(n) => total += n,
                            Err(err) => tracing::warn!("Codex sessions: {}", err),
                        }
                        match indexer.index_vibe_sessions(&self.sources.vibe_dir) {
                            Ok(n) => total += n,
                            Err(err) => tracing::warn!("Vibe sessions: {}", err),
                        }

                        tracing::info!("Reindex complete: {} sessions indexed", total);
                        self.session_list.emit(SessionListMsg::Reload);
                        self.toast_overlay.add_toast(
                            adw::Toast::builder()
                                .title(format!("Index rebuilt — {} sessions", total))
                                .timeout(3)
                                .build(),
                        );
                    }
                    Err(err) => {
                        tracing::error!("Failed to open indexer for reindex: {}", err);
                        self.toast_overlay.add_toast(
                            adw::Toast::builder()
                                .title("Failed to reset index")
                                .timeout(3)
                                .build(),
                        );
                    }
                }
            }
            AppMsg::ResumeSession(session_id, tool) => {
                tracing::debug!("Resume session requested: {}", session_id);

                // Load session from DB
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

                // Determine workdir
                let workdir = if let Some(project_path) = &session.project_path {
                    PathBuf::from(project_path)
                } else {
                    // Use the directory containing the session file
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

                // Get terminal preference
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

                // Build and spawn resume command
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
            AppMsg::ResumeFromPane => {
                if let Some(ref session) = self.active_session {
                    _sender.input(AppMsg::ResumeSession(session.id.clone(), session.tool));
                } else {
                    tracing::warn!("ResumeFromPane ignored — no active session");
                }
            }
        }
    }

    fn shutdown(&mut self, widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        widgets.save_window_size().unwrap();
    }
}

/// Pure transition: switch to detail mode (session context pane, open).
fn transition_to_detail(pane_mode: &mut UtilityPaneMode, pane_open: &mut bool) {
    *pane_mode = UtilityPaneMode::SessionContext;
    *pane_open = true;
}

/// Pure transition: switch to list mode (filters pane), preserving pane visibility.
fn transition_to_list(pane_mode: &mut UtilityPaneMode) {
    *pane_mode = UtilityPaneMode::Filters;
}

impl App {
    /// Apply the current `pane_mode` to the Stack widget, with verification.
    fn apply_pane_stack_switch(&self) {
        let target = self.pane_mode.stack_child_name();
        self.pane_stack.set_visible_child_name(target);

        // Verify the switch succeeded
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
    fn active_search_query_treats_blank_input_as_none() {
        assert_eq!(active_search_query(""), None);
        assert_eq!(active_search_query("   \n\t  "), None);
        assert_eq!(
            active_search_query("  needle  "),
            Some("needle".to_string())
        );
    }

    #[test]
    fn transition_to_detail_sets_session_context_and_open() {
        let mut mode = UtilityPaneMode::Filters;
        let mut open = false;
        transition_to_detail(&mut mode, &mut open);
        assert_eq!(mode, UtilityPaneMode::SessionContext);
        assert!(open);
    }

    #[test]
    fn transition_to_list_sets_filters_preserving_visibility() {
        let mut mode = UtilityPaneMode::SessionContext;
        transition_to_list(&mut mode);
        assert_eq!(mode, UtilityPaneMode::Filters);
    }

    #[test]
    fn toggle_flips_pane_open_without_changing_mode() {
        let mut pane_open = false;
        let pane_mode = UtilityPaneMode::SessionContext;

        // Simulate TogglePane: flips open, does not change mode
        pane_open = !pane_open;
        assert!(pane_open);
        assert_eq!(pane_mode, UtilityPaneMode::SessionContext);

        pane_open = !pane_open;
        assert!(!pane_open);
        assert_eq!(pane_mode, UtilityPaneMode::SessionContext);
    }

    #[test]
    fn pane_visibility_changed_mirrors_widget_state() {
        let mut pane_open = true;

        // Simulate PaneVisibilityChanged(false) from gesture
        let visible = false;
        if pane_open != visible {
            pane_open = visible;
        }
        assert!(!pane_open);

        // No-op update
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
            UtilityPaneMode::SessionContext.stack_child_name(),
            "session-context"
        );
    }
}
