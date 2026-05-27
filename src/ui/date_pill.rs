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
    listbox: gtk::ListBox,
    popover: gtk::Popover,
}

#[derive(Debug, Clone)]
pub enum DatePillInput {
    PopoverOpened,
    CountsReceived(DateCounts),
    OpenViaShortcut,
    PresetSelected(DateFilter),
    CustomRangeRowSelected,
    CustomFromPicked(NaiveDate),
    CustomToPicked(NaiveDate),
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
        root.set_tooltip_text(Some(&tooltip_for_filter(&DateFilter::AnyTime)));
        root.add_css_class("flat");

        let popover = gtk::Popover::new();
        root.set_popover(Some(&popover));

        let listbox = gtk::ListBox::new();
        listbox.add_css_class("boxed-list");
        listbox.set_selection_mode(gtk::SelectionMode::Browse);

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
                    .send(DatePillInput::PresetSelected(DateFilter::Last7Days))
                    .ok(),
                3 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::Last30Days))
                    .ok(),
                4 => input_sender
                    .send(DatePillInput::PresetSelected(DateFilter::ThisYear))
                    .ok(),
                5 => input_sender
                    .send(DatePillInput::CustomRangeRowSelected)
                    .ok(),
                _ => None,
            };
        });

        let input_sender = sender.input_sender().clone();
        from_calendar.connect_day_selected(move |calendar| {
            if let Some(date) = calendar_to_naive_date(&calendar.date()) {
                input_sender
                    .send(DatePillInput::CustomFromPicked(date))
                    .ok();
            }
        });

        let input_sender = sender.input_sender().clone();
        to_calendar.connect_day_selected(move |calendar| {
            if let Some(date) = calendar_to_naive_date(&calendar.date()) {
                input_sender.send(DatePillInput::CustomToPicked(date)).ok();
            }
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
            custom_revealed: false,
            listbox: listbox.clone(),
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
            DatePillInput::PopoverOpened => {
                self.select_current_row();
                sender.output(DatePillOutput::CountsRequested).ok();
            }
            DatePillInput::CountsReceived(counts) => {
                self.counts = counts;
            }
            DatePillInput::OpenViaShortcut => {
                self.popover.popup();
                self.focus_current_row_when_ready();
            }
            DatePillInput::PresetSelected(filter) => {
                self.current_filter = filter.clone();
                self.custom_revealed = false;
                self.select_current_row();
                sender.output(DatePillOutput::FilterChanged(filter)).ok();
                self.popover.popdown();
            }
            DatePillInput::CustomRangeRowSelected => {
                self.custom_revealed = true;
                self.select_current_row();
            }
            DatePillInput::CustomFromPicked(date) => {
                self.custom_revealed = true;
                self.draft_from = Some(date);
            }
            DatePillInput::CustomToPicked(date) => {
                self.custom_revealed = true;
                self.draft_to = Some(date);
            }
            DatePillInput::CustomClearClicked => {
                self.draft_from = None;
                self.draft_to = None;
            }
            DatePillInput::CustomApplyClicked => {
                if let Some(filter) = valid_custom_filter(self.draft_from, self.draft_to) {
                    self.current_filter = filter.clone();
                    self.custom_revealed = false;
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
        let row_index = current_row_index(&self.current_filter);

        glib::idle_add_local_once(move || {
            let Some(row) = listbox.row_at_index(row_index) else {
                return;
            };

            listbox.select_row(Some(&row));
            listbox.grab_focus();
            row.grab_focus();
        });
    }

    fn select_current_row(&self) {
        let Some(row) = self
            .listbox
            .row_at_index(current_row_index(&self.current_filter))
        else {
            return;
        };

        self.listbox.select_row(Some(&row));
        self.listbox.grab_focus();
        row.grab_focus();
    }

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

fn current_row_index(filter: &DateFilter) -> i32 {
    match filter {
        DateFilter::AnyTime => 0,
        DateFilter::Today => 1,
        DateFilter::Last7Days => 2,
        DateFilter::Last30Days => 3,
        DateFilter::ThisYear => 4,
        DateFilter::Custom { .. } => 5,
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
    use relm4::{Component, ComponentController};
    use std::time::{Duration, Instant};

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

            context.iteration(true);
        }
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

        let window = gtk::Window::new();
        window.set_child(Some(controller.widget()));
        window.present();

        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let list_box = find_list_box(&root).expect("date pill preset list");

        controller.emit(DatePillInput::PresetSelected(DateFilter::Last30Days));
        controller.emit(DatePillInput::OpenViaShortcut);

        pump_main_context(|| list_box.selected_row().map(|row| row.index()) == Some(3));

        let selected_row = list_box.selected_row().expect("selected preset row");
        assert_eq!(selected_row.index(), 3);
    }
}
