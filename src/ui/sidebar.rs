use relm4::{gtk, ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};
use gtk::prelude::*;

#[derive(Debug)]
pub struct Sidebar {}

#[derive(Debug)]
pub enum SidebarMsg {
    FilterByTool(Option<String>),
}

#[derive(Debug)]
pub enum SidebarOutput {
    FilterChanged(Option<String>),
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

                gtk::CheckButton {
                    set_label: Some("Claude Code"),
                    set_active: true,
                },

                gtk::CheckButton {
                    set_label: Some("OpenCode"),
                    set_active: true,
                },

                gtk::CheckButton {
                    set_label: Some("Codex"),
                    set_active: true,
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
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {};
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SidebarMsg::FilterByTool(tool) => {
                let _ = sender.output(SidebarOutput::FilterChanged(tool));
            }
        }
    }
}
