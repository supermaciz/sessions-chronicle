use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, gtk};

use crate::models::session::AiAssistant;

#[derive(Debug)]
pub struct Sidebar {
    claude_enabled: bool,
    opencode_enabled: bool,
    codex_enabled: bool,
    mistral_vibe_enabled: bool,
}

#[derive(Debug)]
pub enum SidebarMsg {
    AiAssistantToggled(AiAssistant, bool),
}

#[derive(Debug)]
pub enum SidebarOutput {
    FiltersChanged(Vec<AiAssistant>),
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

                #[name = "claude_check"]
                gtk::CheckButton {
                    set_label: Some("Claude Code"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::ClaudeCode, btn.is_active()));
                    },
                },

                #[name = "opencode_check"]
                gtk::CheckButton {
                    set_label: Some("OpenCode"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::OpenCode, btn.is_active()));
                    },
                },

                #[name = "codex_check"]
                gtk::CheckButton {
                    set_label: Some("Codex"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::Codex, btn.is_active()));
                    },
                },

                #[name = "mistral_vibe_check"]
                gtk::CheckButton {
                    set_label: Some("Mistral Vibe"),
                    set_active: true,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::AiAssistantToggled(AiAssistant::MistralVibe, btn.is_active()));
                    },
                },
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
            AiAssistant::ClaudeCode,
            AiAssistant::OpenCode,
            AiAssistant::Codex,
            AiAssistant::MistralVibe,
        ]));

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

                let _ = sender.output(SidebarOutput::FiltersChanged(tools));
            }
        }
    }
}
