use chrono::NaiveDate;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, gtk};

use gtk::glib;
use gtk::prelude::*;

use crate::models::{DateCounts, DateFilter};

pub struct DatePill {
    current_filter: DateFilter,
    counts: DateCounts,
    draft_from: Option<NaiveDate>,
    draft_to: Option<NaiveDate>,
    custom_revealed: bool,
    root: gtk::MenuButton,
    popover: gtk::Popover,
}

#[derive(Debug, Clone)]
pub enum DatePillInput {
    SetFilter(DateFilter),
    CountsReceived(DateCounts),
    OpenViaShortcut,
    PresetSelected(DateFilter),
    CustomRequested,
    DraftFromSelected(Option<NaiveDate>),
    DraftToSelected(Option<NaiveDate>),
    ClearDraft,
    ApplyDraft,
}

#[derive(Debug, Clone)]
pub enum DatePillOutput {
    FilterChanged(DateFilter),
    CountsRequested,
}

pub struct DatePillWidgets {
    root: gtk::MenuButton,
    popover: gtk::Popover,
    label: gtk::Label,
    custom_revealer: gtk::Revealer,
    from_calendar: gtk::Calendar,
    to_calendar: gtk::Calendar,
    info_label: gtk::Label,
    apply_button: gtk::Button,
    any_time_count: gtk::Label,
    today_count: gtk::Label,
    last_7_days_count: gtk::Label,
    last_30_days_count: gtk::Label,
    this_year_count: gtk::Label,
}

impl SimpleComponent for DatePill {
    type Init = DateFilter;
    type Input = DatePillInput;
    type Output = DatePillOutput;
    type Root = gtk::MenuButton;
    type Widgets = DatePillWidgets;

    fn init_root() -> Self::Root {
        gtk::MenuButton::new()
    }

    fn init(
        current_filter: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let icon = gtk::Image::from_icon_name("x-office-calendar-symbolic");
        let label = gtk::Label::new(None);
        label.set_visible(false);

        let button_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        button_content.append(&icon);
        button_content.append(&label);

        root.set_child(Some(&button_content));
        root.set_tooltip_text(Some(&tooltip_for_filter(&current_filter)));
        root.add_css_class("flat");

        let popover = gtk::Popover::new();
        root.set_popover(Some(&popover));

        let listbox = gtk::ListBox::new();
        listbox.add_css_class("boxed-list");
        listbox.set_selection_mode(gtk::SelectionMode::None);

        let any_time_count = gtk::Label::new(Some("0"));
        let today_count = gtk::Label::new(Some("0"));
        let last_7_days_count = gtk::Label::new(Some("0"));
        let last_30_days_count = gtk::Label::new(Some("0"));
        let this_year_count = gtk::Label::new(Some("0"));

        listbox.append(&build_preset_row("Any time", &any_time_count));
        listbox.append(&build_preset_row("Today", &today_count));
        listbox.append(&build_preset_row("Last 7 days", &last_7_days_count));
        listbox.append(&build_preset_row("Last 30 days", &last_30_days_count));
        listbox.append(&build_preset_row("This year", &this_year_count));
        listbox.append(&build_preset_row("Custom range...", &gtk::Label::new(None)));

        let from_calendar = gtk::Calendar::new();
        let to_calendar = gtk::Calendar::new();

        let calendars = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        calendars.append(&from_calendar);
        calendars.append(&to_calendar);

        let info_label = gtk::Label::new(Some(&custom_info_text(None, None)));
        info_label.set_xalign(0.0);
        info_label.set_wrap(true);

        let clear_button = gtk::Button::with_label("Clear");
        let apply_button = gtk::Button::with_label("Apply");
        apply_button.add_css_class("suggested-action");
        apply_button.set_sensitive(false);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_halign(gtk::Align::End);
        actions.append(&clear_button);
        actions.append(&apply_button);

        let custom_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
        custom_box.set_margin_top(12);
        custom_box.append(&calendars);
        custom_box.append(&info_label);
        custom_box.append(&actions);

        let custom_revealer = gtk::Revealer::new();
        custom_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        custom_revealer.set_child(Some(&custom_box));
        custom_revealer.set_reveal_child(false);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&listbox);
        content.append(&custom_revealer);
        popover.set_child(Some(&content));

        let output_sender = sender.output_sender().clone();
        popover.connect_visible_notify(move |popover| {
            if popover.is_visible() {
                output_sender.send(DatePillOutput::CountsRequested).ok();
            }
        });

        let input_sender = sender.input_sender().clone();
        listbox.connect_row_activated(move |_list, row| {
            match row.index() {
                0 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::AnyTime))
                    .ok(),
                1 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::Today))
                    .ok(),
                2 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::Last7Days))
                    .ok(),
                3 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::Last30Days))
                    .ok(),
                4 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::ThisYear))
                    .ok(),
                5 => input_sender.send(DatePillInput::CustomRequested).ok(),
                _ => None,
            };
        });

        let input_sender = sender.input_sender().clone();
        from_calendar.connect_day_selected(move |calendar| {
            input_sender
                .send(DatePillInput::DraftFromSelected(calendar_to_naive_date(
                    &calendar.date(),
                )))
                .ok();
        });

        let input_sender = sender.input_sender().clone();
        to_calendar.connect_day_selected(move |calendar| {
            input_sender
                .send(DatePillInput::DraftToSelected(calendar_to_naive_date(
                    &calendar.date(),
                )))
                .ok();
        });

        let input_sender = sender.input_sender().clone();
        clear_button.connect_clicked(move |_| {
            input_sender.send(DatePillInput::ClearDraft).ok();
        });

        let input_sender = sender.input_sender().clone();
        apply_button.connect_clicked(move |_| {
            input_sender.send(DatePillInput::ApplyDraft).ok();
        });

        let model = Self {
            current_filter,
            counts: DateCounts::default(),
            draft_from: None,
            draft_to: None,
            custom_revealed: false,
            root: root.clone(),
            popover: popover.clone(),
        };

        let widgets = DatePillWidgets {
            root,
            popover,
            label,
            custom_revealer,
            from_calendar,
            to_calendar,
            info_label,
            apply_button,
            any_time_count,
            today_count,
            last_7_days_count,
            last_30_days_count,
            this_year_count,
        };

        model.sync_button(&widgets);
        model.sync_counts(&widgets);
        model.sync_custom_state(&widgets);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            DatePillInput::SetFilter(filter) => {
                self.current_filter = filter;
                self.custom_revealed = matches!(self.current_filter, DateFilter::Custom { .. });
            }
            DatePillInput::CountsReceived(counts) => {
                self.counts = counts;
            }
            DatePillInput::OpenViaShortcut => {
                self.root.grab_focus();
                self.popover.popup();
            }
            DatePillInput::PresetSelected(filter) => {
                self.current_filter = filter.clone();
                self.custom_revealed = false;
                sender.output(DatePillOutput::FilterChanged(filter)).ok();
                self.popover.popdown();
            }
            DatePillInput::CustomRequested => {
                self.custom_revealed = true;
            }
            DatePillInput::DraftFromSelected(date) => {
                self.custom_revealed = true;
                self.draft_from = date;
            }
            DatePillInput::DraftToSelected(date) => {
                self.custom_revealed = true;
                self.draft_to = date;
            }
            DatePillInput::ClearDraft => {
                self.draft_from = None;
                self.draft_to = None;
            }
            DatePillInput::ApplyDraft => {
                if let Some(filter) = valid_custom_filter(self.draft_from, self.draft_to) {
                    self.current_filter = filter.clone();
                    self.custom_revealed = false;
                    sender.output(DatePillOutput::FilterChanged(filter)).ok();
                    self.popover.popdown();
                }
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        self.sync_button(widgets);
        self.sync_counts(widgets);
        self.sync_custom_state(widgets);
    }
}

impl DatePill {
    fn sync_button(&self, widgets: &DatePillWidgets) {
        let label = self.current_filter.pill_label();
        widgets.label.set_label(&label);
        widgets.label.set_visible(self.current_filter.is_active());
        widgets
            .root
            .set_tooltip_text(Some(&tooltip_for_filter(&self.current_filter)));
    }

    fn sync_counts(&self, widgets: &DatePillWidgets) {
        widgets
            .any_time_count
            .set_label(&self.counts.any_time.to_string());
        widgets
            .today_count
            .set_label(&self.counts.today.to_string());
        widgets
            .last_7_days_count
            .set_label(&self.counts.last_7_days.to_string());
        widgets
            .last_30_days_count
            .set_label(&self.counts.last_30_days.to_string());
        widgets
            .this_year_count
            .set_label(&self.counts.this_year.to_string());
    }

    fn sync_custom_state(&self, widgets: &DatePillWidgets) {
        widgets
            .custom_revealer
            .set_reveal_child(self.custom_revealed);
        widgets
            .info_label
            .set_label(&custom_info_text(self.draft_from, self.draft_to));
        widgets
            .apply_button
            .set_sensitive(valid_custom_filter(self.draft_from, self.draft_to).is_some());
    }
}

fn build_preset_row(title: &str, count_label: &gtk::Label) -> gtk::ListBoxRow {
    count_label.add_css_class("dim-label");
    count_label.set_xalign(1.0);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row_box.set_margin_top(8);
    row_box.set_margin_bottom(8);
    row_box.set_margin_start(12);
    row_box.set_margin_end(12);
    row_box.append(&title_label);
    row_box.append(count_label);

    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    row.set_child(Some(&row_box));
    row
}

fn tooltip_for_filter(filter: &DateFilter) -> String {
    if filter.is_active() {
        format!("Date: {}", filter.pill_label())
    } else {
        "Filter by date (Ctrl+Shift+D)".to_string()
    }
}

fn valid_custom_filter(from: Option<NaiveDate>, to: Option<NaiveDate>) -> Option<DateFilter> {
    match (from, to) {
        (Some(from), Some(to)) if from <= to => Some(DateFilter::Custom { from, to }),
        _ => None,
    }
}

fn custom_info_text(from: Option<NaiveDate>, to: Option<NaiveDate>) -> String {
    match (from, to) {
        (Some(from), Some(to)) if from <= to => DateFilter::Custom { from, to }.pill_label(),
        (Some(_), Some(_)) => "Start date must be on or before end date".to_string(),
        _ => "Choose a start and end date to apply a custom range".to_string(),
    }
}

fn calendar_to_naive_date(date: &glib::DateTime) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(
        date.year(),
        u32::try_from(date.month()).ok()?,
        u32::try_from(date.day_of_month()).ok()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_custom_filter_requires_both_dates_in_order() {
        let from = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 5, 7).unwrap();

        assert_eq!(valid_custom_filter(None, Some(to)), None);
        assert_eq!(valid_custom_filter(Some(from), None), None);
        assert_eq!(valid_custom_filter(Some(to), Some(from)), None);
        assert_eq!(
            valid_custom_filter(Some(from), Some(to)),
            Some(DateFilter::Custom { from, to })
        );
    }

    #[test]
    fn tooltip_reflects_active_filter() {
        let custom = DateFilter::Custom {
            from: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 5, 7).unwrap(),
        };

        assert_eq!(
            tooltip_for_filter(&DateFilter::AnyTime),
            "Filter by date (Ctrl+Shift+D)"
        );
        assert_eq!(
            tooltip_for_filter(&custom),
            format!("Date: {}", custom.pill_label())
        );
    }
}
