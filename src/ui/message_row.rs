use gtk::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use relm4::gtk;

use crate::models::{MessagePreview, Role};
use crate::ui::highlight;
use crate::ui::markdown;

pub struct MessageRowInit {
    pub preview: MessagePreview,
    pub highlight_query: Option<String>,
}

#[derive(Debug)]
pub enum MessageRowOutput {
    MatchCount { count: usize },
}

#[derive(Debug)]
pub struct MessageRow {
    preview: MessagePreview,
    highlight_query: Option<String>,
}

#[relm4::factory(pub)]
impl FactoryComponent for MessageRow {
    type Init = MessageRowInit;
    type Input = ();
    type Output = MessageRowOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;

    view! {
        root = gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 4,
            add_css_class: "message-row",
            add_css_class: self.preview.role.css_class(),

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 8,

                gtk::Label {
                    set_label: self.preview.role.label(),
                    add_css_class: "caption",
                    add_css_class: "heading",
                    add_css_class: self.preview.role.css_class(),
                    set_halign: gtk::Align::Start,
                },

                gtk::Label {
                    set_label: &self.preview.timestamp.format("%H:%M:%S").to_string(),
                    add_css_class: "caption",
                    add_css_class: "dim-label",
                    set_halign: gtk::Align::Start,
                },
            },

            #[name(content_container)]
            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 4,
            },

            // Truncation badge
            gtk::Label {
                set_label: "(content truncated)",
                add_css_class: "caption",
                add_css_class: "dim-label",
                set_halign: gtk::Align::Start,
                set_margin_top: 4,
                #[watch]
                set_visible: self.preview.is_truncated(),
            },
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            preview: init.preview,
            highlight_query: init.highlight_query,
        }
    }

    fn init_widgets(
        &mut self,
        _index: &DynamicIndex,
        _root: Self::Root,
        _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: FactorySender<Self>,
    ) -> Self::Widgets {
        let widgets = view_output!();
        let mut match_count = 0usize;

        if self.preview.role == Role::Assistant {
            let rendered = markdown::render_markdown(
                &self.preview.content_preview,
                self.highlight_query.as_deref(),
            );
            match_count = rendered.1;
            widgets.content_container.append(&rendered.0);
        } else if let Some(ref query) = self.highlight_query {
            let (markup, count) = highlight::highlight_text(&self.preview.content_preview, query);
            match_count = count;
            let label = gtk::Label::new(None);
            label.set_markup(&markup);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_halign(gtk::Align::Start);
            label.set_xalign(0.0);
            label.set_selectable(true);
            widgets.content_container.append(&label);
        } else {
            let label = gtk::Label::new(Some(&self.preview.content_preview));
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_halign(gtk::Align::Start);
            label.set_xalign(0.0);
            label.set_selectable(true);
            widgets.content_container.append(&label);
        }

        let _ = sender.output(MessageRowOutput::MatchCount { count: match_count });

        widgets
    }
}
