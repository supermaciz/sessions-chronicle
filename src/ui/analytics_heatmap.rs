use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use relm4::gtk;
use std::cell::RefCell;

use crate::models::analytics::{ActivityDay, HeatmapData, HeatmapWeek};

const CELL_SIZE: f32 = 12.0;
const CELL_GAP: f32 = 3.0;
const PADDING: f32 = 6.0;

pub(crate) fn cell_accessible_label(day: &ActivityDay) -> String {
    if day.session_count == 0 {
        format!("{}: no sessions", day.day)
    } else if day.session_count == 1 {
        format!("{}: 1 session", day.day)
    } else {
        format!("{}: {} sessions", day.day, day.session_count)
    }
}

pub(crate) fn summarize_heatmap(data: &HeatmapData) -> String {
    let mut shown_days = 0;
    let mut active_days = 0;
    let mut total_sessions = 0;
    let mut first_day: Option<&ActivityDay> = None;
    let mut last_day: Option<&ActivityDay> = None;

    for day in data.weeks.iter().flat_map(|week| week.days.iter()) {
        if first_day.is_none() {
            first_day = Some(day);
        }
        last_day = Some(day);
        shown_days += 1;
        if day.session_count > 0 {
            active_days += 1;
            total_sessions += day.session_count;
        }
    }

    if shown_days == 0 {
        return "No activity data".to_string();
    }

    let peak_text = if data.max_sessions_in_a_day == 1 {
        "peak 1 session/day".to_string()
    } else {
        format!("peak {} sessions/day", data.max_sessions_in_a_day)
    };

    let mut summary = format!(
        "Heatmap summary: {} sessions across {} active days ({} days shown); {}.",
        total_sessions, active_days, shown_days, peak_text
    );

    if let Some(first_day) = first_day {
        summary.push(' ');
        summary.push_str("Range: ");
        summary.push_str(&cell_accessible_label(first_day));
        if let Some(last_day) = last_day
            && first_day.day != last_day.day
        {
            summary.push_str(" to ");
            summary.push_str(&cell_accessible_label(last_day));
        }
        summary.push('.');
    }

    summary
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
    // Keep these colors synchronized with the legend CSS classes in data/resources/style.css.
    match class {
        "heatmap-cell-low" => gtk::gdk::RGBA::new(0.55, 0.78, 0.58, 1.0),
        "heatmap-cell-medium" => gtk::gdk::RGBA::new(0.31, 0.68, 0.35, 1.0),
        "heatmap-cell-high" => gtk::gdk::RGBA::new(0.16, 0.53, 0.20, 1.0),
        _ => gtk::gdk::RGBA::new(0.72, 0.72, 0.72, 0.45),
    }
}

#[allow(dead_code)]
pub(crate) const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Returns `(week_index, month_abbrev)` pairs for each month boundary.
///
/// A new label is placed at the first week column whose first day belongs
/// to a month not yet seen.
#[allow(dead_code)]
pub(crate) fn month_boundary_labels(weeks: &[HeatmapWeek]) -> Vec<(usize, &'static str)> {
    let mut labels = Vec::new();
    let mut last_month: Option<u32> = None;

    for (week_index, week) in weeks.iter().enumerate() {
        if let Some(first_day) = week.days.first()
            && first_day.day.len() >= 7
            && let Ok(month) = first_day.day[5..7].parse::<u32>()
            && last_month != Some(month)
        {
            last_month = Some(month);
            if (1..=12).contains(&month) {
                labels.push((week_index, MONTH_NAMES[(month - 1) as usize]));
            }
        }
    }

    labels
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
        let summary = summarize_heatmap(&data);

        let imp = self.imp();
        imp.data.replace(data);
        self.set_tooltip_text(Some(&summary));
        self.update_property(&[gtk::accessible::Property::Label(&summary)]);
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
    use super::{cell_accessible_label, intensity_class, month_boundary_labels, summarize_heatmap};
    use crate::models::analytics::{ActivityDay, HeatmapData, HeatmapWeek};

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
                session_count: 1,
            }),
            "2026-03-02: 1 session"
        );
        assert_eq!(
            cell_accessible_label(&ActivityDay {
                day: "2026-03-03".to_string(),
                session_count: 3,
            }),
            "2026-03-03: 3 sessions"
        );
    }

    #[test]
    fn intensity_class_scales_against_max_day() {
        assert_eq!(intensity_class(0, 4), "heatmap-cell-empty");
        assert_eq!(intensity_class(1, 4), "heatmap-cell-low");
        assert_eq!(intensity_class(2, 4), "heatmap-cell-medium");
        assert_eq!(intensity_class(3, 4), "heatmap-cell-high");
        assert_eq!(intensity_class(4, 4), "heatmap-cell-high");
    }

    #[test]
    fn intensity_class_handles_threshold_boundaries() {
        assert_eq!(intensity_class(3, 8), "heatmap-cell-low");
        assert_eq!(intensity_class(4, 8), "heatmap-cell-medium");
        assert_eq!(intensity_class(5, 8), "heatmap-cell-medium");
        assert_eq!(intensity_class(6, 8), "heatmap-cell-high");
        assert_eq!(intensity_class(2, 5), "heatmap-cell-low");
        assert_eq!(intensity_class(3, 5), "heatmap-cell-medium");
        assert_eq!(intensity_class(4, 5), "heatmap-cell-high");
    }

    #[test]
    fn summarize_heatmap_is_concise_and_aggregated() {
        let summary = summarize_heatmap(&HeatmapData {
            weeks: vec![
                HeatmapWeek {
                    days: vec![
                        ActivityDay {
                            day: "2026-03-01".to_string(),
                            session_count: 0,
                        },
                        ActivityDay {
                            day: "2026-03-02".to_string(),
                            session_count: 1,
                        },
                    ],
                },
                HeatmapWeek {
                    days: vec![ActivityDay {
                        day: "2026-03-03".to_string(),
                        session_count: 3,
                    }],
                },
            ],
            max_sessions_in_a_day: 3,
        });

        assert_eq!(
            summary,
            "Heatmap summary: 4 sessions across 2 active days (3 days shown); peak 3 sessions/day. Range: 2026-03-01: no sessions to 2026-03-03: 3 sessions."
        );
    }

    #[test]
    fn summarize_heatmap_returns_no_activity_when_no_days_are_shown() {
        let summary = summarize_heatmap(&HeatmapData {
            weeks: vec![],
            max_sessions_in_a_day: 0,
        });

        assert_eq!(summary, "No activity data");
    }

    #[test]
    fn summarize_heatmap_uses_singular_peak_wording_for_one_session_day() {
        let summary = summarize_heatmap(&HeatmapData {
            weeks: vec![HeatmapWeek {
                days: vec![
                    ActivityDay {
                        day: "2026-03-04".to_string(),
                        session_count: 1,
                    },
                    ActivityDay {
                        day: "2026-03-05".to_string(),
                        session_count: 0,
                    },
                ],
            }],
            max_sessions_in_a_day: 1,
        });

        assert_eq!(
            summary,
            "Heatmap summary: 1 sessions across 1 active days (2 days shown); peak 1 session/day. Range: 2026-03-04: 1 session to 2026-03-05: no sessions."
        );
    }

    #[test]
    fn summarize_heatmap_formats_single_day_range_without_to_separator() {
        let summary = summarize_heatmap(&HeatmapData {
            weeks: vec![HeatmapWeek {
                days: vec![ActivityDay {
                    day: "2026-03-06".to_string(),
                    session_count: 2,
                }],
            }],
            max_sessions_in_a_day: 2,
        });

        assert_eq!(
            summary,
            "Heatmap summary: 2 sessions across 1 active days (1 days shown); peak 2 sessions/day. Range: 2026-03-06: 2 sessions."
        );
    }

    #[test]
    fn month_labels_detects_boundaries_across_weeks() {
        let weeks = vec![
            HeatmapWeek {
                days: vec![
                    ActivityDay {
                        day: "2026-01-26".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-01-27".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-01-28".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-01-29".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-01-30".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-01-31".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-01".into(),
                        session_count: 0,
                    },
                ],
            },
            HeatmapWeek {
                days: vec![
                    ActivityDay {
                        day: "2026-02-02".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-03".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-04".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-05".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-06".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-07".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-08".into(),
                        session_count: 0,
                    },
                ],
            },
            HeatmapWeek {
                days: vec![
                    ActivityDay {
                        day: "2026-02-09".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-10".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-11".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-12".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-13".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-14".into(),
                        session_count: 0,
                    },
                    ActivityDay {
                        day: "2026-02-15".into(),
                        session_count: 0,
                    },
                ],
            },
        ];

        let labels = month_boundary_labels(&weeks);
        // Week 0 first day is Jan 26 → label "Jan" at column 0
        // Week 1 first day is Feb 02 → label "Feb" at column 1
        assert_eq!(labels, vec![(0, "Jan"), (1, "Feb")]);
    }

    #[test]
    fn month_labels_returns_empty_for_no_weeks() {
        let labels = month_boundary_labels(&[]);
        assert!(labels.is_empty());
    }

    #[test]
    fn month_labels_single_month_produces_one_label() {
        let weeks = vec![HeatmapWeek {
            days: vec![
                ActivityDay {
                    day: "2026-03-02".into(),
                    session_count: 0,
                },
                ActivityDay {
                    day: "2026-03-03".into(),
                    session_count: 0,
                },
                ActivityDay {
                    day: "2026-03-04".into(),
                    session_count: 0,
                },
                ActivityDay {
                    day: "2026-03-05".into(),
                    session_count: 0,
                },
                ActivityDay {
                    day: "2026-03-06".into(),
                    session_count: 0,
                },
                ActivityDay {
                    day: "2026-03-07".into(),
                    session_count: 0,
                },
                ActivityDay {
                    day: "2026-03-08".into(),
                    session_count: 0,
                },
            ],
        }];

        let labels = month_boundary_labels(&weeks);
        assert_eq!(labels, vec![(0, "Mar")]);
    }
}
