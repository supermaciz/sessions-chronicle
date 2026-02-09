use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, gtk};

use crate::models::session::Tool;

#[derive(Debug)]
pub struct DetailContextPane {
    project_name: Option<String>,
    tool: Option<Tool>,
}

#[derive(Debug)]
pub enum DetailContextPaneMsg {
    SetSession { project_name: String, tool: Tool },
    ResumeClicked,
}

#[derive(Debug)]
pub enum DetailContextPaneOutput {
    ResumeClicked,
}

#[relm4::component(pub)]
impl SimpleComponent for DetailContextPane {
    type Init = ();
    type Input = DetailContextPaneMsg;
    type Output = DetailContextPaneOutput;
    type Widgets = DetailContextPaneWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 12,
            set_margin_all: 12,
            set_width_request: 200,

            gtk::Label {
                set_label: "Session",
                set_halign: gtk::Align::Start,
                add_css_class: "title-4",
                set_margin_bottom: 6,
            },

            gtk::Separator {
                set_margin_bottom: 12,
            },

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 8,
                set_halign: gtk::Align::Start,

                #[name = "tool_icon"]
                gtk::Image {
                    set_pixel_size: 16,
                    #[watch]
                    set_visible: model.tool.is_some(),
                },

                #[name = "project_label"]
                gtk::Label {
                    set_halign: gtk::Align::Start,
                    add_css_class: "heading",
                    set_wrap: true,
                    set_wrap_mode: gtk::pango::WrapMode::WordChar,
                    #[watch]
                    set_label: model.project_name.as_deref().unwrap_or("No session"),
                },
            },

            #[name = "resume_button"]
            gtk::Button {
                set_label: "Resume in Terminal",
                add_css_class: "suggested-action",
                set_halign: gtk::Align::Start,
                set_margin_top: 12,
                #[watch]
                set_sensitive: model.project_name.is_some(),
                connect_clicked => DetailContextPaneMsg::ResumeClicked,
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            project_name: None,
            tool: None,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            DetailContextPaneMsg::SetSession { project_name, tool } => {
                self.project_name = Some(project_name);
                self.tool = Some(tool);
            }
            DetailContextPaneMsg::ResumeClicked => {
                let _ = sender.output(DetailContextPaneOutput::ResumeClicked);
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        if let Some(tool) = &self.tool {
            widgets.tool_icon.set_icon_name(Some(tool.icon_name()));
        }
    }
}
