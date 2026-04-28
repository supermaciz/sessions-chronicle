use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use relm4::gtk;
use std::cell::Cell;

const BAR_HEIGHT: i32 = 8;
const EDIT_COLOR: (f32, f32, f32) = (0.9019608, 0.38039216, 0.0); // #e66100
const COMMAND_COLOR: (f32, f32, f32) = (0.14901961, 0.63529414, 0.4117647); // #26a269
const READ_COLOR: (f32, f32, f32) = (0.20784314, 0.5176471, 0.89411765); // #3584e4

fn activity_accessible_label(edit_count: usize, command_count: usize, read_count: usize) -> String {
    format!(
        "Activity: {}, {}, {}",
        crate::ui::format::format_count(edit_count, "edit", "edits"),
        crate::ui::format::format_count(command_count, "command", "commands"),
        crate::ui::format::format_count(read_count, "read", "reads"),
    )
}

pub(crate) fn segment_widths(
    edit_count: usize,
    command_count: usize,
    read_count: usize,
    total_width: f32,
) -> [f32; 3] {
    if total_width <= 0.0 {
        return [0.0, 0.0, 0.0];
    }

    let counts = [edit_count as f32, command_count as f32, read_count as f32];
    let total = counts.iter().sum::<f32>();
    if total <= 0.0 {
        return [0.0, 0.0, 0.0];
    }

    let mut widths = [0.0, 0.0, 0.0];
    let mut used = 0.0;
    let last_visible = counts.iter().rposition(|count| *count > 0.0).unwrap();

    for (index, count) in counts.iter().enumerate() {
        if *count == 0.0 {
            continue;
        }

        let width = if index == last_visible {
            total_width - used
        } else {
            (total_width * *count / total).floor()
        };

        widths[index] = width.max(0.0);
        used += widths[index];
    }

    widths
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SessionActivityBar {
        pub edit_count: Cell<usize>,
        pub command_count: Cell<usize>,
        pub read_count: Cell<usize>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionActivityBar {
        const NAME: &'static str = "ScSessionActivityBar";
        type Type = super::SessionActivityBar;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SessionActivityBar {}

    impl WidgetImpl for SessionActivityBar {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (0, 0, -1, -1),
                gtk::Orientation::Vertical => (BAR_HEIGHT, BAR_HEIGHT, -1, -1),
                _ => (BAR_HEIGHT, BAR_HEIGHT, -1, -1),
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let total_width = widget.width() as f32;
            let total_height = widget.height().max(BAR_HEIGHT) as f32;

            let widths = segment_widths(
                self.edit_count.get(),
                self.command_count.get(),
                self.read_count.get(),
                total_width,
            );

            let colors = [
                gtk::gdk::RGBA::new(EDIT_COLOR.0, EDIT_COLOR.1, EDIT_COLOR.2, 1.0),
                gtk::gdk::RGBA::new(COMMAND_COLOR.0, COMMAND_COLOR.1, COMMAND_COLOR.2, 1.0),
                gtk::gdk::RGBA::new(READ_COLOR.0, READ_COLOR.1, READ_COLOR.2, 1.0),
            ];

            let mut x = 0.0;
            for (width, color) in widths.into_iter().zip(colors) {
                if width <= 0.0 {
                    continue;
                }
                let rect = gtk::graphene::Rect::new(x, 0.0, width, total_height);
                snapshot.append_color(&color, &rect);
                x += width;
            }
        }
    }
}

glib::wrapper! {
    pub struct SessionActivityBar(ObjectSubclass<imp::SessionActivityBar>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SessionActivityBar {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn set_counts(&self, edit_count: usize, command_count: usize, read_count: usize) {
        let imp = self.imp();
        imp.edit_count.set(edit_count);
        imp.command_count.set(command_count);
        imp.read_count.set(read_count);

        let label = activity_accessible_label(edit_count, command_count, read_count);
        self.set_tooltip_text(Some(&label));
        self.update_property(&[gtk::accessible::Property::Label(&label)]);

        self.queue_draw();
    }
}

impl Default for SessionActivityBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_widths_fill_the_full_allocated_width() {
        assert_eq!(segment_widths(14, 9, 3, 70.0), [37.0, 24.0, 9.0]);
    }

    #[test]
    fn segment_widths_return_zeroes_when_no_activity_exists() {
        assert_eq!(segment_widths(0, 0, 0, 70.0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn segment_widths_allow_full_shrink_when_width_is_zero() {
        assert_eq!(segment_widths(4, 2, 1, 0.0), [0.0, 0.0, 0.0]);
    }

    #[gtk::test]
    fn activity_bar_updates_accessible_label_from_counts() {
        let bar = SessionActivityBar::new();
        bar.set_counts(4, 2, 1);

        assert_eq!(
            bar.tooltip_text().as_deref(),
            Some("Activity: 4 edits, 2 commands, 1 read")
        );
    }
}
