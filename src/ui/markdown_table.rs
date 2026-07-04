use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use relm4::gtk;
use std::cell::{Cell, RefCell};

pub(crate) const COLUMN_MIN_WIDTH: i32 = 120;

pub(crate) fn create_table_label(
    text: &str,
    query: &str,
    is_header: bool,
    wraps: bool,
) -> (gtk::Label, usize) {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    label.set_valign(gtk::Align::Start);
    label.set_wrap(wraps);
    label.set_single_line_mode(false);
    if wraps {
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_width_chars(1);
        label.set_max_width_chars(1);
    }
    label.add_css_class("markdown-table-cell");
    if is_header {
        label.add_css_class("markdown-table-header");
    }

    let match_count = if query.is_empty() {
        label.set_text(text);
        0
    } else {
        let (markup, count) = crate::ui::highlight::highlight_text(text, query);
        label.set_use_markup(true);
        label.set_markup(&markup);
        count
    };

    (label, match_count)
}

pub(crate) const COLUMN_SPACING: i32 = 12;
pub(crate) const ROW_SPACING: i32 = 4;
pub(crate) const HEADER_SEPARATOR_HEIGHT: i32 = 1;
pub(crate) const SCROLLBAR_HEIGHT: i32 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableLayout {
    pub total_width: i32,
    pub content_height: i32,
    pub scrollbar_visible: bool,
    pub allocated_height: i32,
    pub row_heights: Vec<i32>,
}

pub(crate) fn total_table_width(column_count: usize) -> i32 {
    if column_count == 0 {
        return 0;
    }

    (column_count as i32 * COLUMN_MIN_WIDTH) + ((column_count as i32 - 1) * COLUMN_SPACING)
}

fn calculate_layout(
    labels: &[gtk::Label],
    column_count: usize,
    row_count: usize,
    allocated_width: i32,
) -> TableLayout {
    let mut row_heights = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let mut row_height = 0;
        for col in 0..column_count {
            let index = row * column_count + col;
            if let Some(label) = labels.get(index) {
                let (_minimum, natural, _minimum_baseline, _natural_baseline) =
                    label.measure(gtk::Orientation::Vertical, COLUMN_MIN_WIDTH);
                row_height = row_height.max(natural);
            }
        }
        row_heights.push(row_height);
    }

    let rows_height: i32 = row_heights.iter().sum();
    let row_spacing = row_count.saturating_sub(1) as i32 * ROW_SPACING;
    let separator_height = if row_count > 1 {
        HEADER_SEPARATOR_HEIGHT
    } else {
        0
    };
    let content_height = rows_height + row_spacing + separator_height;
    let total_width = total_table_width(column_count);
    let scrollbar_visible = allocated_width > 0 && allocated_width < total_width;
    let allocated_height = content_height
        + if scrollbar_visible {
            SCROLLBAR_HEIGHT
        } else {
            0
        };

    TableLayout {
        total_width,
        content_height,
        scrollbar_visible,
        allocated_height,
        row_heights,
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub(crate) struct MarkdownTable {
        pub labels: RefCell<Vec<gtk::Label>>,
        pub column_count: Cell<usize>,
        pub row_count: Cell<usize>,
        pub adjustment: gtk::Adjustment,
        pub scrollbar: gtk::Scrollbar,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MarkdownTable {
        const NAME: &'static str = "ScMarkdownTable";
        type Type = super::MarkdownTable;
        type ParentType = gtk::Widget;

        fn new() -> Self {
            let adjustment = gtk::Adjustment::new(0.0, 0.0, 0.0, 1.0, 24.0, 0.0);
            let scrollbar = gtk::Scrollbar::new(gtk::Orientation::Horizontal, Some(&adjustment));
            scrollbar.set_visible(false);

            Self {
                labels: RefCell::new(Vec::new()),
                column_count: Cell::new(0),
                row_count: Cell::new(0),
                adjustment,
                scrollbar,
            }
        }
    }

    impl ObjectImpl for MarkdownTable {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.scrollbar.set_parent(&*obj);
        }

        fn dispose(&self) {
            for label in self.labels.borrow_mut().drain(..) {
                label.unparent();
            }
            self.scrollbar.unparent();
        }
    }

    impl WidgetImpl for MarkdownTable {}
}

glib::wrapper! {
    pub(crate) struct MarkdownTable(ObjectSubclass<imp::MarkdownTable>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl MarkdownTable {
    pub(crate) fn new(headers: &[String], rows: &[Vec<String>], query: &str) -> Self {
        let table: Self = glib::Object::new();
        table.add_css_class("markdown-table");
        table.set_halign(gtk::Align::Start);
        table.set_valign(gtk::Align::Start);
        table.set_hexpand(true);
        table.set_vexpand(false);

        let imp = table.imp();
        imp.column_count.set(headers.len());
        imp.row_count.set(rows.len() + 1);

        let mut labels = Vec::new();
        for header in headers {
            let (label, _match_count) = create_table_label(header, query, true, true);
            label.set_parent(&table);
            labels.push(label);
        }

        for row in rows {
            for col in 0..headers.len() {
                let text = row.get(col).map(String::as_str).unwrap_or("");
                let (label, _match_count) = create_table_label(text, query, false, true);
                label.set_parent(&table);
                labels.push(label);
            }
        }

        *imp.labels.borrow_mut() = labels;
        table
    }

    pub(crate) fn adjustment(&self) -> gtk::Adjustment {
        self.imp().adjustment.clone()
    }

    pub(crate) fn scrollbar(&self) -> gtk::Scrollbar {
        self.imp().scrollbar.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gtk::test]
    fn shared_table_label_can_be_non_wrapping_for_existing_renderer() {
        let (label, count) = create_table_label("Rust", "", true, false);

        assert_eq!(count, 0);
        assert_eq!(label.text(), "Rust");
        assert!(!label.wraps());
        assert!(label.has_css_class("markdown-table-cell"));
        assert!(label.has_css_class("markdown-table-header"));
    }

    #[gtk::test]
    fn shared_table_label_can_wrap_for_markdown_table_widget() {
        let (label, count) = create_table_label("Rust language", "Rust", false, true);

        assert_eq!(count, 1);
        assert!(label.uses_markup());
        assert!(label.wraps());
        assert_eq!(label.wrap_mode(), gtk::pango::WrapMode::WordChar);
        assert!(label.has_css_class("markdown-table-cell"));
        assert!(!label.has_css_class("markdown-table-header"));
    }

    #[test]
    fn total_table_width_uses_fixed_column_width_and_spacing() {
        assert_eq!(total_table_width(0), 0);
        assert_eq!(total_table_width(1), COLUMN_MIN_WIDTH);
        assert_eq!(
            total_table_width(3),
            COLUMN_MIN_WIDTH * 3 + COLUMN_SPACING * 2
        );
    }

    #[gtk::test]
    fn layout_marks_scrollbar_visible_only_on_overflow() {
        let labels = vec![
            create_table_label("A", "", true, true).0,
            create_table_label("B", "", true, true).0,
            create_table_label("one", "", false, true).0,
            create_table_label("two", "", false, true).0,
        ];

        let total = total_table_width(2);
        let fitting = calculate_layout(&labels, 2, 2, total);
        let overflowing = calculate_layout(&labels, 2, 2, total - 1);

        assert!(!fitting.scrollbar_visible);
        assert!(overflowing.scrollbar_visible);
        assert_eq!(
            overflowing.allocated_height,
            overflowing.content_height + SCROLLBAR_HEIGHT
        );
    }

    #[gtk::test]
    fn markdown_table_constructs_wrapping_label_children() {
        let table = MarkdownTable::new(
            &["Column A".to_string(), "Column B".to_string()],
            &[vec![
                "Long prose cell".to_string(),
                "Second cell".to_string(),
            ]],
            "prose",
        );

        assert!(table.has_css_class("markdown-table"));

        let mut labels = Vec::new();
        let mut child = table.first_child();
        while let Some(widget) = child {
            if let Ok(label) = widget.clone().downcast::<gtk::Label>() {
                labels.push(label);
            }
            child = widget.next_sibling();
        }

        assert_eq!(labels.len(), 4);
        assert!(labels.iter().all(|label| label.wraps()));
        assert!(labels[0].has_css_class("markdown-table-header"));
    }

    #[gtk::test]
    fn markdown_table_exposes_scrollbar_bound_to_adjustment() {
        let table = MarkdownTable::new(
            &["A".to_string(), "B".to_string(), "C".to_string()],
            &[vec!["1".to_string(), "2".to_string(), "3".to_string()]],
            "",
        );

        assert_eq!(
            table.scrollbar().orientation(),
            gtk::Orientation::Horizontal
        );
        assert_eq!(table.scrollbar().adjustment(), table.adjustment());
    }
}
