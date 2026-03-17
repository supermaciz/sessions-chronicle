use adw::prelude::ActionRowExt;
use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};

use crate::models::{ProjectFilter, ProjectInfo, session::AiAssistant};

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
}

#[derive(Debug)]
pub enum SidebarMsg {
    AiAssistantToggled(AiAssistant, bool),
    ProjectSelected(ProjectFilter),
    ProjectsLoaded {
        projects: Vec<ProjectInfo>,
        all_sessions_count: usize,
        unassigned_count: usize,
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
            set_spacing: 12,
            set_margin_all: 12,
            set_width_request: 200,

            gtk::Label {
                set_label: "Filters",
                set_halign: gtk::Align::Start,
                add_css_class: "title-4",
                set_margin_bottom: 6,
            },

            gtk::Separator {
                set_margin_bottom: 12,
            },

            gtk::Label {
                set_label: "AI Assistants",
                set_halign: gtk::Align::Start,
                add_css_class: "heading",
                set_margin_bottom: 6,
            },

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 6,

                gtk::CheckButton {
                    set_label: Some("Claude Code"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::ClaudeCode, btn.is_active()));
                    },
                },

                gtk::CheckButton {
                    set_label: Some("OpenCode"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::OpenCode, btn.is_active()));
                    },
                },

                gtk::CheckButton {
                    set_label: Some("Codex"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::Codex, btn.is_active()));
                    },
                },

                gtk::CheckButton {
                    set_label: Some("Mistral Vibe"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::MistralVibe, btn.is_active()));
                    },
                },
            },

            gtk::Separator {
                set_margin_top: 6,
                set_margin_bottom: 6,
            },

            gtk::Label {
                set_label: "Projects",
                set_halign: gtk::Align::Start,
                add_css_class: "heading",
                set_margin_bottom: 6,
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
        };
        let widgets = view_output!();
        model.projects_list = Some(widgets.projects_list.clone());

        let _ = sender;
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
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
                show_unassigned,
                selected_filter,
            } => {
                self.selected_project_filter = selected_filter;
                self.rebuilding_projects = true;
                self.rebuild_project_rows(
                    projects,
                    all_sessions_count,
                    unassigned_count,
                    show_unassigned,
                );
                self.rebuilding_projects = false;
            }
        }
    }
}

impl Sidebar {
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

        key.strip_prefix("project:")
            .and_then(|id| id.parse::<i64>().ok())
            .map(ProjectFilter::Project)
    }

    fn make_row(
        title: &str,
        subtitle: Option<&str>,
        badge_count: usize,
        italic: bool,
    ) -> gtk::ListBoxRow {
        let action_row = adw::ActionRow::builder().title(title).build();
        if let Some(subtitle) = subtitle {
            action_row.set_subtitle(subtitle);
        }

        if italic {
            action_row.add_css_class("unassigned-label");
        }

        let badge = gtk::Label::new(Some(&badge_count.to_string()));
        badge.add_css_class("project-badge");
        action_row.add_suffix(&badge);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&action_row));
        row
    }

    fn rebuild_project_rows(
        &mut self,
        projects: Vec<ProjectInfo>,
        all_sessions_count: usize,
        unassigned_count: usize,
        show_unassigned: bool,
    ) {
        let Some(list_box) = self.projects_list.as_ref() else {
            return;
        };

        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }

        self.project_row_filters.clear();

        let all_filter = ProjectFilter::AllSessions;
        let all_row = Self::make_row("All Sessions", None, all_sessions_count, false);
        all_row.set_widget_name(&Self::project_filter_key(&all_filter));
        list_box.append(&all_row);
        self.project_row_filters.push(all_filter);

        for project in projects {
            let filter = ProjectFilter::Project(project.id);
            let row = Self::make_row(
                &project.name,
                Some(&project.path),
                project.session_count,
                false,
            );
            row.set_widget_name(&Self::project_filter_key(&filter));
            list_box.append(&row);
            self.project_row_filters.push(filter);
        }

        if show_unassigned {
            let unassigned_filter = ProjectFilter::Unassigned;
            let unassigned_row = Self::make_row("Unassigned", None, unassigned_count, true);
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
            vec!["All Sessions", "alpha", "beta", "Unassigned"]
        );

        assert!(
            outputs.borrow().is_empty(),
            "project row rebuild should not emit synthetic filters output"
        );
    }

    #[gtk::test]
    fn project_sidebar_list_uses_dedicated_css_class() {
        let controller = Sidebar::builder().launch(());

        let parts = controller.state().get();
        let projects_list = &parts.widgets.projects_list;

        assert!(projects_list.has_css_class("project-sidebar-list"));
        assert!(!projects_list.has_css_class("boxed-list"));
    }
}
