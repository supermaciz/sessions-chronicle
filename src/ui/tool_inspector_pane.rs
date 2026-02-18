use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, gtk};

/// Phase 4 placeholder: inspector pane for tool calls and subagents.
/// Shows an empty state until a selection is made. Full inspector
/// navigation stack is implemented in Phase 4.
#[derive(Debug)]
pub struct ToolInspectorPane;

#[derive(Debug)]
pub enum ToolInspectorPaneMsg {
    #[allow(dead_code)]
    SelectToolCall(String),
    #[allow(dead_code)]
    SelectSubagent(String),
    #[allow(dead_code)]
    Clear,
}

#[relm4::component(pub)]
impl SimpleComponent for ToolInspectorPane {
    type Init = ();
    type Input = ToolInspectorPaneMsg;
    type Output = ();
    type Widgets = ToolInspectorPaneWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 12,
            set_margin_all: 24,
            set_vexpand: true,
            set_hexpand: true,
            set_halign: gtk::Align::Center,
            set_valign: gtk::Align::Center,
            set_width_request: 220,

            gtk::Image {
                set_icon_name: Some("system-search-symbolic"),
                set_pixel_size: 48,
                add_css_class: "dim-label",
            },

            gtk::Label {
                set_label: "Select a tool call or subagent to inspect",
                add_css_class: "dim-label",
                set_wrap: true,
                set_justify: gtk::Justification::Center,
                set_halign: gtk::Align::Center,
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = ToolInspectorPane;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, _message: Self::Input, _sender: ComponentSender<Self>) {}
}
