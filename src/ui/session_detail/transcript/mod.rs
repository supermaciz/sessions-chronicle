//! Internal implementation of the `SessionDetail` transcript view.
//!
//! These modules are coupled to `SessionDetail` (they depend on
//! `SessionDetailMsg`) and are not meant as a general-purpose UI API, so they
//! live here rather than at the top level of `ui`.

use relm4::gtk;
use relm4::gtk::prelude::*;

pub(crate) mod context_row;
pub(crate) mod display;
pub(crate) mod item_data;
pub(crate) mod item_init;
pub(crate) mod row_rendering;
pub(crate) mod tool_call_row;
pub(crate) mod tool_preview;
pub(crate) mod typed_row;

fn disconnect_handlers(handlers: &mut Vec<(gtk::glib::Object, gtk::glib::SignalHandlerId)>) {
    while let Some((object, id)) = handlers.pop() {
        object.disconnect(id);
    }
}

fn clear_box_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
