use std::path::Path;
use std::sync::Arc;

use relm4::{
    Component, ComponentController, ComponentSender, Controller, WorkerController,
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw, gtk,
};

use adw::prelude::*;
use gtk::prelude::{BoxExt, Cast, EditableExt, ObjectExt, WidgetExt};
use gtk::{gdk, glib};

use crate::analytics_worker::{AnalyticsWorker, AnalyticsWorkerOutput};
use crate::indexing_worker::{IndexingWorker, IndexingWorkerOutput};
use crate::ui::modals::{
    about::AboutDialog,
    preferences::{PreferencesDialog, PreferencesOutput},
    shortcuts::ShortcutsDialog,
};
use crate::ui::{
    analytics_view::{AnalyticsView, AnalyticsViewOutput},
    session_detail::{SessionDetail, SessionDetailOutput},
    session_list::{SessionList, SessionListMsg, SessionListOutput},
    sidebar::{Sidebar, SidebarOutput},
    tool_inspector_pane::{ToolInspectorPane, ToolInspectorPaneOutput},
};

use super::helpers::workspace_allows_search;
use super::types::Workspace;
use super::{
    AboutAction, App, AppMsg, EscapeAction, IndexingStatusAction, PreferencesAction, QuitAction,
    ShortcutsAction, ShowSearchAction, TogglePaneAction, WindowActionGroup,
};

/// Holds all child controllers and workers created during init.
pub(super) struct ChildComponents {
    pub(super) session_list: Controller<SessionList>,
    pub(super) analytics_view: Controller<AnalyticsView>,
    pub(super) session_detail: Controller<SessionDetail>,
    pub(super) sidebar: Controller<Sidebar>,
    pub(super) tool_inspector_pane: Controller<ToolInspectorPane>,
    pub(super) preferences_dialog: Controller<PreferencesDialog>,
    pub(super) indexing_worker: WorkerController<IndexingWorker>,
    pub(super) analytics_worker: WorkerController<AnalyticsWorker>,
}

/// Holds the NavigationView, detail page, and pane stack built during init.
pub(super) struct NavigationSetup {
    pub(super) nav_view: adw::NavigationView,
    pub(super) detail_page: adw::NavigationPage,
    pub(super) pane_stack: gtk::Stack,
}

pub(super) fn init_child_components(
    db_path: &Path,
    sender: &ComponentSender<App>,
) -> ChildComponents {
    let session_list = SessionList::builder()
        .launch(db_path.to_path_buf())
        .forward(sender.input_sender(), |msg| match msg {
            SessionListOutput::SessionSelected(id) => AppMsg::SessionSelected(id),
            SessionListOutput::ResumeRequested(id, tool) => AppMsg::ResumeSession(id, tool),
        });
    let analytics_view = AnalyticsView::builder().launch(None).forward(
        sender.input_sender(),
        |output| match output {
            AnalyticsViewOutput::RefreshRequested => AppMsg::AnalyticsRefreshRequested,
        },
    );
    let session_detail = SessionDetail::builder()
        .launch(db_path.to_path_buf())
        .forward(sender.input_sender(), |msg| match msg {
            SessionDetailOutput::InspectToolCall(id) => AppMsg::InspectToolCall(id),
            SessionDetailOutput::InspectSubagent(id) => AppMsg::InspectSubagent(id),
        });
    let sidebar =
        Sidebar::builder()
            .launch(())
            .forward(sender.input_sender(), |output| match output {
                SidebarOutput::FiltersChanged {
                    tools,
                    project_filter,
                } => AppMsg::FiltersChanged {
                    tools,
                    project_filter,
                },
            });
    let tool_inspector_pane = ToolInspectorPane::builder()
        .launch(Arc::new(db_path.to_path_buf()))
        .forward(sender.input_sender(), |output| match output {
            ToolInspectorPaneOutput::OpenChildSession(id) => AppMsg::OpenChildSession(id),
        });

    // Create preferences dialog once, with forwarded outputs
    let preferences_dialog = PreferencesDialog::builder()
        .launch(db_path.to_path_buf())
        .forward(sender.input_sender(), |msg| match msg {
            PreferencesOutput::ReindexRequested => AppMsg::ReindexRequested,
        });

    let indexing_worker = IndexingWorker::builder()
        .detach_worker(db_path.to_path_buf())
        .forward(sender.input_sender(), |output| match output {
            IndexingWorkerOutput::Completed {
                indexed,
                skipped,
                per_source,
                errors_detail,
            } => AppMsg::IndexingCompleted {
                indexed,
                skipped,
                per_source,
                errors_detail,
            },
            IndexingWorkerOutput::Failed => AppMsg::IndexingFailed,
        });

    let analytics_worker = AnalyticsWorker::builder()
        .detach_worker(db_path.to_path_buf())
        .forward(sender.input_sender(), |output| match output {
            AnalyticsWorkerOutput::Loaded(data) => AppMsg::AnalyticsLoaded(data),
            AnalyticsWorkerOutput::Failed(error) => AppMsg::AnalyticsLoadFailed(error),
        });

    ChildComponents {
        session_list,
        analytics_view,
        session_detail,
        sidebar,
        tool_inspector_pane,
        preferences_dialog,
        indexing_worker,
        analytics_worker,
    }
}

pub(super) fn build_navigation(
    session_list_widget: &impl IsA<gtk::Widget>,
    session_detail_widget: &impl IsA<gtk::Widget>,
    sidebar_widget: &impl IsA<gtk::Widget>,
    tool_inspector_widget: &impl IsA<gtk::Widget>,
    sender: &ComponentSender<App>,
) -> NavigationSetup {
    // Create NavigationView and pages before model
    let nav_view = adw::NavigationView::new();
    // Esc is routed via EscapeAction; disable native pop to avoid conflicts.
    nav_view.set_pop_on_escape(false);

    let session_list_page = adw::NavigationPage::builder()
        .title("Sessions")
        .tag("sessions")
        .child(session_list_widget)
        .build();
    nav_view.add(&session_list_page);

    // Register the detail page with the nav_view permanently (add without pushing).
    // This keeps the page parented to nav_view across push/pop cycles so it can
    // be safely re-pushed after the user navigates back.  Transient push()-only
    // pages are unparented on pop(), causing a GTK assertion on the next push().
    let detail_page = adw::NavigationPage::builder()
        .title("Session")
        .tag("detail")
        .child(session_detail_widget)
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
    let pane_stack = build_pane_stack(sidebar_widget, tool_inspector_widget);

    NavigationSetup {
        nav_view,
        detail_page,
        pane_stack,
    }
}

pub(super) fn build_pane_stack(
    sidebar_widget: &impl IsA<gtk::Widget>,
    tool_inspector_widget: &impl IsA<gtk::Widget>,
) -> gtk::Stack {
    let pane_stack = gtk::Stack::new();
    pane_stack.set_transition_type(gtk::StackTransitionType::None);
    pane_stack.set_hhomogeneous(false);
    pane_stack.add_named(sidebar_widget, Some("filters"));
    pane_stack.add_named(tool_inspector_widget, Some("tool-inspector"));
    pane_stack.set_visible_child_name("filters");
    pane_stack
}

pub(super) fn wire_search_bar(
    search_bar: &gtk::SearchBar,
    search_entry: &gtk::SearchEntry,
    search_toggle: &gtk::ToggleButton,
    main_window: &adw::ApplicationWindow,
    app_sender: &ComponentSender<App>,
    session_list_sender: &relm4::Sender<SessionListMsg>,
) {
    // Enable type-to-search: keystrokes captured from main window open SearchBar
    search_bar.set_key_capture_widget(Some(main_window));

    // Bidirectional binding: ToggleButton.active <-> SearchBar.search-mode-enabled
    search_bar
        .bind_property("search-mode-enabled", search_toggle, "active")
        .bidirectional()
        .sync_create()
        .build();

    // Sync SearchBar state changes (Escape, type-to-search, ToggleButton) back to model
    {
        let search_mode_sender = app_sender.input_sender().clone();
        let search_entry = search_entry.clone();
        search_bar.connect_search_mode_enabled_notify(move |bar| {
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
        let session_list_sender = session_list_sender.clone();
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
        search_entry.add_controller(key_controller);
    }

    // Enter in SearchEntry activates the selected session directly
    {
        let session_list_sender = session_list_sender.clone();
        search_entry.connect_activate(move |_| {
            session_list_sender
                .send(SessionListMsg::ActivateSelected)
                .ok();
        });
    }
}

pub(super) fn setup_workspace_stack(
    model: &mut App,
    overlay_split: &adw::OverlaySplitView,
    search_bar: &gtk::SearchBar,
    workspace_switcher: &adw::ViewSwitcher,
    workspace_switcher_bar: &adw::ViewSwitcherBar,
    nav_view: &adw::NavigationView,
    sender: &ComponentSender<App>,
) {
    // Set up OverlaySplitView: sidebar = pane Stack, content = NavigationView
    overlay_split.set_sidebar(Some(&model.pane_stack));
    overlay_split.set_content(Some(nav_view));
    overlay_split.set_max_sidebar_width(720.0);

    // Build top-level workspace stack and switchers.
    let sessions_workspace_added = if let Some(parent) = overlay_split.parent() {
        if let Ok(content_box) = parent.downcast::<gtk::Box>() {
            overlay_split.unparent();

            if overlay_split.parent().is_none() {
                content_box.insert_child_after(
                    &model.workspace_stack,
                    Some(&search_bar.clone().upcast::<gtk::Widget>()),
                );

                // Wrap banner + overlay_split in a dedicated Sessions-page container
                // so the banner is scoped to the Sessions workspace only.
                let sessions_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
                sessions_page.append(&model.banner);
                sessions_page.append(overlay_split);

                model.workspace_stack.add_titled_with_icon(
                    &sessions_page,
                    Some(Workspace::Sessions.stack_name()),
                    "Sessions",
                    Workspace::Sessions.icon_name(),
                );
                true
            } else {
                tracing::warn!(
                    "overlay_split remained parented after unparent(); skipping sessions workspace page"
                );
                false
            }
        } else {
            tracing::warn!(
                "overlay_split parent was not gtk::Box; skipping sessions workspace page setup"
            );
            false
        }
    } else {
        tracing::warn!(
            "overlay_split has no parent during workspace setup; skipping sessions workspace page"
        );
        false
    };

    model.workspace_stack.add_titled_with_icon(
        model.analytics_view.widget(),
        Some(Workspace::Analytics.stack_name()),
        "Analytics",
        Workspace::Analytics.icon_name(),
    );
    workspace_switcher.set_stack(Some(&model.workspace_stack));
    workspace_switcher_bar.set_stack(Some(&model.workspace_stack));

    if sessions_workspace_added {
        model
            .workspace_stack
            .set_visible_child_name(Workspace::Sessions.stack_name());
    } else {
        model.active_workspace = Workspace::Analytics;
        model
            .workspace_stack
            .set_visible_child_name(Workspace::Analytics.stack_name());
    }

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
    overlay_split.connect_show_sidebar_notify(move |split| {
        visibility_sender
            .send(AppMsg::PaneVisibilityChanged(split.shows_sidebar()))
            .ok();
    });
}

pub(super) fn setup_breakpoints(
    root: &adw::ApplicationWindow,
    overlay_split: &adw::OverlaySplitView,
    workspace_switcher: &adw::ViewSwitcher,
    workspace_switcher_bar: &adw::ViewSwitcherBar,
) {
    // Add responsive collapse breakpoint
    let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        400.0,
        adw::LengthUnit::Sp,
    ));
    breakpoint.add_setter(overlay_split, "collapsed", Some(&true.into()));
    breakpoint.add_setter(workspace_switcher, "visible", Some(&false.into()));
    breakpoint.add_setter(workspace_switcher_bar, "reveal", Some(&true.into()));
    root.add_breakpoint(breakpoint);
}

pub(super) fn register_actions(
    root: &adw::ApplicationWindow,
    main_window: &adw::ApplicationWindow,
    sender: &ComponentSender<App>,
    banner: &adw::Banner,
    search_bar: &gtk::SearchBar,
    search_entry: &gtk::SearchEntry,
    workspace_stack: &adw::ViewStack,
) {
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

    let indexing_status_action = {
        let sender = sender.clone();
        RelmAction::<IndexingStatusAction>::new_stateless(move |_| {
            sender.input(AppMsg::ShowIndexingStatus);
        })
    };

    let about_action = {
        RelmAction::<AboutAction>::new_stateless(move |_| {
            AboutDialog::builder().launch(()).detach();
        })
    };

    let show_search_action = {
        let search_bar = search_bar.clone();
        let search_entry = search_entry.clone();
        let workspace_stack = workspace_stack.clone();
        RelmAction::<ShowSearchAction>::new_stateless(move |_| {
            let workspace = workspace_stack
                .visible_child_name()
                .as_deref()
                .and_then(Workspace::from_stack_name)
                .unwrap_or(Workspace::Sessions);

            if !workspace_allows_search(workspace) {
                return;
            }

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

    let banner_sender = sender.input_sender().clone();
    banner.connect_button_clicked(move |_| {
        banner_sender.send(AppMsg::ShowIndexingStatus).ok();
    });

    // Connect actions with hotkeys
    app.set_accelerators_for_action::<QuitAction>(&["<Control>q"]);
    app.set_accelerators_for_action::<TogglePaneAction>(&["F9"]);
    app.set_accelerators_for_action::<ShowSearchAction>(&["<Control>f"]);
    app.set_accelerators_for_action::<ShortcutsAction>(&["<Control>question"]);
    app.set_accelerators_for_action::<IndexingStatusAction>(&["<Control><Shift>i"]);
    app.set_accelerators_for_action::<PreferencesAction>(&["<Control>comma"]);
    app.set_accelerators_for_action::<EscapeAction>(&["Escape"]);

    actions.add_action(preferences_action);
    actions.add_action(shortcuts_action);
    actions.add_action(indexing_status_action);
    actions.add_action(about_action);
    actions.add_action(show_search_action);
    actions.add_action(toggle_pane_action);
    actions.add_action(quit_action);
    actions.add_action(escape_action);
    actions.register_for_widget(main_window);
}
