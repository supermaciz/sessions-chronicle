use gtk::prelude::*;
use relm4::adw::prelude::ActionRowExt;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};

use crate::models::AnalyticsData;
use crate::ui::analytics_heatmap::AnalyticsHeatmap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyticsPageState {
    Loading,
    Ready,
    Empty,
    Error,
}

#[derive(Debug, Default, Clone)]
pub struct AnalyticsViewModel {
    pub data: Option<AnalyticsData>,
    pub stale: bool,
    pub refresh_in_flight: bool,
}

impl AnalyticsViewModel {
    pub fn from_data(data: AnalyticsData) -> Self {
        Self {
            data: Some(data),
            stale: false,
            refresh_in_flight: false,
        }
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub fn on_entered(&mut self) -> bool {
        if !self.refresh_in_flight && (self.data.is_none() || self.stale) {
            self.refresh_in_flight = true;
            return true;
        }

        false
    }

    pub fn page_state(&self, has_error: bool) -> AnalyticsPageState {
        if self.data.is_some() {
            AnalyticsPageState::Ready
        } else if has_error {
            AnalyticsPageState::Error
        } else if self.refresh_in_flight {
            AnalyticsPageState::Loading
        } else {
            AnalyticsPageState::Empty
        }
    }

    pub fn inline_warning_message<'a>(&self, load_error: Option<&'a str>) -> Option<&'a str> {
        if self.data.is_some() {
            load_error
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct AnalyticsView {
    model: AnalyticsViewModel,
    load_error: Option<String>,
}

#[derive(Debug)]
pub enum AnalyticsViewMsg {
    Entered,
    LoadingStarted,
    Loaded(AnalyticsData),
    LoadFailed(String),
    MarkStale,
    Retry,
}

#[derive(Debug)]
pub enum AnalyticsViewOutput {
    RefreshRequested,
}

#[relm4::component(pub)]
impl SimpleComponent for AnalyticsView {
    type Init = Option<AnalyticsData>;
    type Input = AnalyticsViewMsg;
    type Output = AnalyticsViewOutput;
    type Widgets = AnalyticsViewWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_vexpand: true,
            set_hexpand: true,
            add_css_class: "analytics-page",

            #[name = "state_stack"]
            gtk::Stack {
                set_vexpand: true,
                set_hexpand: true,

                #[name = "loading_state"]
                adw::StatusPage {
                    set_title: "Loading analytics",
                    set_description: Some("Computing dashboard metrics..."),
                    set_icon_name: Some("view-refresh-symbolic"),
                },

                #[name = "empty_state"]
                adw::StatusPage {
                    set_title: "No analytics yet",
                    set_description: Some("Analytics will appear after your sessions are indexed."),
                    set_icon_name: Some("view-grid-symbolic"),
                },

                #[name = "error_state"]
                adw::StatusPage {
                    set_title: "Unable to load analytics",
                    set_description: Some("Try refreshing to load analytics again."),
                    set_icon_name: Some("dialog-warning-symbolic"),

                    #[wrap(Some)]
                    set_child = &gtk::Button {
                        set_label: "Retry",
                        connect_clicked[sender] => move |_| {
                            sender.input(AnalyticsViewMsg::Retry);
                        }
                    }
                },

                #[name = "ready_scroller"]
                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    adw::Clamp {
                        set_maximum_size: 960,
                        set_tightening_threshold: 640,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 16,
                            set_margin_all: 16,

                            #[name = "refresh_warning_revealer"]
                            gtk::Revealer {
                                set_reveal_child: false,
                                set_transition_type: gtk::RevealerTransitionType::SlideDown,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 8,
                                    set_margin_all: 12,
                                    add_css_class: "analytics-inline-warning",

                                    gtk::Image {
                                        set_icon_name: Some("dialog-warning-symbolic"),
                                        set_valign: gtk::Align::Center,
                                    },

                                    #[name = "refresh_warning_label"]
                                    gtk::Label {
                                        set_label: "",
                                        set_halign: gtk::Align::Start,
                                        set_xalign: 0.0,
                                        set_wrap: true,
                                    }
                                }
                            },

                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,
                                add_css_class: "analytics-section",

                                gtk::Label {
                                    set_label: "Overview",
                                    set_halign: gtk::Align::Start,
                                    add_css_class: "analytics-section-title",
                                },

                                gtk::FlowBox {
                                    set_selection_mode: gtk::SelectionMode::None,
                                    set_row_spacing: 8,
                                    set_column_spacing: 8,
                                    set_homogeneous: true,
                                    set_max_children_per_line: 4,
                                    set_min_children_per_line: 1,

                                    append = &gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 4,
                                        set_margin_all: 8,
                                        add_css_class: "analytics-metric-card",

                                        #[name = "total_sessions_value"]
                                        gtk::Label {
                                            set_label: "0",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-value",
                                        },

                                        gtk::Label {
                                            set_label: "Total sessions",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-label",
                                        }
                                    },

                                    append = &gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 4,
                                        set_margin_all: 8,
                                        add_css_class: "analytics-metric-card",

                                        #[name = "total_messages_value"]
                                        gtk::Label {
                                            set_label: "0",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-value",
                                        },

                                        gtk::Label {
                                            set_label: "Total messages",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-label",
                                        }
                                    },

                                    append = &gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 4,
                                        set_margin_all: 8,
                                        add_css_class: "analytics-metric-card",

                                        #[name = "distinct_projects_value"]
                                        gtk::Label {
                                            set_label: "0",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-value",
                                        },

                                        gtk::Label {
                                            set_label: "Distinct projects",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-label",
                                        }
                                    },

                                    append = &gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 4,
                                        set_margin_all: 8,
                                        add_css_class: "analytics-metric-card",

                                        #[name = "active_days_value"]
                                        gtk::Label {
                                            set_label: "0",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-value",
                                        },

                                        gtk::Label {
                                            set_label: "Active days",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "analytics-metric-label",
                                        }
                                    }
                                }
                            },

                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,
                                add_css_class: "analytics-section",

                                gtk::Label {
                                    set_label: "Activity",
                                    set_halign: gtk::Align::Start,
                                    add_css_class: "analytics-section-title",
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    set_valign: gtk::Align::Start,

                                    #[name = "activity_heatmap"]
                                    AnalyticsHeatmap {
                                        set_halign: gtk::Align::Start,
                                        set_vexpand: false,
                                        set_hexpand: false,
                                    },

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 6,

                                        gtk::Label {
                                            set_label: "Legend",
                                            set_halign: gtk::Align::Start,
                                            add_css_class: "caption",
                                        },

                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Horizontal,
                                            set_spacing: 8,

                                            gtk::Box {
                                                add_css_class: "analytics-heatmap-legend-swatch",
                                                add_css_class: "heatmap-cell-empty",
                                            },

                                            gtk::Label {
                                                set_label: "No sessions",
                                                set_halign: gtk::Align::Start,
                                            }
                                        },

                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Horizontal,
                                            set_spacing: 8,

                                            gtk::Box {
                                                add_css_class: "analytics-heatmap-legend-swatch",
                                                add_css_class: "heatmap-cell-low",
                                            },

                                            gtk::Label {
                                                set_label: "Low",
                                                set_halign: gtk::Align::Start,
                                            }
                                        },

                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Horizontal,
                                            set_spacing: 8,

                                            gtk::Box {
                                                add_css_class: "analytics-heatmap-legend-swatch",
                                                add_css_class: "heatmap-cell-medium",
                                            },

                                            gtk::Label {
                                                set_label: "Medium",
                                                set_halign: gtk::Align::Start,
                                            }
                                        },

                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Horizontal,
                                            set_spacing: 8,

                                            gtk::Box {
                                                add_css_class: "analytics-heatmap-legend-swatch",
                                                add_css_class: "heatmap-cell-high",
                                            },

                                            gtk::Label {
                                                set_label: "High",
                                                set_halign: gtk::Align::Start,
                                            }
                                        }
                                    }
                                }
                            },

                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,
                                add_css_class: "analytics-section",

                            gtk::Label {
                                set_label: "Sessions by tool",
                                set_halign: gtk::Align::Start,
                                add_css_class: "analytics-section-title",
                            },

                            #[name = "tool_progress_placeholder"]
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 6,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 8,
                                    add_css_class: "analytics-progress-row",

                                    gtk::Label {
                                        set_label: "Claude Code",
                                        set_width_chars: 14,
                                        set_halign: gtk::Align::Start,
                                    },

                                    gtk::ProgressBar {
                                        set_hexpand: true,
                                        set_fraction: 0.0,
                                    }
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 8,
                                    add_css_class: "analytics-progress-row",

                                    gtk::Label {
                                        set_label: "OpenCode",
                                        set_width_chars: 14,
                                        set_halign: gtk::Align::Start,
                                    },

                                    gtk::ProgressBar {
                                        set_hexpand: true,
                                        set_fraction: 0.0,
                                    }
                                }
                            }
                            },

                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,
                                add_css_class: "analytics-section",

                            gtk::Label {
                                set_label: "Token consumption",
                                set_halign: gtk::Align::Start,
                                add_css_class: "analytics-section-title",
                            },

                            #[name = "token_rows"]
                            gtk::ListBox {
                                add_css_class: "boxed-list",
                                set_selection_mode: gtk::SelectionMode::None,

                                append = &adw::ActionRow::builder()
                                    .title("Input tokens")
                                    .build() {
                                    add_suffix = &gtk::Label::new(Some("-")) {}
                                },

                                append = &adw::ActionRow::builder()
                                    .title("Output tokens")
                                    .build() {
                                    add_suffix = &gtk::Label::new(Some("-")) {}
                                },

                                append = &adw::ActionRow::builder()
                                    .title("Total tokens")
                                    .build() {
                                    add_suffix = &gtk::Label::new(Some("-")) {}
                                }
                            }
                            },

                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,
                                add_css_class: "analytics-section",

                            gtk::Label {
                                set_label: "Session span distribution",
                                set_halign: gtk::Align::Start,
                                add_css_class: "analytics-section-title",
                            },

                            #[name = "span_progress_placeholder"]
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 6,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 8,
                                    add_css_class: "analytics-progress-row",

                                    gtk::Label {
                                        set_label: "< 5 min",
                                        set_width_chars: 14,
                                        set_halign: gtk::Align::Start,
                                    },

                                    gtk::ProgressBar {
                                        set_hexpand: true,
                                        set_fraction: 0.0,
                                    }
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 8,
                                    add_css_class: "analytics-progress-row",

                                    gtk::Label {
                                        set_label: "5-15 min",
                                        set_width_chars: 14,
                                        set_halign: gtk::Align::Start,
                                    },

                                    gtk::ProgressBar {
                                        set_hexpand: true,
                                        set_fraction: 0.0,
                                    }
                                }
                            }
                            }
                        }
                    }
                }
            }
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            model: init.map(AnalyticsViewModel::from_data).unwrap_or_default(),
            load_error: None,
        };
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AnalyticsViewMsg::Entered => {
                if self.model.on_entered() {
                    self.load_error = None;
                    let _ = sender.output(AnalyticsViewOutput::RefreshRequested);
                }
            }
            AnalyticsViewMsg::LoadingStarted => {
                self.model.refresh_in_flight = true;
                self.load_error = None;
            }
            AnalyticsViewMsg::Loaded(data) => {
                self.model = AnalyticsViewModel::from_data(data);
                self.load_error = None;
            }
            AnalyticsViewMsg::LoadFailed(error) => {
                self.model.refresh_in_flight = false;
                self.load_error = Some(error);
            }
            AnalyticsViewMsg::MarkStale => {
                self.model.mark_stale();
            }
            AnalyticsViewMsg::Retry => {
                self.model.refresh_in_flight = true;
                self.load_error = None;
                let _ = sender.output(AnalyticsViewOutput::RefreshRequested);
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        if let Some(data) = &self.model.data {
            widgets
                .total_sessions_value
                .set_label(&data.overview.total_sessions.to_string());
            widgets
                .total_messages_value
                .set_label(&data.overview.total_messages.to_string());
            widgets
                .distinct_projects_value
                .set_label(&data.overview.distinct_projects.to_string());
            widgets
                .active_days_value
                .set_label(&data.overview.active_days.to_string());
            widgets
                .activity_heatmap
                .set_heatmap_data(data.heatmap.clone());
        }

        let state = self.model.page_state(self.load_error.is_some());
        let warning_message = self
            .model
            .inline_warning_message(self.load_error.as_deref());

        if let Some(message) = warning_message {
            widgets.refresh_warning_label.set_label(message);
            widgets.refresh_warning_revealer.set_reveal_child(true);
        } else {
            widgets.refresh_warning_revealer.set_reveal_child(false);
        }

        match state {
            AnalyticsPageState::Loading => {
                widgets
                    .state_stack
                    .set_visible_child(&widgets.loading_state);
            }
            AnalyticsPageState::Ready => {
                widgets
                    .state_stack
                    .set_visible_child(&widgets.ready_scroller);
            }
            AnalyticsPageState::Empty => {
                widgets.state_stack.set_visible_child(&widgets.empty_state);
            }
            AnalyticsPageState::Error => {
                widgets.state_stack.set_visible_child(&widgets.error_state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_state_keeps_cached_content_visible_on_refresh_failure() {
        let model = AnalyticsViewModel::from_data(AnalyticsData::default());

        assert_eq!(model.page_state(true), AnalyticsPageState::Ready);
        assert_eq!(
            model.inline_warning_message(Some("Load failed")),
            Some("Load failed")
        );
    }

    #[test]
    fn non_ready_states_hide_inline_warning() {
        let model = AnalyticsViewModel::default();

        assert_eq!(model.page_state(true), AnalyticsPageState::Error);
        assert_eq!(model.inline_warning_message(Some("Load failed")), None);
    }

    #[test]
    fn entered_requests_refresh_when_empty() {
        let mut model = AnalyticsViewModel::default();

        assert_eq!(model.page_state(false), AnalyticsPageState::Empty);
        assert!(model.on_entered());
        assert!(model.refresh_in_flight);
        assert_eq!(model.page_state(false), AnalyticsPageState::Loading);
    }

    #[test]
    fn stale_cache_keeps_content_visible_while_refreshing() {
        let data = AnalyticsData::default();
        let mut model = AnalyticsViewModel::from_data(data.clone());

        model.mark_stale();

        assert!(model.on_entered());
        assert!(model.refresh_in_flight);
        assert_eq!(model.data, Some(data));
        assert_eq!(model.page_state(false), AnalyticsPageState::Ready);
    }
}
