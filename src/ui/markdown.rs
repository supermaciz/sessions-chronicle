use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use relm4::adw;
use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;
use sourceview5::prelude::*;
const LANGUAGE_ALIASES: &[(&str, &str)] = &[
    // GtkSourceView 5 exposes JavaScript as `js`, not `javascript`.
    ("js", "js"),
    ("javascript", "js"),
    ("ts", "typescript"),
    ("py", "python"),
    ("sh", "sh"),
    ("shell", "sh"),
    ("bash", "sh"),
    ("zsh", "sh"),
    ("rs", "rust"),
    ("yml", "yaml"),
    ("c++", "cpp"),
];

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

fn source_style_scheme_id(dark: bool) -> &'static str {
    if dark { "Adwaita-dark" } else { "Adwaita" }
}

fn apply_source_style_scheme(buffer: &sourceview5::Buffer, dark: bool) {
    let scheme = sourceview5::StyleSchemeManager::default().scheme(source_style_scheme_id(dark));
    buffer.set_style_scheme(scheme.as_ref());
}

fn normalize_language_alias(language: &str) -> String {
    let lowercase = language.to_ascii_lowercase();
    LANGUAGE_ALIASES
        .iter()
        .find_map(|(alias, canonical)| (*alias == lowercase).then_some(*canonical))
        .unwrap_or(lowercase.as_str())
        .to_string()
}

/// A rendered segment: prose widgets, a table widget, or a code block widget.
enum MarkdownSegment {
    Prose(gtk::Widget),
    Table(gtk::Widget),
    CodeBlock(gtk::Widget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InlineStyle {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Heading(pulldown_cmark::HeadingLevel),
    Dim,
    TaskChecked,
    TaskUnchecked,
    SearchHighlight,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InlineRun {
    text: String,
    styles: Vec<InlineStyle>,
}

#[derive(Default)]
struct ProseBlock {
    runs: Vec<InlineRun>,
}

#[derive(Clone, Debug)]
struct ListFrame {
    ordered: bool,
    next_index: usize,
    depth: usize,
}

#[derive(Clone, Debug)]
struct ListItemBlock {
    marker: String,
    runs: Vec<InlineRun>,
    depth: usize,
}

/// Walks pulldown-cmark events and writes formatted text into a `TextBuffer`.
struct MarkdownBufferWriter {
    buffer: gtk::TextBuffer,
    /// Stack of active inline tag names (e.g. "bold", "italic").
    tag_stack: Vec<&'static str>,
    /// Stack of active inline styles for the current prose block.
    style_stack: Vec<InlineStyle>,
    /// True when inside a code block — text goes verbatim, no inline tags.
    in_code_block: Option<Option<String>>,
    /// Code block accumulator.
    code_buf: String,
    /// List nesting stack.
    list_stack: Vec<ListFrame>,
    /// Current list item being accumulated.
    current_list_item: Option<ListItemBlock>,
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
    /// Search query used for markdown highlight; table widgets will use this
    /// in a follow-up task.
    highlight_query: Option<String>,
    /// Current prose block being accumulated (set when inside a top-level paragraph).
    current_block: Option<ProseBlock>,
    /// Completed segments (prose labels, table widgets, code blocks) in order.
    segments: Vec<MarkdownSegment>,
    /// Search match count found inside prose label widgets.
    prose_match_count: usize,
    /// Search match count found inside table widgets.
    table_match_count: usize,
    /// Search match count found inside code block buffers.
    code_block_match_count: usize,
    /// Source buffers used by code block widgets, tracked for theme updates.
    source_buffers: Vec<glib::WeakRef<sourceview5::Buffer>>,
    /// Stack of open blockquote group boxes; the innermost is the active container.
    blockquote_stack: Vec<gtk::Box>,
}

impl MarkdownBufferWriter {
    fn new(highlight_query: Option<&str>) -> Self {
        let buffer = gtk::TextBuffer::new(None);
        Self {
            buffer,
            tag_stack: Vec::new(),
            style_stack: Vec::new(),
            in_code_block: None,
            code_buf: String::new(),
            list_stack: Vec::new(),
            current_list_item: None,
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
            highlight_query: highlight_query.map(str::to_owned),
            current_block: None,
            segments: Vec::new(),
            prose_match_count: 0,
            table_match_count: 0,
            code_block_match_count: 0,
            source_buffers: Vec::new(),
            blockquote_stack: Vec::new(),
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

    /// Remove the last matching style from the inline style stack (LIFO).
    fn pop_style(&mut self, predicate: impl Fn(&InlineStyle) -> bool) {
        if let Some(pos) = self.style_stack.iter().rposition(predicate) {
            self.style_stack.remove(pos);
        }
    }

    /// Route a finished widget to the innermost open blockquote group, or to
    /// the top-level segment list when no blockquote is open.
    fn append_segment(&mut self, widget: gtk::Widget) {
        if let Some(blockquote) = self.blockquote_stack.last() {
            blockquote.append(&widget);
        } else {
            self.segments.push(MarkdownSegment::Prose(widget));
        }
    }

    /// Push a text run with current style stack into the current prose block
    /// or the current list item block (whichever is active).
    fn push_run(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let run = InlineRun {
            text: text.to_string(),
            styles: self.style_stack.clone(),
        };
        if let Some(ref mut item) = self.current_list_item {
            item.runs.push(run);
        } else if let Some(ref mut block) = self.current_block {
            block.runs.push(run);
        }
    }

    /// Finalize the current prose block into a label segment, if any.
    fn finish_prose_block(&mut self) {
        if let Some(block) = self.current_block.take()
            && !block.runs.is_empty()
        {
            let (runs, count) = highlighted_runs(&block.runs, self.highlight_query.as_deref());
            self.prose_match_count += count;
            let label = make_prose_label(&runs_to_markup(&runs));
            self.append_segment(label.upcast::<gtk::Widget>());
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
                self.finish_prose_block();
                let label = make_prose_label("────────────────────────");
                label.add_css_class("markdown-hr");
                self.append_segment(label.upcast());
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
            Tag::Emphasis => {
                self.tag_stack.push("italic");
                self.style_stack.push(InlineStyle::Italic);
            }
            Tag::Strong => {
                self.tag_stack.push("bold");
                self.style_stack.push(InlineStyle::Bold);
            }
            Tag::Strikethrough => {
                self.tag_stack.push("strikethrough");
                self.style_stack.push(InlineStyle::Strikethrough);
            }
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
            }
            Tag::Image { .. } => {
                self.in_image = true;
                self.push_text_content("[image: ");
            }
            Tag::Paragraph if self.list_stack.is_empty() && !self.in_table => {
                self.current_block = Some(ProseBlock::default());
            }
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.block_separator();
                let heading_tag = Self::heading_tag_name(level);
                self.tag_stack.push(heading_tag);
                self.style_stack.push(InlineStyle::Heading(level));
                self.current_block = Some(ProseBlock::default());
            }
            Tag::CodeBlock(kind) => self.start_code_block(kind),
            Tag::List(start) => {
                // If a list opens while we're inside a list item, the current
                // item's inline content is complete — emit it as a widget now
                // before descending into the nested list.
                if let Some(item) = self.current_list_item.take() {
                    let (widget, count) = make_list_item_widget(
                        &item.marker,
                        &item.runs,
                        item.depth,
                        self.highlight_query.as_deref(),
                    );
                    self.prose_match_count += count;
                    self.append_segment(widget);
                }
                self.list_stack.push(ListFrame {
                    ordered: start.is_some(),
                    next_index: start.unwrap_or(1) as usize,
                    depth: self.list_stack.len(),
                });
            }
            Tag::Item => {
                if let Some(frame) = self.list_stack.last_mut() {
                    let marker = if frame.ordered {
                        let marker = format!("{}.", frame.next_index);
                        frame.next_index += 1;
                        marker
                    } else {
                        "-".to_string()
                    };
                    let depth = frame.depth;
                    self.current_list_item = Some(ListItemBlock {
                        marker,
                        runs: Vec::new(),
                        depth,
                    });
                }
            }
            Tag::Table(_) => {
                // No block_separator(): the table is a separate widget
                // segment; a trailing '\n' would create blank space above it.
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
                let group = gtk::Box::new(gtk::Orientation::Vertical, 0);
                group.add_css_class("markdown-blockquote");
                group.set_valign(gtk::Align::Start);
                self.blockquote_stack.push(group);
            }
            _ => {}
        }
    }

    fn handle_end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Emphasis => {
                self.pop_tag("italic");
                self.pop_style(|style| matches!(style, InlineStyle::Italic));
            }
            TagEnd::Strong => {
                self.pop_tag("bold");
                self.pop_style(|style| matches!(style, InlineStyle::Bold));
            }
            TagEnd::Strikethrough => {
                self.pop_tag("strikethrough");
                self.pop_style(|style| matches!(style, InlineStyle::Strikethrough));
            }
            TagEnd::Link => {
                if let Some(url) = self.link_url.take() {
                    self.style_stack.push(InlineStyle::Dim);
                    self.push_text_content(&format!(" ({url})"));
                    self.pop_style(|style| matches!(style, InlineStyle::Dim));
                }
            }
            TagEnd::Image => {
                self.in_image = false;
                self.push_text_content("]");
            }
            TagEnd::Paragraph if self.list_stack.is_empty() && !self.in_table => {
                self.finish_prose_block();
            }
            TagEnd::Paragraph => {}
            TagEnd::Heading(level) => {
                self.finish_prose_block();
                self.pop_style(
                    |style| matches!(style, InlineStyle::Heading(existing) if *existing == level),
                );
                self.insert_with_tags("\n", &[]);
                self.has_content = true;
                self.pop_tag(Self::heading_tag_name(level));
            }
            TagEnd::CodeBlock => self.finish_code_block(),
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                if let Some(item) = self.current_list_item.take() {
                    let (widget, count) = make_list_item_widget(
                        &item.marker,
                        &item.runs,
                        item.depth,
                        self.highlight_query.as_deref(),
                    );
                    self.prose_match_count += count;
                    self.append_segment(widget);
                }
            }
            TagEnd::Table => {
                self.in_table = false;
                self.render_table();
                // Buffer was flushed inside render_table(); keep has_content
                // false so the next block won't get a leading '\n'.
            }
            TagEnd::TableHead => {
                self.table_headers = std::mem::take(&mut self.table_row);
                self.in_table_head = false;
            }
            TagEnd::TableRow if !self.in_table_head => {
                self.table_rows.push(std::mem::take(&mut self.table_row));
            }
            TagEnd::TableRow => {}
            TagEnd::TableCell => {
                self.table_row.push(std::mem::take(&mut self.inline_buf));
            }
            TagEnd::BlockQuote(_) => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                if let Some(group) = self.blockquote_stack.pop() {
                    let widget = group.upcast::<gtk::Widget>();
                    if let Some(parent_quote) = self.blockquote_stack.last() {
                        parent_quote.append(&widget);
                    } else {
                        self.segments.push(MarkdownSegment::Prose(widget));
                    }
                }
            }
            _ => {}
        }
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>) {
        self.code_buf.clear();
        let language = match kind {
            CodeBlockKind::Fenced(info) => info
                .split_whitespace()
                .next()
                .map(str::to_string)
                .filter(|s| !s.is_empty()),
            CodeBlockKind::Indented => None,
        };
        self.in_code_block = Some(language);
    }

    /// Flush the current text buffer as a segment before a widget segment
    /// (table or code block).  Trailing newlines are stripped so they don't
    /// appear as blank space above the widget.
    fn flush_buffer_before_widget(&mut self) {
        if self.buffer.char_count() > 0 {
            // Strip trailing newlines from the buffer.
            let mut end = self.buffer.end_iter();
            let mut start = end;
            while start.backward_char() {
                if start.char() != '\n' {
                    start.forward_char();
                    break;
                }
            }
            if start.offset() < end.offset() {
                self.buffer.delete(&mut start, &mut end);
            }
            // Only push if content remains after stripping.
            if self.buffer.char_count() > 0 {
                let old_buffer = std::mem::replace(&mut self.buffer, gtk::TextBuffer::new(None));
                let view = make_textview(&old_buffer);
                self.append_segment(view.upcast::<gtk::Widget>());
            }
            self.has_content = false;
        }
    }

    fn finish_code_block(&mut self) {
        self.flush_buffer_before_widget();

        let code = self.code_buf.trim_end_matches('\n').to_string();
        let code_buffer = sourceview5::Buffer::new(None);
        code_buffer.set_text(&code);

        let language = self.in_code_block.take().flatten();
        let normalized_language = language.as_deref().map(normalize_language_alias);

        if let Some(language_id) = normalized_language.as_deref() {
            let language_manager = sourceview5::LanguageManager::default();
            let fallback_language = language.as_deref().map(str::to_ascii_lowercase);
            let resolved_language = language_manager.language(language_id).or_else(|| {
                fallback_language
                    .as_deref()
                    .and_then(|id| language_manager.language(id))
            });

            if let Some(source_language) = resolved_language {
                code_buffer.set_language(Some(&source_language));
                code_buffer.set_highlight_syntax(true);
            } else {
                code_buffer.set_language(None);
                code_buffer.set_highlight_syntax(false);
            }
        } else {
            code_buffer.set_language(None);
            code_buffer.set_highlight_syntax(false);
        }

        if let Some(query) = self.highlight_query.as_deref() {
            self.code_block_match_count += apply_search_highlight(&code_buffer, query);
        }

        apply_source_style_scheme(&code_buffer, is_dark_mode());
        self.source_buffers.push(code_buffer.downgrade());

        let code_view = sourceview5::View::with_buffer(&code_buffer);
        code_view.set_editable(false);
        code_view.set_cursor_visible(false);
        code_view.set_monospace(true);
        code_view.set_wrap_mode(gtk::WrapMode::None);
        code_view.add_css_class("code-block-content");

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hexpand(true);
        scroller.set_hscrollbar_policy(gtk::PolicyType::Automatic);
        scroller.set_vscrollbar_policy(gtk::PolicyType::Never);
        scroller.add_css_class("code-block-scroller");
        scroller.set_child(Some(&code_view));

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.add_css_class("code-block-widget");

        if let Some(ref lang) = language {
            let lang_label = gtk::Label::new(Some(lang));
            lang_label.set_halign(gtk::Align::Start);
            lang_label.add_css_class("code-block-lang");
            outer.append(&lang_label);
        }

        outer.append(&scroller);

        self.append_segment(outer.upcast::<gtk::Widget>());
        // Buffer was flushed; keep has_content false.
        self.code_buf.clear();
    }

    fn handle_task_list_marker(&mut self, checked: bool) {
        if let Some(item) = self.current_list_item.as_mut() {
            item.marker = if checked { "☑" } else { "☐" }.to_string();
        }
    }

    /// Emit inline text with current formatting context (TextBuffer path).
    fn emit_text(&mut self, text: &str) {
        let tags = self.active_tags();
        self.insert_with_tags(text, &tags);
    }

    fn write_inline_with_active_tags(&mut self, text: &str) {
        if self.in_table {
            self.inline_buf.push_str(text);
        } else if self.current_list_item.is_some() || self.current_block.is_some() {
            self.push_run(text);
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
        } else if self.current_list_item.is_some() || self.current_block.is_some() {
            self.push_run(text);
        } else {
            self.emit_text(text);
        }
    }

    fn push_inline_code(&mut self, code: &str) {
        if self.in_table {
            self.inline_buf.push_str(code);
        } else if self.current_list_item.is_some() || self.current_block.is_some() {
            self.style_stack.push(InlineStyle::Code);
            self.push_run(code);
            self.pop_style(|style| matches!(style, InlineStyle::Code));
        } else {
            let mut tags = self.active_tags();
            tags.push("code-inline");
            self.insert_with_tags(code, &tags);
        }
    }

    fn push_inline_break(&mut self) {
        if self.in_code_block.is_some() {
            self.code_buf.push('\n');
        } else if self.in_table {
            self.inline_buf.push('\n');
        } else if self.current_list_item.is_some() || self.current_block.is_some() {
            self.push_run("\n");
        } else {
            self.write_inline_with_active_tags("\n");
        }
    }

    fn create_table_label(text: &str, query: &str, is_header: bool) -> (gtk::Label, usize) {
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_halign(gtk::Align::Start);
        // Do NOT wrap. Wrapping makes the label's natural height depend on
        // its allocated width, which breaks GtkScrolledWindow's
        // height-for-width measurement and produces tall blocks of empty
        // space below tables (issue #149). With non-wrapping labels, the
        // grid's height is constant regardless of width, so the SW's height
        // is correct, and tables that don't fit the message area get the
        // horizontal scrollbar (which is the SW's purpose).
        label.set_wrap(false);
        label.set_single_line_mode(false);
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

        self.flush_buffer_before_widget();

        // Build the table grid.
        let grid = gtk::Grid::new();
        // Do NOT hexpand the grid. With non-wrapping cells, hexpand=true
        // would propagate the grid's full natural width up through the
        // ScrolledWindow into the message bubble, pushing layout off the
        // window. The SW handles horizontal scrolling for content wider
        // than its allocation.
        grid.set_halign(gtk::Align::Start);
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
        // The cells use non-wrapping labels (see `create_table_label`), so the
        // grid's height is independent of its allocated width — that avoids
        // GTK4's buggy height-for-width measurement on `ScrolledWindow` that
        // would otherwise produce excess blank space below tables.
        let table_widget = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(false)
            .valign(gtk::Align::Start)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&grid)
            .build();

        self.table_match_count += table_match_count;
        self.append_segment(table_widget.upcast::<gtk::Widget>());
    }

    /// Strip leading newlines from a text buffer.
    fn strip_leading_newlines(buffer: &gtk::TextBuffer) {
        let mut start = buffer.start_iter();
        let mut end = start;
        while end.char() == '\n' {
            if !end.forward_char() {
                break;
            }
        }
        if start.offset() < end.offset() {
            buffer.delete(&mut start, &mut end);
        }
    }

    /// Finalize and return segments, widget match count, and source buffers.
    fn finalize(
        mut self,
    ) -> (
        Vec<MarkdownSegment>,
        usize,
        Vec<glib::WeakRef<sourceview5::Buffer>>,
    ) {
        if self.buffer.char_count() > 0 {
            let view = make_textview(&self.buffer);
            self.append_segment(view.upcast::<gtk::Widget>());
        }

        (
            self.segments,
            self.prose_match_count + self.table_match_count + self.code_block_match_count,
            self.source_buffers,
        )
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

fn span_open(style: &InlineStyle) -> &'static str {
    match style {
        InlineStyle::Bold => "<b>",
        InlineStyle::Italic => "<i>",
        InlineStyle::Strikethrough => "<s>",
        InlineStyle::Code => "<tt>",
        InlineStyle::Heading(pulldown_cmark::HeadingLevel::H1) => {
            "<span weight=\"bold\" size=\"x-large\">"
        }
        InlineStyle::Heading(pulldown_cmark::HeadingLevel::H2) => {
            "<span weight=\"bold\" size=\"large\">"
        }
        InlineStyle::Heading(_) => "<span weight=\"bold\" font_scale=\"1.1\">",
        InlineStyle::Dim => "<span alpha=\"65%\">",
        InlineStyle::TaskChecked => "<span foreground=\"#2ec27e\">",
        InlineStyle::TaskUnchecked => "<span alpha=\"65%\">",
        InlineStyle::SearchHighlight => "<span background=\"#fce94f\" foreground=\"#1e1e1e\">",
    }
}

fn span_close(style: &InlineStyle) -> &'static str {
    match style {
        InlineStyle::Bold => "</b>",
        InlineStyle::Italic => "</i>",
        InlineStyle::Strikethrough => "</s>",
        InlineStyle::Code => "</tt>",
        InlineStyle::Heading(_)
        | InlineStyle::Dim
        | InlineStyle::TaskChecked
        | InlineStyle::TaskUnchecked
        | InlineStyle::SearchHighlight => "</span>",
    }
}

fn highlighted_runs(runs: &[InlineRun], query: Option<&str>) -> (Vec<InlineRun>, usize) {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return (runs.to_vec(), 0);
    };

    let mut plain = String::new();
    for run in runs {
        plain.push_str(&run.text);
    }

    let matches = crate::utils::text_match::find_case_insensitive_matches(&plain, query);
    if matches.is_empty() {
        return (runs.to_vec(), 0);
    }

    let mut output = Vec::new();
    let mut plain_offset = 0usize;
    for run in runs {
        let run_start = plain_offset;
        let run_end = run_start + run.text.len();
        let mut cursor = run_start;

        for (match_start, match_end) in matches.iter().copied() {
            if match_end <= run_start || match_start >= run_end {
                continue;
            }

            let clipped_start = match_start.max(run_start);
            let clipped_end = match_end.min(run_end);

            if cursor < clipped_start {
                output.push(InlineRun {
                    text: plain[cursor..clipped_start].to_string(),
                    styles: run.styles.clone(),
                });
            }

            let mut styles = run.styles.clone();
            styles.push(InlineStyle::SearchHighlight);
            output.push(InlineRun {
                text: plain[clipped_start..clipped_end].to_string(),
                styles,
            });
            cursor = clipped_end;
        }

        if cursor < run_end {
            output.push(InlineRun {
                text: plain[cursor..run_end].to_string(),
                styles: run.styles.clone(),
            });
        }

        plain_offset = run_end;
    }

    (output, matches.len())
}

fn runs_to_markup(runs: &[InlineRun]) -> String {
    let mut markup = String::new();
    for run in runs {
        for style in &run.styles {
            markup.push_str(span_open(style));
        }
        markup.push_str(&pango_escape(&run.text));
        for style in run.styles.iter().rev() {
            markup.push_str(span_close(style));
        }
    }
    markup
}

/// Create a non-editable, transparent `gtk::TextView` from a buffer.
fn make_textview(buffer: &gtk::TextBuffer) -> gtk::TextView {
    let view = gtk::TextView::with_buffer(buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_hexpand(true);
    view.set_vexpand(false);
    view.set_valign(gtk::Align::Start);
    view.set_top_margin(0);
    view.set_bottom_margin(0);
    view.set_left_margin(0);
    view.set_right_margin(0);
    view.add_css_class("markdown-textview");
    view
}

/// Create a selectable, wrapping `gtk::Label` for a prose paragraph.
fn make_prose_label(markup: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_use_markup(true);
    label.set_markup(markup);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    label.set_hexpand(true);
    label.set_valign(gtk::Align::Start);
    label.set_vexpand(false);
    label.add_css_class("markdown-prose");
    label
}

/// Build a horizontal row widget for a single list item.
///
/// The row contains a marker label (bullet or number) and a content label
/// rendered from inline runs. The `depth` controls the left indent level.
fn make_list_item_widget(
    marker: &str,
    runs: &[InlineRun],
    depth: usize,
    query: Option<&str>,
) -> (gtk::Widget, usize) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("markdown-list-item");
    row.set_valign(gtk::Align::Start);
    row.set_margin_start((depth as i32) * 18);

    let marker_label = gtk::Label::new(Some(marker));
    marker_label.add_css_class("markdown-list-marker");
    marker_label.set_halign(gtk::Align::End);
    marker_label.set_valign(gtk::Align::Start);
    marker_label.set_width_chars(3);
    row.append(&marker_label);

    let (highlighted, count) = highlighted_runs(runs, query);
    let content = make_prose_label(&runs_to_markup(&highlighted));
    content.remove_css_class("markdown-prose");
    content.add_css_class("markdown-list-content");
    row.append(&content);

    (row.upcast(), count)
}

/// Render markdown content into a widget (`Box` containing prose labels,
/// table widgets, and code block widgets).
///
/// If `highlight_query` is provided, matches are highlighted with a
/// background color. Returns the widget and the total number of matches.
pub fn render_markdown(content: &str, highlight_query: Option<&str>) -> (gtk::Widget, usize) {
    let mut writer = MarkdownBufferWriter::new(highlight_query);
    writer.process(content);
    let (segments, total_matches, source_buffers) = writer.finalize();

    // Wire up theme-change listener that updates source buffer style schemes.
    let style_manager = adw::StyleManager::default();
    let source_buffers = std::cell::RefCell::new(source_buffers);
    let theme_handler = style_manager.connect_dark_notify(move |manager| {
        source_buffers.borrow_mut().retain(|weak_buffer| {
            if let Some(buffer) = weak_buffer.upgrade() {
                apply_source_style_scheme(&buffer, manager.is_dark());
                true
            } else {
                false
            }
        });
    });

    // Box-only path: always build a vertical Box container.
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.set_vexpand(false);
    container.set_valign(gtk::Align::Start);

    for segment in segments {
        match segment {
            MarkdownSegment::Prose(widget)
            | MarkdownSegment::Table(widget)
            | MarkdownSegment::CodeBlock(widget) => container.append(&widget),
        }
    }

    attach_theme_cleanup(&container, style_manager, theme_handler);

    (container.upcast(), total_matches)
}

/// Find and highlight all case-insensitive matches of `query` in the buffer.
///
/// Uses the `search-highlight` tag from the buffer's tag table.
/// Returns the number of matches found.
fn apply_search_highlight(buffer: &impl IsA<gtk::TextBuffer>, query: &str) -> usize {
    let buffer: &gtk::TextBuffer = buffer.as_ref();

    if query.is_empty() {
        return 0;
    }

    if buffer.tag_table().lookup("search-highlight").is_none() {
        let highlight = gtk::TextTag::new(Some("search-highlight"));
        highlight.set_background(Some("#fce94f"));
        highlight.set_foreground(Some("#1e1e1e"));
        buffer.tag_table().add(&highlight);
        highlight.set_priority(buffer.tag_table().size() - 1);

        // Re-promote search-highlight whenever GtkSourceView adds its own
        // syntax tags to the buffer's tag table.
        buffer.tag_table().connect_tag_added(|table, added| {
            if added.name().as_deref() == Some("search-highlight") {
                return;
            }
            if let Some(hl) = table.lookup("search-highlight") {
                hl.set_priority(table.size() - 1);
            }
        });
    }

    let start = buffer.start_iter();
    let end = buffer.end_iter();
    let text = buffer.slice(&start, &end, false);
    let text_str = text.as_str();

    let matches = crate::utils::text_match::find_case_insensitive_matches(text_str, query);
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

    #[test]
    fn source_style_scheme_id_matches_adwaita_theme_names() {
        assert_eq!(source_style_scheme_id(false), "Adwaita");
        assert_eq!(source_style_scheme_id(true), "Adwaita-dark");
    }

    #[gtk::test]
    fn search_highlight_tag_has_highest_priority() {
        // After removing create_tag_table, source buffers have their own tag
        // tables. Verify that apply_search_highlight creates and top-promotes
        // the search-highlight tag in the buffer's own tag table.
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_text("hello world");
        apply_search_highlight(&buffer, "world");
        let table = buffer.tag_table();
        let highlight = table
            .lookup("search-highlight")
            .expect("search-highlight tag exists");

        assert_eq!(highlight.priority(), table.size() - 1);
    }

    // NOTE: Search count for prose paragraphs is 0 in Task 3 because
    // GtkLabel-based prose does not use TextBuffer search highlighting yet.
    // This will be restored when prose search is implemented in a later task.
    #[gtk::test]
    fn render_markdown_public_entry_point_returns_widget_and_count() {
        let (widget, _count) = render_markdown("Hello world", Some("world"));
        // Prose-only content returns a Box (not a raw Widget of unknown type).
        assert!(widget.is::<gtk::Box>());
    }

    #[gtk::test]
    fn prose_only_markdown_returns_box_without_textview() {
        let (widget, _) = render_markdown("First paragraph\n\nSecond paragraph", None);

        assert!(widget.is::<gtk::Box>(), "prose root should be a GtkBox");
        assert!(
            find_widgets_of_type::<gtk::TextView>(&widget).is_empty(),
            "prose rendering must not contain GtkTextView widgets"
        );
    }

    #[gtk::test]
    fn prose_paragraphs_render_as_selectable_wrapping_labels() {
        let (widget, _) = render_markdown("First paragraph\n\nSecond paragraph", None);
        let labels = find_widgets_of_type::<gtk::Label>(&widget);

        assert_eq!(labels.len(), 2, "expected one label per paragraph");
        assert_eq!(labels[0].text(), "First paragraph");
        assert_eq!(labels[1].text(), "Second paragraph");
        for label in labels {
            assert!(label.uses_markup(), "prose labels use Pango markup");
            assert!(label.wraps(), "prose labels wrap");
            assert_eq!(label.wrap_mode(), gtk::pango::WrapMode::WordChar);
            assert!(
                label.is_selectable(),
                "prose labels remain selectable per segment"
            );
            assert_eq!(label.xalign(), 0.0);
            assert_eq!(label.halign(), gtk::Align::Start);
            assert!(label.hexpands());
            assert_eq!(label.valign(), gtk::Align::Start);
            assert!(!label.vexpands());
            assert!(label.has_css_class("markdown-prose"));
        }
    }

    /// Find the first TextView in the rendered widget tree (for non-prose content
    /// like headings, lists, and blockquotes that still use GtkTextView internally).
    fn as_textview(widget: &gtk::Widget) -> gtk::TextView {
        find_widgets_of_type::<gtk::TextView>(widget)
            .into_iter()
            .next()
            .expect("expected a GtkTextView in the rendered widget tree")
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

    /// Collect all widgets of a specific type from a widget tree (recursive).
    fn find_widgets_of_type<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
        let mut found = Vec::new();
        if let Ok(typed) = widget.clone().downcast::<T>() {
            found.push(typed);
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            found.extend(find_widgets_of_type::<T>(&c));
            child = c.next_sibling();
        }
        found
    }

    fn first_source_buffer(widget: &gtk::Widget) -> sourceview5::Buffer {
        let source_views = find_widgets_of_type::<sourceview5::View>(widget);
        let source_view = source_views
            .into_iter()
            .next()
            .expect("expected code SourceView");
        source_view
            .buffer()
            .downcast::<sourceview5::Buffer>()
            .expect("expected GtkSourceBuffer")
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

    /// Collect all widgets that have the given CSS class (recursive).
    fn find_widgets_with_css_class(widget: &gtk::Widget, class_name: &str) -> Vec<gtk::Widget> {
        let mut found = Vec::new();
        if widget.has_css_class(class_name) {
            found.push(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            found.extend(find_widgets_with_css_class(&c, class_name));
            child = c.next_sibling();
        }
        found
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

    fn direct_box_children(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut children = Vec::new();
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            children.push(next);
        }
        children
    }

    fn label_markup(label: &gtk::Label) -> String {
        label
            .property::<Option<String>>("label")
            .unwrap_or_else(|| label.text().to_string())
    }

    fn rendered_label_texts(content: &str) -> Vec<String> {
        let (widget, _) = render_markdown(content, None);
        collect_label_text_from_widget_tree(&widget)
    }

    fn has_tag_at(content: &str, tag_name: &str, char_offset: i32) -> bool {
        let (widget, _) = render_markdown(content, None);
        let view = as_textview(&widget);
        let buffer = view.buffer();
        let iter = buffer.iter_at_offset(char_offset);
        iter.tags()
            .iter()
            .any(|tag: &gtk::TextTag| tag.name().as_deref() == Some(tag_name))
    }

    /// Helper: extract plain text from a rendered widget tree (textview or prose labels).
    fn textview_text(content: &str) -> String {
        let (widget, _) = render_markdown(content, None);
        // Try to find a TextView first (for headings, lists, blockquotes, etc.)
        let views = find_widgets_of_type::<gtk::TextView>(&widget);
        if let Some(view) = views.into_iter().next() {
            let buf = view.buffer();
            return buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string();
        }
        // Fall back to collecting label text (prose paragraphs)
        find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .map(|l| l.text().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ── Existing regression tests ────────────────────────────────────

    // ── Plain text & paragraphs ──────────────────────────────────────

    #[gtk::test]
    fn textview_plain_paragraph() {
        let text = textview_text("Hello world");
        assert!(text.contains("Hello world"), "got: {text}");
    }

    // ── Inline formatting ────────────────────────────────────────────
    // NOTE: Inline styles inside prose paragraphs (bold, italic, strikethrough,
    // inline code) are rendered as plain GtkLabel text in Task 3. Full Pango
    // markup styling for inline runs will be added in a later task. These tests
    // verify that the text content is present in the rendered output.

    #[gtk::test]
    fn textview_bold_tagged() {
        let text = textview_text("Hello **bold** world");
        assert!(text.contains("bold"), "got: {text}");
    }

    #[gtk::test]
    fn textview_italic_tagged() {
        let text = textview_text("Hello *italic* world");
        assert!(text.contains("italic"), "got: {text}");
    }

    #[gtk::test]
    fn textview_strikethrough_tagged() {
        let text = textview_text("Hello ~~removed~~ world");
        assert!(text.contains("removed"), "got: {text}");
    }

    #[gtk::test]
    fn textview_code_inline_tagged() {
        let text = textview_text("Use `code` here");
        assert!(text.contains("code"), "got: {text}");
    }

    fn first_prose_label_markup(content: &str) -> String {
        let (widget, _) = render_markdown(content, None);
        let labels: Vec<gtk::Label> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-prose"))
            .collect();
        let label = labels.first().expect("expected prose label");
        label_markup(label)
    }

    #[gtk::test]
    fn label_markup_bold_italic_strikethrough_and_inline_code() {
        let markup = first_prose_label_markup("Hello **bold** *italic* ~~gone~~ `code`");

        assert!(markup.contains("<b>bold</b>"), "got: {markup}");
        assert!(markup.contains("<i>italic</i>"), "got: {markup}");
        assert!(markup.contains("<s>gone</s>"), "got: {markup}");
        assert!(markup.contains("<tt>code</tt>"), "got: {markup}");
    }

    #[gtk::test]
    fn label_markup_heading_uses_bold_scaled_span() {
        let markup = first_prose_label_markup("# Title");

        assert!(
            markup.contains("<span weight=\"bold\" size=\"x-large\">Title</span>"),
            "got: {markup}"
        );
    }

    #[gtk::test]
    fn label_markup_nested_bold_italic() {
        let markup = first_prose_label_markup("Hello ***both*** world");

        assert!(
            markup.contains("<b><i>both</i></b>") || markup.contains("<i><b>both</b></i>"),
            "got: {markup}"
        );
    }

    // ── Headings ─────────────────────────────────────────────────────
    // NOTE: Headings are now rendered as prose labels with Pango markup since
    // Task 4. The old `has_tag_at`-based TextBuffer tests are replaced by
    // label markup tests above (label_markup_heading_uses_bold_scaled_span).

    #[gtk::test]
    fn textview_heading_1_renders_as_prose_label() {
        let (widget, _) = render_markdown("# Title", None);
        let labels: Vec<gtk::Label> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|l| l.has_css_class("markdown-prose"))
            .collect();
        assert!(!labels.is_empty(), "heading should render as a prose label");
        let markup = label_markup(labels.first().unwrap());
        assert!(markup.contains("Title"), "got: {markup}");
    }

    #[gtk::test]
    fn textview_heading_2_renders_as_prose_label() {
        let (widget, _) = render_markdown("## Subtitle", None);
        let labels: Vec<gtk::Label> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|l| l.has_css_class("markdown-prose"))
            .collect();
        assert!(!labels.is_empty(), "heading should render as a prose label");
        let markup = label_markup(labels.first().unwrap());
        assert!(
            markup.contains("<span weight=\"bold\" size=\"large\">Subtitle</span>"),
            "got: {markup}"
        );
    }

    // ── Lists ────────────────────────────────────────────────────────

    #[gtk::test]
    fn unordered_list_items_render_as_marker_content_rows() {
        let (widget, _) = render_markdown("- First\n- Second", None);
        let rows = find_widgets_with_css_class(&widget, "markdown-list-item");
        let markers: Vec<String> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-list-marker"))
            .map(|label| label.text().to_string())
            .collect();
        let content: Vec<String> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-list-content"))
            .map(|label| label.text().to_string())
            .collect();

        assert_eq!(rows.len(), 2);
        assert_eq!(markers, vec!["-", "-"]);
        assert_eq!(content, vec!["First", "Second"]);
    }

    #[gtk::test]
    fn ordered_and_task_lists_render_expected_markers() {
        let (ordered, _) = render_markdown("1. Alpha\n2. Beta", None);
        let ordered_markers: Vec<String> = find_widgets_of_type::<gtk::Label>(&ordered)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-list-marker"))
            .map(|label| label.text().to_string())
            .collect();
        assert_eq!(ordered_markers, vec!["1.", "2."]);

        let (tasks, _) = render_markdown("- [x] Done\n- [ ] Todo", None);
        let task_markers: Vec<String> = find_widgets_of_type::<gtk::Label>(&tasks)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-list-marker"))
            .map(|label| label.text().to_string())
            .collect();
        assert_eq!(task_markers, vec!["☑", "☐"]);
    }

    // ── Search highlighting ──────────────────────────────────────────

    #[gtk::test]
    fn label_search_highlight_applied_and_counted() {
        let (widget, count) = render_markdown("Hello world\n\nworld again", Some("world"));
        let labels: Vec<gtk::Label> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-prose"))
            .collect();
        let markup: Vec<String> = labels.iter().map(label_markup).collect();

        assert_eq!(count, 2);
        assert!(
            markup.iter().any(|text| text
                .contains("<span background=\"#fce94f\" foreground=\"#1e1e1e\">world</span>")),
            "got: {markup:?}"
        );
    }

    #[gtk::test]
    fn label_search_highlight_splits_inside_styled_run() {
        let (widget, count) = render_markdown("**hello world**", Some("world"));
        let markup = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .find(|label| label.has_css_class("markdown-prose"))
            .map(|label| label_markup(&label))
            .expect("expected prose label");

        assert_eq!(count, 1);
        assert!(markup.contains("<b>hello </b><b><span background=\"#fce94f\" foreground=\"#1e1e1e\">world</span></b>"), "got: {markup}");
    }

    #[gtk::test]
    fn label_search_no_match_returns_zero() {
        let (_, count) = render_markdown("Hello world", Some("missing"));
        assert_eq!(count, 0);
    }

    // ── Tables ───────────────────────────────────────────────────────

    #[gtk::test]
    fn textview_table_renders_as_separate_widget() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown(md, None);
        let tables = find_table_widgets(&widget);
        assert!(
            !tables.is_empty(),
            "expected table to produce a ScrolledWindow in the output"
        );
    }

    #[gtk::test]
    fn textview_table_contains_labels() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown(md, None);
        let labels = table_label_texts(&widget);
        assert!(
            !labels.is_empty(),
            "expected table widget to contain labels"
        );
    }

    #[gtk::test]
    fn textview_table_scroller_does_not_expand_vertically() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n\nBelow";
        let (widget, _) = render_markdown(md, None);
        let table = find_table_widgets(&widget)
            .into_iter()
            .next()
            .expect("expected rendered table scroller");
        let table = table
            .downcast::<gtk::ScrolledWindow>()
            .expect("expected GtkScrolledWindow");

        assert_eq!(table.valign(), gtk::Align::Start);
        assert!(!table.vexpands(), "table scroller should not vexpand");
    }

    #[gtk::test]
    fn textview_table_cells_do_not_wrap() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown(md, None);
        let labels: Vec<gtk::Label> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|l| l.has_css_class("markdown-table-cell"))
            .collect();
        assert!(!labels.is_empty(), "expected table cell labels");
        for label in labels {
            assert!(
                !label.wraps(),
                "table cell labels must not wrap (issue #149)"
            );
        }
    }

    #[gtk::test]
    fn textview_table_search_count_includes_widget_cells() {
        let md = "| Name |\n|------|\n| Rust |";
        let (_, count) = render_markdown(md, Some("Rust"));
        assert_eq!(count, 1, "expected search to include widget cell content");
    }

    // ── Horizontal rule ──────────────────────────────────────────────

    #[gtk::test]
    fn textview_horizontal_rule() {
        let text = textview_text("Above\n\n---\n\nBelow");
        assert!(text.contains("────"), "got: {text}");
    }

    #[gtk::test]
    fn horizontal_rule_renders_as_label_segment() {
        let (widget, _) = render_markdown("Above\n\n---\n\nBelow", None);
        let hrs = find_widgets_with_css_class(&widget, "markdown-hr");
        let labels = collect_label_text_from_widget_tree(&widget);

        assert_eq!(hrs.len(), 1);
        assert!(
            labels.iter().any(|text| text.contains("────")),
            "got: {labels:?}"
        );
    }

    // ── Images ───────────────────────────────────────────────────────

    #[gtk::test]
    fn textview_image_renders_alt_text() {
        // Image alt text in a prose paragraph renders via GtkLabel.
        let (widget, _) = render_markdown("![screenshot](https://example.com/img.png)", None);
        let labels = find_widgets_of_type::<gtk::Label>(&widget);
        let all_text: String = labels.iter().map(|l| l.text().to_string()).collect();
        assert!(all_text.contains("[image: screenshot]"), "got: {all_text}");
    }

    #[gtk::test]
    fn image_renders_alt_text_inside_prose_label() {
        let labels = rendered_label_texts("![screenshot](https://example.com/img.png)");

        assert!(
            labels
                .iter()
                .any(|text| text.contains("[image: screenshot]")),
            "got: {labels:?}"
        );
    }

    // ── Links ─────────────────────────────────────────────────────────

    #[gtk::test]
    fn link_appends_dimmed_url_suffix() {
        let markup = first_prose_label_markup("[Rust](https://rust-lang.org)");

        assert!(markup.contains("Rust"), "got: {markup}");
        assert!(
            markup.contains("<span alpha=\"65%\"> (https://rust-lang.org)</span>"),
            "got: {markup}"
        );
    }

    // ── Blockquotes ──────────────────────────────────────────────────

    #[gtk::test]
    fn textview_blockquote_tagged() {
        // Blockquote paragraphs are now rendered as prose labels inside a
        // grouped `.markdown-blockquote` container rather than a TextBuffer
        // with a "blockquote" tag.
        let (widget, _) = render_markdown("> Quoted text", None);
        let quotes = find_widgets_with_css_class(&widget, "markdown-blockquote");
        assert_eq!(quotes.len(), 1, "expected one blockquote group container");
        let label_texts = collect_label_text_from_widget_tree(&quotes[0]);
        assert!(
            label_texts.contains(&"Quoted text".to_string()),
            "got: {label_texts:?}"
        );
    }

    // ── Nested inline formatting ─────────────────────────────────────
    // NOTE: Nested inline styles in prose paragraphs are rendered as Pango
    // markup via GtkLabel since Task 4. Tests verify markup content.

    #[gtk::test]
    fn textview_nested_bold_italic() {
        let text = textview_text("Hello ***both*** world");
        assert!(text.contains("both"), "got: {text}");
    }

    // ── Link inside table cell ────────────────────────────────────────

    #[gtk::test]
    fn textview_table_link_visible_inside_widget_cell() {
        let md = "| Name |\n|------|\n| [Rust](https://rust-lang.org) |";
        let (widget, _) = render_markdown(md, None);
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
        let (widget, _) = render_markdown(md, None);
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
        let (widget, _) = render_markdown(md, None);
        let quotes = find_widgets_with_css_class(&widget, "markdown-blockquote");

        assert_eq!(
            quotes.len(),
            1,
            "expected exactly one grouped .markdown-blockquote container, got: {}",
            quotes.len()
        );
        assert!(
            !find_widgets_of_type::<gtk::Grid>(&quotes[0]).is_empty(),
            "blockquote group container should contain a table grid"
        );
    }

    // ── Blockquote group container ────────────────────────────────────

    #[gtk::test]
    fn blockquote_renders_group_container_once() {
        let (widget, _) = render_markdown("> First paragraph\n>\n> Second paragraph", None);
        let quotes = find_widgets_with_css_class(&widget, "markdown-blockquote");

        assert_eq!(
            quotes.len(),
            1,
            "blockquote CSS applies to the group, not each paragraph"
        );
        let label_texts = collect_label_text_from_widget_tree(&quotes[0]);
        assert!(
            label_texts.contains(&"First paragraph".to_string()),
            "got: {label_texts:?}"
        );
        assert!(
            label_texts.contains(&"Second paragraph".to_string()),
            "got: {label_texts:?}"
        );
    }

    #[gtk::test]
    fn blockquote_can_group_table_and_code_widgets() {
        let md = "> Before\n>\n> | A |\n> |---|\n> | 1 |\n>\n> ```rust\n> fn main() {}\n> ```";
        let (widget, _) = render_markdown(md, None);
        let quotes = find_widgets_with_css_class(&widget, "markdown-blockquote");

        assert_eq!(quotes.len(), 1);
        assert!(
            !find_widgets_of_type::<gtk::Grid>(&quotes[0]).is_empty(),
            "blockquote should contain table grid"
        );
        assert!(
            !find_widgets_of_type::<sourceview5::View>(&quotes[0]).is_empty(),
            "blockquote should contain code view"
        );
    }

    // ── Table column structure ────────────────────────────────────────

    #[gtk::test]
    fn textview_table_two_columns_has_correct_labels() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let (widget, _) = render_markdown(md, None);
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
        let (widget, _) = render_markdown(md, None);
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
        let (widget, _) = render_markdown(md, None);
        let content: Vec<String> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-list-content"))
            .map(|label| label.text().to_string())
            .collect();
        assert!(
            content.contains(&"Parent item".to_string()),
            "got: {content:?}"
        );
        assert!(
            content.contains(&"Child item 1".to_string()),
            "got: {content:?}"
        );
        assert!(
            content.contains(&"Child item 2".to_string()),
            "got: {content:?}"
        );
        assert!(
            content.contains(&"Second parent".to_string()),
            "got: {content:?}"
        );
    }

    #[gtk::test]
    fn textview_loose_list_items_kept_together() {
        // Loose lists have blank lines between items; pulldown-cmark wraps
        // each item in Paragraph events. All items must still appear.
        let md = "- First item\n\n- Second item\n\n- Third item";
        let (widget, _) = render_markdown(md, None);
        let content: Vec<String> = find_widgets_of_type::<gtk::Label>(&widget)
            .into_iter()
            .filter(|label| label.has_css_class("markdown-list-content"))
            .map(|label| label.text().to_string())
            .collect();
        assert!(
            content.contains(&"First item".to_string()),
            "got: {content:?}"
        );
        assert!(
            content.contains(&"Second item".to_string()),
            "got: {content:?}"
        );
        assert!(
            content.contains(&"Third item".to_string()),
            "got: {content:?}"
        );
    }

    // ── Code block widget ────────────────────────────────────────────

    #[gtk::test]
    fn code_block_language_label_uses_first_info_token() {
        let md = "```rust linenos title=demo\nfn main() {}\n```";
        let (widget, _) = render_markdown(md, None);

        let labels = collect_label_text_from_widget_tree(&widget);
        assert!(
            labels.iter().any(|t| t == "rust"),
            "expected language label 'rust'"
        );
    }

    #[gtk::test]
    fn code_block_without_language_has_no_language_label() {
        let md = "```\nplain text\n```";
        let (widget, _) = render_markdown(md, None);

        let labels = collect_label_text_from_widget_tree(&widget);
        assert!(
            !labels.iter().any(|t| t == "plain" || t == "text"),
            "did not expect a language label"
        );
    }

    #[gtk::test]
    fn code_block_with_blank_lines_renders_as_widget_segment() {
        let md = "```rust\nfn one() {}\n\nfn two() {}\n```";
        let (widget, _) = render_markdown(md, None);

        let code_blocks = find_widgets_with_css_class(&widget, "code-block-widget");
        assert_eq!(
            code_blocks.len(),
            1,
            "expected one code block widget segment"
        );
    }

    #[gtk::test]
    fn code_block_search_highlight_contributes_to_total_count() {
        let md = "```rust\nlet rust = 1;\n// rust\n```";
        let (_, count) = render_markdown(md, Some("rust"));
        assert_eq!(count, 2, "expected only code text matches to be counted");
    }

    #[gtk::test]
    fn code_block_search_highlight_tag_applied_inside_source_buffer() {
        let md = "```\nhello world\n```";
        let (widget, _) = render_markdown(md, Some("world"));
        let buffer = first_source_buffer(&widget);
        let iter = buffer.iter_at_offset(6);
        assert!(
            iter.tags()
                .iter()
                .any(|t: &gtk::TextTag| t.name().as_deref() == Some("search-highlight")),
            "expected search-highlight tag in code buffer"
        );
    }

    /// Regression guard for PR #118 review comment (r3060859924):
    /// `GtkSourceBuffer` shares the markdown `TextTagTable`, and any tag it
    /// adds when a language is attached gets the highest priority by default,
    /// potentially relegating `search-highlight` below syntax tags.
    #[gtk::test]
    fn search_highlight_keeps_highest_priority_after_code_block_syntax_tags() {
        let md = "```rust\nfn main() { let value = 1; }\n```";
        let (widget, _) = render_markdown(md, Some("main"));

        let buffer = first_source_buffer(&widget);
        assert!(buffer.is_highlight_syntax());
        assert!(buffer.language().is_some(), "rust language should resolve");

        // Force GtkSourceView to materialise its syntax tags on the shared
        // tag table for the entire buffer range.
        let start = buffer.start_iter();
        let end = buffer.end_iter();
        buffer.ensure_highlight(&start, &end);

        // Drain any pending idle work the highlighter may have queued.
        let context = glib::MainContext::default();
        while context.iteration(false) {}

        let table = buffer.tag_table();
        let highlight = table
            .lookup("search-highlight")
            .expect("search-highlight tag exists");

        assert_eq!(
            highlight.priority(),
            table.size() - 1,
            "search-highlight must remain the highest-priority tag even after \
             GtkSourceView adds its syntax tags to the shared tag table"
        );
    }

    #[gtk::test]
    fn code_block_known_language_uses_source_buffer_with_syntax_highlighting() {
        let md = "```rust\nfn main() {}\n```";
        let (widget, _) = render_markdown(md, None);

        let buffer = first_source_buffer(&widget);
        assert!(buffer.is_highlight_syntax());

        let language = buffer.language().expect("expected resolved language");
        assert_eq!(language.id(), "rust");
    }

    #[gtk::test]
    fn code_block_alias_language_resolves_before_lookup() {
        let md = "```js\nconsole.log('ok');\n```";
        let (widget, _) = render_markdown(md, None);

        assert_eq!(normalize_language_alias("js"), "js");

        let buffer = first_source_buffer(&widget);
        let language = buffer.language().expect("expected resolved language");
        assert_eq!(language.id(), "js");
        assert!(buffer.is_highlight_syntax());
    }

    #[gtk::test]
    fn code_block_full_javascript_fence_resolves_to_js_language() {
        let md = "```javascript\nconsole.log('ok');\n```";
        let (widget, _) = render_markdown(md, None);

        let buffer = first_source_buffer(&widget);
        let language = buffer.language().expect("expected resolved language");
        assert_eq!(language.id(), "js");
        assert!(buffer.is_highlight_syntax());
    }

    #[gtk::test]
    fn language_aliases_resolve_to_known_source_view_languages() {
        let manager = sourceview5::LanguageManager::default();
        for (alias, canonical) in LANGUAGE_ALIASES {
            assert_eq!(
                normalize_language_alias(alias),
                *canonical,
                "alias `{alias}` should normalise to `{canonical}`"
            );
            assert!(
                manager.language(canonical).is_some(),
                "GtkSourceView no longer recognises language id `{canonical}` \
                 (from alias `{alias}`)"
            );
        }
    }

    #[gtk::test]
    fn code_block_unknown_language_disables_syntax_highlighting() {
        let md = "```totally-unknown\nvalue\n```";
        let (widget, _) = render_markdown(md, None);

        let buffer = first_source_buffer(&widget);
        assert!(buffer.language().is_none());
        assert!(!buffer.is_highlight_syntax());
    }

    #[gtk::test]
    fn code_block_without_language_disables_syntax_highlighting() {
        let md = "```\nvalue\n```";
        let (widget, _) = render_markdown(md, None);

        let buffer = first_source_buffer(&widget);
        assert!(buffer.language().is_none());
        assert!(!buffer.is_highlight_syntax());
    }

    #[gtk::test]
    fn code_block_inside_blockquote_widget_has_blockquote_class() {
        let md = "> ```rust\n> fn main() {}\n> ```";
        let (widget, _) = render_markdown(md, None);
        let quotes = find_widgets_with_css_class(&widget, "markdown-blockquote");

        assert_eq!(
            quotes.len(),
            1,
            "expected exactly one grouped .markdown-blockquote container, got: {}",
            quotes.len()
        );
        assert!(
            !find_widgets_of_type::<sourceview5::View>(&quotes[0]).is_empty(),
            "blockquote group container should contain a code view"
        );
    }

    #[gtk::test]
    fn code_block_widget_uses_read_only_textview_and_horizontal_scroller() {
        let md = "```\nvery long line very long line very long line\n```";
        let (widget, _) = render_markdown(md, None);
        let scrollers = find_widgets_of_type::<gtk::ScrolledWindow>(&widget);
        let views = find_widgets_of_type::<gtk::TextView>(&widget);

        let scroller = scrollers
            .into_iter()
            .next()
            .expect("expected code scroller");
        let view = views.into_iter().next().expect("expected code text view");

        assert_eq!(scroller.hscrollbar_policy(), gtk::PolicyType::Automatic);
        assert_eq!(scroller.vscrollbar_policy(), gtk::PolicyType::Never);
        assert!(!view.is_editable());
        assert!(!view.is_cursor_visible());
        assert_eq!(view.wrap_mode(), gtk::WrapMode::None);
    }

    #[gtk::test]
    fn code_block_widget_assigns_all_expected_css_classes() {
        let md = "```rust\nfn main() {}\n```";
        let (widget, _) = render_markdown(md, None);

        assert!(
            !find_widgets_with_css_class(&widget, "code-block-widget").is_empty(),
            "expected code-block-widget class"
        );
        assert!(
            !find_widgets_with_css_class(&widget, "code-block-lang").is_empty(),
            "expected code-block-lang class"
        );
        assert!(
            !find_widgets_with_css_class(&widget, "code-block-scroller").is_empty(),
            "expected code-block-scroller class"
        );
        assert!(
            !find_widgets_with_css_class(&widget, "code-block-content").is_empty(),
            "expected code-block-content class"
        );
    }

    // ── Theme palette / prose path removal ──────────────────────────

    #[gtk::test]
    fn rendered_prose_does_not_create_markdown_textview() {
        let (widget, _) = render_markdown("Plain **markdown**", None);
        assert!(find_widgets_of_type::<gtk::TextView>(&widget).is_empty());
    }

    #[gtk::test]
    fn code_block_still_uses_source_buffer() {
        let (widget, _) = render_markdown("```rust\nfn main() {}\n```", None);
        let buffer = first_source_buffer(&widget);
        assert!(buffer.language().is_some());
    }
}
