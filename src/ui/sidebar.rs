use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, gtk};

use crate::models::session::Tool;

#[derive(Debug)]
pub struct Sidebar {
    claude_enabled: bool,
    opencode_enabled: bool,
    codex_enabled: bool,
    mistral_vibe_enabled: bool,
}

#[derive(Debug)]
pub enum SidebarMsg {
    ToolToggled(Tool, bool),
}

#[derive(Debug)]
pub enum SidebarOutput {
    FiltersChanged(Vec<Tool>),
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
                set_label: "Tools",
                set_halign: gtk::Align::Start,
                add_css_class: "heading",
                set_margin_bottom: 6,
            },

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 6,

                #[name = "claude_check"]
                gtk::CheckButton {
                    set_label: Some("Claude Code"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::ToolToggled(Tool::ClaudeCode, btn.is_active()));
                    },
                },

                #[name = "opencode_check"]
                gtk::CheckButton {
                    set_label: Some("OpenCode"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::ToolToggled(Tool::OpenCode, btn.is_active()));
                    },
                },

                #[name = "codex_check"]
                gtk::CheckButton {
                    set_label: Some("Codex"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::ToolToggled(Tool::Codex, btn.is_active()));
                    },
                },

                #[name = "mistral_vibe_check"]
                gtk::CheckButton {
                    set_label: Some("Mistral Vibe"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::ToolToggled(Tool::MistralVibe, btn.is_active()));
                    },
                },
            },

            gtk::Separator {
                set_margin_top: 12,
                set_margin_bottom: 12,
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

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 6,

                    gtk::Label {
                        set_label: "No projects yet",
                        set_halign: gtk::Align::Start,
                        add_css_class: "dim-label",
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            claude_enabled: true,
            opencode_enabled: true,
            codex_enabled: true,
            mistral_vibe_enabled: true,
        };
        let widgets = view_output!();

        let _ = sender.output(SidebarOutput::FiltersChanged(vec![
            Tool::ClaudeCode,
            Tool::OpenCode,
            Tool::Codex,
            Tool::MistralVibe,
        ]));

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SidebarMsg::ToolToggled(tool, active) => {
                match tool {
                    Tool::ClaudeCode => self.claude_enabled = active,
                    Tool::OpenCode => self.opencode_enabled = active,
                    Tool::Codex => self.codex_enabled = active,
                    Tool::MistralVibe => self.mistral_vibe_enabled = active,
                }

                let mut tools = Vec::new();
                if self.claude_enabled {
                    tools.push(Tool::ClaudeCode);
                }
                if self.opencode_enabled {
                    tools.push(Tool::OpenCode);
                }
                if self.codex_enabled {
                    tools.push(Tool::Codex);
                }
                if self.mistral_vibe_enabled {
                    tools.push(Tool::MistralVibe);
                }

                let _ = sender.output(SidebarOutput::FiltersChanged(tools));
            }
        }
    }
}
