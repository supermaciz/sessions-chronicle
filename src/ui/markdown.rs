use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use relm4::adw;
use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;
// Theme-dependent color palette (dark / light variants).
const DARK_CODE_BG: &str = "#2c2c2c";
const LIGHT_CODE_BG: &str = "#f4f4f4";
const DARK_DIM_FG: &str = "#aaaaaa";
const LIGHT_DIM_FG: &str = "#666666";
const DARK_CHECK_FG: &str = "#57e389";
const LIGHT_CHECK_FG: &str = "#2ec27e";

/// Escape characters that are special in Pango markup.
///
/// Used for User and ToolResult messages which still render via `gtk::Label`
/// with Pango markup. Assistant messages use `TextBuffer` + `TextTag`s instead.
pub fn pango_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Returns `true` when the current Adwaita color scheme is dark.
fn is_dark_mode() -> bool {
    adw::StyleManager::default().is_dark()
}

fn apply_theme_palette_to_tags(table: &gtk::TextTagTable, dark: bool) {
    let code_bg = if dark { DARK_CODE_BG } else { LIGHT_CODE_BG };
    let dim_fg = if dark { DARK_DIM_FG } else { LIGHT_DIM_FG };
    let check_fg = if dark { DARK_CHECK_FG } else { LIGHT_CHECK_FG };

    if let Some(tag) = table.lookup("code-block") {
        tag.set_paragraph_background(Some(code_bg));
    }
    if let Some(tag) = table.lookup("code-lang") {
        tag.set_foreground(Some(dim_fg));
    }
    if let Some(tag) = table.lookup("blockquote") {
        tag.set_foreground(Some(dim_fg));
    }
    if let Some(tag) = table.lookup("task-checked") {
        tag.set_foreground(Some(check_fg));
    }
    if let Some(tag) = table.lookup("task-unchecked") {
        tag.set_foreground(Some(dim_fg));
    }
    if let Some(tag) = table.lookup("horizontal-rule") {
        tag.set_foreground(Some(dim_fg));
    }
}

/// Create a `TextTagTable` with all markdown formatting tags.
///
/// Theme-dependent colors are updated both at creation time and when Adwaita
/// dark mode changes while the app is running.
fn create_tag_table() -> gtk::TextTagTable {
    let table = gtk::TextTagTable::new();

    // -- Inline formatting --
    let bold = gtk::TextTag::new(Some("bold"));
    bold.set_weight(700); // pango::Weight::Bold
    table.add(&bold);

    let italic = gtk::TextTag::new(Some("italic"));
    italic.set_style(gtk::pango::Style::Italic);
    table.add(&italic);

    let strikethrough = gtk::TextTag::new(Some("strikethrough"));
    strikethrough.set_strikethrough(true);
    table.add(&strikethrough);

    let code_inline = gtk::TextTag::new(Some("code-inline"));
    code_inline.set_family(Some("monospace"));
    table.add(&code_inline);

    // -- Headings --
    for (name, scale, above, below) in [
        ("heading-1", 1.6, 8, 4),
        ("heading-2", 1.4, 6, 3),
        ("heading-3", 1.2, 4, 2),
        ("heading-4", 1.1, 0, 0),
    ] {
        let tag = gtk::TextTag::new(Some(name));
        tag.set_scale(scale);
        tag.set_weight(700);
        if above > 0 {
            tag.set_pixels_above_lines(above);
        }
        if below > 0 {
            tag.set_pixels_below_lines(below);
        }
        table.add(&tag);
    }

    // -- Block-level --
    let code_block = gtk::TextTag::new(Some("code-block"));
    code_block.set_family(Some("monospace"));
    code_block.set_pixels_above_lines(0);
    code_block.set_pixels_below_lines(0);
    code_block.set_left_margin(12);
    code_block.set_right_margin(12);
    table.add(&code_block);

    let code_lang = gtk::TextTag::new(Some("code-lang"));
    code_lang.set_scale(0.85);
    table.add(&code_lang);

    let blockquote = gtk::TextTag::new(Some("blockquote"));
    blockquote.set_left_margin(16);
    table.add(&blockquote);

    let list_item = gtk::TextTag::new(Some("list-item"));
    list_item.set_left_margin(24);
    list_item.set_indent(-16);
    table.add(&list_item);

    // -- Task list checkboxes --
    let task_checked = gtk::TextTag::new(Some("task-checked"));
    table.add(&task_checked);

    let task_unchecked = gtk::TextTag::new(Some("task-unchecked"));
    table.add(&task_unchecked);

    // -- Search highlight --
    let highlight = gtk::TextTag::new(Some("search-highlight"));
    highlight.set_background(Some("#fce94f"));
    highlight.set_foreground(Some("#1e1e1e"));
    table.add(&highlight);

    // -- Horizontal rule --
    let hr = gtk::TextTag::new(Some("horizontal-rule"));
    hr.set_justification(gtk::Justification::Center);
    table.add(&hr);

    apply_theme_palette_to_tags(&table, is_dark_mode());

    table
}

/// A rendered segment: either styled text in a buffer or a table widget.
enum MarkdownSegment {
    Text(gtk::TextBuffer),
    Table(gtk::Widget),
}

/// Walks pulldown-cmark events and writes formatted text into a `TextBuffer`.
struct MarkdownBufferWriter {
    tag_table: gtk::TextTagTable,
    buffer: gtk::TextBuffer,
    /// Stack of active inline tag names (e.g. "bold", "italic").
    tag_stack: Vec<&'static str>,
    /// True when inside a code block — text goes verbatim, no inline tags.
    in_code_block: Option<Option<String>>,
    /// Code block accumulator.
    code_buf: String,
    /// List nesting stack: (ordered, item_index, is_task_list).
    list_stack: Vec<(bool, usize, bool)>,
    /// Current task-list checked state.
    current_task_checked: Option<bool>,
    /// Blockquote nesting depth.
    blockquote_depth: usize,
    /// Table state: headers collected, then rows.
    in_table: bool,
    in_table_head: bool,
    table_headers: Vec<String>,
    table_rows: Vec<Vec<String>>,
    table_row: Vec<String>,
    /// Inline text accumulator (used for table cells).
    inline_buf: String,
    /// Link URL being collected.
    link_url: Option<String>,
    /// True when inside an image — text events become alt text.
    in_image: bool,
    /// Whether any block has been written (for inter-block spacing).
    has_content: bool,
    /// Deferred item marker: true when we entered a list item but haven't
    /// yet decided whether it's a regular or task-list item.
    pending_item_marker: bool,
    /// Search query used for markdown highlight; table widgets will use this
    /// in a follow-up task.
    highlight_query: Option<String>,
    /// Completed segments (text buffers and table widgets) in order.
    segments: Vec<MarkdownSegment>,
    /// Search match count found inside table widgets.
    table_match_count: usize,
}

impl MarkdownBufferWriter {
    fn new(tag_table: gtk::TextTagTable, highlight_query: Option<&str>) -> Self {
        let buffer = gtk::TextBuffer::new(Some(&tag_table));
        Self {
            tag_table,
            buffer,
            tag_stack: Vec::new(),
            in_code_block: None,
            code_buf: String::new(),
            list_stack: Vec::new(),
            current_task_checked: None,
            blockquote_depth: 0,
            in_table: false,
            in_table_head: false,
            table_headers: Vec::new(),
            table_rows: Vec::new(),
            table_row: Vec::new(),
            inline_buf: String::new(),
            link_url: None,
            in_image: false,
            has_content: false,
            pending_item_marker: false,
            highlight_query: highlight_query.map(str::to_owned),
            segments: Vec::new(),
            table_match_count: 0,
        }
    }

    /// Insert text at the end of the buffer with the given tag names applied.
    fn insert_with_tags(&self, text: &str, tag_names: &[&str]) {
        if text.is_empty() {
            return;
        }
        let mut end_iter = self.buffer.end_iter();
        let start_offset = end_iter.offset();
        self.buffer.insert(&mut end_iter, text);
        let start_iter = self.buffer.iter_at_offset(start_offset);
        let end_iter = self.buffer.end_iter();
        for name in tag_names {
            if let Some(tag) = self.buffer.tag_table().lookup(name) {
                self.buffer.apply_tag(&tag, &start_iter, &end_iter);
            }
        }
    }

    /// Collect current active tags (inline + block context).
    fn active_tags(&self) -> Vec<&str> {
        let mut tags: Vec<&str> = self.tag_stack.clone();
        if self.blockquote_depth > 0 {
            tags.push("blockquote");
        }
        tags
    }

    /// Remove the last occurrence of `tag` from the tag stack (LIFO).
    fn pop_tag(&mut self, tag: &str) {
        if let Some(pos) = self.tag_stack.iter().rposition(|t| *t == tag) {
            self.tag_stack.remove(pos);
        }
    }

    /// Insert a newline to separate blocks (only if content has been written).
    fn block_separator(&mut self) {
        if self.has_content {
            self.insert_with_tags("\n", &[]);
        }
    }

    fn heading_tag_name(level: pulldown_cmark::HeadingLevel) -> &'static str {
        match level {
            pulldown_cmark::HeadingLevel::H1 => "heading-1",
            pulldown_cmark::HeadingLevel::H2 => "heading-2",
            pulldown_cmark::HeadingLevel::H3 => "heading-3",
            _ => "heading-4",
        }
    }

    /// Process all pulldown-cmark events from the given markdown content.
    fn process(&mut self, content: &str) {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);

        let parser = Parser::new_ext(content, options);

        for event in parser {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.handle_start_tag(tag),
            Event::End(tag_end) => self.handle_end_tag(tag_end),
            Event::TaskListMarker(checked) => self.handle_task_list_marker(checked),
            Event::Rule => {
                self.block_separator();
                self.insert_with_tags("────────────────────────", &["horizontal-rule"]);
                self.insert_with_tags("\n", &[]);
                self.has_content = true;
            }
            Event::Text(text) => self.push_text_content(&text),
            Event::Code(code) => self.push_inline_code(&code),
            Event::SoftBreak | Event::HardBreak => self.push_inline_break(),
            Event::Html(html) | Event::InlineHtml(html) => self.push_text_content(&html),

            // Intentionally ignored: FootnoteReference, MetadataBlock,
            // DefinitionList variants — these are either disabled in parser
            // options or have no meaningful visual representation.
            _ => {}
        }
    }

    fn handle_start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Emphasis => self.tag_stack.push("italic"),
            Tag::Strong => self.tag_stack.push("bold"),
            Tag::Strikethrough => self.tag_stack.push("strikethrough"),
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
            }
            Tag::Image { .. } => {
                self.in_image = true;
                self.write_inline_with_active_tags("[image: ");
            }
            Tag::Paragraph => {
                if self.list_stack.is_empty() && !self.in_table {
                    self.block_separator();
                }
            }
            Tag::Heading { level, .. } => {
                self.block_separator();
                let heading_tag = Self::heading_tag_name(level);
                self.tag_stack.push(heading_tag);
            }
            Tag::CodeBlock(kind) => self.start_code_block(kind),
            Tag::List(start) => {
                if self.list_stack.is_empty() {
                    self.block_separator();
                } else {
                    self.insert_with_tags("\n", &[]);
                }
                self.list_stack.push((start.is_some(), 0, false));
            }
            Tag::Item => {
                if let Some(frame) = self.list_stack.last_mut() {
                    frame.1 += 1;
                    if !frame.2 {
                        self.pending_item_marker = true;
                    }
                }
            }
            Tag::Table(_) => {
                self.block_separator();
                self.in_table = true;
                self.table_headers.clear();
                self.table_rows.clear();
            }
            Tag::TableHead => {
                self.in_table_head = true;
                self.table_row.clear();
            }
            Tag::TableRow => {
                self.table_row.clear();
            }
            Tag::TableCell => {
                self.inline_buf.clear();
            }
            Tag::BlockQuote(_) => {
                self.blockquote_depth += 1;
            }
            _ => {}
        }
    }

    fn handle_end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Emphasis => self.pop_tag("italic"),
            TagEnd::Strong => self.pop_tag("bold"),
            TagEnd::Strikethrough => self.pop_tag("strikethrough"),
            TagEnd::Link => {
                if let Some(url) = self.link_url.take() {
                    let suffix = format!(" ({})", url);
                    self.write_inline_with_active_tags(&suffix);
                }
            }
            TagEnd::Image => {
                self.in_image = false;
                self.write_inline_with_active_tags("]");
            }
            TagEnd::Paragraph => {
                if self.list_stack.is_empty() && !self.in_table {
                    self.insert_with_tags("\n", &[]);
                    self.has_content = true;
                }
            }
            TagEnd::Heading(level) => {
                self.insert_with_tags("\n", &[]);
                self.has_content = true;
                self.pop_tag(Self::heading_tag_name(level));
            }
            TagEnd::CodeBlock => self.finish_code_block(),
            TagEnd::List(_) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.has_content = true;
                }
            }
            TagEnd::Item => {
                self.flush_pending_marker();
                self.insert_with_tags("\n", &[]);
            }
            TagEnd::Table => {
                self.in_table = false;
                self.render_table();
                self.has_content = true;
            }
            TagEnd::TableHead => {
                self.table_headers = std::mem::take(&mut self.table_row);
                self.in_table_head = false;
            }
            TagEnd::TableRow => {
                if !self.in_table_head {
                    self.table_rows.push(std::mem::take(&mut self.table_row));
                }
            }
            TagEnd::TableCell => {
                self.table_row.push(std::mem::take(&mut self.inline_buf));
            }
            TagEnd::BlockQuote(_) => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>) {
        self.code_buf.clear();
        let language = match kind {
            CodeBlockKind::Fenced(info) => {
                let lang = info.trim().to_string();
                if lang.is_empty() { None } else { Some(lang) }
            }
            CodeBlockKind::Indented => None,
        };
        self.in_code_block = Some(language);
    }

    fn finish_code_block(&mut self) {
        self.block_separator();
        let language = self.in_code_block.take().flatten();
        let mut code_tags = vec!["code-block"];
        if self.blockquote_depth > 0 {
            code_tags.push("blockquote");
        }
        if let Some(ref lang) = language {
            let mut lang_tags = code_tags.clone();
            lang_tags.push("code-lang");
            self.insert_with_tags(lang, &lang_tags);
            self.insert_with_tags("\n", &code_tags);
        }
        let code = self.code_buf.trim_end_matches('\n').to_string();
        self.insert_with_tags(&code, &code_tags);
        self.insert_with_tags("\n", &[]);
        self.has_content = true;
    }

    fn handle_task_list_marker(&mut self, checked: bool) {
        self.pending_item_marker = false;
        if let Some(frame) = self.list_stack.last_mut() {
            frame.2 = true;
        }
        self.current_task_checked = Some(checked);

        let (symbol, style_tag) = if checked {
            ("\u{2611} ", "task-checked")
        } else {
            ("\u{2610} ", "task-unchecked")
        };
        let mut tags = self.active_tags();
        tags.push("list-item");
        tags.push(style_tag);
        self.insert_with_tags(symbol, &tags);
    }

    /// If a list-item marker was deferred, emit it now.
    fn flush_pending_marker(&mut self) {
        if self.pending_item_marker {
            self.pending_item_marker = false;
            if let Some(frame) = self.list_stack.last() {
                let marker = if frame.0 {
                    format!("{}. ", frame.1)
                } else {
                    "- ".to_string()
                };
                let mut tags = self.active_tags();
                tags.push("list-item");
                self.insert_with_tags(&marker, &tags);
            }
        }
    }

    /// Emit inline text with current formatting context.
    fn emit_text(&mut self, text: &str) {
        self.flush_pending_marker();
        let mut tags = self.active_tags();
        // If inside a list, add list-item tag for indentation
        if !self.list_stack.is_empty() {
            tags.push("list-item");
        }
        self.insert_with_tags(text, &tags);
    }

    fn write_inline_with_active_tags(&mut self, text: &str) {
        if self.in_table {
            self.inline_buf.push_str(text);
        } else {
            let tags = self.active_tags();
            self.insert_with_tags(text, &tags);
        }
    }

    fn push_text_content(&mut self, text: &str) {
        if self.in_code_block.is_some() {
            self.code_buf.push_str(text);
        } else if self.in_table {
            self.inline_buf.push_str(text);
        } else {
            self.emit_text(text);
        }
    }

    fn push_inline_code(&mut self, code: &str) {
        if self.in_table {
            self.inline_buf.push_str(code);
        } else {
            self.flush_pending_marker();
            let mut tags = self.active_tags();
            tags.push("code-inline");
            if !self.list_stack.is_empty() {
                tags.push("list-item");
            }
            self.insert_with_tags(code, &tags);
        }
    }

    fn push_inline_break(&mut self) {
        if self.in_code_block.is_some() {
            self.code_buf.push('\n');
        } else if self.in_table {
            self.inline_buf.push('\n');
        } else {
            self.write_inline_with_active_tags("\n");
        }
    }

    /// Minimum width (in characters) a table cell requests — prevents
    /// columns from collapsing to a single word.
    const TABLE_CELL_MIN_CHARS: i32 = 12;
    /// Maximum width (in characters) before a table cell wraps its text.
    const TABLE_CELL_MAX_CHARS: i32 = 50;

    fn create_table_label(text: &str, query: &str, is_header: bool) -> (gtk::Label, usize) {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_halign(gtk::Align::Start);
        label.set_width_chars(Self::TABLE_CELL_MIN_CHARS);
        label.set_max_width_chars(Self::TABLE_CELL_MAX_CHARS);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
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

    /// Flush the current buffer as a text segment and store the table widget.
    fn render_table(&mut self) {
        if self.table_headers.is_empty() {
            return;
        }

        // Flush current text buffer into a segment (if it has content).
        if self.buffer.char_count() > 0 {
            let old_buffer = std::mem::replace(
                &mut self.buffer,
                gtk::TextBuffer::new(Some(&self.tag_table)),
            );
            self.segments.push(MarkdownSegment::Text(old_buffer));
            self.has_content = false;
        }

        // Build the table grid.
        let grid = gtk::Grid::new();
        grid.set_hexpand(true);
        grid.add_css_class("markdown-table");
        grid.set_row_spacing(4);
        grid.set_column_spacing(12);
        let query = self.highlight_query.as_deref().unwrap_or("");
        let mut table_match_count = 0usize;

        for (col, header) in self.table_headers.iter().enumerate() {
            let (label, match_count) = Self::create_table_label(header, query, true);
            table_match_count += match_count;
            grid.attach(&label, col as i32, 0, 1, 1);
        }

        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.set_hexpand(true);
        grid.attach(&separator, 0, 1, self.table_headers.len() as i32, 1);

        for (row_idx, row) in self.table_rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let (label, match_count) = Self::create_table_label(cell, query, false);
                table_match_count += match_count;
                grid.attach(&label, col_idx as i32, row_idx as i32 + 2, 1, 1);
            }
        }

        // Wrap in ScrolledWindow for horizontal scrolling of wide tables.
        let table_widget = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&grid)
            .build();

        if self.blockquote_depth > 0 {
            table_widget.add_css_class("markdown-blockquote");
        }

        self.table_match_count += table_match_count;
        self.segments
            .push(MarkdownSegment::Table(table_widget.upcast::<gtk::Widget>()));
    }

    /// Finalize and return all segments plus table match count.
    fn finalize(mut self) -> (Vec<MarkdownSegment>, usize) {
        if self.buffer.char_count() > 0 {
            self.segments.push(MarkdownSegment::Text(self.buffer));
        }
        (self.segments, self.table_match_count)
    }
}

/// Connect a `destroy` signal on `widget` that disconnects the given
/// `StyleManager` signal handler, preventing leaked theme-change callbacks.
fn attach_theme_cleanup(
    widget: &impl IsA<gtk::Widget>,
    sm: adw::StyleManager,
    handler: glib::SignalHandlerId,
) {
    let handler_id = std::cell::Cell::new(Some(handler));
    widget.connect_destroy(move |_| {
        if let Some(id) = handler_id.take() {
            sm.disconnect(id);
        }
    });
}

/// Create a non-editable, transparent `gtk::TextView` from a buffer.
fn make_textview(buffer: &gtk::TextBuffer) -> gtk::TextView {
    let view = gtk::TextView::with_buffer(buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_hexpand(true);
    view.set_top_margin(0);
    view.set_bottom_margin(0);
    view.set_left_margin(0);
    view.set_right_margin(0);
    view.add_css_class("markdown-textview");
    view
}

/// Render markdown content into a widget (single `TextView` or a `Box`
/// containing `TextView` segments interleaved with table widgets).
///
/// If `highlight_query` is provided, matches are highlighted with a
/// background color. Returns the widget and the total number of matches.
pub fn render_markdown_to_textview(
    content: &str,
    highlight_query: Option<&str>,
) -> (gtk::Widget, usize) {
    let tag_table = create_tag_table();
    let query = highlight_query.unwrap_or("");

    let mut writer = MarkdownBufferWriter::new(tag_table.clone(), highlight_query);
    writer.process(content);
    let (segments, table_match_count) = writer.finalize();

    // Wire up theme-change listener that updates tag colours. Since all
    // buffers share the same tag_table, one handler covers everything.
    let style_manager = adw::StyleManager::default();
    let tt_weak = tag_table.downgrade();
    let theme_handler = style_manager.connect_dark_notify(move |manager| {
        if let Some(tt) = tt_weak.upgrade() {
            apply_theme_palette_to_tags(&tt, manager.is_dark());
        }
    });

    let has_tables = segments
        .iter()
        .any(|s| matches!(s, MarkdownSegment::Table(_)));

    // Fast path: no tables — return a single TextView (common case).
    if !has_tables {
        let mut match_count = table_match_count;
        // All segments are Text; in practice there's exactly one.
        if let Some(MarkdownSegment::Text(buffer)) = segments.into_iter().next() {
            match_count += apply_search_highlight(&buffer, query);
            let view = make_textview(&buffer);
            attach_theme_cleanup(&view, style_manager, theme_handler);
            return (view.upcast(), match_count);
        }
        // Empty content — return an empty widget.
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 0);
        attach_theme_cleanup(&empty, style_manager, theme_handler);
        return (empty.upcast(), match_count);
    }

    // Multiple segments with tables: build a vertical Box.
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let mut total_matches = table_match_count;

    for segment in segments {
        match segment {
            MarkdownSegment::Text(buffer) => {
                total_matches += apply_search_highlight(&buffer, query);
                let view = make_textview(&buffer);
                container.append(&view);
            }
            MarkdownSegment::Table(widget) => {
                container.append(&widget);
            }
        }
    }

    attach_theme_cleanup(&container, style_manager, theme_handler);

    (container.upcast(), total_matches)
}

/// Find and highlight all case-insensitive matches of `query` in the buffer.
///
/// Uses the `search-highlight` tag from the buffer's tag table.
/// Returns the number of matches found.
fn apply_search_highlight(buffer: &gtk::TextBuffer, query: &str) -> usize {
    if query.is_empty() {
        return 0;
    }

    let start = buffer.start_iter();
    let end = buffer.end_iter();
    let text = buffer.slice(&start, &end, false);
    let text_str = text.as_str();

    let matches = crate::ui::highlight::find_case_insensitive_matches_in_text(text_str, query);
    let count = matches.len();

    if count == 0 {
        return 0;
    }

    // Build a byte-offset → char-offset map in a single forward pass (O(n)).
    // Collect only the byte offsets we need, then scan once.
    let mut needed_offsets: Vec<usize> = Vec::with_capacity(matches.len() * 2);
    for (bs, be) in &matches {
        needed_offsets.push(*bs);
        needed_offsets.push(*be);
    }
    needed_offsets.sort_unstable();
    needed_offsets.dedup();

    // Single forward scan converting byte offsets to char offsets
    let mut byte_to_char: std::collections::HashMap<usize, i32> =
        std::collections::HashMap::with_capacity(needed_offsets.len());
    let mut offset_idx = 0;
    for (char_count, (byte_pos, _)) in (0_i32..).zip(text_str.char_indices()) {
        while offset_idx < needed_offsets.len() && needed_offsets[offset_idx] == byte_pos {
            byte_to_char.insert(byte_pos, char_count);
            offset_idx += 1;
        }
        if offset_idx >= needed_offsets.len() {
            break;
        }
    }
    // Handle offsets at the very end of the string
    let total_chars = text_str.chars().count() as i32;
    let text_len = text_str.len();
    while offset_idx < needed_offsets.len() {
        byte_to_char.insert(needed_offsets[offset_idx], total_chars);
        // Only the end-of-string offset should land here
        debug_assert_eq!(needed_offsets[offset_idx], text_len);
        offset_idx += 1;
    }

    for (byte_start, byte_end) in &matches {
        let char_start = byte_to_char[byte_start];
        let char_end = byte_to_char[byte_end];
        let iter_start = buffer.iter_at_offset(char_start);
        let iter_end = buffer.iter_at_offset(char_end);
        buffer.apply_tag_by_name("search-highlight", &iter_start, &iter_end);
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Downcast the rendered widget to a single TextView (for non-table content).
    fn as_textview(widget: &gtk::Widget) -> gtk::TextView {
        widget
            .clone()
            .downcast::<gtk::TextView>()
            .expect("expected a single TextView (no tables)")
    }

    /// Collect all label texts from a widget tree (recursive).
    fn collect_label_text_from_widget_tree(widget: &gtk::Widget) -> Vec<String> {
        let mut texts = Vec::new();

        if let Ok(label) = widget.clone().downcast::<gtk::Label>() {
            texts.push(label.text().to_string());
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            texts.extend(collect_label_text_from_widget_tree(&child_widget));
            child = child_widget.next_sibling();
        }

        texts
    }

    /// Collect all table widgets (ScrolledWindows containing Grids) from
    /// the rendered output. For the Box-based layout, these are direct
    /// children that are ScrolledWindows.
    fn find_table_widgets(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut tables = Vec::new();
        let mut child = widget.first_child();
        while let Some(c) = child {
            if c.clone().downcast::<gtk::ScrolledWindow>().is_ok() {
                tables.push(c.clone());
            }
            child = c.next_sibling();
        }
        tables
    }

    /// Collect label texts from all table widgets in the rendered output.
    fn table_label_texts(widget: &gtk::Widget) -> Vec<String> {
        find_table_widgets(widget)
            .iter()
            .flat_map(collect_label_text_from_widget_tree)
            .collect()
    }

    fn widget_tree_has_css_class(widget: &gtk::Widget, class_name: &str) -> bool {
        if widget.has_css_class(class_name) {
            return true;
        }

        let mut child = widget.first_child();
        while let Some(child_widget) = child {
            if widget_tree_has_css_class(&child_widget, class_name) {
                return true;
            }
            child = child_widget.next_sibling();
        }

        false
    }

    fn has_tag_at(content: &str, tag_name: &str, char_offset: i32) -> bool {
        let (widget, _) = render_markdown_to_textview(content, None);
        let view = as_textview(&widget);
        let buffer = view.buffer();
        let iter = buffer.iter_at_offset(char_offset);
        iter.tags()
            .iter()
            .any(|tag: &gtk::TextTag| tag.name().as_deref() == Some(tag_name))
    }

    /// Helper: extract plain text from a rendered textview.
    fn textview_text(content: &str) -> String {
        let (widget, _) = render_markdown_to_textview(content, None);
        let view = as_textview(&widget);
        let buf = view.buffer();
        buf.text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string()
    }

    // ── Existing regression tests ────────────────────────────────────

    #[gtk::test]
    fn code_block_language_line_uses_code_block_tag() {
        let markdown = "```rust\nfn main() {}\n```";
        assert!(has_tag_at(markdown, "code-lang", 0));
        assert!(has_tag_at(markdown, "code-block", 0));
    }

    #[gtk::test]
    fn code_block_inside_blockquote_uses_blockquote_tag() {
        let markdown = "> ```rust\n> fn main() {}\n> ```";
        assert!(has_tag_at(markdown, "blockquote", 0));
        assert!(has_tag_at(markdown, "code-block", 0));
    }

    // ── Plain text & paragraphs ──────────────────────────────────────

    #[gtk::test]
    fn textview_plain_paragraph() {
        let text = textview_text("Hello world");
        assert!(text.contains("Hello world"), "got: {text}");
    }

    // ── Inline formatting ────────────────────────────────────────────

    #[gtk::test]
    fn textview_bold_tagged() {
        assert!(has_tag_at("Hello **bold** world", "bold", 6));
    }

    #[gtk::test]
    fn textview_italic_tagged() {
        assert!(has_tag_at("Hello *italic* world", "italic", 6));
    }

    #[gtk::test]
    fn textview_strikethrough_tagged() {
        assert!(has_tag_at("Hello ~~removed~~ world", "strikethrough", 6));
    }

    #[gtk::test]
    fn textview_code_inline_tagged() {
        assert!(has_tag_at("Use `code` here", "code-inline", 4));
    }

    // ── Headings ─────────────────────────────────────────────────────

    #[gtk::test]
    fn textview_heading_1_tagged() {
        assert!(has_tag_at("# Title", "heading-1", 0));
    }

    #[gtk::test]
    fn textview_heading_2_tagged() {
        assert!(has_tag_at("## Subtitle", "heading-2", 0));
    }

    // ── Lists ────────────────────────────────────────────────────────

    #[gtk::test]
    fn textview_unordered_list_contains_marker() {
        let text = textview_text("- First\n- Second");
        assert!(text.contains("- First"), "got: {text}");
        assert!(text.contains("- Second"), "got: {text}");
    }

    #[gtk::test]
    fn textview_ordered_list_contains_numbers() {
        let text = textview_text("1. Alpha\n2. Beta");
        assert!(text.contains("1."), "got: {text}");
        assert!(text.contains("2."), "got: {text}");
    }

    #[gtk::test]
    fn textview_task_list_contains_checkboxes() {
        let text = textview_text("- [x] Done\n- [ ] Todo");
        // U+2611 (checked) and U+2610 (unchecked)
        assert!(text.contains('\u{2611}'), "got: {text}");
        assert!(text.contains('\u{2610}'), "got: {text}");
    }

    // ── Code blocks ──────────────────────────────────────────────────

    #[gtk::test]
    fn textview_code_block_tagged() {
        let text = textview_text("```\ncode line\n```");
        assert!(text.contains("code line"), "got: {text}");
        assert!(has_tag_at("```\ncode line\n```", "code-block", 0));
    }

    // ── Search highlighting ──────────────────────────────────────────

    #[gtk::test]
    fn textview_search_highlight_applied() {
        let (widget, count) = render_markdown_to_textview("Hello world", Some("world"));
        assert_eq!(count, 1);
        let view = as_textview(&widget);
        let buf = view.buffer();
        // "world" starts at char 6 in "Hello world\n"
        let iter = buf.iter_at_offset(6);
        assert!(
            iter.tags()
                .iter()
                .any(|t: &gtk::TextTag| t.name().as_deref() == Some("search-highlight"))
        );
    }

    #[gtk::test]
    fn textview_search_no_match_returns_zero() {
        let (_, count) = render_markdown_to_textview("Hello world", Some("missing"));
        assert_eq!(count, 0);
    }

    // ── Tables ───────────────────────────────────────────────────────

    #[gtk::test]
    fn textview_table_renders_as_separate_widget() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown_to_textview(md, None);
        let tables = find_table_widgets(&widget);
        assert!(
            !tables.is_empty(),
            "expected table to produce a ScrolledWindow in the output"
        );
    }

    #[gtk::test]
    fn textview_table_contains_labels() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown_to_textview(md, None);
        let labels = table_label_texts(&widget);
        assert!(
            !labels.is_empty(),
            "expected table widget to contain labels"
        );
    }

    #[gtk::test]
    fn textview_table_search_count_includes_widget_cells() {
        let md = "| Name |\n|------|\n| Rust |";
        let (_, count) = render_markdown_to_textview(md, Some("Rust"));
        assert_eq!(count, 1, "expected search to include widget cell content");
    }

    // ── Horizontal rule ──────────────────────────────────────────────

    #[gtk::test]
    fn textview_horizontal_rule() {
        let text = textview_text("Above\n\n---\n\nBelow");
        assert!(text.contains("────"), "got: {text}");
    }

    // ── Images ───────────────────────────────────────────────────────

    #[gtk::test]
    fn textview_image_renders_alt_text() {
        let text = textview_text("![screenshot](https://example.com/img.png)");
        assert!(text.contains("[image: screenshot]"), "got: {text}");
    }

    // ── Blockquotes ──────────────────────────────────────────────────

    #[gtk::test]
    fn textview_blockquote_tagged() {
        assert!(has_tag_at("> Quoted text", "blockquote", 0));
    }

    // ── Nested inline formatting ─────────────────────────────────────

    #[gtk::test]
    fn textview_nested_bold_italic() {
        // "Hello " (6 chars) then "both" has bold+italic
        assert!(has_tag_at("Hello ***both*** world", "bold", 6));
        assert!(has_tag_at("Hello ***both*** world", "italic", 6));
    }

    // ── Link inside table cell ────────────────────────────────────────

    #[gtk::test]
    fn textview_table_link_visible_inside_widget_cell() {
        let md = "| Name |\n|------|\n| [Rust](https://rust-lang.org) |";
        let (widget, _) = render_markdown_to_textview(md, None);
        let label_texts = table_label_texts(&widget);

        assert!(
            label_texts
                .iter()
                .any(|text| text.contains("Rust (https://rust-lang.org)")),
            "expected link text to be visible inside table widget labels, got: {label_texts:?}"
        );
    }

    #[gtk::test]
    fn textview_table_image_visible_inside_widget_cell() {
        let md = "| Screenshot |\n|------------|\n| ![Session List](docs/screenshots/session_list.png) |";
        let (widget, _) = render_markdown_to_textview(md, None);
        let label_texts = table_label_texts(&widget);

        assert!(
            label_texts
                .iter()
                .any(|text| text.contains("[image: Session List]")),
            "expected image alt text placeholder inside table widget labels, got: {label_texts:?}"
        );
    }

    // ── Blockquote table styling ──────────────────────────────────────

    #[gtk::test]
    fn textview_table_inside_blockquote_widget_has_blockquote_class() {
        let md = "> | A | B |\n> |---|---|\n> | 1 | 2 |";
        let (widget, _) = render_markdown_to_textview(md, None);

        assert!(
            find_table_widgets(&widget)
                .iter()
                .any(|w| widget_tree_has_css_class(w, "markdown-blockquote")),
            "expected blockquote table widget tree to include a widget with the blockquote css class"
        );
    }

    // ── Table column structure ────────────────────────────────────────

    #[gtk::test]
    fn textview_table_two_columns_has_correct_labels() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown_to_textview(md, None);
        let label_texts = table_label_texts(&widget);

        assert!(
            label_texts.contains(&"A".to_string()),
            "expected header 'A' in labels, got: {label_texts:?}"
        );
        assert!(
            label_texts.contains(&"B".to_string()),
            "expected header 'B' in labels, got: {label_texts:?}"
        );
        assert!(
            label_texts.contains(&"1".to_string()),
            "expected cell '1' in labels, got: {label_texts:?}"
        );
        assert!(
            label_texts.contains(&"2".to_string()),
            "expected cell '2' in labels, got: {label_texts:?}"
        );
        let non_empty: Vec<_> = label_texts.iter().filter(|t| !t.is_empty()).collect();
        assert_eq!(
            non_empty.len(),
            4,
            "expected 4 labels (2 headers + 2 data cells), got: {non_empty:?}"
        );
    }

    fn collect_grid_positions(widget: &gtk::Widget, out: &mut Vec<(String, i32, i32)>) {
        if let Some(grid) = widget.parent().and_then(|p| p.downcast::<gtk::Grid>().ok()) {
            if let Ok(label) = widget.clone().downcast::<gtk::Label>() {
                let layout_child = grid
                    .layout_manager()
                    .unwrap()
                    .layout_child(widget)
                    .downcast::<gtk::GridLayoutChild>()
                    .unwrap();
                out.push((
                    label.text().to_string(),
                    layout_child.column(),
                    layout_child.row(),
                ));
            }
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            collect_grid_positions(&c, out);
            child = c.next_sibling();
        }
    }

    #[gtk::test]
    fn textview_table_grid_positions_correct() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown_to_textview(md, None);
        let tables = find_table_widgets(&widget);
        assert_eq!(tables.len(), 1);
        let mut positions = Vec::new();
        collect_grid_positions(&tables[0], &mut positions);
        // Should have: A at (0,0), B at (1,0), 1 at (0,2), 2 at (1,2)
        assert!(
            positions.contains(&("A".to_string(), 0, 0)),
            "expected A at (0,0), got: {positions:?}"
        );
        assert!(
            positions.contains(&("B".to_string(), 1, 0)),
            "expected B at (1,0), got: {positions:?}"
        );
        assert!(
            positions.contains(&("1".to_string(), 0, 2)),
            "expected 1 at (0,2), got: {positions:?}"
        );
        assert!(
            positions.contains(&("2".to_string(), 1, 2)),
            "expected 2 at (1,2), got: {positions:?}"
        );
    }

    // ── Nested lists ──────────────────────────────────────────────────

    #[gtk::test]
    fn textview_nested_unordered_list() {
        let md = "- Parent item\n  - Child item 1\n  - Child item 2\n- Second parent";
        let text = textview_text(md);
        assert!(text.contains("Parent item"), "got: {text}");
        assert!(text.contains("Child item 1"), "got: {text}");
        assert!(text.contains("Child item 2"), "got: {text}");
        assert!(text.contains("Second parent"), "got: {text}");
    }

    #[gtk::test]
    fn textview_loose_list_items_kept_together() {
        // Loose lists have blank lines between items; pulldown-cmark wraps
        // each item in Paragraph events. All items must still appear.
        let md = "- First item\n\n- Second item\n\n- Third item";
        let text = textview_text(md);
        assert!(text.contains("First item"), "got: {text}");
        assert!(text.contains("Second item"), "got: {text}");
        assert!(text.contains("Third item"), "got: {text}");
    }

    // ── Theme palette ───────────────────────────────────────────────

    #[gtk::test]
    fn theme_palette_update_refreshes_existing_tags() {
        let table = create_tag_table();

        apply_theme_palette_to_tags(&table, false);
        let light_code_bg = table
            .lookup("code-block")
            .expect("code-block tag exists")
            .paragraph_background_rgba();

        apply_theme_palette_to_tags(&table, true);
        let dark_code_bg = table
            .lookup("code-block")
            .expect("code-block tag exists")
            .paragraph_background_rgba();

        assert_ne!(light_code_bg, dark_code_bg);
    }
}
