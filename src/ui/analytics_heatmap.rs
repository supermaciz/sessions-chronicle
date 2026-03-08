use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use relm4::gtk;
use std::cell::RefCell;

use crate::models::analytics::{ActivityDay, HeatmapData};

const CELL_SIZE: f32 = 12.0;
const CELL_GAP: f32 = 3.0;
const PADDING: f32 = 6.0;

pub(crate) fn cell_accessible_label(day: &ActivityDay) -> String {
    if day.session_count == 0 {
        format!("{}: no sessions", day.day)
    } else {
        format!("{}: {} sessions", day.day, day.session_count)
    }
}

pub(crate) fn intensity_class(session_count: i64, max_day: i64) -> &'static str {
    if session_count <= 0 || max_day <= 0 {
        "heatmap-cell-empty"
    } else if session_count * 4 >= max_day * 3 {
        "heatmap-cell-high"
    } else if session_count * 2 >= max_day {
        "heatmap-cell-medium"
    } else {
        "heatmap-cell-low"
    }
}

fn intensity_color(class: &str) -> gtk::gdk::RGBA {
    match class {
        "heatmap-cell-low" => gtk::gdk::RGBA::new(0.55, 0.78, 0.58, 1.0),
        "heatmap-cell-medium" => gtk::gdk::RGBA::new(0.31, 0.68, 0.35, 1.0),
        "heatmap-cell-high" => gtk::gdk::RGBA::new(0.16, 0.53, 0.20, 1.0),
        _ => gtk::gdk::RGBA::new(0.72, 0.72, 0.72, 0.45),
    }
}

fn draw_heatmap(widget: &AnalyticsHeatmap, snapshot: &gtk::Snapshot, data: &HeatmapData) {
    let width = widget.width() as f32;
    let height = widget.height() as f32;
    let clip = gtk::graphene::Rect::new(0.0, 0.0, width.max(1.0), height.max(1.0));
    snapshot.push_clip(&clip);

    for (week_index, week) in data.weeks.iter().enumerate() {
        let x = PADDING + (week_index as f32 * (CELL_SIZE + CELL_GAP));

        for (day_index, day) in week.days.iter().enumerate() {
            let y = PADDING + (day_index as f32 * (CELL_SIZE + CELL_GAP));
            let class = intensity_class(day.session_count, data.max_sessions_in_a_day);
            let color = intensity_color(class);
            let rect = gtk::graphene::Rect::new(x, y, CELL_SIZE, CELL_SIZE);

            snapshot.append_color(&color, &rect);
        }
    }

    snapshot.pop();
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AnalyticsHeatmap {
        pub data: RefCell<HeatmapData>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AnalyticsHeatmap {
        const NAME: &'static str = "ScAnalyticsHeatmap";
        type Type = super::AnalyticsHeatmap;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for AnalyticsHeatmap {}

    impl WidgetImpl for AnalyticsHeatmap {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let data = self.data.borrow();
            let week_count = data.weeks.len().max(1);
            let row_count = data
                .weeks
                .iter()
                .map(|week| week.days.len())
                .max()
                .unwrap_or(7)
                .max(1);

            let width = (PADDING * 2.0
                + (week_count as f32 * CELL_SIZE)
                + ((week_count.saturating_sub(1)) as f32 * CELL_GAP))
                as i32;
            let height = (PADDING * 2.0
                + (row_count as f32 * CELL_SIZE)
                + ((row_count.saturating_sub(1)) as f32 * CELL_GAP))
                as i32;

            match orientation {
                gtk::Orientation::Horizontal => (width, width, -1, -1),
                gtk::Orientation::Vertical => (height, height, -1, -1),
                _ => (height, height, -1, -1),
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            super::draw_heatmap(&widget, snapshot, &self.data.borrow());
        }
    }
}

glib::wrapper! {
    pub struct AnalyticsHeatmap(ObjectSubclass<imp::AnalyticsHeatmap>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AnalyticsHeatmap {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn set_heatmap_data(&self, data: HeatmapData) {
        let summary = data
            .weeks
            .iter()
            .flat_map(|week| week.days.iter())
            .map(cell_accessible_label)
            .collect::<Vec<_>>()
            .join("; ");

        let imp = self.imp();
        imp.data.replace(data);
        if summary.is_empty() {
            self.set_tooltip_text(Some("No activity data"));
        } else {
            self.set_tooltip_text(Some(&summary));
        }
        self.queue_draw();
        self.queue_resize();
    }
}

impl Default for AnalyticsHeatmap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{cell_accessible_label, intensity_class};
    use crate::models::analytics::ActivityDay;

    #[test]
    fn accessible_label_describes_empty_and_non_empty_cells() {
        assert_eq!(
            cell_accessible_label(&ActivityDay {
                day: "2026-03-01".to_string(),
                session_count: 0,
            }),
            "2026-03-01: no sessions"
        );
        assert_eq!(
            cell_accessible_label(&ActivityDay {
                day: "2026-03-02".to_string(),
                session_count: 3,
            }),
            "2026-03-02: 3 sessions"
        );
    }

    #[test]
    fn intensity_class_scales_against_max_day() {
        assert_eq!(intensity_class(0, 4), "heatmap-cell-empty");
        assert_eq!(intensity_class(1, 4), "heatmap-cell-low");
        assert_eq!(intensity_class(4, 4), "heatmap-cell-high");
    }
}
