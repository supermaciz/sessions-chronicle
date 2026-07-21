use std::cell::Cell;
use std::rc::Rc;

use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, SimpleComponent, gtk};

use crate::models::SortOrder;

const NAMED_ORDERS: [SortOrder; 4] = [
    SortOrder::RecentActivity,
    SortOrder::OldestFirst,
    SortOrder::NewestFirst,
    SortOrder::MostMessages,
];

pub struct SortPill {
    sort_order: SortOrder,
    fts_search_active: bool,
    override_active: bool,
    narrow: bool,
    listbox: gtk::ListBox,
    popover: gtk::Popover,
    row_activation_fts_flag: Rc<Cell<bool>>,
}

#[derive(Debug, Clone, Copy)]
pub enum SortPillInput {
    OrderSelected(SortOrder),
    RelevanceSelected,
    SyncState {
        sort_order: SortOrder,
        fts_search_active: bool,
        override_active: bool,
    },
    OpenViaShortcut,
    SetNarrow(bool),
}

#[derive(Debug, Clone, Copy)]
pub enum SortPillOutput {
    OrderPicked(SortOrder),
    RelevancePicked,
}

pub struct SortPillWidgets {
    root: gtk::MenuButton,
    pub(crate) label: gtk::Label,
}

impl SimpleComponent for SortPill {
    type Init = SortOrder;
    type Input = SortPillInput;
    type Output = SortPillOutput;
    type Root = gtk::MenuButton;
    type Widgets = SortPillWidgets;

    fn init_root() -> Self::Root {
        gtk::MenuButton::new()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let icon = gtk::Image::from_icon_name("view-sort-descending-symbolic");
        let label = gtk::Label::new(None);
        label.set_visible(false);

        let button_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        button_content.append(&icon);
        button_content.append(&label);

        root.set_child(Some(&button_content));
        root.add_css_class("flat");

        let popover = gtk::Popover::new();
        root.set_popover(Some(&popover));

        let listbox = gtk::ListBox::new();
        listbox.add_css_class("boxed-list");
        listbox.set_selection_mode(gtk::SelectionMode::Browse);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.append(&listbox);
        popover.set_child(Some(&content));

        let fts_search_active = Rc::new(Cell::new(false));

        let input_sender = sender.input_sender().clone();
        let fts_flag = fts_search_active.clone();
        listbox.connect_row_activated(move |_list, row| {
            let index = row.index() as usize;
            if fts_flag.get() && index == 0 {
                input_sender.send(SortPillInput::RelevanceSelected).ok();
            } else {
                let offset = usize::from(fts_flag.get());
                if index >= offset
                    && let Some(order) = NAMED_ORDERS.get(index - offset).copied()
                {
                    input_sender.send(SortPillInput::OrderSelected(order)).ok();
                }
            }
        });

        let model = Self {
            sort_order: init,
            fts_search_active: false,
            override_active: false,
            narrow: false,
            listbox: listbox.clone(),
            popover: popover.clone(),
            row_activation_fts_flag: fts_search_active,
        };

        model.rebuild_rows();

        let widgets = SortPillWidgets { root, label };

        model.sync_button(&widgets);
        model.select_effective_row();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SortPillInput::OrderSelected(order) => {
                self.sort_order = order;
                self.override_active = true;
                self.select_effective_row();
                sender.output(SortPillOutput::OrderPicked(order)).ok();
                self.popover.popdown();
            }
            SortPillInput::RelevanceSelected => {
                self.override_active = false;
                self.select_effective_row();
                sender.output(SortPillOutput::RelevancePicked).ok();
                self.popover.popdown();
            }
            SortPillInput::SyncState {
                sort_order,
                fts_search_active,
                override_active,
            } => {
                let fts_changed = self.fts_search_active != fts_search_active;
                self.sort_order = sort_order;
                self.fts_search_active = fts_search_active;
                self.override_active = override_active;
                self.row_activation_fts_flag.set(fts_search_active);
                if fts_changed {
                    self.rebuild_rows();
                }
                self.select_effective_row();
            }
            SortPillInput::OpenViaShortcut => {
                self.popover.popup();
                self.focus_effective_row_when_ready();
            }
            SortPillInput::SetNarrow(narrow) => {
                self.narrow = narrow;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        self.sync_button(widgets);
    }
}

impl SortPill {
    fn rebuild_rows(&self) {
        while let Some(row) = self.listbox.row_at_index(0) {
            self.listbox.remove(&row);
        }

        if self.fts_search_active {
            self.listbox.append(&build_row(&gettext("Relevance")));
        }

        for order in NAMED_ORDERS {
            self.listbox
                .append(&build_row(&localized_order_label(order)));
        }

        if self.fts_search_active {
            self.listbox.set_header_func(|row, before| {
                if row.index() == 1 && before.is_some() {
                    row.set_header(Some(&gtk::Separator::new(gtk::Orientation::Horizontal)));
                } else {
                    row.set_header(gtk::Widget::NONE);
                }
            });
        } else {
            self.listbox.unset_header_func();
        }
    }

    fn effective_row_index(&self) -> i32 {
        if self.fts_search_active && !self.override_active {
            0
        } else {
            let offset = usize::from(self.fts_search_active);
            let named_index = NAMED_ORDERS
                .iter()
                .position(|order| *order == self.sort_order)
                .unwrap_or(0);
            (named_index + offset) as i32
        }
    }

    fn select_effective_row(&self) {
        let index = self.effective_row_index();
        if let Some(row) = self.listbox.row_at_index(index) {
            self.listbox.select_row(Some(&row));
        }
    }

    fn focus_effective_row_when_ready(&self) {
        let listbox = self.listbox.clone();
        let row_index = self.effective_row_index();

        glib::idle_add_local_once(move || {
            let Some(row) = listbox.row_at_index(row_index) else {
                return;
            };

            listbox.select_row(Some(&row));
            listbox.grab_focus();
            row.grab_focus();
        });
    }

    fn sync_button(&self, widgets: &SortPillWidgets) {
        let effective = effective_label(
            self.sort_order,
            self.fts_search_active,
            self.override_active,
        );
        widgets.label.set_label(&effective);
        widgets.label.set_visible(
            !self.narrow
                && should_show_label(
                    self.sort_order,
                    self.fts_search_active,
                    self.override_active,
                ),
        );
        widgets.root.set_tooltip_text(Some(&tooltip_text(
            self.sort_order,
            self.fts_search_active,
            self.override_active,
        )));
    }
}

fn localized_order_label(order: SortOrder) -> String {
    match order.label_msgid() {
        "Recent activity" => gettext("Recent activity"),
        "Oldest first" => gettext("Oldest first"),
        "Newest first" => gettext("Newest first"),
        "Most messages" => gettext("Most messages"),
        _ => unreachable!("SortOrder returned an unknown label message ID"),
    }
}

fn build_row(label: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::new(Some(label));
    label.set_xalign(0.0);
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row_box.set_margin_top(8);
    row_box.set_margin_bottom(8);
    row_box.set_margin_start(12);
    row_box.set_margin_end(12);
    row_box.append(&label);
    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    row.set_focusable(true);
    row.set_child(Some(&row_box));
    row
}

fn should_show_label(
    sort_order: SortOrder,
    fts_search_active: bool,
    _override_active: bool,
) -> bool {
    fts_search_active || sort_order != SortOrder::RecentActivity
}

fn effective_label(
    sort_order: SortOrder,
    fts_search_active: bool,
    override_active: bool,
) -> String {
    if fts_search_active && !override_active {
        gettext("Relevance")
    } else {
        localized_order_label(sort_order)
    }
}

fn tooltip_text(sort_order: SortOrder, fts_search_active: bool, override_active: bool) -> String {
    if !fts_search_active && sort_order == SortOrder::RecentActivity {
        gettext("Sort sessions (Ctrl+Shift+O)")
    } else {
        gettext("Sort by: {}").replace(
            "{}",
            &effective_label(sort_order, fts_search_active, override_active),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relm4::{Component, ComponentController};
    use std::time::{Duration, Instant};

    #[test]
    fn tooltip_and_label_rules_match_effective_order() {
        assert_eq!(
            tooltip_text(SortOrder::RecentActivity, false, false),
            "Sort sessions (Ctrl+Shift+O)"
        );
        assert_eq!(
            tooltip_text(SortOrder::OldestFirst, false, false),
            "Sort by: Oldest first"
        );
        assert_eq!(
            tooltip_text(SortOrder::RecentActivity, true, false),
            "Sort by: Relevance"
        );
        assert!(!should_show_label(SortOrder::RecentActivity, false, false));
        assert!(should_show_label(SortOrder::RecentActivity, true, true));
        assert!(should_show_label(SortOrder::MostMessages, false, false));
    }

    fn pump_main_context(condition: impl Fn() -> bool) {
        let context = gtk::glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "condition not met before timeout"
            );
            while context.pending() {
                context.iteration(false);
            }
            // Never block on `iteration(true)`: with no ready source the call
            // parks forever and the deadline above can never be reached.
            std::thread::sleep(Duration::from_millis(10));
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

    /// Count only the rows. `observe_children` also yields the separator
    /// installed as a row header while FTS is active, which is not a row.
    fn row_count(list: &gtk::ListBox) -> i32 {
        let mut count = 0;
        while list.row_at_index(count).is_some() {
            count += 1;
        }
        count
    }

    #[gtk::test]
    fn relevance_row_exists_only_during_fts() {
        let controller = SortPill::builder().launch(SortOrder::RecentActivity);
        let list = find_list_box(&controller.widget().clone().upcast()).unwrap();
        assert_eq!(row_count(&list), 4);

        controller.emit(SortPillInput::SyncState {
            sort_order: SortOrder::RecentActivity,
            fts_search_active: true,
            override_active: false,
        });
        pump_main_context(|| row_count(&list) == 5);

        controller.emit(SortPillInput::SyncState {
            sort_order: SortOrder::RecentActivity,
            fts_search_active: false,
            override_active: false,
        });
        pump_main_context(|| row_count(&list) == 4);
    }

    #[gtk::test]
    fn shortcut_selects_the_effective_row() {
        let controller = SortPill::builder().launch(SortOrder::RecentActivity);
        let window = gtk::Window::new();
        window.set_child(Some(controller.widget()));
        window.present();
        let list = find_list_box(&controller.widget().clone().upcast()).unwrap();

        controller.emit(SortPillInput::SyncState {
            sort_order: SortOrder::MostMessages,
            fts_search_active: true,
            override_active: true,
        });
        controller.emit(SortPillInput::OpenViaShortcut);
        pump_main_context(|| list.selected_row().map(|row| row.index()) == Some(4));
    }

    #[gtk::test]
    fn label_hides_for_default_and_narrow_states() {
        let controller = SortPill::builder().launch(SortOrder::RecentActivity);
        let label = controller.widgets().label.clone();
        assert!(!label.is_visible());

        controller.emit(SortPillInput::SyncState {
            sort_order: SortOrder::OldestFirst,
            fts_search_active: false,
            override_active: false,
        });
        pump_main_context(|| label.is_visible());
        controller.emit(SortPillInput::SetNarrow(true));
        pump_main_context(|| !label.is_visible());
        controller.emit(SortPillInput::SetNarrow(false));
        pump_main_context(|| label.is_visible());
        assert!(!controller.widget().has_css_class("accent"));
    }
}
