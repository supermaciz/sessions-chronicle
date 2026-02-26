use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use relm4::gtk;
use relm4::gtk::prelude::*;

/// Intermediate representation of a parsed markdown block.
/// Used by `render_markdown()` to produce GTK widgets, and directly testable.
#[derive(Debug, Clone)]
pub enum MarkdownBlock {
    Paragraph(String),
    Heading {
        level: u8,
        content: String,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    List {
        ordered: bool,
        items: Vec<String>,
    },
    TaskList(Vec<(bool, String)>),
    Blockquote(Vec<MarkdownBlock>),
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    HorizontalRule,
}

/// Per-level state for the list stack used in `markdown_to_blocks`.
struct ListFrame {
    ordered: bool,
    is_task_list: bool,
    current_task_checked: Option<bool>,
    items: Vec<String>,
    task_items: Vec<(bool, String)>,
    /// The `inline_buf` content of the parent item at the moment this nested
    /// list started. Restored when the list ends.
    parent_item_buf: String,
}

/// Render a nested list as indented plain text appended to the parent item's
/// Pango-markup string.
fn format_nested_list_as_text(frame: &ListFrame) -> String {
    let mut result = String::new();
    if frame.is_task_list {
        for (checked, text) in &frame.task_items {
            result.push('\n');
            result.push_str(if *checked { "  [x] " } else { "  [ ] " });
            result.push_str(text);
        }
    } else {
        for (i, item) in frame.items.iter().enumerate() {
            result.push('\n');
            if frame.ordered {
                result.push_str(&format!("  {}. ", i + 1));
            } else {
                result.push_str("  - ");
            }
            result.push_str(item);
        }
    }
    result
}

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

/// Parse markdown into intermediate blocks with Pango-markup strings.
///
/// # Known Limitations
///
/// - **Nested blockquotes** are not fully supported. When a blockquote contains
///   another blockquote (`> outer\n>\n> > inner`), only the innermost quote
///   content is preserved. This is due to the single-level `in_blockquote` flag
///   and `blockquote_blocks` buffer being cleared on each new quote start.
///   In practice, Claude sessions rarely contain nested blockquotes, so this
///   limitation has minimal impact.
pub fn markdown_to_blocks(content: &str) -> Vec<MarkdownBlock> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(content, options);
    let mut blocks = Vec::new();
    let mut inline_buf = String::new();

    let mut in_code_block: Option<Option<String>> = None;
    let mut code_buf = String::new();
    let mut list_stack: Vec<ListFrame> = Vec::new();
    let mut in_blockquote = false;
    let mut blockquote_blocks: Vec<MarkdownBlock> = Vec::new();
    let mut table_headers: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row: Vec<String> = Vec::new();
    let mut in_table_head = false;
    let mut link_url: Option<String> = None;

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => {
                inline_buf.clear();
            }
            Event::Start(Tag::Heading { .. }) => {
                inline_buf.clear();
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                code_buf.clear();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let language = info.trim().to_string();
                        if language.is_empty() {
                            None
                        } else {
                            Some(language)
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                in_code_block = Some(language);
            }
            Event::Start(Tag::List(start)) => {
                let parent_item_buf = std::mem::take(&mut inline_buf);
                list_stack.push(ListFrame {
                    ordered: start.is_some(),
                    is_task_list: false,
                    current_task_checked: None,
                    items: Vec::new(),
                    task_items: Vec::new(),
                    parent_item_buf,
                });
            }
            Event::Start(Tag::Item) => {
                inline_buf.clear();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                in_blockquote = true;
                blockquote_blocks.clear();
            }
            Event::Start(Tag::Table(_)) => {
                table_headers.clear();
                table_rows.clear();
            }
            Event::Start(Tag::TableHead) => {
                in_table_head = true;
                table_row.clear();
            }
            Event::Start(Tag::TableRow) => {
                table_row.clear();
            }
            Event::Start(Tag::TableCell) => {
                inline_buf.clear();
            }
            Event::Start(Tag::Emphasis) => inline_buf.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => inline_buf.push_str("</i>"),
            Event::Start(Tag::Strong) => inline_buf.push_str("<b>"),
            Event::End(TagEnd::Strong) => inline_buf.push_str("</b>"),
            Event::Start(Tag::Strikethrough) => inline_buf.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => inline_buf.push_str("</s>"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_url = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = link_url.take() {
                    inline_buf.push_str(&format!(
                        " <span size=\"small\" alpha=\"60%\">({})</span>",
                        pango_escape(&url)
                    ));
                }
            }
            Event::Text(text) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&text);
                } else {
                    inline_buf.push_str(&pango_escape(&text));
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&html);
                } else {
                    inline_buf.push_str(&pango_escape(&html));
                }
            }
            Event::Code(code) => {
                inline_buf.push_str(&format!("<tt>{}</tt>", pango_escape(&code)));
            }
            Event::SoftBreak => {
                if in_code_block.is_some() {
                    code_buf.push('\n');
                } else {
                    inline_buf.push('\n');
                }
            }
            Event::HardBreak => {
                if in_code_block.is_some() {
                    code_buf.push('\n');
                } else {
                    inline_buf.push('\n');
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some(frame) = list_stack.last_mut() {
                    frame.is_task_list = true;
                    frame.current_task_checked = Some(checked);
                }
            }
            Event::End(TagEnd::Paragraph) => {
                // For loose lists (items separated by blank lines), pulldown-cmark
                // wraps item content in paragraphs. We must NOT drain inline_buf here,
                // or End(Item) will receive empty text and the paragraph will appear
                // outside the list. Only emit standalone paragraphs when NOT in a list.
                if list_stack.is_empty() {
                    let text = std::mem::take(&mut inline_buf);
                    if !text.is_empty() {
                        if in_blockquote {
                            blockquote_blocks.push(MarkdownBlock::Paragraph(text));
                        } else {
                            blocks.push(MarkdownBlock::Paragraph(text));
                        }
                    }
                }
            }
            Event::End(TagEnd::Heading(level)) => {
                let text = std::mem::take(&mut inline_buf);
                let block = MarkdownBlock::Heading {
                    level: level as u8,
                    content: text,
                };
                if in_blockquote {
                    blockquote_blocks.push(block);
                } else {
                    blocks.push(block);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let code = code_buf.trim_end_matches('\n').to_string();
                let language = in_code_block.take().flatten();
                let block = MarkdownBlock::CodeBlock { language, code };
                if in_blockquote {
                    blockquote_blocks.push(block);
                } else {
                    blocks.push(block);
                }
            }
            Event::End(TagEnd::Item) => {
                let text = std::mem::take(&mut inline_buf);
                if let Some(frame) = list_stack.last_mut() {
                    if frame.is_task_list {
                        let checked = frame.current_task_checked.take().unwrap_or(false);
                        frame.task_items.push((checked, text));
                    } else {
                        frame.items.push(text);
                    }
                }
            }
            Event::End(TagEnd::List(_)) => {
                if let Some(frame) = list_stack.pop() {
                    if list_stack.is_empty() {
                        // Top-level list: emit as a block.
                        let block = if frame.is_task_list {
                            MarkdownBlock::TaskList(frame.task_items)
                        } else {
                            MarkdownBlock::List {
                                ordered: frame.ordered,
                                items: frame.items,
                            }
                        };
                        inline_buf = frame.parent_item_buf;
                        if in_blockquote {
                            blockquote_blocks.push(block);
                        } else {
                            blocks.push(block);
                        }
                    } else {
                        // Nested list: inline the items into the outer item's text.
                        let nested_text = format_nested_list_as_text(&frame);
                        inline_buf = frame.parent_item_buf;
                        inline_buf.push_str(&nested_text);
                    }
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                in_blockquote = false;
                blocks.push(MarkdownBlock::Blockquote(std::mem::take(
                    &mut blockquote_blocks,
                )));
            }
            Event::End(TagEnd::TableCell) => {
                let text = std::mem::take(&mut inline_buf);
                table_row.push(text);
            }
            Event::End(TagEnd::TableHead) => {
                table_headers = std::mem::take(&mut table_row);
                in_table_head = false;
            }
            Event::End(TagEnd::TableRow) => {
                if !in_table_head {
                    table_rows.push(std::mem::take(&mut table_row));
                }
            }
            Event::End(TagEnd::Table) => {
                let block = MarkdownBlock::Table {
                    headers: std::mem::take(&mut table_headers),
                    rows: std::mem::take(&mut table_rows),
                };
                if in_blockquote {
                    blockquote_blocks.push(block);
                } else {
                    blocks.push(block);
                }
            }
            Event::Rule => {
                if in_blockquote {
                    blockquote_blocks.push(MarkdownBlock::HorizontalRule);
                } else {
                    blocks.push(MarkdownBlock::HorizontalRule);
                }
            }
            _ => {}
        }
    }

    blocks
}

/// Render markdown content as a vertical `gtk::Box` of native widgets.
///
/// If `highlight_query` is provided, matches are highlighted in text blocks
/// (paragraphs, headings, lists, tables) but not inside code blocks.
/// Returns the widget and the total number of highlighted matches.
pub fn render_markdown(content: &str, highlight_query: Option<&str>) -> (gtk::Box, usize) {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let mut total_matches = 0usize;

    for block in markdown_to_blocks(content) {
        total_matches += render_block(&container, block, highlight_query);
    }

    (container, total_matches)
}

/// Apply highlighting to Pango markup if a query is provided.
/// Returns the (possibly highlighted) markup and the match count.
fn apply_highlight(markup: &str, highlight_query: Option<&str>) -> (String, usize) {
    if let Some(query) = highlight_query
        && !query.is_empty()
    {
        return crate::ui::highlight::highlight_in_markup(markup, query);
    }
    (markup.to_string(), 0)
}

/// Render a single `MarkdownBlock` as GTK widgets appended to `container`.
/// Called recursively for blockquotes. Returns number of highlighted matches.
fn render_block(
    container: &gtk::Box,
    block: MarkdownBlock,
    highlight_query: Option<&str>,
) -> usize {
    let mut matches = 0usize;

    match block {
        MarkdownBlock::Paragraph(markup) => {
            let (highlighted, count) = apply_highlight(&markup, highlight_query);
            matches += count;
            let label = gtk::Label::new(None);
            label.set_markup(&highlighted);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_halign(gtk::Align::Start);
            label.set_xalign(0.0);
            label.set_selectable(true);
            container.append(&label);
        }
        MarkdownBlock::Heading { level, content } => {
            let (highlighted, count) = apply_highlight(&content, highlight_query);
            matches += count;
            let label = gtk::Label::new(None);
            label.set_markup(&highlighted);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_halign(gtk::Align::Start);
            label.set_xalign(0.0);
            match level {
                1 => label.add_css_class("title-1"),
                2 => label.add_css_class("title-2"),
                3 => label.add_css_class("title-3"),
                4 => label.add_css_class("title-4"),
                _ => label.add_css_class("heading"),
            }
            container.append(&label);
        }
        MarkdownBlock::CodeBlock { language, code } => {
            // No highlighting inside code blocks (v1 scope exclusion)
            let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 4);
            wrapper.add_css_class("code-block");

            if let Some(language) = language {
                let language_label = gtk::Label::new(Some(&language));
                language_label.add_css_class("caption");
                language_label.add_css_class("dim-label");
                language_label.set_halign(gtk::Align::Start);
                wrapper.append(&language_label);
            }

            let label = gtk::Label::new(Some(&code));
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_halign(gtk::Align::Fill);
            label.set_xalign(0.0);
            label.set_selectable(true);
            wrapper.append(&label);
            container.append(&wrapper);
        }
        MarkdownBlock::List { ordered, items } => {
            let list_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            for (index, item_markup) in items.iter().enumerate() {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);

                let marker = if ordered {
                    format!("{}.", index + 1)
                } else {
                    String::from("-")
                };

                let marker_label = gtk::Label::new(Some(&marker));
                marker_label.set_valign(gtk::Align::Start);
                marker_label.set_halign(gtk::Align::Start);
                row.append(&marker_label);

                let (highlighted, count) = apply_highlight(item_markup, highlight_query);
                matches += count;
                let text_label = gtk::Label::new(None);
                text_label.set_markup(&highlighted);
                text_label.set_wrap(true);
                text_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                text_label.set_halign(gtk::Align::Start);
                text_label.set_xalign(0.0);
                text_label.set_selectable(true);
                text_label.set_hexpand(true);
                row.append(&text_label);

                list_box.append(&row);
            }
            container.append(&list_box);
        }
        MarkdownBlock::TaskList(items) => {
            let list_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            for (checked, item_markup) in items {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);

                let check = gtk::CheckButton::new();
                check.set_active(checked);
                check.set_sensitive(false);
                check.set_valign(gtk::Align::Start);
                row.append(&check);

                let (highlighted, count) = apply_highlight(&item_markup, highlight_query);
                matches += count;
                let text_label = gtk::Label::new(None);
                text_label.set_markup(&highlighted);
                text_label.set_wrap(true);
                text_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                text_label.set_halign(gtk::Align::Start);
                text_label.set_xalign(0.0);
                text_label.set_selectable(true);
                text_label.set_hexpand(true);
                row.append(&text_label);

                list_box.append(&row);
            }
            container.append(&list_box);
        }
        MarkdownBlock::Blockquote(inner_blocks) => {
            let quote_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
            quote_box.add_css_class("markdown-blockquote");

            for inner in inner_blocks {
                matches += render_block(&quote_box, inner, highlight_query);
            }

            container.append(&quote_box);
        }
        MarkdownBlock::Table { headers, rows } => {
            let grid = gtk::Grid::new();
            grid.add_css_class("markdown-table");
            grid.set_column_spacing(12);
            grid.set_row_spacing(4);

            for (col, header) in headers.iter().enumerate() {
                let (highlighted, count) = apply_highlight(header, highlight_query);
                matches += count;
                let label = gtk::Label::new(None);
                label.set_markup(&highlighted);
                label.add_css_class("markdown-table-header");
                label.set_halign(gtk::Align::Start);
                label.set_hexpand(true);
                grid.attach(&label, col as i32, 0, 1, 1);
            }

            for (row_index, row) in rows.iter().enumerate() {
                for (col, cell) in row.iter().enumerate() {
                    let (highlighted, count) = apply_highlight(cell, highlight_query);
                    matches += count;
                    let label = gtk::Label::new(None);
                    label.set_markup(&highlighted);
                    label.set_halign(gtk::Align::Start);
                    label.set_wrap(true);
                    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                    label.set_selectable(true);
                    grid.attach(&label, col as i32, (row_index + 1) as i32, 1, 1);
                }
            }

            container.append(&grid);
        }
        MarkdownBlock::HorizontalRule => {
            let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
            separator.add_css_class("markdown-hr");
            container.append(&separator);
        }
    }

    matches
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
    code_block.set_paragraph_background(Some("#1e1e1e"));
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
                        let tag_refs: Vec<&str> = tags.iter().copied().collect();
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
                        if !frame.2 {
                            // Not a task list — insert marker now
                            let marker = if frame.0 {
                                format!("{}. ", frame.1)
                            } else {
                                "- ".to_string()
                            };
                            let mut tags = self.active_tags();
                            tags.push("list-item");
                            let tag_refs: Vec<&str> = tags.iter().copied().collect();
                            self.insert_with_tags(&marker, &tag_refs);
                        }
                    }
                }
                Event::End(TagEnd::Item) => {
                    self.insert_with_tags("\n", &[]);
                }

                Event::TaskListMarker(checked) => {
                    if let Some(frame) = self.list_stack.last_mut() {
                        frame.2 = true; // mark as task list
                    }
                    self.current_task_checked = Some(checked);

                    // Insert the task marker
                    let marker = if checked { "[x] " } else { "[ ] " };
                    let mut tags = self.active_tags();
                    tags.push("list-item");
                    let tag_refs: Vec<&str> = tags.iter().copied().collect();
                    self.insert_with_tags(marker, &tag_refs);
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
                        let mut tags = self.active_tags();
                        tags.push("code-inline");
                        if !self.list_stack.is_empty() {
                            tags.push("list-item");
                        }
                        let tag_refs: Vec<&str> = tags.iter().copied().collect();
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
                        let tag_refs: Vec<&str> = tags.iter().copied().collect();
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

    /// Emit inline text with current formatting context.
    fn emit_text(&mut self, text: &str) {
        let mut tags = self.active_tags();
        // If inside a list, add list-item tag for indentation
        if !self.list_stack.is_empty() {
            tags.push("list-item");
        }
        let tag_refs: Vec<&str> = tags.iter().copied().collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_produces_single_paragraph() {
        let blocks = markdown_to_blocks("Hello world");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Paragraph(text) if text == "Hello world"));
    }

    #[test]
    fn bold_text_uses_pango_bold() {
        let blocks = markdown_to_blocks("Hello **bold** world");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::Paragraph(text) if text.contains("<b>bold</b>"))
        );
    }

    #[test]
    fn italic_text_uses_pango_italic() {
        let blocks = markdown_to_blocks("Hello *italic* world");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::Paragraph(text) if text.contains("<i>italic</i>"))
        );
    }

    #[test]
    fn inline_code_uses_pango_tt() {
        let blocks = markdown_to_blocks("Use `cargo test` here");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::Paragraph(text) if text.contains("<tt>cargo test</tt>"))
        );
    }

    #[test]
    fn strikethrough_uses_pango_s() {
        let blocks = markdown_to_blocks("This is ~~removed~~ text");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::Paragraph(text) if text.contains("<s>removed</s>"))
        );
    }

    #[test]
    fn heading_levels() {
        let blocks = markdown_to_blocks("# Title\n\n## Subtitle\n\n### Third");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(
            &blocks[0],
            MarkdownBlock::Heading { level: 1, .. }
        ));
        assert!(matches!(
            &blocks[1],
            MarkdownBlock::Heading { level: 2, .. }
        ));
        assert!(matches!(
            &blocks[2],
            MarkdownBlock::Heading { level: 3, .. }
        ));
    }

    #[test]
    fn fenced_code_block() {
        let blocks = markdown_to_blocks("```rust\nfn main() {}\n```");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::CodeBlock { language, code }
                if language.as_deref() == Some("rust") && code == "fn main() {}")
        );
    }

    #[test]
    fn code_block_trailing_newline_trimmed() {
        let blocks = markdown_to_blocks("```\nline1\nline2\n```");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::CodeBlock { code, .. } if code == "line1\nline2")
        );
    }

    #[test]
    fn unordered_list() {
        let blocks = markdown_to_blocks("- First\n- Second\n- Third");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::List { ordered: false, items }
                if items.len() == 3)
        );
    }

    #[test]
    fn ordered_list() {
        let blocks = markdown_to_blocks("1. First\n2. Second");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], MarkdownBlock::List { ordered: true, items }
                if items.len() == 2)
        );
    }

    #[test]
    fn blockquote() {
        let blocks = markdown_to_blocks("> Quoted text");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Blockquote(_)));
    }

    #[test]
    fn horizontal_rule() {
        let blocks = markdown_to_blocks("Above\n\n---\n\nBelow");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[1], MarkdownBlock::HorizontalRule));
    }

    #[test]
    fn link_renders_text_and_url() {
        let blocks = markdown_to_blocks("Visit [Rust](https://rust-lang.org)");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Paragraph(text)
                if text.contains("Rust") && text.contains("https://rust-lang.org")));
    }

    #[test]
    fn html_entities_escaped() {
        let blocks = markdown_to_blocks("Use <script> & \"quotes\"");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Paragraph(text)
                if text.contains("&lt;script&gt;") && text.contains("&amp;")));
    }

    #[test]
    fn task_list() {
        let blocks = markdown_to_blocks("- [x] Done\n- [ ] Todo");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::TaskList(items)
                if items.len() == 2
                && items[0].0
                && !items[1].0));
    }

    #[test]
    fn table_basic() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
        let blocks = markdown_to_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Table { headers, rows }
                if headers.len() == 2 && rows.len() == 2));
    }

    #[test]
    fn nested_bold_italic() {
        let blocks = markdown_to_blocks("***bold italic***");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Paragraph(text)
                if text.contains("<b>") && text.contains("<i>")));
    }

    #[test]
    fn soft_break_becomes_space() {
        let blocks = markdown_to_blocks("Line one\nLine two");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Paragraph(_)));
    }

    #[test]
    fn blockquote_contains_heading() {
        let blocks = markdown_to_blocks("> ## Heading inside quote");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Blockquote(inner)
            if inner.len() == 1 && matches!(&inner[0], MarkdownBlock::Heading { level: 2, .. })));
    }

    #[test]
    fn blockquote_contains_code_block() {
        let blocks = markdown_to_blocks("> ```rust\n> fn main() {}\n> ```");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Blockquote(inner)
            if inner.len() == 1 && matches!(&inner[0], MarkdownBlock::CodeBlock { .. })));
    }

    #[test]
    fn blockquote_contains_list() {
        let blocks = markdown_to_blocks("> - First item\n> - Second item");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Blockquote(inner)
            if inner.len() == 1 && matches!(&inner[0], MarkdownBlock::List { .. })));
    }

    #[test]
    fn blockquote_contains_task_list() {
        let blocks = markdown_to_blocks("> - [x] Done\n> - [ ] Todo");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Blockquote(inner)
            if inner.len() == 1 && matches!(&inner[0], MarkdownBlock::TaskList(_))));
    }

    #[test]
    fn blockquote_contains_horizontal_rule() {
        let blocks = markdown_to_blocks("> ---");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Blockquote(inner)
            if inner.len() == 1 && matches!(&inner[0], MarkdownBlock::HorizontalRule)));
    }

    #[test]
    fn blockquote_contains_multiple_blocks() {
        let blocks = markdown_to_blocks("> Text\n> \n> ## Heading\n> \n> More text");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MarkdownBlock::Blockquote(inner)
            if inner.len() == 3
            && matches!(&inner[0], MarkdownBlock::Paragraph(_))
            && matches!(&inner[1], MarkdownBlock::Heading { level: 2, .. })
            && matches!(&inner[2], MarkdownBlock::Paragraph(_))));
    }

    #[test]
    fn nested_unordered_list_rendered_in_parent_item() {
        let md = "- Parent item\n  - Child item 1\n  - Child item 2\n- Second parent";
        let blocks = markdown_to_blocks(md);
        assert_eq!(
            blocks.len(),
            1,
            "Expected single list block, got {:?}",
            blocks
        );
        match &blocks[0] {
            MarkdownBlock::List { ordered, items } => {
                assert!(!ordered, "Expected unordered list");
                assert_eq!(
                    items.len(),
                    2,
                    "Expected 2 top-level items, got {}",
                    items.len()
                );
                assert!(
                    items[0].contains("Parent item"),
                    "First item missing 'Parent item': {:?}",
                    items[0]
                );
                assert!(
                    items[0].contains("Child item 1"),
                    "First item missing 'Child item 1': {:?}",
                    items[0]
                );
                assert!(
                    items[0].contains("Child item 2"),
                    "First item missing 'Child item 2': {:?}",
                    items[0]
                );
                assert!(
                    items[1].contains("Second parent"),
                    "Second item missing 'Second parent': {:?}",
                    items[1]
                );
            }
            _ => panic!("Expected List block, got {:?}", blocks[0]),
        }
    }

    #[test]
    fn loose_list_items_kept_in_list() {
        // Loose lists have blank lines between items, so pulldown-cmark wraps
        // each item's content in Paragraph events. We must keep that content
        // inside the list items, not emit it as standalone paragraphs.
        let md = "- First item\n\n- Second item\n\n- Third item";
        let blocks = markdown_to_blocks(md);

        // Should be exactly one list block, not a list plus paragraphs
        assert_eq!(
            blocks.len(),
            1,
            "Expected single list block, got {:?}",
            blocks
        );

        match &blocks[0] {
            MarkdownBlock::List { ordered, items } => {
                assert!(!ordered, "Expected unordered list");
                assert_eq!(items.len(), 3, "Expected 3 items, got {}", items.len());
                assert!(items[0].contains("First item"));
                assert!(items[1].contains("Second item"));
                assert!(items[2].contains("Third item"));
            }
            _ => panic!("Expected List block, got {:?}", blocks[0]),
        }
    }

    // ── render_markdown_to_textview tests ─────────────────────────────
    //
    // These tests require GTK initialization. GTK can only be initialized
    // once and must happen from the same thread. We use `std::sync::Once`
    // to guard init and skip tests if GTK is unavailable (headless CI).

    // ── render_markdown_to_textview ──────────────────────────────
    //
    // GTK4 widget tests cannot run in the binary test target because
    // `main.rs` registers GResources on the main thread at load time,
    // making `gtk::init()` from test worker threads impossible.
    //
    // The TextBuffer/TextTag rendering is validated via:
    // 1. The `markdown_to_blocks` unit tests above (parser correctness)
    // 2. Manual visual testing with `--sessions-dir tests/fixtures`
    //    (see Task 6 in the implementation plan)
}
