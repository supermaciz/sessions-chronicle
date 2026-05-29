use adw::prelude::ActionRowExt;
use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};
use std::collections::HashMap;

use crate::models::{
    PerSourceResult, ProjectFilter, ProjectInfo, SourceStatus, session::AiAssistant,
};

#[derive(Debug)]
pub struct Sidebar {
    claude_enabled: bool,
    opencode_enabled: bool,
    codex_enabled: bool,
    mistral_vibe_enabled: bool,
    selected_project_filter: ProjectFilter,
    project_row_filters: Vec<ProjectFilter>,
    rebuilding_projects: bool,
    projects_list: Option<gtk::ListBox>,
    pinned_count_label: Option<gtk::Label>,
    source_statuses: HashMap<AiAssistant, PerSourceResult>,
    status_dots: HashMap<AiAssistant, gtk::Box>,
}

#[derive(Debug)]
pub enum SidebarMsg {
    AiAssistantToggled(AiAssistant, bool),
    ProjectSelected(ProjectFilter),
    SourceStatusesUpdated(HashMap<AiAssistant, PerSourceResult>),
    ProjectsLoaded {
        projects: Vec<ProjectInfo>,
        all_sessions_count: usize,
        unassigned_count: usize,
        pinned_count: usize,
        show_unassigned: bool,
        selected_filter: ProjectFilter,
    },
}

#[derive(Debug)]
pub enum SidebarOutput {
    FiltersChanged {
        tools: Vec<AiAssistant>,
        project_filter: ProjectFilter,
    },
}

#[relm4::component(pub)]
impl SimpleComponent for Sidebar {
    type Init = ();
    type Input = SidebarMsg;
    type Output = SidebarOutput;
    type Widgets = SidebarWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 32,
            set_margin_all: 12,
            set_width_request: 200,

            #[name = "assistants_list"]
            gtk::ListBox {
                add_css_class: "assistant-sidebar-list",
                set_selection_mode: gtk::SelectionMode::None,
            },

            gtk::ScrolledWindow {
                set_vexpand: true,
                set_hscrollbar_policy: gtk::PolicyType::Never,

                #[name = "projects_list"]
                gtk::ListBox {
                    add_css_class: "project-sidebar-list",
                    set_selection_mode: gtk::SelectionMode::Single,
                    connect_row_selected[sender] => move |_, row| {
                        if let Some(row) = row {
                            let key = row.widget_name().to_string();
                            if let Some(project_filter) = Sidebar::project_filter_from_key(&key) {
                                sender.input(SidebarMsg::ProjectSelected(project_filter));
                            }
                        }
                    },
                }
            },
        }
    }

    fn init(
        _init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut model = Self {
            claude_enabled: true,
            opencode_enabled: true,
            codex_enabled: true,
            mistral_vibe_enabled: true,
            selected_project_filter: ProjectFilter::AllSessions,
            project_row_filters: Vec::new(),
            rebuilding_projects: false,
            projects_list: None,
            pinned_count_label: None,
            source_statuses: HashMap::new(),
            status_dots: HashMap::new(),
        };
        let widgets = view_output!();
        widgets
            .assistants_list
            .update_property(&[gtk::accessible::Property::Label("AI Assistants")]);
        widgets
            .projects_list
            .update_property(&[gtk::accessible::Property::Label("Projects")]);
        model.projects_list = Some(widgets.projects_list.clone());

        for (assistant, title) in [
            (AiAssistant::ClaudeCode, "Claude Code"),
            (AiAssistant::OpenCode, "OpenCode"),
            (AiAssistant::Codex, "Codex"),
            (AiAssistant::MistralVibe, "Mistral Vibe"),
        ] {
            let (row, dot) = Self::build_assistant_row(assistant, title, sender.clone());
            widgets.assistants_list.append(&row);
            model.status_dots.insert(assistant, dot);
        }

        let _ = sender;
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SidebarMsg::SourceStatusesUpdated(statuses) => {
                self.source_statuses = statuses;
                for (assistant, dot) in &self.status_dots {
                    apply_status_dot(dot, self.source_statuses.get(assistant));
                }
            }
            SidebarMsg::AiAssistantToggled(tool, active) => {
                match tool {
                    AiAssistant::ClaudeCode => self.claude_enabled = active,
                    AiAssistant::OpenCode => self.opencode_enabled = active,
                    AiAssistant::Codex => self.codex_enabled = active,
                    AiAssistant::MistralVibe => self.mistral_vibe_enabled = active,
                }

                self.emit_filters_changed(&sender);
            }
            SidebarMsg::ProjectSelected(project_filter) => {
                if self.selected_project_filter != project_filter {
                    self.selected_project_filter = project_filter;
                    if !self.rebuilding_projects {
                        self.emit_filters_changed(&sender);
                    }
                }
            }
            SidebarMsg::ProjectsLoaded {
                projects,
                all_sessions_count,
                unassigned_count,
                pinned_count,
                show_unassigned,
                selected_filter,
            } => {
                self.selected_project_filter = selected_filter;
                self.rebuilding_projects = true;
                self.rebuild_project_rows(
                    projects,
                    all_sessions_count,
                    unassigned_count,
                    pinned_count,
                    show_unassigned,
                );
                self.rebuilding_projects = false;
            }
        }
    }
}

fn apply_status_dot(dot: &gtk::Box, result: Option<&PerSourceResult>) {
    dot.remove_css_class("source-status-ok");
    dot.remove_css_class("source-status-degraded");
    dot.remove_css_class("source-status-not-found");

    let Some(r) = result else {
        dot.set_visible(false);
        dot.set_tooltip_text(None);
        return;
    };

    let (css_class, tooltip) = match r.status {
        SourceStatus::Indexed => {
            let n = r.indexed + r.skipped;
            (Some("source-status-ok"), format!("{n} sessions indexed"))
        }
        SourceStatus::Degraded => (
            Some("source-status-degraded"),
            format!("Indexed with {} errors", r.errors),
        ),
        SourceStatus::Failed => (
            Some("source-status-degraded"),
            format!("Indexing failed — {} errors", r.errors),
        ),
        SourceStatus::Empty => (
            Some("source-status-not-found"),
            "No sessions found".to_string(),
        ),
        SourceStatus::NotFound => (
            Some("source-status-not-found"),
            "Source directory not found".to_string(),
        ),
    };

    if let Some(css_class) = css_class {
        dot.add_css_class(css_class);
    }
    dot.set_tooltip_text(Some(&tooltip));
    dot.set_visible(true);
}

impl Sidebar {
    fn build_assistant_row(
        assistant: AiAssistant,
        title: &'static str,
        sender: ComponentSender<Self>,
    ) -> (adw::ActionRow, gtk::Box) {
        let check = gtk::CheckButton::builder().active(true).build();

        let row = adw::ActionRow::builder()
            .title(title)
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);

        let icon = gtk::Image::from_icon_name(assistant.icon_name());
        icon.set_valign(gtk::Align::Center);
        row.add_prefix(&icon);

        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.set_visible(false);
        dot.set_valign(gtk::Align::Center);
        dot.set_width_request(12);
        dot.set_height_request(12);
        dot.add_css_class("source-status-dot");
        row.add_suffix(&dot);

        check.connect_toggled(move |check| {
            sender.input(SidebarMsg::AiAssistantToggled(assistant, check.is_active()));
        });

        (row, dot)
    }

    fn active_tools(&self) -> Vec<AiAssistant> {
        let mut tools = Vec::new();
        if self.claude_enabled {
            tools.push(AiAssistant::ClaudeCode);
        }
        if self.opencode_enabled {
            tools.push(AiAssistant::OpenCode);
        }
        if self.codex_enabled {
            tools.push(AiAssistant::Codex);
        }
        if self.mistral_vibe_enabled {
            tools.push(AiAssistant::MistralVibe);
        }
        tools
    }

    fn emit_filters_changed(&self, sender: &ComponentSender<Self>) {
        let _ = sender.output(SidebarOutput::FiltersChanged {
            tools: self.active_tools(),
            project_filter: self.selected_project_filter.clone(),
        });
    }

    fn project_filter_key(project_filter: &ProjectFilter) -> String {
        match project_filter {
            ProjectFilter::AllSessions => "all".to_string(),
            ProjectFilter::Pinned => "pinned".to_string(),
            ProjectFilter::Unassigned => "unassigned".to_string(),
            ProjectFilter::Project(project_id) => format!("project:{}", project_id),
        }
    }

    fn project_filter_from_key(key: &str) -> Option<ProjectFilter> {
        if key == "all" {
            return Some(ProjectFilter::AllSessions);
        }

        if key == "unassigned" {
            return Some(ProjectFilter::Unassigned);
        }

        if key == "pinned" {
            return Some(ProjectFilter::Pinned);
        }

        key.strip_prefix("project:")
            .and_then(|id| id.parse::<i64>().ok())
            .map(ProjectFilter::Project)
    }

    fn make_row(
        title: &str,
        subtitle: Option<&str>,
        badge_count: usize,
        italic: bool,
        prefix_icon: Option<&str>,
    ) -> (gtk::ListBoxRow, gtk::Label) {
        let action_row = adw::ActionRow::builder().title(title).build();
        if let Some(subtitle) = subtitle {
            action_row.set_subtitle(subtitle);
        }

        if let Some(icon_name) = prefix_icon {
            let icon = gtk::Image::from_icon_name(icon_name);
            action_row.add_prefix(&icon);
        }

        if italic {
            action_row.add_css_class("unassigned-label");
        }

        let badge = gtk::Label::new(Some(&badge_count.to_string()));
        badge.add_css_class("project-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_height_request(29);
        action_row.add_suffix(&badge);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&action_row));
        (row, badge)
    }

    fn rebuild_project_rows(
        &mut self,
        projects: Vec<ProjectInfo>,
        all_sessions_count: usize,
        unassigned_count: usize,
        pinned_count: usize,
        show_unassigned: bool,
    ) {
        let Some(list_box) = self.projects_list.as_ref() else {
            return;
        };

        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }

        self.project_row_filters.clear();
        self.pinned_count_label = None;

        let all_filter = ProjectFilter::AllSessions;
        let (all_row, _) = Self::make_row("All Sessions", None, all_sessions_count, false, None);
        all_row.set_widget_name(&Self::project_filter_key(&all_filter));
        list_box.append(&all_row);
        self.project_row_filters.push(all_filter);

        let pinned_filter = ProjectFilter::Pinned;
        let (pinned_row, pinned_badge) = Self::make_row(
            "Pinned",
            None,
            pinned_count,
            false,
            Some("view-pin-symbolic"),
        );
        pinned_row.set_widget_name(&Self::project_filter_key(&pinned_filter));
        list_box.append(&pinned_row);
        self.project_row_filters.push(pinned_filter);
        self.pinned_count_label = Some(pinned_badge);

        for project in projects {
            let filter = ProjectFilter::Project(project.id);
            let (row, _) = Self::make_row(
                &project.name,
                Some(&project.path),
                project.session_count,
                false,
                None,
            );
            row.set_widget_name(&Self::project_filter_key(&filter));
            list_box.append(&row);
            self.project_row_filters.push(filter);
        }

        if show_unassigned {
            let unassigned_filter = ProjectFilter::Unassigned;
            let (unassigned_row, _) =
                Self::make_row("Unassigned", None, unassigned_count, true, None);
            unassigned_row.set_widget_name(&Self::project_filter_key(&unassigned_filter));
            list_box.append(&unassigned_row);
            self.project_row_filters.push(unassigned_filter);
        }

        if let Some(selected_index) = self
            .project_row_filters
            .iter()
            .position(|project_filter| project_filter == &self.selected_project_filter)
            && let Some(row) = list_box.row_at_index(selected_index as i32)
        {
            list_box.select_row(Some(&row));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adw::prelude::PreferencesRowExt;
    use relm4::{Component, ComponentController};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::time::Duration;

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

    fn image_icon_names(widget: &gtk::Widget) -> Vec<String> {
        let mut names = Vec::new();

        if let Ok(image) = widget.clone().downcast::<gtk::Image>()
            && let Some(icon_name) = image.icon_name()
        {
            names.push(icon_name.to_string());
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            names.extend(image_icon_names(&child_widget));
            child = child_widget.next_sibling();
        }

        names
    }

    fn visible_project_row_titles(list_box: &gtk::ListBox) -> Vec<String> {
        let mut titles = Vec::new();
        let mut child = list_box.first_child();
        while let Some(widget) = child {
            if let Ok(row) = widget.clone().downcast::<gtk::ListBoxRow>()
                && let Some(row_child) = row.child()
                && let Ok(action_row) = row_child.downcast::<adw::ActionRow>()
            {
                titles.push(action_row.title().to_string());
            }
            child = widget.next_sibling();
        }
        titles
    }

    #[gtk::test]
    fn assistant_sidebar_rows_include_assistant_icons() {
        let controller = Sidebar::builder().launch(());

        let expected_icons = [
            AiAssistant::ClaudeCode.icon_name(),
            AiAssistant::OpenCode.icon_name(),
            AiAssistant::Codex.icon_name(),
            AiAssistant::MistralVibe.icon_name(),
        ];

        let parts = controller.state().get();
        for (index, expected_icon) in expected_icons.into_iter().enumerate() {
            let row = parts
                .widgets
                .assistants_list
                .row_at_index(index as i32)
                .unwrap_or_else(|| panic!("assistant row {index} should exist"));
            let row_widget = row.upcast::<gtk::Widget>();
            let icon_names = image_icon_names(&row_widget);

            assert!(
                icon_names
                    .iter()
                    .any(|icon_name| icon_name == expected_icon),
                "assistant row {index} should include icon {expected_icon}; found {icon_names:?}"
            );
        }
    }

    #[gtk::test]
    fn indexing_diagnostics_source_status_dots_start_hidden() {
        let controller = Sidebar::builder().launch(());
        let parts = controller.state().get();

        for assistant in [
            AiAssistant::ClaudeCode,
            AiAssistant::OpenCode,
            AiAssistant::Codex,
            AiAssistant::MistralVibe,
        ] {
            let dot = parts
                .model
                .status_dots
                .get(&assistant)
                .expect("status dot should exist for assistant");
            assert!(!dot.is_visible());
        }
    }

    #[gtk::test]
    fn indexing_diagnostics_source_status_updates_apply_css_classes_and_tooltips() {
        use crate::models::{PerSourceResult, SourceStatus};

        let controller = Sidebar::builder().launch(());
        controller.emit(SidebarMsg::SourceStatusesUpdated(HashMap::from([
            (
                AiAssistant::ClaudeCode,
                PerSourceResult {
                    assistant: AiAssistant::ClaudeCode,
                    display_path: "/tmp/claude".into(),
                    indexed: 12,
                    skipped: 3,
                    removed: 0,
                    errors: 0,
                    status: SourceStatus::Indexed,
                },
            ),
            (
                AiAssistant::OpenCode,
                PerSourceResult {
                    assistant: AiAssistant::OpenCode,
                    display_path: "/tmp/opencode".into(),
                    indexed: 5,
                    skipped: 0,
                    removed: 0,
                    errors: 2,
                    status: SourceStatus::Degraded,
                },
            ),
            (
                AiAssistant::Codex,
                PerSourceResult {
                    assistant: AiAssistant::Codex,
                    display_path: "/tmp/codex".into(),
                    indexed: 0,
                    skipped: 0,
                    removed: 0,
                    errors: 0,
                    status: SourceStatus::NotFound,
                },
            ),
        ])));

        pump_main_context(|| {
            let parts = controller.state().get();
            parts
                .model
                .status_dots
                .get(&AiAssistant::ClaudeCode)
                .is_some_and(|dot| dot.is_visible())
        });

        let parts = controller.state().get();
        let claude_dot = parts
            .model
            .status_dots
            .get(&AiAssistant::ClaudeCode)
            .expect("Claude Code status dot should exist");
        let opencode_dot = parts
            .model
            .status_dots
            .get(&AiAssistant::OpenCode)
            .expect("OpenCode status dot should exist");
        let codex_dot = parts
            .model
            .status_dots
            .get(&AiAssistant::Codex)
            .expect("Codex status dot should exist");

        assert!(claude_dot.has_css_class("source-status-ok"));
        assert_eq!(
            claude_dot.tooltip_text().as_deref(),
            Some("15 sessions indexed")
        );
        assert!(opencode_dot.has_css_class("source-status-degraded"));
        assert_eq!(
            opencode_dot.tooltip_text().as_deref(),
            Some("Indexed with 2 errors")
        );
        assert!(codex_dot.has_css_class("source-status-not-found"));
        assert_eq!(
            codex_dot.tooltip_text().as_deref(),
            Some("Source directory not found")
        );
    }

    #[gtk::test]
    fn project_sidebar_projects_loaded_rebuilds_rows_and_preserves_selection() {
        let outputs: Rc<RefCell<Vec<SidebarOutput>>> = Rc::new(RefCell::new(Vec::new()));
        let outputs_ref = outputs.clone();

        let controller = Sidebar::builder()
            .launch(())
            .connect_receiver(move |_, output| {
                outputs_ref.borrow_mut().push(output);
            });

        outputs.borrow_mut().clear();

        controller.emit(SidebarMsg::ProjectsLoaded {
            projects: vec![
                ProjectInfo {
                    id: 1,
                    name: "alpha".to_string(),
                    path: "/tmp/alpha".to_string(),
                    session_count: 2,
                },
                ProjectInfo {
                    id: 2,
                    name: "beta".to_string(),
                    path: "/tmp/beta".to_string(),
                    session_count: 0,
                },
            ],
            all_sessions_count: 3,
            unassigned_count: 1,
            pinned_count: 0,
            show_unassigned: true,
            selected_filter: ProjectFilter::Project(2),
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts.model.selected_project_filter == ProjectFilter::Project(2)
        });

        {
            let parts = controller.state().get();
            assert_eq!(
                parts.model.selected_project_filter,
                ProjectFilter::Project(2)
            );
        }

        let row_titles = {
            let parts = controller.state().get();
            visible_project_row_titles(&parts.widgets.projects_list)
        };

        assert_eq!(
            row_titles,
            vec!["All Sessions", "Pinned", "alpha", "beta", "Unassigned"]
        );

        assert!(
            outputs.borrow().is_empty(),
            "project row rebuild should not emit synthetic filters output"
        );
    }

    #[gtk::test]
    fn sidebar_simplification_removes_visible_group_headings() {
        let controller = Sidebar::builder().launch(());

        let parts = controller.state().get();
        let root = parts
            .widgets
            .assistants_list
            .parent()
            .expect("assistant list should be inside the sidebar root");

        let mut direct_child_types = Vec::new();
        let mut child = root.first_child();
        while let Some(widget) = child {
            direct_child_types.push(widget.type_().name().to_string());
            child = widget.next_sibling();
        }

        assert_eq!(direct_child_types, vec!["GtkListBox", "GtkScrolledWindow"]);
    }

    #[gtk::test]
    fn project_sidebar_list_uses_dedicated_css_class() {
        let controller = Sidebar::builder().launch(());

        let parts = controller.state().get();
        let projects_list = &parts.widgets.projects_list;
        let assistants_list = &parts.widgets.assistants_list;

        assert!(projects_list.has_css_class("project-sidebar-list"));
        assert!(!projects_list.has_css_class("boxed-list"));
        assert!(assistants_list.has_css_class("assistant-sidebar-list"));
        assert!(!assistants_list.has_css_class("boxed-list"));
    }

    #[test]
    fn project_filter_key_round_trips_pinned() {
        let key = Sidebar::project_filter_key(&ProjectFilter::Pinned);
        assert_eq!(key, "pinned");
        assert_eq!(
            Sidebar::project_filter_from_key(&key),
            Some(ProjectFilter::Pinned)
        );
    }

    #[gtk::test]
    fn project_sidebar_selecting_pinned_row_emits_filters_changed() {
        let outputs: Rc<RefCell<Vec<SidebarOutput>>> = Rc::new(RefCell::new(Vec::new()));
        let outputs_ref = outputs.clone();

        let controller = Sidebar::builder()
            .launch(())
            .connect_receiver(move |_, output| outputs_ref.borrow_mut().push(output));

        controller.emit(SidebarMsg::ProjectsLoaded {
            projects: vec![ProjectInfo {
                id: 1,
                name: "alpha".to_string(),
                path: "/tmp/alpha".to_string(),
                session_count: 2,
            }],
            all_sessions_count: 3,
            unassigned_count: 0,
            pinned_count: 2,
            show_unassigned: false,
            selected_filter: ProjectFilter::AllSessions,
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            visible_project_row_titles(&parts.widgets.projects_list)
                == vec!["All Sessions", "Pinned", "alpha"]
        });

        {
            let parts = controller.state().get();
            let pinned_row = parts
                .widgets
                .projects_list
                .row_at_index(1)
                .expect("pinned row");
            parts.widgets.projects_list.select_row(Some(&pinned_row));
        }

        pump_main_context(|| !outputs.borrow().is_empty());

        assert!(matches!(
            outputs.borrow().as_slice(),
            [SidebarOutput::FiltersChanged {
                project_filter: ProjectFilter::Pinned,
                ..
            }]
        ));
    }

    #[gtk::test]
    fn projects_loaded_places_pinned_row_before_projects_and_updates_badge() {
        let controller = Sidebar::builder().launch(());

        controller.emit(SidebarMsg::ProjectsLoaded {
            projects: vec![ProjectInfo {
                id: 1,
                name: "alpha".to_string(),
                path: "/tmp/alpha".to_string(),
                session_count: 2,
            }],
            all_sessions_count: 3,
            unassigned_count: 1,
            pinned_count: 4,
            show_unassigned: true,
            selected_filter: ProjectFilter::Pinned,
        });

        pump_main_context(|| {
            let parts = controller.state().get();
            parts
                .model
                .pinned_count_label
                .as_ref()
                .is_some_and(|label| label.label() == "4")
        });

        let parts = controller.state().get();
        assert_eq!(
            visible_project_row_titles(&parts.widgets.projects_list),
            vec!["All Sessions", "Pinned", "alpha", "Unassigned"]
        );
    }
}
