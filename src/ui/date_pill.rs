use chrono::{Datelike, Local, NaiveDate};
use gettextrs::gettext;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw, gtk};

use gtk::glib;
use gtk::prelude::*;

#[cfg(test)]
use std::{cell::RefCell, rc::Rc};

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

/// Index of the *Custom range...* row in the preset list.
const CUSTOM_ROW_INDEX: i32 = 6;

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
    #[cfg(test)]
    announcement_log: Rc<RefCell<Vec<(String, gtk::AccessibleAnnouncementPriority)>>>,
    #[cfg(test)]
    accessible_label_log: Rc<RefCell<(String, String)>>,
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
    calendar_clicks: gtk::GestureClick,
    calendar_keys: gtk::EventControllerKey,
    endpoint_toggles: adw::ToggleGroup,
    start_toggle: adw::Toggle,
    end_toggle: adw::Toggle,
    start_date_label: gtk::Label,
    end_date_label: gtk::Label,
    summary_label: gtk::Label,
    apply_button: gtk::Button,
    escape_controller: gtk::ShortcutController,
    back_button: gtk::Button,
    any_time_count: gtk::Label,
    today_count: gtk::Label,
    yesterday_count: gtk::Label,
    last_7_days_count: gtk::Label,
    last_30_days_count: gtk::Label,
    this_year_count: gtk::Label,
    #[cfg(test)]
    announcement_log: Rc<RefCell<Vec<(String, gtk::AccessibleAnnouncementPriority)>>>,
    #[cfg(test)]
    accessible_label_log: Rc<RefCell<(String, String)>>,
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

        #[cfg(test)]
        let announcement_log = Rc::new(RefCell::new(Vec::new()));
        #[cfg(test)]
        let accessible_label_log = Rc::new(RefCell::new((String::new(), String::new())));

        let back_button = gtk::Button::from_icon_name("go-previous-symbolic");
        back_button.add_css_class("flat");
        let back_accessible_label = gettext("Back to date presets");
        back_button.update_property(&[gtk::accessible::Property::Label(&back_accessible_label)]);
        #[cfg(test)]
        {
            accessible_label_log.borrow_mut().0 = back_accessible_label;
        }
        let heading = gtk::Label::new(Some(&gettext("Custom range")));
        heading.add_css_class("heading");
        heading.set_hexpand(true);
        heading.set_xalign(0.0);

        let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        title_row.append(&back_button);
        title_row.append(&heading);

        let (start_toggle, start_date_label) =
            build_endpoint_toggle(&endpoint_title(RangeEndpoint::Start));
        let (end_toggle, end_date_label) =
            build_endpoint_toggle(&endpoint_title(RangeEndpoint::End));
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

        // `GtkCalendar` only emits `day-selected` when the selected day actually
        // changes, so clicking the day it already shows — today, on a freshly
        // seeded custom page — is a silent no-op and Apply would never enable.
        // The calendar's own click gesture never claims the event sequence, so
        // this bubble-phase gesture sees the same release and re-picks the day
        // unconditionally. `apply_pick` is idempotent for a re-pick of the
        // current endpoint value, so the extra pick cannot corrupt the draft;
        // `CustomDayPicked` only has to keep it from announcing twice.
        // Gestures only run on real input, so the guarded `set_date()` in
        // `sync_custom_state` still cannot look like a pick.
        let input_sender = sender.input_sender().clone();
        let calendar_clicks = gtk::GestureClick::new();
        calendar_clicks.connect_released(move |gesture, _, x, y| {
            let Some(calendar) = gesture.widget().and_downcast::<gtk::Calendar>() else {
                return;
            };
            if !release_landed_on_a_day(&calendar, x, y) {
                return;
            }
            if let Some(date) = calendar_to_naive_date(&calendar.date()) {
                input_sender.send(DatePillInput::CustomDayPicked(date)).ok();
            }
        });
        calendar.add_controller(calendar_clicks.clone());

        // The keyboard equivalent. `GtkCalendar` ignores Space until a click or
        // an arrow key has placed its internal focus cell, so a keyboard user
        // who has just tabbed in gets nothing at all. It stops the key whenever
        // it does act on Space, so this bubble-phase controller only runs when
        // the calendar left the key unhandled, and then picks the day the
        // calendar visibly highlights.
        let input_sender = sender.input_sender().clone();
        let calendar_keys = gtk::EventControllerKey::new();
        calendar_keys.connect_key_pressed(move |controller, keyval, _, _| {
            if keyval != gtk::gdk::Key::space && keyval != gtk::gdk::Key::KP_Space {
                return glib::Propagation::Proceed;
            }

            let Some(calendar) = controller.widget().and_downcast::<gtk::Calendar>() else {
                return glib::Propagation::Proceed;
            };
            let Some(date) = calendar_to_naive_date(&calendar.date()) else {
                return glib::Propagation::Proceed;
            };

            input_sender.send(DatePillInput::CustomDayPicked(date)).ok();
            glib::Propagation::Stop
        });
        calendar.add_controller(calendar_keys.clone());

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
            #[cfg(test)]
            announcement_log: announcement_log.clone(),
            #[cfg(test)]
            accessible_label_log: accessible_label_log.clone(),
        };

        let widgets = DatePillWidgets {
            root,
            popover,
            label,
            stack,
            calendar,
            calendar_clicks,
            calendar_keys,
            endpoint_toggles,
            start_toggle,
            end_toggle,
            start_date_label,
            end_date_label,
            summary_label,
            apply_button,
            escape_controller,
            back_button,
            any_time_count,
            today_count,
            yesterday_count,
            last_7_days_count,
            last_30_days_count,
            this_year_count,
            #[cfg(test)]
            announcement_log,
            #[cfg(test)]
            accessible_label_log,
        };

        model.sync_button(&widgets);
        model.sync_counts(&widgets);
        model.sync_custom_state(&widgets);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            DatePillInput::PopoverOpened => {
                self.focus_row_when_ready(current_row_index(&self.current_filter));
                sender.output(DatePillOutput::CountsRequested).ok();
            }
            DatePillInput::CountsReceived(counts) => {
                self.counts = counts;
            }
            DatePillInput::OpenViaShortcut => {
                self.page = Page::Presets;
                self.popover.popup();
                self.focus_row_when_ready(current_row_index(&self.current_filter));
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
                // Only a real return trip from the custom page moves focus. The
                // popover's `closed` handler also lands here, but by then an
                // applied preset has already put the page back to `Presets`, so
                // its row selection survives. Keying off the page instead of
                // popover visibility keeps this deterministic: whether the
                // popover reports itself mapped at this instant is not reliable.
                let returning_from_custom = self.page == Page::Custom;
                self.page = Page::Presets;
                if returning_from_custom {
                    self.focus_row_when_ready(CUSTOM_ROW_INDEX);
                }
            }
            DatePillInput::CustomDayPicked(day) => {
                let previous = (self.draft_from, self.draft_to);
                (self.draft_from, self.draft_to) =
                    apply_pick(self.draft_from, self.draft_to, self.active_endpoint, day);
                // One click reaches this twice whenever the day changes: once
                // from `day-selected`, once from the gesture that covers the
                // day the calendar already shows. The second pass is a no-op
                // thanks to `apply_pick` being idempotent, and skipping the
                // announcement when nothing moved keeps the screen reader from
                // repeating the same summary.
                if (self.draft_from, self.draft_to) == previous {
                    return;
                }
                let summary =
                    custom_info_text(self.draft_from, self.draft_to, Local::now().date_naive());
                self.summary_label
                    .announce(&summary, gtk::AccessibleAnnouncementPriority::Medium);
                #[cfg(test)]
                self.announcement_log
                    .borrow_mut()
                    .push((summary, gtk::AccessibleAnnouncementPriority::Medium));
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
    fn focus_row_when_ready(&self, focus_index: i32) {
        let listbox = self.listbox.clone();
        let stack = self.stack.clone();
        let selected_index = current_row_index(&self.current_filter);

        glib::idle_add_local_once(move || {
            if !shows_page(&stack, "presets") {
                return;
            }

            focus_row(&listbox, focus_index, selected_index);
        });
    }

    fn select_current_row(&self) {
        let row_index = current_row_index(&self.current_filter);
        focus_row(&self.listbox, row_index, row_index);
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
        widgets.stack.set_visible_child_name(match self.page {
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
            .start_toggle
            .set_label(Some(&endpoint_accessible_label(
                RangeEndpoint::Start,
                self.draft_from,
                display,
            )));
        widgets
            .end_toggle
            .set_label(Some(&endpoint_accessible_label(
                RangeEndpoint::End,
                self.draft_to,
                display,
            )));
        let calendar_accessible_label = endpoint_title(self.active_endpoint);
        widgets
            .calendar
            .update_property(&[gtk::accessible::Property::Label(&calendar_accessible_label)]);
        #[cfg(test)]
        {
            self.accessible_label_log.borrow_mut().1 = calendar_accessible_label;
        }
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

/// Focuses `focus_index` and leaves `selected_index` selected.
///
/// `SelectionMode::Browse` couples focus and selection, so focusing a row also
/// selects it. When the two indices differ — returning from the custom page
/// focuses *Custom range...* while another preset is still filtering — the
/// selection is restored afterwards so the highlight never lies about the
/// active filter.
fn focus_row(listbox: &gtk::ListBox, focus_index: i32, selected_index: i32) {
    let Some(row) = listbox.row_at_index(focus_index) else {
        return;
    };

    listbox.select_row(Some(&row));
    listbox.grab_focus();
    row.grab_focus();

    if selected_index != focus_index
        && let Some(selected_row) = listbox.row_at_index(selected_index)
    {
        listbox.select_row(Some(&selected_row));
    }
}

/// Whether a pointer release at `(x, y)`, in the calendar's own coordinate
/// space, landed on one of its day cells rather than on the heading, the week
/// numbers, or the day-name row.
///
/// `GtkCalendar` draws its days as labels carrying the `.day-number` CSS class
/// and makes the same distinction on button press, by picking the widget under
/// the pointer and reacting only for those labels. This compares layout bounds
/// instead of calling `pick()`, which additionally requires the widget to be
/// mapped. Should GTK ever rename that class, the gesture would simply stop
/// re-picking and fall back to `day-selected`; it can never pick a wrong day.
fn release_landed_on_a_day(calendar: &gtk::Calendar, x: f64, y: f64) -> bool {
    let point = gtk::graphene::Point::new(x as f32, y as f32);
    let mut cells = Vec::new();
    collect_day_cell_bounds(calendar.upcast_ref(), calendar, &mut cells);

    cells.iter().any(|bounds| bounds.contains_point(&point))
}

fn collect_day_cell_bounds(
    widget: &gtk::Widget,
    calendar: &gtk::Calendar,
    bounds: &mut Vec<gtk::graphene::Rect>,
) {
    if widget.has_css_class("day-number")
        && let Some(cell) = widget.compute_bounds(calendar)
    {
        bounds.push(cell);
    }

    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_day_cell_bounds(&current, calendar, bounds);
        child = current.next_sibling();
    }
}

fn endpoint_title(endpoint: RangeEndpoint) -> String {
    match endpoint {
        RangeEndpoint::Start => gettext("Start date"),
        RangeEndpoint::End => gettext("End date"),
    }
}

fn endpoint_accessible_label(
    endpoint: RangeEndpoint,
    date: Option<NaiveDate>,
    display: YearDisplay,
) -> String {
    let value = date
        .map(|date| format_date(date, display))
        .unwrap_or_else(|| gettext("Not set"));
    // Translators: accessible name for a date range endpoint, e.g. "Start date, Jun 3".
    replace_pair(&gettext("{}, {}"), &endpoint_title(endpoint), &value)
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
        return iso_fallback(date);
    };

    date_time
        .format(translated)
        .or_else(|_| date_time.format(fallback))
        .map(|formatted| formatted.to_string())
        .unwrap_or_else(|_| iso_fallback(date))
}

/// Last-resort date rendering, deliberately going through chrono rather than
/// glib: this is only reached once glib has already refused the date or the
/// format string, so glib could not render it either. ISO 8601 is unambiguous
/// in every locale, so it needs no translation.
fn iso_fallback(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
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
        DateFilter::Custom { .. } => CUSTOM_ROW_INDEX,
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

/// Writes `day` into `endpoint`, mirroring the missing endpoint and clamping
/// the other one so `from > to` is unreachable after any user pick.
///
/// The draft pair is never mixed (one endpoint `Some`, the other `None`): the
/// only writers are this function, which always returns both `Some`,
/// `CustomClearClicked`, which clears both, and page seeding, which sets both
/// or neither. That is why the test table has no mixed-draft row.
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
    use glib::translate::IntoGlib;
    use relm4::{Component, ComponentController, component::Connector};
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
        let calendar = laid_out_custom_page(&controller);
        // The custom page is on screen with no draft endpoints: Apply being
        // insensitive here is an observed state, not the initial widget value.
        assert!(!controller.widgets().apply_button.is_sensitive());

        // Selecting another day in the calendar is what a real click does
        // whenever the day actually changes.
        let picked = Local::now().date_naive() - chrono::TimeDelta::days(3);
        calendar.set_date(&naive_to_glib_date(picked).expect("glib date"));
        pump_main_context(|| controller.widgets().apply_button.is_sensitive());

        controller.emit(DatePillInput::CustomClearClicked);
        pump_main_context(|| !controller.widgets().apply_button.is_sensitive());
    }

    #[gtk::test]
    fn clicking_the_day_the_calendar_already_shows_is_a_pick() {
        let controller = DatePill::builder().launch(());
        let calendar = laid_out_custom_page(&controller);
        let today = Local::now().date_naive();
        assert_eq!(calendar_to_naive_date(&calendar.date()), Some(today));
        assert!(!controller.widgets().apply_button.is_sensitive());

        release_on_calendar(&controller, today_cell_center(&calendar));

        pump_main_context(|| controller.widgets().apply_button.is_sensitive());
        let expected = format_date(today, YearDisplay::WithoutYear);
        assert_eq!(controller.widgets().start_date_label.label(), expected);
        assert_eq!(controller.widgets().end_date_label.label(), expected);
        assert_eq!(
            controller.widgets().summary_label.label(),
            custom_info_text(Some(today), Some(today), today)
        );
    }

    #[gtk::test]
    fn a_release_outside_the_day_grid_is_not_a_pick() {
        let controller = DatePill::builder().launch(());
        let calendar = laid_out_custom_page(&controller);
        let today = Local::now().date_naive();

        // Top-left corner of the calendar: its heading row, never a day cell.
        release_on_calendar(&controller, (2.0, 2.0));
        drain_main_context();

        assert!(!controller.widgets().apply_button.is_sensitive());
        assert_eq!(
            controller.widgets().start_date_label.label(),
            gettext("Not set")
        );
        assert_eq!(
            controller.widgets().summary_label.label(),
            custom_info_text(None, None, today)
        );
        assert_eq!(calendar_to_naive_date(&calendar.date()), Some(today));
    }

    #[gtk::test]
    fn returning_from_the_custom_page_keeps_the_active_preset_selected() {
        let controller = DatePill::builder().launch(());
        let root = controller.widget().clone().upcast::<gtk::Widget>();
        let listbox = find_list_box(&root).expect("date pill preset list");
        let stack = controller.widgets().stack.clone();

        controller.emit(DatePillInput::PresetSelected(DateFilter::Last30Days));
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| shows_page(&stack, "custom"));

        // Returning focuses "Custom range..." so the trip back is
        // discoverable, and `SelectionMode::Browse` selects whatever gets
        // focus. Start from that selection so the wait below observes the
        // restore rather than a highlight that never moved. Focus itself is
        // not asserted here: grabbing it needs a mapped toplevel, which the
        // shared harness thread cannot guarantee.
        let custom_row = listbox
            .row_at_index(CUSTOM_ROW_INDEX)
            .expect("custom range row");
        listbox.select_row(Some(&custom_row));

        controller.emit(DatePillInput::BackToPresets);
        // The highlighted row must go back to the filter that is applied.
        pump_main_context(|| listbox.selected_row().map(|row| row.index()) == Some(4));
    }

    #[gtk::test]
    fn a_repeated_pick_of_the_same_day_announces_only_once() {
        let controller = DatePill::builder().launch(());
        let calendar = laid_out_custom_page(&controller);
        let center = today_cell_center(&calendar);

        // A real click on a day that does change the selection reaches
        // `CustomDayPicked` twice — from `day-selected` and from the gesture.
        // Replaying the same pick twice has the same shape: only the first one
        // moves the draft, so only the first one is worth announcing.
        release_on_calendar(&controller, center);
        pump_main_context(|| controller.widgets().announcement_log.borrow().len() == 1);

        release_on_calendar(&controller, center);
        drain_main_context();

        assert_eq!(controller.widgets().announcement_log.borrow().len(), 1);
        assert!(controller.widgets().apply_button.is_sensitive());
    }

    #[gtk::test]
    fn space_on_a_freshly_focused_calendar_is_a_pick() {
        let controller = DatePill::builder().launch(());
        let calendar = laid_out_custom_page(&controller);
        let today = Local::now().date_naive();
        assert!(!controller.widgets().apply_button.is_sensitive());

        // `GtkCalendar` has no focus cell yet, so it leaves Space unhandled and
        // the picker's own key controller gets it.
        assert!(press_space_on_calendar(&controller));

        pump_main_context(|| controller.widgets().apply_button.is_sensitive());
        let expected = format_date(today, YearDisplay::WithoutYear);
        assert_eq!(controller.widgets().start_date_label.label(), expected);
        assert_eq!(controller.widgets().end_date_label.label(), expected);
        assert_eq!(calendar_to_naive_date(&calendar.date()), Some(today));
    }

    #[gtk::test]
    fn other_keys_are_left_to_the_calendar() {
        let controller = DatePill::builder().launch(());
        laid_out_custom_page(&controller);

        assert!(!press_key_on_calendar(&controller, gtk::gdk::Key::Right));
        drain_main_context();

        assert!(!controller.widgets().apply_button.is_sensitive());
    }

    /// Emits `key-pressed` on the picker's own calendar key controller,
    /// exercising the real handler. `GtkCalendar`'s controller runs first in
    /// the real chain and consumes Space whenever it acts on it, so replaying
    /// only this one matches the case under test: a calendar that has no focus
    /// cell yet and therefore ignored the key.
    fn press_space_on_calendar(controller: &Connector<DatePill>) -> bool {
        press_key_on_calendar(controller, gtk::gdk::Key::space)
    }

    fn press_key_on_calendar(controller: &Connector<DatePill>, key: gtk::gdk::Key) -> bool {
        let keys = controller.widgets().calendar_keys.clone();
        keys.emit_by_name::<bool>(
            "key-pressed",
            &[&key.into_glib(), &0u32, &gtk::gdk::ModifierType::empty()],
        )
    }

    /// Switches to the custom page and gives the calendar a real allocation,
    /// so its day cells have layout bounds.
    ///
    /// The calendar is laid out directly rather than by showing the popover: a
    /// popover only lays out once its toplevel is mapped, which depends on the
    /// display backend and on whatever other tests left on the shared harness
    /// thread. Allocating here is deterministic, and layout is all the day-cell
    /// geometry needs.
    fn laid_out_custom_page(controller: &Connector<DatePill>) -> gtk::Calendar {
        let stack = controller.widgets().stack.clone();
        let calendar = controller.widgets().calendar.clone();

        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| shows_page(&stack, "custom"));

        let (_, width, _, _) = calendar.measure(gtk::Orientation::Horizontal, -1);
        let (_, height, _, _) = calendar.measure(gtk::Orientation::Vertical, width);
        calendar.allocate(width, height, -1, None);
        calendar
    }

    /// Centre of the day cell the calendar marks as today, in the calendar's
    /// own coordinate space.
    fn today_cell_center(calendar: &gtk::Calendar) -> (f64, f64) {
        let bounds = find_today_cell(calendar.clone().upcast_ref(), calendar)
            .expect("the calendar marks today's cell");

        (
            f64::from(bounds.x() + bounds.width() / 2.0),
            f64::from(bounds.y() + bounds.height() / 2.0),
        )
    }

    fn find_today_cell(
        widget: &gtk::Widget,
        calendar: &gtk::Calendar,
    ) -> Option<gtk::graphene::Rect> {
        if widget.has_css_class("day-number") && widget.has_css_class("today") {
            return widget.compute_bounds(calendar);
        }

        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find_today_cell(&current, calendar) {
                return Some(found);
            }
            child = current.next_sibling();
        }

        None
    }

    /// Emits `released` on the picker's own calendar gesture, exercising the
    /// real handler and its day-cell filter with real coordinates.
    ///
    /// `GtkCalendar`'s own press handling is deliberately not replayed: for the
    /// day the calendar already shows it is a no-op — `calendar_select_day_internal`
    /// returns before emitting `day-selected` when nothing changed — which is
    /// exactly the case these tests cover.
    fn release_on_calendar(controller: &Connector<DatePill>, (x, y): (f64, f64)) {
        let gesture = controller.widgets().calendar_clicks.clone();
        gesture.emit_by_name::<()>("released", &[&1i32, &x, &y]);
    }

    fn drain_main_context() {
        let context = glib::MainContext::default();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
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
    // MenuButton popover does not reliably report `is_visible() == true` under
    // `#[gtk::test]` once the main loop is drained, so asserting it here would
    // mostly measure the harness. Confirming that capture ordering really beats
    // popover autohide needs real Escape key presses, which Task 5's manual
    // verification delivers.
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

    #[gtk::test]
    fn picker_exposes_endpoint_and_back_accessible_labels() {
        let controller = DatePill::builder().launch(());
        let from = date(2025, 12, 28);
        let to = date(2026, 1, 4);
        let expected_start = replace_pair(
            &gettext("{}, {}"),
            &gettext("Start date"),
            &format_date(from, YearDisplay::WithYear),
        );
        let expected_end = replace_pair(
            &gettext("{}, {}"),
            &gettext("End date"),
            &format_date(to, YearDisplay::WithYear),
        );

        controller.emit(DatePillInput::PresetSelected(DateFilter::Custom {
            from,
            to,
        }));
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| {
            controller.widgets().stack.visible_child_name().as_deref() == Some("custom")
                && calendar_to_naive_date(&controller.widgets().calendar.date()) == Some(from)
                && controller.widgets().start_toggle.label().as_deref()
                    == Some(expected_start.as_str())
        });

        assert_eq!(
            controller.widgets().start_toggle.label().as_deref(),
            Some(expected_start.as_str())
        );
        assert_eq!(
            controller.widgets().end_toggle.label().as_deref(),
            Some(expected_end.as_str())
        );
        assert!(gtk::test_accessible_has_property(
            &controller.widgets().calendar,
            gtk::AccessibleProperty::Label,
        ));
        assert!(gtk::test_accessible_has_property(
            &controller.widgets().back_button,
            gtk::AccessibleProperty::Label,
        ));
        // `controller.widgets()` hands back a `Ref`, so the recorder has to be
        // cloned out before borrowing it; holding the widgets `Ref` across the
        // assertions would also deadlock the next `update_view`.
        let accessible_label_log = controller.widgets().accessible_label_log.clone();
        let labels = accessible_label_log.borrow();
        assert_eq!(labels.0, gettext("Back to date presets"));
        assert_eq!(labels.1, gettext("Start date"));
        drop(labels);

        controller.widgets().endpoint_toggles.set_active(1);
        pump_main_context(|| {
            controller.widgets().accessible_label_log.borrow().1 == gettext("End date")
        });
    }

    #[gtk::test]
    fn only_user_picks_request_a_medium_summary_announcement() {
        let controller = DatePill::builder().launch(());
        let from = date(2026, 6, 3);
        let to = date(2026, 6, 9);

        controller.emit(DatePillInput::PresetSelected(DateFilter::Custom {
            from,
            to,
        }));
        controller.emit(DatePillInput::CustomRangeRowSelected);
        pump_main_context(|| {
            controller.widgets().stack.visible_child_name().as_deref() == Some("custom")
        });
        assert!(controller.widgets().announcement_log.borrow().is_empty());

        let summary_before_switch = controller.widgets().summary_label.label();
        let start_before_switch = controller.widgets().start_date_label.label();
        let end_before_switch = controller.widgets().end_date_label.label();
        controller.widgets().endpoint_toggles.set_active(1);
        pump_main_context(|| {
            controller.widgets().endpoint_toggles.active() == 1
                && calendar_to_naive_date(&controller.widgets().calendar.date()) == Some(to)
        });
        while glib::MainContext::default().pending() {
            glib::MainContext::default().iteration(false);
        }
        assert_eq!(
            controller.widgets().summary_label.label(),
            summary_before_switch
        );
        assert_eq!(
            controller.widgets().start_date_label.label(),
            start_before_switch
        );
        assert_eq!(
            controller.widgets().end_date_label.label(),
            end_before_switch
        );
        assert!(controller.widgets().announcement_log.borrow().is_empty());

        let picked = date(2026, 6, 12);
        controller
            .widgets()
            .calendar
            .set_date(&naive_to_glib_date(picked).unwrap());
        pump_main_context(|| controller.widgets().announcement_log.borrow().len() == 1);
        let announcement_log = controller.widgets().announcement_log.clone();
        let announcements = announcement_log.borrow();
        assert_eq!(
            announcements[0].1,
            gtk::AccessibleAnnouncementPriority::Medium
        );
        assert_eq!(
            announcements[0].0,
            custom_info_text(Some(from), Some(picked), Local::now().date_naive())
        );
    }
}
