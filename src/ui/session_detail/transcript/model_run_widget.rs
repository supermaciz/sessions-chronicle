use relm4::gtk;
use relm4::gtk::prelude::*;

use super::model_attribution::ModelRunText;

pub(super) struct ModelRunWidgets {
    pub root: gtk::Box,
    caption: gtk::Label,
    previous: gtk::Label,
}

impl ModelRunWidgets {
    pub fn prepend_to(row: &gtk::Box) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("model-run-marker");
        root.set_focusable(false);
        root.set_visible(false);
        row.prepend(&root);

        let leading = gtk::Separator::new(gtk::Orientation::Horizontal);
        leading.set_width_request(16);
        leading.set_valign(gtk::Align::Center);
        root.append(&leading);

        let caption = gtk::Label::new(None);
        caption.add_css_class("caption");
        caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
        root.append(&caption);

        let previous = gtk::Label::new(None);
        previous.add_css_class("caption");
        previous.add_css_class("dim-label");
        previous.set_ellipsize(gtk::pango::EllipsizeMode::End);
        root.append(&previous);

        let trailing = gtk::Separator::new(gtk::Orientation::Horizontal);
        trailing.set_hexpand(true);
        trailing.set_valign(gtk::Align::Center);
        root.append(&trailing);

        Self {
            root,
            caption,
            previous,
        }
    }

    pub fn bind(&self, row: &gtk::Box, text: Option<&ModelRunText>) {
        let Some(text) = text else {
            self.clear(row);
            return;
        };

        self.caption.set_label(&text.caption);
        self.set_caption_monospace(text.monospace);
        self.previous
            .set_markup(text.previous_markup.as_deref().unwrap_or_default());
        self.previous.set_visible(text.previous_markup.is_some());
        self.root.set_visible(true);
        row.update_property(&[gtk::accessible::Property::Description(&text.description)]);
    }

    pub fn clear(&self, row: &gtk::Box) {
        self.root.set_visible(false);
        self.caption.set_label("");
        self.set_caption_monospace(false);
        self.previous.set_label("");
        self.previous.set_visible(false);
        row.reset_property(gtk::AccessibleProperty::Description);
    }

    fn set_caption_monospace(&self, monospace: bool) {
        if monospace {
            self.caption.add_css_class("monospace");
        } else {
            self.caption.remove_css_class("monospace");
        }
    }
}
