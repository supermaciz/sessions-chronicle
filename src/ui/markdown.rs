use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use relm4::adw;
use relm4::gtk;
use relm4::gtk::prelude::*;

/// Escape characters that are special in Pango markup.
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

/// Create a `TextTagTable` with all markdown formatting tags.
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
    let code_bg = if is_dark_mode() { "#2c2c2c" } else { "#f4f4f4" };
    code_block.set_paragraph_background(Some(code_bg));
    code_block.set_pixels_above_lines(4);
    code_block.set_pixels_below_lines(4);
    code_block.set_left_margin(12);
    code_block.set_right_margin(12);
    table.add(&code_block);

    let code_lang = gtk::TextTag::new(Some("code-lang"));
    code_lang.set_scale(0.85);
    code_lang.set_foreground(Some("#888888"));
    table.add(&code_lang);

    let blockquote = gtk::TextTag::new(Some("blockquote"));
    blockquote.set_left_margin(16);
    blockquote.set_foreground(Some("#888888"));
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
    task_checked.set_foreground(Some("#2ec27e")); // GNOME green
    table.add(&task_checked);

    let task_unchecked = gtk::TextTag::new(Some("task-unchecked"));
    task_unchecked.set_foreground(Some("#888888"));
    table.add(&task_unchecked);

    // -- Search highlight --
    let highlight = gtk::TextTag::new(Some("search-highlight"));
    highlight.set_background(Some("#fce94f"));
    highlight.set_foreground(Some("#1e1e1e"));
    table.add(&highlight);

    // -- Horizontal rule --
    let hr = gtk::TextTag::new(Some("horizontal-rule"));
    hr.set_foreground(Some("#888888"));
    hr.set_justification(gtk::Justification::Center);
    table.add(&hr);

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
    /// Whether any block has been written (for inter-block spacing).
    has_content: bool,
    /// Deferred item marker: true when we entered a list item but haven't
    /// yet decided whether it's a regular or task-list item.
    pending_item_marker: bool,
}

impl<'a> MarkdownBufferWriter<'a> {
    fn new(buffer: &'a gtk::TextBuffer) -> Self {
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
            has_content: false,
            pending_item_marker: false,
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
                Event::End(TagEnd::Emphasis) => {
                    self.tag_stack.retain(|t| *t != "italic");
                }
                Event::End(TagEnd::Strong) => {
                    self.tag_stack.retain(|t| *t != "bold");
                }
                Event::End(TagEnd::Strikethrough) => {
                    self.tag_stack.retain(|t| *t != "strikethrough");
                }

                // -- Links --
                Event::Start(Tag::Link { dest_url, .. }) => {
                    self.link_url = Some(dest_url.to_string());
                }
                Event::End(TagEnd::Link) => {
                    if let Some(url) = self.link_url.take() {
                        let tags = self.active_tags();
                        let tag_refs: Vec<&str> = tags.to_vec();
                        self.insert_with_tags(&format!(" ({})", url), &tag_refs);
                    }
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
                    let heading_tag = Self::heading_tag_name(level);
                    self.tag_stack.retain(|t| *t != heading_tag);
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
                    if let Some(ref lang) = language {
                        self.insert_with_tags(lang, &["code-lang"]);
                        self.insert_with_tags("\n", &[]);
                    }
                    let code = self.code_buf.trim_end_matches('\n').to_string();
                    self.insert_with_tags(&code, &["code-block"]);
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
                    let tag_refs: Vec<&str> = tags.to_vec();
                    self.insert_with_tags(symbol, &tag_refs);
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
                        let tag_refs: Vec<&str> = tags.to_vec();
                        self.insert_with_tags(&code, &tag_refs);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if self.in_code_block.is_some() {
                        self.code_buf.push('\n');
                    } else if self.in_table {
                        self.inline_buf.push('\n');
                    } else {
                        let tags = self.active_tags();
                        let tag_refs: Vec<&str> = tags.to_vec();
                        self.insert_with_tags("\n", &tag_refs);
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
                let tag_refs: Vec<&str> = tags.to_vec();
                self.insert_with_tags(&marker, &tag_refs);
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
        let tag_refs: Vec<&str> = tags.to_vec();
        self.insert_with_tags(text, &tag_refs);
    }

    /// Render collected table data as monospace-aligned text.
    fn render_table(&mut self) {
        if self.table_headers.is_empty() {
            return;
        }

        let num_cols = self.table_headers.len();

        // Calculate column widths
        let mut col_widths: Vec<usize> = self.table_headers.iter().map(|h| h.len()).collect();
        for row in &self.table_rows {
            for (i, cell) in row.iter().enumerate() {
                if i < num_cols {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Render header row
        let header_line: String = self
            .table_headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:<width$}", h, width = col_widths[i]))
            .collect::<Vec<_>>()
            .join("  ");
        self.insert_with_tags(&header_line, &["table-header"]);
        self.insert_with_tags("\n", &[]);

        // Render separator
        let sep_line: String = col_widths
            .iter()
            .map(|w| "─".repeat(*w))
            .collect::<Vec<_>>()
            .join("  ");
        self.insert_with_tags(&sep_line, &["table-text"]);
        self.insert_with_tags("\n", &[]);

        // Render data rows
        for row in &self.table_rows {
            let row_line: String = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let width = col_widths.get(i).copied().unwrap_or(cell.len());
                    format!("{:<width$}", cell, width = width)
                })
                .collect::<Vec<_>>()
                .join("  ");
            self.insert_with_tags(&row_line, &["table-text"]);
            self.insert_with_tags("\n", &[]);
        }
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

    let mut writer = MarkdownBufferWriter::new(&buffer);
    writer.process(content);

    // Apply search highlighting in a second pass
    let match_count = if let Some(query) = highlight_query {
        apply_search_highlight(&buffer, query)
    } else {
        0
    };

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

    (view, match_count)
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
    let text = buffer.text(&start, &end, false);
    let text_str = text.as_str();

    let matches = crate::ui::highlight::find_case_insensitive_matches_in_text(text_str, query);
    let count = matches.len();

    if count == 0 {
        return 0;
    }

    // The match positions are byte offsets into the plain text string.
    // TextBuffer works with char offsets, so convert.
    for (byte_start, byte_end) in &matches {
        let char_start = text_str[..*byte_start].chars().count() as i32;
        let char_end = text_str[..*byte_end].chars().count() as i32;
        let iter_start = buffer.iter_at_offset(char_start);
        let iter_end = buffer.iter_at_offset(char_end);
        buffer.apply_tag_by_name("search-highlight", &iter_start, &iter_end);
    }

    count
}
