use relm4::{adw, gtk, ComponentParts, ComponentSender, SimpleComponent};
use gtk::prelude::*;

#[derive(Debug)]
pub struct SessionList {}

#[derive(Debug)]
pub enum SessionListMsg {
    SelectSession(String),
}

#[derive(Debug)]
pub enum SessionListOutput {
    SessionSelected(String),
}

#[relm4::component(pub)]
impl SimpleComponent for SessionList {
    type Init = ();
    type Input = SessionListMsg;
    type Output = SessionListOutput;
    type Widgets = SessionListWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 0,

            adw::StatusPage {
                set_vexpand: true,
                set_icon_name: Some("document-open-recent-symbolic"),
                set_title: "No Sessions Yet",
                set_description: Some("Your AI coding sessions will appear here"),
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
            SessionListMsg::SelectSession(id) => {
                let _ = sender.output(SessionListOutput::SessionSelected(id));
            }
        }
    }
}
