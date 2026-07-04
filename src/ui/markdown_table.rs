use gtk::prelude::*;
use relm4::gtk;

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
}
