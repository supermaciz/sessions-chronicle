use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, SimpleComponent,
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw, gtk, main_application,
};

use adw::prelude::{AdwApplicationWindowExt, AdwDialogExt, AlertDialogExt, NavigationPageExt};
use gtk::prelude::{
    ApplicationExt, ButtonExt, Cast, EditableExt, GtkApplicationExt, GtkWindowExt, OrientableExt,
    SettingsExt, ToggleButtonExt, WidgetExt,
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
    session_detail::{SessionDetail, SessionDetailMsg, SessionDetailOutput},
    session_list::{SessionList, SessionListMsg, SessionListOutput},
    sidebar::{Sidebar, SidebarOutput},
};
use crate::utils::terminal::{self, Terminal};

/// Timeout in seconds for resume failure toast notifications
const RESUME_FAILURE_TOAST_TIMEOUT_SECS: u32 = 4;

pub(super) struct App {
    search_visible: bool,
    detail_visible: bool,
    search_query: String,
    session_list: Controller<SessionList>,
    session_detail: Controller<SessionDetail>,
    sidebar: Controller<Sidebar>,
    preferences_dialog: Controller<PreferencesDialog>,
    nav_view: adw::NavigationView,
    detail_page: adw::NavigationPage,
    toast_overlay: adw::ToastOverlay,
    db_path: PathBuf,
    sources: SessionSources,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    ToggleSearch,
    SearchQueryChanged(String),
    FiltersChanged(Vec<Tool>),
    SessionSelected(String),
    NavigateBack,
    ResumeSession(String, Tool),
    ShowPreferences,
    ReindexRequested,
}

relm4::new_action_group!(pub(super) WindowActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(pub(super) ShortcutsAction, WindowActionGroup, "show-help-overlay");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");

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

                        pack_start = &gtk::ToggleButton {
                            set_icon_name: "system-search-symbolic",
                            set_tooltip_text: Some("Search sessions"),
                            #[watch]
                            set_active: model.search_visible,
                            connect_toggled => AppMsg::ToggleSearch,
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
                            #[watch]
                            set_search_mode: model.search_visible,

                            #[wrap(Some)]
                            set_child = &gtk::SearchEntry {
                                set_placeholder_text: Some("Search sessions..."),
                                set_hexpand: true,
                                connect_search_changed[sender] => move |entry| {
                                    sender.input(AppMsg::SearchQueryChanged(entry.text().to_string()));
                                },
                            },
                        },

                        #[name = "nav_split"]
                        adw::NavigationSplitView {
                            set_vexpand: true,
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
        let session_detail = SessionDetail::builder().launch(db_path.clone()).forward(
            sender.input_sender(),
            |msg| match msg {
                SessionDetailOutput::ResumeRequested(id, tool) => AppMsg::ResumeSession(id, tool),
            },
        );
        let sidebar = Sidebar::builder()
            .launch(())
            .forward(sender.input_sender(), |output| match output {
                SidebarOutput::FiltersChanged(tools) => AppMsg::FiltersChanged(tools),
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

        // Create model with a temporary toast_overlay (will be replaced after view_output!)
        let mut model = Self {
            search_visible: false,
            detail_visible: false,
            search_query: String::new(),
            session_list,
            session_detail,
            sidebar,
            preferences_dialog,
            nav_view: nav_view.clone(),
            detail_page: detail_page.clone(),
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

        // Add child components to NavigationSplitView
        let sidebar_page = adw::NavigationPage::builder()
            .title("Filters")
            .child(model.sidebar.widget())
            .build();
        widgets.nav_split.set_sidebar(Some(&sidebar_page));

        // Wrap NavigationView in a NavigationPage for the split view
        let content_page = adw::NavigationPage::builder()
            .title("Sessions")
            .child(&nav_view)
            .build();
        widgets.nav_split.set_content(Some(&content_page));

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

        let quit_action = {
            RelmAction::<QuitAction>::new_stateless(move |_| {
                sender.input(AppMsg::Quit);
            })
        };

        // Connect action with hotkeys
        app.set_accelerators_for_action::<QuitAction>(&["<Control>q"]);

        actions.add_action(preferences_action);
        actions.add_action(shortcuts_action);
        actions.add_action(about_action);
        actions.add_action(quit_action);
        actions.register_for_widget(&widgets.main_window);

        widgets.load_window_size();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            AppMsg::Quit => main_application().quit(),
            AppMsg::ToggleSearch => {
                self.search_visible = !self.search_visible;
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
                // Load the session in the detail view with search query
                self.session_detail
                    .emit(SessionDetailMsg::SetSession { id, search_query });
                // Push the detail page onto the navigation stack
                if !self.detail_visible {
                    self.nav_view.push(&self.detail_page);
                    self.detail_visible = true;
                }
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
        }
    }

    fn shutdown(&mut self, widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        widgets.save_window_size().unwrap();
    }
}

impl App {
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
}
