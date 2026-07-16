use gtk::prelude::*;
use gtk::subclass::prelude::*;
use relm4::gtk::{self, gdk, glib};
use std::cell::{Cell, RefCell};

pub(crate) const COLUMN_MIN_WIDTH: i32 = 160;

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
/// Fallback used only when the scrollbar has no themed natural height yet
/// (e.g. before it is styled). The real reservation measures the scrollbar.
pub(crate) const SCROLLBAR_HEIGHT: i32 = 15;

/// The vertical space to reserve for the horizontal scrollbar, taken from the
/// scrollbar's own natural height so themes, CSS overrides, and accessibility
/// settings that change its thickness are honored instead of a fixed constant.
fn measured_scrollbar_height(scrollbar: &gtk::Scrollbar) -> i32 {
    let natural = scrollbar.measure(gtk::Orientation::Vertical, -1).1;
    if natural > 0 {
        natural
    } else {
        SCROLLBAR_HEIGHT
    }
}

/// The vertical space reserved for the themed header/data separator.
fn measured_separator_height(separator: &gtk::Separator) -> i32 {
    let natural = separator.measure(gtk::Orientation::Vertical, -1).1;
    if natural > 0 {
        natural
    } else {
        HEADER_SEPARATOR_HEIGHT
    }
}

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

/// Mirrors GTK's current `MAGIC_SCROLL_FACTOR` for surface-pixel scrolling.
/// This is application behavior, not a stable public GTK constant.
const SURFACE_SCROLL_FACTOR: f64 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Default)]
pub(crate) struct ScrollGesture {
    active: bool,
    axis: Option<ScrollAxis>,
}

impl ScrollGesture {
    fn begin(&mut self) {
        self.active = true;
        self.axis = None;
    }

    fn end(&mut self) {
        self.active = false;
        self.axis = None;
    }

    fn classify(&mut self, dx: f64, dy: f64, shift: bool) -> Option<ScrollAxis> {
        if dx == 0.0 && dy == 0.0 {
            return None;
        }
        if self.active && self.axis.is_some() {
            return self.axis;
        }

        let axis = if shift || dx.abs() > dy.abs() {
            ScrollAxis::Horizontal
        } else {
            ScrollAxis::Vertical
        };
        if self.active {
            self.axis = Some(axis);
        }
        Some(axis)
    }
}

/// Apply a scroll delta to the table's horizontal adjustment.
/// Returns `true` when the event must not bubble to the transcript scroller.
fn apply_horizontal_scroll(
    adjustment: &gtk::Adjustment,
    dx: f64,
    dy: f64,
    shift: bool,
    unit: gdk::ScrollUnit,
    gesture: &mut ScrollGesture,
) -> bool {
    let lower = adjustment.lower();
    let max_value = adjustment.upper() - adjustment.page_size();
    if max_value <= lower {
        return false;
    }

    if gesture.classify(dx, dy, shift) != Some(ScrollAxis::Horizontal) {
        return false;
    }
    let delta = if shift { dy } else { dx };
    if delta == 0.0 {
        // A locked gesture keeps consuming events until `scroll-end`, otherwise
        // this frame's other axis would bubble to the transcript scroller.
        return gesture.active;
    }

    let normalized = match unit {
        gdk::ScrollUnit::Wheel => delta * adjustment.page_size().powf(2.0 / 3.0),
        gdk::ScrollUnit::Surface => delta * SURFACE_SCROLL_FACTOR,
        _ => delta,
    };
    let value = (adjustment.value() + normalized).clamp(lower, max_value);
    adjustment.set_value(value);
    true
}

fn calculate_layout(
    labels: &[gtk::Label],
    column_count: usize,
    row_count: usize,
    allocated_width: i32,
    scrollbar_height: i32,
    separator_height: i32,
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
    let reserved_separator_height = if row_count > 1 { separator_height } else { 0 };
    let content_height = rows_height + row_spacing + reserved_separator_height;
    let total_width = total_table_width(column_count);
    let scrollbar_visible = allocated_width > 0 && allocated_width < total_width;
    let allocated_height = content_height
        + if scrollbar_visible {
            scrollbar_height
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
        pub match_count: Cell<usize>,
        pub adjustment: gtk::Adjustment,
        pub scroll_gesture: RefCell<ScrollGesture>,
        pub separator: gtk::Separator,
        pub scrollbar: gtk::Scrollbar,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MarkdownTable {
        const NAME: &'static str = "ScMarkdownTable";
        type Type = super::MarkdownTable;
        type ParentType = gtk::Widget;

        fn new() -> Self {
            let adjustment = gtk::Adjustment::new(0.0, 0.0, 0.0, 1.0, 24.0, 0.0);
            let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
            separator.set_visible(false);
            let scrollbar = gtk::Scrollbar::new(gtk::Orientation::Horizontal, Some(&adjustment));
            scrollbar.set_visible(false);

            Self {
                labels: RefCell::new(Vec::new()),
                column_count: Cell::new(0),
                row_count: Cell::new(0),
                match_count: Cell::new(0),
                adjustment,
                scroll_gesture: RefCell::new(ScrollGesture::default()),
                separator,
                scrollbar,
            }
        }
    }

    impl ObjectImpl for MarkdownTable {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.separator.set_parent(&*obj);
            self.scrollbar.set_parent(&*obj);

            let controller =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);

            let weak = obj.downgrade();
            controller.connect_scroll_begin(move |_| {
                if let Some(obj) = weak.upgrade() {
                    obj.imp().scroll_gesture.borrow_mut().begin();
                }
            });

            let weak = obj.downgrade();
            controller.connect_scroll(move |ctrl, dx, dy| {
                let Some(obj) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let shift = ctrl
                    .current_event_state()
                    .contains(gdk::ModifierType::SHIFT_MASK);
                if apply_horizontal_scroll(
                    &obj.imp().adjustment,
                    dx,
                    dy,
                    shift,
                    ctrl.unit(),
                    &mut obj.imp().scroll_gesture.borrow_mut(),
                ) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });

            let weak = obj.downgrade();
            controller.connect_scroll_end(move |_| {
                if let Some(obj) = weak.upgrade() {
                    obj.imp().scroll_gesture.borrow_mut().end();
                }
            });

            obj.add_controller(controller);

            // The cell offset is derived from the adjustment value during
            // size_allocate, so scrolling must queue a fresh allocation;
            // otherwise dragging the scrollbar moves the thumb while the cells
            // stay put until an unrelated resize occurs.
            let weak = obj.downgrade();
            self.adjustment.connect_value_changed(move |_| {
                if let Some(obj) = weak.upgrade() {
                    obj.queue_allocate();
                }
            });
        }

        fn dispose(&self) {
            for label in self.labels.borrow_mut().drain(..) {
                label.unparent();
            }
            self.separator.unparent();
            self.scrollbar.unparent();
        }
    }

    impl WidgetImpl for MarkdownTable {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            // The reported height depends on the allocated width (a narrow
            // allocation reserves room for the internal scrollbar), so GTK
            // must re-measure the height for the width it will allocate.
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let column_count = self.column_count.get();
            let row_count = self.row_count.get();
            let labels = self.labels.borrow();
            let total_width = total_table_width(column_count);

            match orientation {
                gtk::Orientation::Horizontal => {
                    // Report a single column as the minimum so the widget can
                    // be underallocated inside a narrow transcript bubble: the
                    // parent may shrink us down to one column and we scroll the
                    // rest internally. Reporting `total_width` as the minimum
                    // would force every ancestor to honor the full table width
                    // (GTK also clamps the orthogonal measurement to the
                    // minimum), so the internal scrollbar would never engage.
                    let minimum = COLUMN_MIN_WIDTH.min(total_width);
                    (minimum, total_width, -1, -1)
                }
                gtk::Orientation::Vertical => {
                    let scrollbar_height = measured_scrollbar_height(&self.scrollbar);
                    let separator_height = measured_separator_height(&self.separator);
                    let layout = calculate_layout(
                        &labels,
                        column_count,
                        row_count,
                        for_size,
                        scrollbar_height,
                        separator_height,
                    );
                    (layout.allocated_height, layout.allocated_height, -1, -1)
                }
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, baseline: i32) {
            let column_count = self.column_count.get();
            let row_count = self.row_count.get();
            let labels = self.labels.borrow();
            let scrollbar_height = measured_scrollbar_height(&self.scrollbar);
            let separator_height = measured_separator_height(&self.separator);
            let layout = calculate_layout(
                &labels,
                column_count,
                row_count,
                width,
                scrollbar_height,
                separator_height,
            );

            self.adjustment.set_lower(0.0);
            self.adjustment.set_upper(layout.total_width as f64);
            self.adjustment.set_page_size(width.max(0) as f64);
            self.adjustment.set_step_increment(24.0);
            self.adjustment
                .set_page_increment(width.max(0) as f64 * 0.9);
            self.scrollbar.set_visible(layout.scrollbar_visible);
            self.separator.set_visible(row_count > 1);

            let max_value = (layout.total_width - width).max(0) as f64;
            if self.adjustment.value() > max_value {
                self.adjustment.set_value(max_value);
            }

            let x_offset = -(self.adjustment.value().round() as i32);
            let mut y = 0;
            for row in 0..row_count {
                let row_height = layout.row_heights.get(row).copied().unwrap_or(0);
                let mut x = x_offset;
                for col in 0..column_count {
                    let index = row * column_count + col;
                    if let Some(label) = labels.get(index) {
                        let transform = gtk::gsk::Transform::new()
                            .translate(&gtk::graphene::Point::new(x as f32, y as f32));
                        label.allocate(COLUMN_MIN_WIDTH, row_height, baseline, Some(transform));
                    }
                    x += COLUMN_MIN_WIDTH + COLUMN_SPACING;
                }

                y += row_height;
                if row == 0 && row_count > 1 {
                    let transform = gtk::gsk::Transform::new()
                        .translate(&gtk::graphene::Point::new(0.0, y as f32));
                    self.separator.allocate(
                        width.max(0),
                        separator_height,
                        baseline,
                        Some(transform),
                    );
                    y += separator_height;
                }
                if row + 1 < row_count {
                    y += ROW_SPACING;
                }
            }

            if layout.scrollbar_visible {
                let transform = gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(
                    0.0,
                    layout.content_height as f32,
                ));
                self.scrollbar
                    .allocate(width.max(0), scrollbar_height, baseline, Some(transform));
            }
        }
    }
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
        // Clip children to the viewport: size_allocate places scrolled-away
        // columns at negative x (or beyond the allocated width), and GtkWidget
        // does not clip its children by default, so without this they would
        // paint over surrounding transcript content instead of being hidden.
        table.set_overflow(gtk::Overflow::Hidden);
        table.set_halign(gtk::Align::Start);
        table.set_valign(gtk::Align::Start);
        table.set_hexpand(true);
        table.set_vexpand(false);

        let imp = table.imp();
        imp.column_count.set(headers.len());
        imp.row_count.set(rows.len() + 1);
        // Decide the separator's visibility up front so the very first
        // `measure` sees a visible widget. GTK4's `gtk_widget_measure` returns
        // 0 for a non-visible child, so measuring it while hidden would yield
        // the `HEADER_SEPARATOR_HEIGHT` fallback and under-reserve its themed
        // height on the first layout pass. `size_allocate` keeps this in sync.
        imp.separator.set_visible(!rows.is_empty());

        let mut labels = Vec::new();
        // Aggregate the per-cell search-hit counts so the count survives when
        // this widget replaces the grid renderer; render_markdown adds it to
        // the reported total (see markdown::render_table).
        let mut match_count = 0usize;
        for header in headers {
            let (label, cell_matches) = create_table_label(header, query, true, true);
            match_count += cell_matches;
            label.set_parent(&table);
            labels.push(label);
        }

        for row in rows {
            for col in 0..headers.len() {
                let text = row.get(col).map(String::as_str).unwrap_or("");
                let (label, cell_matches) = create_table_label(text, query, false, true);
                match_count += cell_matches;
                label.set_parent(&table);
                labels.push(label);
            }
        }

        *imp.labels.borrow_mut() = labels;
        imp.match_count.set(match_count);
        table
    }

    /// Total number of search-query matches across all cells, so a caller
    /// wiring this widget into `render_table` can add it to the reported
    /// search-result count instead of losing table hits from navigation.
    pub(crate) fn match_count(&self) -> usize {
        self.imp().match_count.get()
    }

    pub(crate) fn adjustment(&self) -> gtk::Adjustment {
        self.imp().adjustment.clone()
    }

    pub(crate) fn scrollbar(&self) -> gtk::Scrollbar {
        self.imp().scrollbar.clone()
    }

    pub(crate) fn separator(&self) -> gtk::Separator {
        self.imp().separator.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll_adjustment(value: f64, upper: f64, page_size: f64) -> gtk::Adjustment {
        gtk::Adjustment::new(value, 0.0, upper, 1.0, page_size * 0.9, page_size)
    }

    fn assert_approx_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[gtk::test]
    fn dominant_horizontal_surface_delta_moves_right() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            4.0,
            1.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(consumed);
        assert_approx_eq(adjustment.value(), 110.0);
    }

    #[gtk::test]
    fn negative_horizontal_delta_clamps_at_lower_bound() {
        let adjustment = scroll_adjustment(5.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            -4.0,
            0.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(consumed);
        assert_eq!(adjustment.value(), adjustment.lower());
    }

    #[gtk::test]
    fn shift_remaps_vertical_delta_to_horizontal() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            0.0,
            4.0,
            true,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(consumed);
        assert_approx_eq(adjustment.value(), 110.0);
    }

    #[gtk::test]
    fn plain_vertical_delta_propagates_without_moving_table() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            0.0,
            4.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(!consumed);
        assert_eq!(adjustment.value(), 100.0);
    }

    #[gtk::test]
    fn dominant_vertical_delta_with_diagonal_noise_propagates() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            1.0,
            4.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(!consumed);
        assert_eq!(adjustment.value(), 100.0);
    }

    #[gtk::test]
    fn equal_axis_deltas_favor_vertical_propagation() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            4.0,
            4.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(!consumed);
        assert_eq!(adjustment.value(), 100.0);
    }

    #[gtk::test]
    fn horizontal_delta_propagates_when_table_does_not_overflow() {
        let adjustment = scroll_adjustment(0.0, 100.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            4.0,
            0.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(!consumed);
        assert_eq!(adjustment.value(), 0.0);
    }

    #[gtk::test]
    fn overflowing_table_clamps_delta_past_right_edge() {
        let adjustment = scroll_adjustment(895.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            4.0,
            0.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(consumed);
        assert_eq!(adjustment.value(), 900.0);
    }

    #[gtk::test]
    fn overflowing_table_consumes_horizontal_delta_when_already_at_edge() {
        let adjustment = scroll_adjustment(900.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            4.0,
            0.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(consumed);
        assert_eq!(adjustment.value(), 900.0);
    }

    #[gtk::test]
    fn wheel_delta_scales_with_page_size() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 125.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            1.0,
            0.0,
            false,
            gdk::ScrollUnit::Wheel,
            &mut gesture,
        );

        assert!(consumed);
        assert_approx_eq(adjustment.value(), 100.0 + 125.0_f64.powf(2.0 / 3.0));
    }

    #[gtk::test]
    fn surface_delta_uses_gtk_scroll_factor() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        let consumed = apply_horizontal_scroll(
            &adjustment,
            4.0,
            0.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        );

        assert!(consumed);
        assert_approx_eq(adjustment.value(), 100.0 + 4.0 * SURFACE_SCROLL_FACTOR);
    }

    #[gtk::test]
    fn continuous_vertical_gesture_stays_unconsumed_until_end() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();
        gesture.begin();

        assert!(!apply_horizontal_scroll(
            &adjustment,
            1.0,
            8.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
        assert!(!apply_horizontal_scroll(
            &adjustment,
            9.0,
            2.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
        assert_eq!(adjustment.value(), 100.0);

        gesture.end();
        assert!(apply_horizontal_scroll(
            &adjustment,
            9.0,
            2.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
    }

    #[gtk::test]
    fn continuous_horizontal_gesture_stays_consumed_until_end() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();
        gesture.begin();

        assert!(apply_horizontal_scroll(
            &adjustment,
            8.0,
            1.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
        assert!(apply_horizontal_scroll(
            &adjustment,
            1.0,
            6.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
        assert_approx_eq(adjustment.value(), 100.0 + (8.0 + 1.0) * 2.5);

        gesture.end();
        assert!(!apply_horizontal_scroll(
            &adjustment,
            1.0,
            6.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
    }

    #[gtk::test]
    fn locked_horizontal_gesture_consumes_frames_without_horizontal_delta() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();
        gesture.begin();

        assert!(apply_horizontal_scroll(
            &adjustment,
            8.0,
            1.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
        assert!(apply_horizontal_scroll(
            &adjustment,
            0.0,
            6.0,
            false,
            gdk::ScrollUnit::Surface,
            &mut gesture,
        ));
        assert_approx_eq(adjustment.value(), 100.0 + 8.0 * 2.5);
    }

    #[gtk::test]
    fn wheel_without_horizontal_delta_stays_unconsumed() {
        let adjustment = scroll_adjustment(100.0, 1000.0, 100.0);
        let mut gesture = ScrollGesture::default();

        assert!(!apply_horizontal_scroll(
            &adjustment,
            0.0,
            6.0,
            false,
            gdk::ScrollUnit::Wheel,
            &mut gesture,
        ));
        assert_eq!(adjustment.value(), 100.0);
    }

    #[gtk::test]
    fn markdown_table_attaches_both_axes_scroll_controller() {
        let table = MarkdownTable::new(
            &["A".to_string(), "B".to_string()],
            &[vec!["1".to_string(), "2".to_string()]],
            "",
        );
        let controllers = table.observe_controllers();
        let scroll_controller = (0..controllers.n_items())
            .filter_map(|index| controllers.item(index))
            .find_map(|controller| controller.downcast::<gtk::EventControllerScroll>().ok())
            .expect("MarkdownTable should own an EventControllerScroll");

        assert_eq!(
            scroll_controller.flags(),
            gtk::EventControllerScrollFlags::BOTH_AXES
        );
        assert_eq!(
            scroll_controller.propagation_phase(),
            gtk::PropagationPhase::Bubble
        );
    }

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
        assert_eq!(label.width_chars(), -1);
        assert_eq!(label.max_width_chars(), -1);
        assert!(label.has_css_class("markdown-table-cell"));
        assert!(!label.has_css_class("markdown-table-header"));
    }

    #[test]
    fn total_table_width_uses_fixed_column_width_and_spacing() {
        assert_eq!(COLUMN_MIN_WIDTH, 160);
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
        let fitting = calculate_layout(
            &labels,
            2,
            2,
            total,
            SCROLLBAR_HEIGHT,
            HEADER_SEPARATOR_HEIGHT,
        );
        let overflowing = calculate_layout(
            &labels,
            2,
            2,
            total - 1,
            SCROLLBAR_HEIGHT,
            HEADER_SEPARATOR_HEIGHT,
        );

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
    fn markdown_table_does_not_wrap_ordinary_text_character_by_character() {
        let table = MarkdownTable::new(
            &["Nom".to_string()],
            &[vec!["Projet Alpha".to_string()]],
            "",
        );
        let full_width = total_table_width(1);
        let (_, natural_height, _, _) = table.measure(gtk::Orientation::Vertical, full_width);
        table.size_allocate(&gtk::Allocation::new(0, 0, full_width, natural_height), -1);

        let body_label = table.imp().labels.borrow()[1].clone();
        assert_eq!(
            body_label.layout().line_count(),
            1,
            "ordinary text should fit on one line at the fixed column width"
        );
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

    #[gtk::test]
    fn markdown_table_shows_separator_only_with_body_rows() {
        let with_body = MarkdownTable::new(
            &["A".to_string(), "B".to_string()],
            &[vec!["1".to_string(), "2".to_string()]],
            "",
        );
        let header_only = MarkdownTable::new(&["A".to_string()], &[], "");

        let (_, body_height, _, _) =
            with_body.measure(gtk::Orientation::Vertical, total_table_width(2));
        with_body.size_allocate(
            &gtk::Allocation::new(0, 0, total_table_width(2), body_height),
            -1,
        );
        let (_, header_height, _, _) =
            header_only.measure(gtk::Orientation::Vertical, total_table_width(1));
        header_only.size_allocate(
            &gtk::Allocation::new(0, 0, total_table_width(1), header_height),
            -1,
        );

        assert_eq!(
            with_body.separator().orientation(),
            gtk::Orientation::Horizontal
        );
        assert!(with_body.separator().is_visible());
        assert!(!header_only.separator().is_visible());
    }

    #[gtk::test]
    fn layout_reserves_measured_separator_height_once() {
        let table = MarkdownTable::new(
            &["A".to_string(), "B".to_string()],
            &[vec!["1".to_string(), "2".to_string()]],
            "",
        );
        let labels = table.imp().labels.borrow();
        let scrollbar_height = measured_scrollbar_height(&table.scrollbar());
        let separator_height = measured_separator_height(&table.separator());
        let with_separator = calculate_layout(
            &labels,
            2,
            2,
            total_table_width(2),
            scrollbar_height,
            separator_height,
        );
        let without_separator =
            calculate_layout(&labels, 2, 2, total_table_width(2), scrollbar_height, 0);

        assert!(separator_height > 0);
        assert_eq!(
            with_separator.content_height,
            without_separator.content_height + separator_height
        );
        assert_eq!(
            with_separator.allocated_height,
            without_separator.allocated_height + separator_height
        );
    }

    fn prose_heavy_markdown_table() -> MarkdownTable {
        let headers = vec![
            "Name".to_string(),
            "Summary".to_string(),
            "Notes".to_string(),
        ];
        let rows = (0..15)
            .map(|index| {
                vec![
                    format!("Row {index}"),
                    "This cell contains prose-heavy markdown table content that should wrap inside a fixed-width column instead of forcing the table to request a huge natural width.".to_string(),
                    "Additional notes with enough words to exercise height-for-width measurement across multiple transcript-like widths.".to_string(),
                ]
            })
            .collect::<Vec<_>>();

        MarkdownTable::new(&headers, &rows, "")
    }

    #[gtk::test]
    fn markdown_table_reports_stable_wrapped_height_at_transcript_widths() {
        let table = prose_heavy_markdown_table();
        let full_width = total_table_width(3);

        // At or above the fixed table width there is no internal scrollbar, and
        // because the columns never re-wrap the content height is independent
        // of the allocated width.
        let (_min_wide, height_wide, _mb_wide, _nb_wide) =
            table.measure(gtk::Orientation::Vertical, full_width);
        let (_min_wider, height_wider, _mb_wider, _nb_wider) =
            table.measure(gtk::Orientation::Vertical, full_width + 336);

        // Below the fixed table width the widget turns into an internal
        // scroller and reserves exactly the scrollbar height on top of the same
        // content height.
        let (_min_narrow, height_narrow, _mb_narrow, _nb_narrow) =
            table.measure(gtk::Orientation::Vertical, full_width - 24);

        assert!(
            height_wide > 0 && height_wide < 4000,
            "expected a sane content height: height_wide={height_wide}, height_wider={height_wider}, height_narrow={height_narrow}"
        );
        assert_eq!(
            height_wide, height_wider,
            "content height must be width-independent above the table width: height_wide={height_wide}, height_wider={height_wider}, height_narrow={height_narrow}"
        );
        let scrollbar_height = measured_scrollbar_height(&table.scrollbar());
        assert_eq!(
            height_narrow,
            height_wide + scrollbar_height,
            "underallocation must reserve exactly the measured scrollbar height: height_wide={height_wide}, height_wider={height_wider}, height_narrow={height_narrow}, scrollbar_height={scrollbar_height}"
        );
    }

    #[gtk::test]
    fn markdown_table_updates_horizontal_adjustment_on_allocate() {
        let table = MarkdownTable::new(
            &["A".to_string(), "B".to_string(), "C".to_string()],
            &[vec!["1".to_string(), "2".to_string(), "3".to_string()]],
            "",
        );
        let (_minimum, natural_height, _minimum_baseline, _natural_baseline) =
            table.measure(gtk::Orientation::Vertical, 240);

        table.size_allocate(&gtk::Allocation::new(0, 0, 240, natural_height), -1);

        assert_eq!(table.adjustment().upper(), total_table_width(3) as f64);
        assert_eq!(table.adjustment().page_size(), 240.0);
        assert!(table.scrollbar().is_visible());
    }

    #[gtk::test]
    fn table_widget_wrapped_cells_keep_stable_height() {
        let headers = vec![
            "Step".to_string(),
            "Description".to_string(),
            "Result".to_string(),
        ];
        let rows = (0..15)
            .map(|index| {
                vec![
                    format!("{index}"),
                    "A prose-heavy cell that reproduces the old wrapped GtkGrid inside GtkScrolledWindow failure mode by requiring several wrapped lines at a fixed column width.".to_string(),
                    "The custom widget should report only the sum of fixed-column row heights and should not reserve a large blank area below the table.".to_string(),
                ]
            })
            .collect::<Vec<_>>();
        let table = MarkdownTable::new(&headers, &rows, "");

        let full_width = total_table_width(headers.len());

        // Two allocations at or above the fixed table width: no scrollbar, so
        // both report the plain content height regardless of extra slack.
        let (_min_420, natural_420, _min_base_420, _natural_base_420) =
            table.measure(gtk::Orientation::Vertical, full_width + 36);
        let (_min_720, natural_720, _min_base_720, _natural_base_720) =
            table.measure(gtk::Orientation::Vertical, full_width + 336);
        // One underallocation below the fixed table width: internal scroller,
        // so it reserves the scrollbar height on top of the content height.
        let (_min_narrow, natural_narrow, _min_base_narrow, _natural_base_narrow) =
            table.measure(gtk::Orientation::Vertical, full_width - 24);

        let scrollbar_height = measured_scrollbar_height(&table.scrollbar());
        let labels = table.imp().labels.borrow();
        let content_height = calculate_layout(
            &labels,
            headers.len(),
            rows.len() + 1,
            full_width,
            scrollbar_height,
            measured_separator_height(&table.separator()),
        )
        .allocated_height;

        assert_eq!(
            natural_420, content_height,
            "reported height should match the fixed-column row sum: content={content_height}, measured_420={natural_420}, measured_720={natural_720}, measured_narrow={natural_narrow}, column_width={COLUMN_MIN_WIDTH}"
        );
        assert_eq!(
            natural_420, natural_720,
            "content height should stay stable across wide transcript widths: content={content_height}, measured_420={natural_420}, measured_720={natural_720}, measured_narrow={natural_narrow}, column_width={COLUMN_MIN_WIDTH}"
        );
        assert_eq!(
            natural_narrow,
            content_height + scrollbar_height,
            "an underallocation should reserve only the scrollbar, not a large blank area: content={content_height}, measured_420={natural_420}, measured_720={natural_720}, measured_narrow={natural_narrow}, scrollbar_height={scrollbar_height}, column_width={COLUMN_MIN_WIDTH}"
        );
        assert!(
            content_height < 4000,
            "height exploded like the old wrapped scroller: content={content_height}, measured_420={natural_420}, measured_720={natural_720}, measured_narrow={natural_narrow}, column_width={COLUMN_MIN_WIDTH}"
        );
    }

    #[gtk::test]
    fn markdown_table_reports_shrinkable_minimum_width() {
        let table = MarkdownTable::new(
            &["A".to_string(), "B".to_string(), "C".to_string()],
            &[vec!["1".to_string(), "2".to_string(), "3".to_string()]],
            "",
        );

        let (minimum, natural, _min_baseline, _natural_baseline) =
            table.measure(gtk::Orientation::Horizontal, -1);

        assert_eq!(
            minimum, COLUMN_MIN_WIDTH,
            "widget must be shrinkable to one column so a narrow bubble can scroll it: minimum={minimum}, natural={natural}"
        );
        assert_eq!(
            natural,
            total_table_width(3),
            "natural width must stay the full fixed-column table width: minimum={minimum}, natural={natural}"
        );
        assert!(
            minimum < natural,
            "reporting the full width as the minimum would defeat the internal scroller: minimum={minimum}, natural={natural}"
        );
    }

    #[gtk::test]
    fn markdown_table_aggregates_search_match_count_across_cells() {
        let table = MarkdownTable::new(
            &["Rust".to_string(), "Notes".to_string()],
            &[
                vec!["Rust is great".to_string(), "no hit here".to_string()],
                vec!["more Rust".to_string(), "Rust again".to_string()],
            ],
            "Rust",
        );

        // "Rust" appears in the header cell and three body cells (once each).
        assert_eq!(
            table.match_count(),
            4,
            "table widget must expose the aggregate match count so wiring it in keeps table hits in the search total"
        );
    }

    #[gtk::test]
    fn markdown_table_reports_zero_matches_without_query() {
        let table = MarkdownTable::new(&["Rust".to_string()], &[vec!["Rust".to_string()]], "");

        assert_eq!(table.match_count(), 0);
    }

    #[gtk::test]
    fn markdown_table_clips_scrolled_cells_to_viewport() {
        let table = MarkdownTable::new(
            &["A".to_string(), "B".to_string(), "C".to_string()],
            &[vec!["1".to_string(), "2".to_string(), "3".to_string()]],
            "",
        );

        assert_eq!(
            table.overflow(),
            gtk::Overflow::Hidden,
            "scrolled-away columns must be clipped to the viewport, not painted over surrounding transcript content"
        );
    }

    #[gtk::test]
    fn markdown_table_scrolls_cells_but_keeps_separator_pinned() {
        let table = MarkdownTable::new(
            &["A".to_string(), "B".to_string(), "C".to_string()],
            &[vec!["1".to_string(), "2".to_string(), "3".to_string()]],
            "",
        );
        let narrow = COLUMN_MIN_WIDTH;
        let (_minimum, natural_height, _minimum_baseline, _natural_baseline) =
            table.measure(gtk::Orientation::Vertical, narrow);
        table.size_allocate(&gtk::Allocation::new(0, 0, narrow, natural_height), -1);

        let first_label = {
            let mut child = table.first_child();
            let mut found = None;
            while let Some(widget) = child {
                if widget.is::<gtk::Label>() {
                    found = Some(widget.clone());
                    break;
                }
                child = widget.next_sibling();
            }
            found.expect("table should have label children")
        };
        let before = first_label
            .compute_bounds(&table)
            .expect("label bounds")
            .x();
        let separator_before = table
            .separator()
            .compute_bounds(&table)
            .expect("separator bounds before scrolling");

        // Scrolling changes the adjustment value; the value-changed handler
        // must queue a fresh allocation so the cells shift left.
        table.adjustment().set_value(48.0);
        table.size_allocate(&gtk::Allocation::new(0, 0, narrow, natural_height), -1);
        let after = first_label
            .compute_bounds(&table)
            .expect("label bounds")
            .x();
        let separator_after = table
            .separator()
            .compute_bounds(&table)
            .expect("separator bounds after scrolling");

        assert!(
            after < before,
            "cells should shift left when the scroll value increases: before_x={before}, after_x={after}"
        );
        assert_eq!(separator_before.x(), 0.0);
        assert_eq!(separator_after.x(), 0.0);
        assert_eq!(separator_before.width(), narrow as f32);
        assert_eq!(separator_after.width(), narrow as f32);
    }
}
