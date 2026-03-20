use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use relm4::adw;
use relm4::gtk;
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

    let table_text = gtk::TextTag::new(Some("table-text"));
    table_text.set_family(Some("monospace"));
    table.add(&table_text);

    let table_header = gtk::TextTag::new(Some("table-header"));
    table_header.set_family(Some("monospace"));
    table_header.set_weight(700);
    table.add(&table_header);

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

/// Walks pulldown-cmark events and writes formatted text into a `TextBuffer`.
struct MarkdownBufferWriter<'a> {
    buffer: &'a gtk::TextBuffer,
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
    /// Deferred table widgets to attach after TextView creation.
    pending_table_widgets: Vec<(gtk::TextChildAnchor, gtk::Widget)>,
    /// Search match count found inside table widgets.
    table_match_count: usize,
}

impl<'a> MarkdownBufferWriter<'a> {
    fn new(buffer: &'a gtk::TextBuffer, highlight_query: Option<&str>) -> Self {
        Self {
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
            pending_table_widgets: Vec::new(),
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
            match event {
                // -- Inline tag starts --
                Event::Start(Tag::Emphasis) => self.tag_stack.push("italic"),
                Event::Start(Tag::Strong) => self.tag_stack.push("bold"),
                Event::Start(Tag::Strikethrough) => self.tag_stack.push("strikethrough"),

                // -- Inline tag ends --
                Event::End(TagEnd::Emphasis) => self.pop_tag("italic"),
                Event::End(TagEnd::Strong) => self.pop_tag("bold"),
                Event::End(TagEnd::Strikethrough) => self.pop_tag("strikethrough"),

                // -- Links --
                Event::Start(Tag::Link { dest_url, .. }) => {
                    self.link_url = Some(dest_url.to_string());
                }
                Event::End(TagEnd::Link) => {
                    if let Some(url) = self.link_url.take() {
                        let suffix = format!(" ({})", url);
                        if self.in_table {
                            self.inline_buf.push_str(&suffix);
                        } else {
                            let tags = self.active_tags();
                            self.insert_with_tags(&suffix, &tags);
                        }
                    }
                }

                // -- Images (rendered as [image: alt_text]) --
                Event::Start(Tag::Image { .. }) => {
                    self.in_image = true;
                    let tags = self.active_tags();
                    self.insert_with_tags("[image: ", &tags);
                }
                Event::End(TagEnd::Image) => {
                    self.in_image = false;
                    let tags = self.active_tags();
                    self.insert_with_tags("]", &tags);
                }

                // -- Paragraphs --
                Event::Start(Tag::Paragraph) => {
                    if self.list_stack.is_empty() && !self.in_table {
                        self.block_separator();
                    }
                }
                Event::End(TagEnd::Paragraph) => {
                    if self.list_stack.is_empty() && !self.in_table {
                        self.insert_with_tags("\n", &[]);
                        self.has_content = true;
                    }
                }

                // -- Headings --
                Event::Start(Tag::Heading { level, .. }) => {
                    self.block_separator();
                    let heading_tag = Self::heading_tag_name(level);
                    self.tag_stack.push(heading_tag);
                }
                Event::End(TagEnd::Heading(level)) => {
                    self.insert_with_tags("\n", &[]);
                    self.has_content = true;
                    self.pop_tag(Self::heading_tag_name(level));
                }

                // -- Code blocks --
                Event::Start(Tag::CodeBlock(kind)) => {
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
                Event::End(TagEnd::CodeBlock) => {
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

                // -- Lists --
                Event::Start(Tag::List(start)) => {
                    if self.list_stack.is_empty() {
                        self.block_separator();
                    } else {
                        // Nested list: start on a new line
                        self.insert_with_tags("\n", &[]);
                    }
                    self.list_stack.push((start.is_some(), 0, false));
                }
                Event::End(TagEnd::List(_)) => {
                    self.list_stack.pop();
                    if self.list_stack.is_empty() {
                        self.has_content = true;
                    }
                }
                Event::Start(Tag::Item) => {
                    if let Some(frame) = self.list_stack.last_mut() {
                        frame.1 += 1;
                        if frame.2 {
                            // Already known to be a task list — skip normal marker
                        } else {
                            // Defer marker: TaskListMarker may arrive before first text
                            self.pending_item_marker = true;
                        }
                    }
                }
                Event::End(TagEnd::Item) => {
                    self.flush_pending_marker();
                    self.insert_with_tags("\n", &[]);
                }

                Event::TaskListMarker(checked) => {
                    // Cancel the deferred normal marker — this is a task list
                    self.pending_item_marker = false;
                    if let Some(frame) = self.list_stack.last_mut() {
                        frame.2 = true; // mark as task list
                    }
                    self.current_task_checked = Some(checked);

                    // Insert styled checkbox symbol
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

                // -- Tables --
                Event::Start(Tag::Table(_)) => {
                    self.block_separator();
                    self.in_table = true;
                    self.table_headers.clear();
                    self.table_rows.clear();
                }
                Event::End(TagEnd::Table) => {
                    self.in_table = false;
                    self.render_table();
                    self.has_content = true;
                }
                Event::Start(Tag::TableHead) => {
                    self.in_table_head = true;
                    self.table_row.clear();
                }
                Event::End(TagEnd::TableHead) => {
                    self.table_headers = std::mem::take(&mut self.table_row);
                    self.in_table_head = false;
                }
                Event::Start(Tag::TableRow) => {
                    self.table_row.clear();
                }
                Event::End(TagEnd::TableRow) => {
                    if !self.in_table_head {
                        self.table_rows.push(std::mem::take(&mut self.table_row));
                    }
                }
                Event::Start(Tag::TableCell) => {
                    self.inline_buf.clear();
                }
                Event::End(TagEnd::TableCell) => {
                    self.table_row.push(std::mem::take(&mut self.inline_buf));
                }

                // -- Blockquotes --
                Event::Start(Tag::BlockQuote(_)) => {
                    self.blockquote_depth += 1;
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                }

                // -- Horizontal rule --
                Event::Rule => {
                    self.block_separator();
                    self.insert_with_tags("────────────────────────", &["horizontal-rule"]);
                    self.insert_with_tags("\n", &[]);
                    self.has_content = true;
                }

                // -- Text content --
                Event::Text(text) => {
                    if self.in_code_block.is_some() {
                        self.code_buf.push_str(&text);
                    } else if self.in_table {
                        self.inline_buf.push_str(&text);
                    } else {
                        self.emit_text(&text);
                    }
                }
                Event::Code(code) => {
                    if self.in_table {
                        self.inline_buf.push_str(&code);
                    } else {
                        self.flush_pending_marker();
                        let mut tags = self.active_tags();
                        tags.push("code-inline");
                        if !self.list_stack.is_empty() {
                            tags.push("list-item");
                        }
                        self.insert_with_tags(&code, &tags);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if self.in_code_block.is_some() {
                        self.code_buf.push('\n');
                    } else if self.in_table {
                        self.inline_buf.push('\n');
                    } else {
                        let tags = self.active_tags();
                        self.insert_with_tags("\n", &tags);
                    }
                }
                Event::Html(html) | Event::InlineHtml(html) => {
                    if self.in_code_block.is_some() {
                        self.code_buf.push_str(&html);
                    } else if self.in_table {
                        self.inline_buf.push_str(&html);
                    } else {
                        self.emit_text(&html);
                    }
                }

                // Intentionally ignored: FootnoteReference, MetadataBlock,
                // DefinitionList variants — these are either disabled in parser
                // options or have no meaningful visual representation.
                _ => {}
            }
        }
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

    fn create_table_label(text: &str, query: &str, is_header: bool) -> (gtk::Label, usize) {
        let (markup, match_count) = crate::ui::highlight::highlight_text(text, query);

        let label = gtk::Label::new(None);
        label.set_use_markup(true);
        label.set_markup(&markup);
        label.set_xalign(0.0);
        label.set_halign(gtk::Align::Start);
        label.add_css_class("markdown-table-cell");
        if is_header {
            label.add_css_class("markdown-table-header");
        }
        (label, match_count)
    }

    /// Defer table rendering by inserting a child anchor and table widget.
    fn render_table(&mut self) {
        if self.table_headers.is_empty() {
            return;
        }

        let mut end_iter = self.buffer.end_iter();
        let anchor = self.buffer.create_child_anchor(&mut end_iter);

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

        if self.blockquote_depth > 0 {
            grid.add_css_class("markdown-blockquote");
        }

        self.table_match_count += table_match_count;

        self.pending_table_widgets
            .push((anchor, grid.upcast::<gtk::Widget>()));
        self.insert_with_tags("\n", &[]);
    }
}

/// Render markdown content as a single selectable `gtk::TextView`.
///
/// If `highlight_query` is provided, matches are highlighted with a
/// background color. Returns the widget and the total number of matches.
pub fn render_markdown_to_textview(
    content: &str,
    highlight_query: Option<&str>,
) -> (gtk::TextView, usize) {
    let tag_table = create_tag_table();
    let buffer = gtk::TextBuffer::new(Some(&tag_table));

    let mut writer = MarkdownBufferWriter::new(&buffer, highlight_query);
    writer.process(content);
    let pending_table_widgets = std::mem::take(&mut writer.pending_table_widgets);
    let table_match_count = writer.table_match_count;

    // Apply search highlighting in a second pass.
    let buffer_match_count = apply_search_highlight(&buffer, highlight_query.unwrap_or(""));

    let view = gtk::TextView::with_buffer(&buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_hexpand(true);
    // Remove default TextView padding — the parent message-row provides padding
    view.set_top_margin(0);
    view.set_bottom_margin(0);
    view.set_left_margin(0);
    view.set_right_margin(0);
    // Make the TextView background transparent so the parent row's
    // background color shows through uniformly in both light and dark mode.
    view.add_css_class("markdown-textview");

    for (anchor, widget) in pending_table_widgets {
        view.add_child_at_anchor(&widget, &anchor);
    }

    let style_manager = adw::StyleManager::default();
    let buffer_weak = buffer.downgrade();
    let handler_id = style_manager.connect_dark_notify(move |manager| {
        if let Some(buffer) = buffer_weak.upgrade() {
            apply_theme_palette_to_tags(&buffer.tag_table(), manager.is_dark());
        }
    });
    let sm = style_manager;
    let handler_id = std::cell::Cell::new(Some(handler_id));
    view.connect_destroy(move |_| {
        if let Some(id) = handler_id.take() {
            sm.disconnect(id);
        }
    });

    (view, buffer_match_count + table_match_count)
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

    fn table_anchors(view: &gtk::TextView) -> Vec<gtk::TextChildAnchor> {
        let buffer = view.buffer();
        let mut iter = buffer.start_iter();
        let mut anchors = Vec::new();

        loop {
            if let Some(anchor) = iter.child_anchor() {
                anchors.push(anchor);
            }
            if !iter.forward_char() {
                break;
            }
        }

        anchors
    }

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

    fn attached_label_text(anchor: &gtk::TextChildAnchor) -> Vec<String> {
        let mut texts = Vec::new();
        for widget in anchor.widgets() {
            texts.extend(collect_label_text_from_widget_tree(&widget));
        }
        texts
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
        let (view, _) = render_markdown_to_textview(content, None);
        let buffer = view.buffer();
        let iter = buffer.iter_at_offset(char_offset);
        iter.tags()
            .iter()
            .any(|tag| tag.name().as_deref() == Some(tag_name))
    }

    /// Helper: extract plain text from a rendered textview.
    fn textview_text(content: &str) -> String {
        let (view, _) = render_markdown_to_textview(content, None);
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
        let (view, count) = render_markdown_to_textview("Hello world", Some("world"));
        assert_eq!(count, 1);
        let buf = view.buffer();
        // "world" starts at char 6 in "Hello world\n"
        let iter = buf.iter_at_offset(6);
        assert!(
            iter.tags()
                .iter()
                .any(|t| t.name().as_deref() == Some("search-highlight"))
        );
    }

    #[gtk::test]
    fn textview_search_no_match_returns_zero() {
        let (_, count) = render_markdown_to_textview("Hello world", Some("missing"));
        assert_eq!(count, 0);
    }

    // ── Tables ───────────────────────────────────────────────────────

    #[gtk::test]
    fn textview_table_creates_child_anchor() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (view, _) = render_markdown_to_textview(md, None);
        let anchors = table_anchors(&view);
        assert!(
            !anchors.is_empty(),
            "expected table to create at least one child anchor"
        );
    }

    #[gtk::test]
    fn textview_table_anchor_has_attached_widget() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (view, _) = render_markdown_to_textview(md, None);
        let anchors = table_anchors(&view);

        assert!(
            anchors.iter().any(|anchor| !anchor.widgets().is_empty()),
            "expected at least one table anchor with an attached widget"
        );
    }

    #[gtk::test]
    fn textview_table_search_count_includes_widget_cells() {
        let md = "| Name |\n|------|\n| Rust |";
        let (view, count) = render_markdown_to_textview(md, Some("Rust"));

        assert!(
            !table_anchors(&view).is_empty(),
            "expected table to render via child anchors"
        );
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
        let (view, _) = render_markdown_to_textview(md, None);
        let anchors = table_anchors(&view);
        let label_texts: Vec<String> = anchors.iter().flat_map(attached_label_text).collect();

        assert!(
            label_texts
                .iter()
                .any(|text| text.contains("Rust (https://rust-lang.org)")),
            "expected link text to be visible inside attached table widget labels, got: {label_texts:?}"
        );
    }

    // ── Blockquote table styling ──────────────────────────────────────

    #[gtk::test]
    fn textview_table_inside_blockquote_widget_has_blockquote_class() {
        let md = "> | A | B |\n> |---|---|\n> | 1 | 2 |";
        let (view, _) = render_markdown_to_textview(md, None);
        let anchors = table_anchors(&view);

        assert!(
            anchors.iter().any(|anchor| {
                anchor
                    .widgets()
                    .iter()
                    .any(|widget| widget_tree_has_css_class(widget, "markdown-blockquote"))
            }),
            "expected blockquote table widget tree to include a widget with the blockquote css class"
        );
    }

    // ── Table column structure ────────────────────────────────────────

    #[gtk::test]
    fn textview_table_two_columns_has_correct_labels() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (view, _) = render_markdown_to_textview(md, None);
        let anchors = table_anchors(&view);
        let label_texts: Vec<String> = anchors.iter().flat_map(attached_label_text).collect();

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
        // Ensure we have exactly 4 labels (2 headers + 2 cells), not counting separator
        let non_empty: Vec<_> = label_texts.iter().filter(|t| !t.is_empty()).collect();
        assert_eq!(
            non_empty.len(),
            4,
            "expected 4 labels (2 headers + 2 data cells), got: {non_empty:?}"
        );
    }

    /// Walk a widget tree and collect (column, row) for every gtk::Label
    /// that is a direct child of a Grid.
    fn grid_label_positions(anchor: &gtk::TextChildAnchor) -> Vec<(String, i32, i32)> {
        let mut results = Vec::new();
        for widget in anchor.widgets() {
            collect_grid_positions(&widget, &mut results);
        }
        results
    }

    fn collect_grid_positions(widget: &gtk::Widget, out: &mut Vec<(String, i32, i32)>) {
        if let Some(grid) = widget.parent().and_then(|p| p.downcast::<gtk::Grid>().ok()) {
            if let Ok(label) = widget.clone().downcast::<gtk::Label>() {
                let col = grid
                    .layout_manager()
                    .unwrap()
                    .layout_child(widget)
                    .downcast::<gtk::GridLayoutChild>()
                    .unwrap()
                    .column();
                let row = grid
                    .layout_manager()
                    .unwrap()
                    .layout_child(widget)
                    .downcast::<gtk::GridLayoutChild>()
                    .unwrap()
                    .row();
                out.push((label.text().to_string(), col, row));
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
        let (view, _) = render_markdown_to_textview(md, None);
        let anchors = table_anchors(&view);
        assert_eq!(anchors.len(), 1);
        let positions = grid_label_positions(&anchors[0]);
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
