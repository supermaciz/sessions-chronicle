use chrono::NaiveDate;
use gtk::prelude::*;
use relm4::adw::prelude::ActionRowExt;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent, adw, gtk};

use crate::models::AnalyticsData;
use crate::models::analytics::{AiAssistantSessionCount, AiAssistantTokenUsage, SessionSpanBucket};
use crate::ui::analytics_heatmap::AnalyticsHeatmap;
use crate::ui::format::format_token_count;

const TOKEN_SECTION_NO_DATA_COPY: &str = "Token data is not available for the indexed sessions";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenSectionState {
    subtitle: Option<String>,
    empty_message: Option<String>,
}

fn token_section_state(rows: &[AiAssistantTokenUsage]) -> TokenSectionState {
    let total_sessions = rows
        .iter()
        .map(|row| row.total_sessions.max(0))
        .sum::<i64>();
    let reported_sessions = rows
        .iter()
        .map(|row| row.reported_sessions.max(0))
        .sum::<i64>();

    if reported_sessions == 0 {
        return TokenSectionState {
            subtitle: None,
            empty_message: Some(TOKEN_SECTION_NO_DATA_COPY.to_string()),
        };
    }

    let subtitle = (reported_sessions < total_sessions).then(|| {
        format!(
            "Based on {} of {} sessions that report token usage",
            reported_sessions, total_sessions
        )
    });

    TokenSectionState {
        subtitle,
        empty_message: None,
    }
}

fn format_heatmap_range_label(start_day: Option<&str>, end_day: Option<&str>) -> Option<String> {
    let start_day = NaiveDate::parse_from_str(start_day?, "%Y-%m-%d").ok()?;
    let end_day = NaiveDate::parse_from_str(end_day?, "%Y-%m-%d").ok()?;

    let start = start_day.format("%b %Y").to_string();
    let end = end_day.format("%b %Y").to_string();

    if start == end {
        Some(start)
    } else {
        Some(format!("{start} - {end}"))
    }
}

fn clear_box_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn clear_listbox_children(container: &gtk::ListBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn progress_fraction(value: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        (value.max(0) as f64 / total as f64).clamp(0.0, 1.0)
    }
}

fn build_progress_row(label: &str, value: i64, total: i64) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("analytics-progress-row");

    let name = gtk::Label::new(Some(label));
    name.set_width_chars(14);
    name.set_halign(gtk::Align::Start);
    name.set_xalign(0.0);
    row.append(&name);

    let progress = gtk::ProgressBar::new();
    progress.set_hexpand(true);
    progress.set_fraction(progress_fraction(value, total));
    row.append(&progress);

    let value_label = gtk::Label::new(Some(&value.to_string()));
    value_label.set_halign(gtk::Align::End);
    value_label.set_xalign(1.0);
    row.append(&value_label);

    row
}

fn render_sessions_by_tool_rows(
    container: &gtk::Box,
    rows: &[AiAssistantSessionCount],
    total_sessions: i64,
) {
    clear_box_children(container);
    for row in rows {
        container.append(&build_progress_row(
            &row.tool,
            row.session_count,
            total_sessions,
        ));
    }
}

fn render_span_bucket_rows(container: &gtk::Box, rows: &[SessionSpanBucket], total_sessions: i64) {
    clear_box_children(container);
    for row in rows {
        container.append(&build_progress_row(
            &row.bucket,
            row.session_count,
            total_sessions,
        ));
    }
}

fn token_row_subtitle(row: &AiAssistantTokenUsage) -> String {
    if row.reported_sessions == row.total_sessions {
        format!("{} sessions report token usage", row.reported_sessions)
    } else {
        format!(
            "{} of {} sessions report token usage",
            row.reported_sessions, row.total_sessions
        )
    }
}

fn render_token_usage_rows(
    container: &gtk::ListBox,
    subtitle_label: &gtk::Label,
    rows: &[AiAssistantTokenUsage],
) {
    clear_listbox_children(container);

    let state = token_section_state(rows);
    if let Some(subtitle) = &state.subtitle {
        subtitle_label.set_label(subtitle);
        subtitle_label.set_visible(true);
    } else {
        subtitle_label.set_visible(false);
    }

    if let Some(message) = state.empty_message {
        let row = adw::ActionRow::builder().title(message).build();
        container.append(&row);
        return;
    }

    for usage in rows {
        let row = adw::ActionRow::builder()
            .title(&usage.tool)
            .subtitle(token_row_subtitle(usage))
            .build();

        let value = if usage.reported_sessions == 0 {
            let label = gtk::Label::new(Some("—"));
            label.set_tooltip_text(Some(&format!(
                "Token data not available for {}",
                usage.tool
            )));
            label
        } else {
            let input = usage.input_tokens.unwrap_or(0);
            let output = usage.output_tokens.unwrap_or(0);
            let total = input + output;
            gtk::Label::new(Some(&format!(
                "{} in / {} out / {} total",
                format_token_count(input),
                format_token_count(output),
                format_token_count(total)
            )))
        };

        row.add_suffix(&value);
        container.append(&row);
    }
}

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

                                #[name = "activity_range_label"]
                                gtk::Label {
                                    set_label: "",
                                    set_halign: gtk::Align::Start,
                                    set_xalign: 0.0,
                                    set_visible: false,
                                    add_css_class: "caption",
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
                                    set_label: "Sessions by AI assistant",
                                    set_halign: gtk::Align::Start,
                                    add_css_class: "analytics-section-title",
                                },

                                #[name = "tool_progress_rows"]
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 6,
                                },
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

                                #[name = "token_section_subtitle"]
                                gtk::Label {
                                    set_label: "",
                                    set_halign: gtk::Align::Start,
                                    set_xalign: 0.0,
                                    set_wrap: true,
                                    set_visible: false,
                                    add_css_class: "caption",
                                },

                                #[name = "token_rows"]
                                gtk::ListBox {
                                    add_css_class: "boxed-list",
                                    set_selection_mode: gtk::SelectionMode::None,
                                },
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

                                #[name = "span_progress_rows"]
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 6,
                                },
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
                self.model.data = Some(data);
                self.model.stale = false;
                self.model.refresh_in_flight = false;
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
            if let Some(range) = format_heatmap_range_label(
                data.heatmap.display_start_day.as_deref(),
                data.heatmap.display_end_day.as_deref(),
            ) {
                widgets.activity_range_label.set_label(&range);
                widgets.activity_range_label.set_visible(true);
            } else {
                widgets.activity_range_label.set_visible(false);
            }
            render_sessions_by_tool_rows(
                &widgets.tool_progress_rows,
                &data.sessions_by_tool,
                data.overview.total_sessions,
            );
            render_token_usage_rows(
                &widgets.token_rows,
                &widgets.token_section_subtitle,
                &data.token_usage_by_tool,
            );
            render_span_bucket_rows(
                &widgets.span_progress_rows,
                &data.session_span_buckets,
                data.overview.total_sessions,
            );
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
    use crate::models::analytics::AiAssistantTokenUsage;

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

    #[test]
    fn loaded_clears_stale_flag() {
        let mut model = AnalyticsViewModel::from_data(AnalyticsData::default());
        model.mark_stale();
        assert!(model.stale);

        // Simulate what update() does on Loaded
        model.data = Some(AnalyticsData::default());
        model.stale = false;
        model.refresh_in_flight = false;

        assert!(!model.stale);
        // Should not request refresh on next enter
        assert!(!model.on_entered());
    }

    #[test]
    fn token_section_state_shows_partial_coverage_copy() {
        let rows = vec![
            AiAssistantTokenUsage {
                tool: "Claude Code".to_string(),
                total_sessions: 8,
                reported_sessions: 5,
                input_tokens: Some(1200),
                output_tokens: Some(800),
            },
            AiAssistantTokenUsage {
                tool: "OpenCode".to_string(),
                total_sessions: 4,
                reported_sessions: 0,
                input_tokens: None,
                output_tokens: None,
            },
        ];

        let state = token_section_state(&rows);

        assert_eq!(
            state.subtitle,
            Some("Based on 5 of 12 sessions that report token usage".to_string())
        );
        assert!(state.empty_message.is_none());
    }

    #[test]
    fn token_section_state_shows_no_data_copy_when_unavailable() {
        let rows = vec![
            AiAssistantTokenUsage {
                tool: "Claude Code".to_string(),
                total_sessions: 6,
                reported_sessions: 0,
                input_tokens: None,
                output_tokens: None,
            },
            AiAssistantTokenUsage {
                tool: "OpenCode".to_string(),
                total_sessions: 3,
                reported_sessions: 0,
                input_tokens: None,
                output_tokens: None,
            },
        ];

        let state = token_section_state(&rows);

        assert_eq!(
            state.empty_message,
            Some("Token data is not available for the indexed sessions".to_string())
        );
    }

    #[test]
    fn heatmap_range_formats_same_month_as_single_label() {
        let range = format_heatmap_range_label(Some("2026-03-02"), Some("2026-03-29"));
        assert_eq!(range, Some("Mar 2026".to_string()));
    }

    #[test]
    fn heatmap_range_formats_cross_month_with_dash() {
        let range = format_heatmap_range_label(Some("2025-10-13"), Some("2026-03-22"));
        assert_eq!(range, Some("Oct 2025 - Mar 2026".to_string()));
    }

    #[test]
    fn heatmap_range_returns_none_for_missing_or_invalid_bounds() {
        assert_eq!(format_heatmap_range_label(None, Some("2026-03-22")), None);
        assert_eq!(format_heatmap_range_label(Some("2025-10-13"), None), None);
        assert_eq!(
            format_heatmap_range_label(Some("bad-date"), Some("2026-03-22")),
            None
        );
    }
}
