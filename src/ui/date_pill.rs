use chrono::{Datelike, Local, NaiveDate};
use gettextrs::gettext;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

use gtk::glib;
use gtk::prelude::*;

use crate::models::{DateCounts, DateFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YearDisplay {
    WithoutYear,
    WithYear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeEndpoint {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Presets,
    Custom,
}

pub struct DatePill {
    current_filter: DateFilter,
    counts: DateCounts,
    draft_from: Option<NaiveDate>,
    draft_to: Option<NaiveDate>,
    active_endpoint: RangeEndpoint,
    page: Page,
    listbox: gtk::ListBox,
    popover: gtk::Popover,
    stack: gtk::Stack,
    calendar: gtk::Calendar,
    summary_label: gtk::Label,
    calendar_handler: glib::SignalHandlerId,
}

#[derive(Debug, Clone)]
pub enum DatePillInput {
    PopoverOpened,
    CountsReceived(DateCounts),
    OpenViaShortcut,
    PresetSelected(DateFilter),
    CustomRangeRowSelected,
    BackToPresets,
    CustomDayPicked(NaiveDate),
    CustomEndpointChanged(RangeEndpoint),
    CustomApplyClicked,
    CustomClearClicked,
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
    stack: gtk::Stack,
    calendar: gtk::Calendar,
    endpoint_toggles: adw::ToggleGroup,
    start_toggle: adw::Toggle,
    end_toggle: adw::Toggle,
    start_date_label: gtk::Label,
    end_date_label: gtk::Label,
    summary_label: gtk::Label,
    apply_button: gtk::Button,
    escape_controller: gtk::ShortcutController,
    any_time_count: gtk::Label,
    today_count: gtk::Label,
    yesterday_count: gtk::Label,
    last_7_days_count: gtk::Label,
    last_30_days_count: gtk::Label,
    this_year_count: gtk::Label,
}

impl SimpleComponent for DatePill {
    type Init = ();
    type Input = DatePillInput;
    type Output = DatePillOutput;
    type Root = gtk::MenuButton;
    type Widgets = DatePillWidgets;

    fn init_root() -> Self::Root {
        gtk::MenuButton::new()
    }

    fn init(
        _init: Self::Init,
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
        root.set_tooltip_text(Some(&tooltip_for_filter(
            &DateFilter::AnyTime,
            Local::now().date_naive(),
        )));
        root.add_css_class("flat");

        let popover = gtk::Popover::new();
        root.set_popover(Some(&popover));

        let listbox = gtk::ListBox::new();
        listbox.add_css_class("boxed-list");
        listbox.set_selection_mode(gtk::SelectionMode::Browse);

        let any_time_count = gtk::Label::new(Some("0"));
        let today_count = gtk::Label::new(Some("0"));
        let yesterday_count = gtk::Label::new(Some("0"));
        let last_7_days_count = gtk::Label::new(Some("0"));
        let last_30_days_count = gtk::Label::new(Some("0"));
        let this_year_count = gtk::Label::new(Some("0"));

        listbox.append(&build_preset_row(&gettext("Any time"), &any_time_count));
        listbox.append(&build_preset_row(&gettext("Today"), &today_count));
        listbox.append(&build_preset_row(&gettext("Yesterday"), &yesterday_count));
        listbox.append(&build_preset_row(
            &gettext("Last 7 days"),
            &last_7_days_count,
        ));
        listbox.append(&build_preset_row(
            &gettext("Last 30 days"),
            &last_30_days_count,
        ));
        listbox.append(&build_preset_row(&gettext("This year"), &this_year_count));
        listbox.append(&build_custom_row(&gettext("Custom range...")));

        let presets_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        presets_page.set_margin_top(12);
        presets_page.set_margin_bottom(12);
        presets_page.set_margin_start(12);
        presets_page.set_margin_end(12);
        presets_page.append(&listbox);

        let back_button = gtk::Button::from_icon_name("go-previous-symbolic");
        back_button.add_css_class("flat");
        let heading = gtk::Label::new(Some(&gettext("Custom range")));
        heading.add_css_class("heading");
        heading.set_hexpand(true);
        heading.set_xalign(0.0);

        let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        title_row.append(&back_button);
        title_row.append(&heading);

        let (start_toggle, start_date_label) = build_endpoint_toggle(&gettext("Start date"));
        let (end_toggle, end_date_label) = build_endpoint_toggle(&gettext("End date"));
        let endpoint_toggles = adw::ToggleGroup::new();
        endpoint_toggles.set_homogeneous(true);
        endpoint_toggles.add(start_toggle.clone());
        endpoint_toggles.add(end_toggle.clone());
        endpoint_toggles.set_active(0);

        let calendar = gtk::Calendar::new();

        let summary_label = gtk::Label::new(Some(&custom_info_text(
            None,
            None,
            Local::now().date_naive(),
        )));
        summary_label.set_xalign(0.0);
        summary_label.set_wrap(true);

        let clear_button = gtk::Button::with_label(&gettext("Clear"));
        let apply_button = gtk::Button::with_label(&gettext("Apply"));
        apply_button.add_css_class("suggested-action");
        apply_button.set_sensitive(false);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_halign(gtk::Align::End);
        actions.append(&clear_button);
        actions.append(&apply_button);

        let custom_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
        custom_page.set_margin_top(12);
        custom_page.set_margin_bottom(12);
        custom_page.set_margin_start(12);
        custom_page.set_margin_end(12);
        custom_page.append(&title_row);
        custom_page.append(&endpoint_toggles);
        custom_page.append(&calendar);
        custom_page.append(&summary_label);
        custom_page.append(&actions);

        let escape_controller = build_escape_controller(sender.input_sender().clone());
        custom_page.add_controller(escape_controller.clone());

        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        stack.set_hhomogeneous(true);
        stack.set_vhomogeneous(false);
        stack.set_interpolate_size(true);
        stack.add_named(&presets_page, Some("presets"));
        stack.add_named(&custom_page, Some("custom"));
        stack.set_visible_child_name("presets");
        popover.set_child(Some(&stack));

        let input_sender = sender.input_sender().clone();
        popover.connect_visible_notify(move |popover| {
            if popover.is_visible() {
                input_sender.send(DatePillInput::PopoverOpened).ok();
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
                    .send(DatePillInput::PresetSelected(DateFilter::Yesterday))
                    .ok(),
                3 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::Last7Days))
                    .ok(),
                4 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::Last30Days))
                    .ok(),
                5 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::ThisYear))
                    .ok(),
                6 => input_sender
                    .send(DatePillInput::CustomRangeRowSelected)
                    .ok(),
                _ => None,
            };
        });

        let input_sender = sender.input_sender().clone();
        let calendar_handler = calendar.connect_day_selected(move |calendar| {
            if let Some(date) = calendar_to_naive_date(&calendar.date()) {
                input_sender.send(DatePillInput::CustomDayPicked(date)).ok();
            }
        });

        let input_sender = sender.input_sender().clone();
        endpoint_toggles.connect_active_notify(move |group| {
            let endpoint = if group.active() == 0 {
                RangeEndpoint::Start
            } else {
                RangeEndpoint::End
            };
            input_sender
                .send(DatePillInput::CustomEndpointChanged(endpoint))
                .ok();
        });

        let input_sender = sender.input_sender().clone();
        back_button.connect_clicked(move |_| {
            input_sender.send(DatePillInput::BackToPresets).ok();
        });

        let input_sender = sender.input_sender().clone();
        popover.connect_closed(move |_| {
            input_sender.send(DatePillInput::BackToPresets).ok();
        });

        let input_sender = sender.input_sender().clone();
        clear_button.connect_clicked(move |_| {
            input_sender.send(DatePillInput::CustomClearClicked).ok();
        });

        let input_sender = sender.input_sender().clone();
        apply_button.connect_clicked(move |_| {
            input_sender.send(DatePillInput::CustomApplyClicked).ok();
        });

        let model = Self {
            current_filter: DateFilter::AnyTime,
            counts: DateCounts::default(),
            draft_from: None,
            draft_to: None,
            active_endpoint: RangeEndpoint::Start,
            page: Page::Presets,
            listbox: listbox.clone(),
            popover: popover.clone(),
            stack: stack.clone(),
            calendar: calendar.clone(),
            summary_label: summary_label.clone(),
            calendar_handler,
        };

        let widgets = DatePillWidgets {
            root,
            popover,
            label,
            stack,
            calendar,
            endpoint_toggles,
            start_toggle,
            end_toggle,
            start_date_label,
            end_date_label,
            summary_label,
            apply_button,
            escape_controller,
            any_time_count,
            today_count,
            yesterday_count,
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
            DatePillInput::PopoverOpened => {
                self.focus_current_row_when_ready();
                sender.output(DatePillOutput::CountsRequested).ok();
            }
            DatePillInput::CountsReceived(counts) => {
                self.counts = counts;
            }
            DatePillInput::OpenViaShortcut => {
                self.page = Page::Presets;
                self.popover.popup();
                self.focus_current_row_when_ready();
            }
            DatePillInput::PresetSelected(filter) => {
                self.current_filter = filter.clone();
                self.page = Page::Presets;
                self.select_current_row();
                sender.output(DatePillOutput::FilterChanged(filter)).ok();
                self.popover.popdown();
            }
            DatePillInput::CustomRangeRowSelected => {
                match &self.current_filter {
                    DateFilter::Custom { from, to } => {
                        self.draft_from = Some(*from);
                        self.draft_to = Some(*to);
                    }
                    _ => {
                        self.draft_from = None;
                        self.draft_to = None;
                    }
                }
                self.active_endpoint = RangeEndpoint::Start;
                self.page = Page::Custom;
                self.focus_calendar_when_ready();
            }
            DatePillInput::BackToPresets => {
                self.page = Page::Presets;
                if self.popover.is_visible() {
                    self.focus_custom_row_when_ready();
                }
            }
            DatePillInput::CustomDayPicked(day) => {
                (self.draft_from, self.draft_to) =
                    apply_pick(self.draft_from, self.draft_to, self.active_endpoint, day);
            }
            DatePillInput::CustomEndpointChanged(endpoint) => {
                self.active_endpoint = endpoint;
            }
            DatePillInput::CustomClearClicked => {
                self.draft_from = None;
                self.draft_to = None;
            }
            DatePillInput::CustomApplyClicked => {
                if let Some(filter) = valid_custom_filter(self.draft_from, self.draft_to) {
                    self.current_filter = filter.clone();
                    self.page = Page::Presets;
                    self.select_current_row();
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
    fn focus_current_row_when_ready(&self) {
        let listbox = self.listbox.clone();
        let stack = self.stack.clone();
        let row_index = current_row_index(&self.current_filter);

        glib::idle_add_local_once(move || {
            if !shows_page(&stack, "presets") {
                return;
            }

            let Some(row) = listbox.row_at_index(row_index) else {
                return;
            };

            listbox.select_row(Some(&row));
            listbox.grab_focus();
            row.grab_focus();
        });
    }

    fn select_current_row(&self) {
        self.select_row(current_row_index(&self.current_filter));
    }

    fn focus_calendar_when_ready(&self) {
        let calendar = self.calendar.clone();
        let stack = self.stack.clone();
        glib::idle_add_local_once(move || {
            if !shows_page(&stack, "custom") {
                return;
            }
            calendar.grab_focus();
        });
    }

    fn focus_custom_row_when_ready(&self) {
        let listbox = self.listbox.clone();
        let stack = self.stack.clone();
        glib::idle_add_local_once(move || {
            if !shows_page(&stack, "presets") {
                return;
            }
            let Some(row) = listbox.row_at_index(6) else {
                return;
            };
            listbox.select_row(Some(&row));
            row.grab_focus();
        });
    }

    fn select_row(&self, row_index: i32) {
        let Some(row) = self.listbox.row_at_index(row_index) else {
            return;
        };

        self.listbox.select_row(Some(&row));
        self.listbox.grab_focus();
        row.grab_focus();
    }

    fn sync_button(&self, widgets: &DatePillWidgets) {
        let today = Local::now().date_naive();
        let label = filter_label(&self.current_filter, today);
        widgets.label.set_label(&label);
        widgets.label.set_visible(self.current_filter.is_active());
        widgets
            .root
            .set_tooltip_text(Some(&tooltip_for_filter(&self.current_filter, today)));
    }

    fn sync_counts(&self, widgets: &DatePillWidgets) {
        widgets
            .any_time_count
            .set_label(&self.counts.any_time.to_string());
        widgets
            .today_count
            .set_label(&self.counts.today.to_string());
        widgets
            .yesterday_count
            .set_label(&self.counts.yesterday.to_string());
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
        let today = Local::now().date_naive();
        self.stack.set_visible_child_name(match self.page {
            Page::Presets => "presets",
            Page::Custom => "custom",
        });

        let display = match (self.draft_from, self.draft_to) {
            (Some(from), Some(to)) => year_display_for_range(from, to, today),
            _ => YearDisplay::WithoutYear,
        };
        widgets.start_date_label.set_label(
            &self
                .draft_from
                .map(|date| format_date(date, display))
                .unwrap_or_else(|| gettext("Not set")),
        );
        widgets.end_date_label.set_label(
            &self
                .draft_to
                .map(|date| format_date(date, display))
                .unwrap_or_else(|| gettext("Not set")),
        );
        widgets
            .endpoint_toggles
            .set_active(match self.active_endpoint {
                RangeEndpoint::Start => 0,
                RangeEndpoint::End => 1,
            });

        let selected = match self.active_endpoint {
            RangeEndpoint::Start => self.draft_from,
            RangeEndpoint::End => self.draft_to,
        }
        .unwrap_or(today);
        if calendar_to_naive_date(&widgets.calendar.date()) != Some(selected)
            && let Some(date) = naive_to_glib_date(selected)
        {
            widgets.calendar.block_signal(&self.calendar_handler);
            widgets.calendar.set_date(&date);
            widgets.calendar.unblock_signal(&self.calendar_handler);
        }

        widgets
            .summary_label
            .set_label(&custom_info_text(self.draft_from, self.draft_to, today));
        widgets
            .apply_button
            .set_sensitive(valid_custom_filter(self.draft_from, self.draft_to).is_some());
    }
}

fn shows_page(stack: &gtk::Stack, name: &str) -> bool {
    stack.visible_child_name().as_deref() == Some(name)
}

fn naive_to_glib_date(date: NaiveDate) -> Option<glib::DateTime> {
    glib::DateTime::from_utc(
        date.year(),
        i32::try_from(date.month()).ok()?,
        i32::try_from(date.day()).ok()?,
        0,
        0,
        0.0,
    )
    .ok()
}

fn filter_label(filter: &DateFilter, today: NaiveDate) -> String {
    match filter {
        DateFilter::AnyTime => String::new(),
        DateFilter::Today => gettext("Today"),
        DateFilter::Yesterday => gettext("Yesterday"),
        DateFilter::Last7Days => gettext("Last 7 days"),
        DateFilter::Last30Days => gettext("Last 30 days"),
        DateFilter::ThisYear => gettext("This year"),
        DateFilter::Custom { from, to } if from == to => {
            format_date(*from, year_display_for_range(*from, *to, today))
        }
        DateFilter::Custom { from, to } => {
            let display = year_display_for_range(*from, *to, today);
            replace_pair(
                &gettext("{} - {}"),
                &format_date(*from, display),
                &format_date(*to, display),
            )
        }
    }
}

fn year_display_for_range(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> YearDisplay {
    if from.year() == today.year() && to.year() == today.year() {
        YearDisplay::WithoutYear
    } else {
        YearDisplay::WithYear
    }
}

fn format_date(date: NaiveDate, display: YearDisplay) -> String {
    let msgid = match display {
        YearDisplay::WithoutYear => {
            // Translators: strftime format for a date without a year, e.g. "Jun 3".
            gettext("%b %-d")
        }
        YearDisplay::WithYear => {
            // Translators: strftime format for a date with a year, e.g. "Jun 3, 2026".
            gettext("%b %-d, %Y")
        }
    };
    let fallback = match display {
        YearDisplay::WithoutYear => "%b %-d",
        YearDisplay::WithYear => "%b %-d, %Y",
    };

    format_date_with_formats(date, &msgid, fallback)
}

fn format_date_with_formats(date: NaiveDate, translated: &str, fallback: &str) -> String {
    let Ok(date_time) = glib::DateTime::from_utc(
        date.year(),
        i32::try_from(date.month()).expect("chrono month fits i32"),
        i32::try_from(date.day()).expect("chrono day fits i32"),
        0,
        0,
        0.0,
    ) else {
        return date.format("%Y-%m-%d").to_string();
    };

    date_time
        .format(translated)
        .or_else(|_| date_time.format(fallback))
        .map(|formatted| formatted.to_string())
        .unwrap_or_else(|_| date.format("%Y-%m-%d").to_string())
}

fn replace_pair(template: &str, first: &str, second: &str) -> String {
    template.replacen("{}", first, 1).replacen("{}", second, 1)
}

fn current_row_index(filter: &DateFilter) -> i32 {
    match filter {
        DateFilter::AnyTime => 0,
        DateFilter::Today => 1,
        DateFilter::Yesterday => 2,
        DateFilter::Last7Days => 3,
        DateFilter::Last30Days => 4,
        DateFilter::ThisYear => 5,
        DateFilter::Custom { .. } => 6,
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
    row.set_focusable(true);
    row.set_child(Some(&row_box));
    row
}

fn build_custom_row(title: &str) -> gtk::ListBoxRow {
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    let chevron = gtk::Image::from_icon_name("go-next-symbolic");

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row_box.set_margin_top(8);
    row_box.set_margin_bottom(8);
    row_box.set_margin_start(12);
    row_box.set_margin_end(12);
    row_box.append(&title_label);
    row_box.append(&chevron);

    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    row.set_focusable(true);
    row.set_child(Some(&row_box));
    row
}

fn build_endpoint_toggle(caption: &str) -> (adw::Toggle, gtk::Label) {
    let caption_label = gtk::Label::new(Some(caption));
    caption_label.add_css_class("caption");
    caption_label.add_css_class("dim-label");
    let date_label = gtk::Label::new(Some(&gettext("Not set")));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&caption_label);
    content.append(&date_label);

    let toggle = adw::Toggle::builder().child(&content).build();
    (toggle, date_label)
}

fn build_escape_controller(input_sender: relm4::Sender<DatePillInput>) -> gtk::ShortcutController {
    let controller = gtk::ShortcutController::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let trigger = gtk::KeyvalTrigger::new(gtk::gdk::Key::Escape, gtk::gdk::ModifierType::empty());
    let action = gtk::CallbackAction::new(move |_, _| {
        input_sender.send(DatePillInput::BackToPresets).ok();
        glib::Propagation::Stop
    });
    controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
    controller
}

fn tooltip_for_filter(filter: &DateFilter, today: NaiveDate) -> String {
    if filter.is_active() {
        gettext("Date: {}").replace("{}", &filter_label(filter, today))
    } else {
        gettext("Filter by date (Ctrl+Shift+D)")
    }
}

fn apply_pick(
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    endpoint: RangeEndpoint,
    day: NaiveDate,
) -> (Option<NaiveDate>, Option<NaiveDate>) {
    match endpoint {
        RangeEndpoint::Start => {
            let to = Some(to.map_or(day, |to| to.max(day)));
            (Some(day), to)
        }
        RangeEndpoint::End => {
            let from = Some(from.map_or(day, |from| from.min(day)));
            (from, Some(day))
        }
    }
}

fn valid_custom_filter(from: Option<NaiveDate>, to: Option<NaiveDate>) -> Option<DateFilter> {
    match (from, to) {
        (Some(from), Some(to)) if from <= to => Some(DateFilter::Custom { from, to }),
        _ => None,
    }
}

fn custom_info_text(from: Option<NaiveDate>, to: Option<NaiveDate>, today: NaiveDate) -> String {
    match valid_custom_filter(from, to) {
        Some(filter) => filter_label(&filter, today),
        None => gettext("Choose a start and end date to apply a custom range"),
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
    use relm4::{Component, ComponentController};
    use std::time::{Duration, Instant};

    #[test]
    fn custom_label_year_display_is_consistent_across_both_endpoints() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 25).unwrap();
        let current_from = NaiveDate::from_ymd_opt(2026, 6, 3).unwrap();
        let current_to = NaiveDate::from_ymd_opt(2026, 6, 9).unwrap();
        let past_from = NaiveDate::from_ymd_opt(2025, 6, 3).unwrap();
        let past_to = NaiveDate::from_ymd_opt(2025, 6, 9).unwrap();
        let cross_year_to = NaiveDate::from_ymd_opt(2026, 1, 4).unwrap();

        assert_eq!(
            year_display_for_range(current_from, current_to, today),
            YearDisplay::WithoutYear
        );
        assert_eq!(
            year_display_for_range(past_from, past_to, today),
            YearDisplay::WithYear
        );
        assert_eq!(
            year_display_for_range(past_to, cross_year_to, today),
            YearDisplay::WithYear
        );
        assert_eq!(
            year_display_for_range(current_from, current_from, today),
            YearDisplay::WithoutYear
        );
        assert_eq!(
            year_display_for_range(past_from, past_from, today),
            YearDisplay::WithYear
        );
    }

    // Date formatting goes through glib and therefore depends on the process
    // locale, which `gtk::init()` changes: run on the GTK test thread so the
    // locale is already settled.
    #[gtk::test]
    fn custom_filter_label_uses_one_date_for_same_day_and_both_years_when_needed() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 25).unwrap();
        let same_day = NaiveDate::from_ymd_opt(2026, 6, 3).unwrap();
        let from = NaiveDate::from_ymd_opt(2025, 12, 28).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 1, 4).unwrap();

        assert_eq!(
            filter_label(
                &DateFilter::Custom {
                    from: same_day,
                    to: same_day,
                },
                today,
            ),
            format_date(same_day, YearDisplay::WithoutYear)
        );
        assert_eq!(
            filter_label(&DateFilter::Custom { from, to }, today),
            replace_pair(
                &gettext("{} - {}"),
                &format_date(from, YearDisplay::WithYear),
                &format_date(to, YearDisplay::WithYear),
            )
        );
    }

    // Date formatting goes through glib and therefore depends on the process
    // locale, which `gtk::init()` changes: run on the GTK test thread so the
    // locale is already settled.
    #[gtk::test]
    fn glib_date_formatting_falls_back_to_msgid_then_iso() {
        let date = NaiveDate::from_ymd_opt(2026, 4, 5).unwrap();

        assert_eq!(
            format_date_with_formats(date, "%Q", "%Y/%m/%d"),
            "2026/04/05"
        );
        assert_eq!(format_date_with_formats(date, "%Q", "%Q"), "2026-04-05");
    }

    #[test]
    fn preset_filter_labels_are_localized_in_the_ui_layer() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 25).unwrap();

        assert_eq!(filter_label(&DateFilter::AnyTime, today), "");
        assert_eq!(filter_label(&DateFilter::Today, today), "Today");
        assert_eq!(filter_label(&DateFilter::Yesterday, today), "Yesterday");
        assert_eq!(filter_label(&DateFilter::Last7Days, today), "Last 7 days");
        assert_eq!(filter_label(&DateFilter::Last30Days, today), "Last 30 days");
        assert_eq!(filter_label(&DateFilter::ThisYear, today), "This year");
    }

    fn pump_main_context(condition: impl Fn() -> bool) {
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(2);

        while !condition() {
            assert!(
                Instant::now() < deadline,
                "condition not met before timeout"
            );

            while context.pending() {
                context.iteration(false);
            }

            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn present_in_shared_window(widget: &impl IsA<gtk::Widget>) {
        // Every `#[gtk::test]` body runs on the same harness thread, so a single
        // long-lived toplevel keeps popover visibility and focus stable instead
        // of churning through one window per test.
        thread_local! {
            static WINDOW: gtk::Window = {
                let window = gtk::Window::new();
                window.present();
                window
            };
        }

        WINDOW.with(|window| window.set_child(Some(widget)));
    }

    fn find_list_box(widget: &gtk::Widget) -> Option<gtk::ListBox> {
        if let Ok(list_box) = widget.clone().downcast::<gtk::ListBox>() {
            return Some(list_box);
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            if let Some(found) = find_list_box(&child_widget) {
                return Some(found);
            }
            child = child_widget.next_sibling();
        }

        None
    }

    fn find_stack(widget: &gtk::Widget) -> Option<gtk::Stack> {
        if let Ok(stack) = widget.clone().downcast::<gtk::Stack>() {
            return Some(stack);
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            if let Some(found) = find_stack(&child_widget) {
                return Some(found);
            }
            child = child_widget.next_sibling();
        }

        None
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn apply_pick_mirrors_and_monotonically_clamps_endpoints() {
        let early = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let middle = NaiveDate::from_ymd_opt(2026, 5, 7).unwrap();
        let late = NaiveDate::from_ymd_opt(2026, 5, 14).unwrap();
        let cases = [
            (
                None,
                None,
                RangeEndpoint::Start,
                middle,
                (Some(middle), Some(middle)),
            ),
            (
                None,
                None,
                RangeEndpoint::End,
                middle,
                (Some(middle), Some(middle)),
            ),
            (
                Some(early),
                Some(middle),
                RangeEndpoint::Start,
                late,
                (Some(late), Some(late)),
            ),
            (
                Some(middle),
                Some(late),
                RangeEndpoint::End,
                early,
                (Some(early), Some(early)),
            ),
            (
                Some(early),
                Some(late),
                RangeEndpoint::Start,
                middle,
                (Some(middle), Some(late)),
            ),
            (
                Some(early),
                Some(late),
                RangeEndpoint::End,
                middle,
                (Some(early), Some(middle)),
            ),
            (
                Some(middle),
                Some(middle),
                RangeEndpoint::Start,
                middle,
                (Some(middle), Some(middle)),
            ),
        ];

        for (from, to, endpoint, picked, expected) in cases {
            assert_eq!(apply_pick(from, to, endpoint, picked), expected);
        }
    }

    #[test]
    fn valid_custom_filter_still_requires_both_dates() {
        let from = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 5, 7).unwrap();

        assert_eq!(valid_custom_filter(None, Some(to)), None);
        assert_eq!(valid_custom_filter(Some(from), None), None);
        assert_eq!(
            valid_custom_filter(Some(from), Some(to)),
            Some(DateFilter::Custom { from, to })
        );
    }

    // Date formatting goes through glib and therefore depends on the process
    // locale, which `gtk::init()` changes: run on the GTK test thread so the
    // locale is already settled.
    #[gtk::test]
    fn tooltip_reflects_active_filter() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 25).unwrap();
        let custom = DateFilter::Custom {
            from: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 5, 7).unwrap(),
        };

        assert_eq!(
            tooltip_for_filter(&DateFilter::AnyTime, today),
            "Filter by date (Ctrl+Shift+D)"
        );
        assert_eq!(
            tooltip_for_filter(&custom, today),
            format!("Date: {}", filter_label(&custom, today))
        );
    }

    #[gtk::test]
    fn preset_list_is_selectable_for_shortcut_navigation() {
        let controller = DatePill::builder().launch(());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("date pill preset list");

        assert_eq!(list_box.selection_mode(), gtk::SelectionMode::Browse);
    }

    #[gtk::test]
    fn open_via_shortcut_selects_current_preset_row() {
        let controller = DatePill::builder().launch(());

        present_in_shared_window(controller.widget());

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("date pill preset list");

        controller.emit(DatePillInput::PresetSelected(DateFilter::Last30Days));
        controller.emit(DatePillInput::OpenViaShortcut);

        pump_main_context(|| list_box.selected_row().map(|row| row.index()) == Some(4));

        let selected_row = list_box.selected_row().expect("selected preset row");
        assert_eq!(selected_row.index(), 4);
    }

    #[gtk::test]
    fn custom_range_activation_switches_to_the_custom_stack_page() {
        let controller = DatePill::builder().launch(());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let stack = find_stack(&root).expect("date pill stack");

        assert_eq!(stack.pages().n_items(), 2);
        assert!(stack.child_by_name("presets").is_some());
        assert!(stack.child_by_name("custom").is_some());
        assert_eq!(
            stack.transition_type(),
            gtk::StackTransitionType::SlideLeftRight
        );
        assert!(stack.is_hhomogeneous());
        assert!(!stack.is_vhomogeneous());
        assert!(stack.interpolates_size());
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| stack.visible_child_name().as_deref() == Some("custom"));
    }

    #[gtk::test]
    fn entering_custom_page_reseeds_from_the_applied_filter() {
        let controller = DatePill::builder().launch(());
        let today = Local::now().date_naive();
        let from = date(2025, 12, 28);
        let to = date(2026, 1, 4);

        controller.emit(DatePillInput::PresetSelected(DateFilter::Custom {
            from,
            to,
        }));
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| {
            calendar_to_naive_date(&controller.widgets().calendar.date()) == Some(from)
        });

        assert!(
            controller
                .widgets()
                .start_date_label
                .label()
                .contains("2025")
        );
        assert!(controller.widgets().end_date_label.label().contains("2026"));
        assert_eq!(controller.widgets().endpoint_toggles.active(), 0);

        let picked = date(2026, 2, 14);
        let picked_label = format_date(picked, year_display_for_range(picked, picked, today));
        controller.emit(DatePillInput::CustomDayPicked(picked));
        pump_main_context(|| controller.widgets().start_date_label.label() == picked_label);
        controller.emit(DatePillInput::PresetSelected(DateFilter::Last30Days));
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| controller.widgets().start_date_label.label() == gettext("Not set"));
        assert_eq!(
            controller.widgets().end_date_label.label(),
            gettext("Not set")
        );
        assert_eq!(controller.widgets().endpoint_toggles.active(), 0);
        assert_eq!(
            calendar_to_naive_date(&controller.widgets().calendar.date()),
            Some(today)
        );
    }

    #[gtk::test]
    fn one_day_pick_enables_apply_and_clear_disables_it_again() {
        let controller = DatePill::builder().launch(());
        controller.emit(DatePillInput::CustomRangeRowSelected);
        assert!(!controller.widgets().apply_button.is_sensitive());

        controller.emit(DatePillInput::CustomDayPicked(date(2026, 7, 25)));
        pump_main_context(|| controller.widgets().apply_button.is_sensitive());

        controller.emit(DatePillInput::CustomClearClicked);
        pump_main_context(|| !controller.widgets().apply_button.is_sensitive());
    }

    #[gtk::test]
    fn open_via_shortcut_always_returns_to_the_presets_page() {
        let controller = DatePill::builder().launch(());
        present_in_shared_window(controller.widget());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let stack = find_stack(&root).expect("date pill stack");
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| stack.visible_child_name().as_deref() == Some("custom"));
        controller.emit(DatePillInput::OpenViaShortcut);
        pump_main_context(|| stack.visible_child_name().as_deref() == Some("presets"));
    }

    // This checks the Escape shortcut's configuration, not popover visibility: a
    // MenuButton popover never reports `is_visible() == true` under `#[gtk::test]`
    // once the main loop is drained, so asserting it here would only measure the
    // harness. Confirming that capture ordering really beats popover autohide
    // needs real Escape key presses, which Task 5's manual verification delivers.
    #[gtk::test]
    fn escape_action_returns_to_presets_without_closing_the_popover() {
        let controller = DatePill::builder().launch(());
        present_in_shared_window(controller.widget());

        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| {
            controller.widgets().stack.visible_child_name().as_deref() == Some("custom")
        });

        assert_eq!(
            controller.widgets().escape_controller.propagation_phase(),
            gtk::PropagationPhase::Capture
        );
        let shortcut = controller
            .widgets()
            .escape_controller
            .item(0)
            .and_then(|item| item.downcast::<gtk::Shortcut>().ok())
            .expect("Escape shortcut");
        let trigger = shortcut
            .trigger()
            .and_then(|trigger| trigger.downcast::<gtk::KeyvalTrigger>().ok())
            .expect("Escape key trigger");
        assert_eq!(trigger.keyval(), gtk::gdk::Key::Escape);
        assert_eq!(trigger.modifiers(), gtk::gdk::ModifierType::empty());
        assert!(shortcut.action().expect("Escape action").activate(
            gtk::ShortcutActionFlags::EXCLUSIVE,
            &controller.widgets().stack,
            None,
        ));

        pump_main_context(|| {
            controller.widgets().stack.visible_child_name().as_deref() == Some("presets")
        });
    }

    #[gtk::test]
    fn stack_page_changes_move_focus_to_a_visible_control() {
        let controller = DatePill::builder().launch(());
        present_in_shared_window(controller.widget());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let listbox = find_list_box(&root).expect("date pill preset list");

        controller.emit(DatePillInput::OpenViaShortcut);
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| controller.widgets().calendar.has_focus());

        controller.emit(DatePillInput::BackToPresets);
        pump_main_context(|| {
            listbox
                .row_at_index(6)
                .is_some_and(|custom_row| custom_row.has_focus())
        });
    }
}
