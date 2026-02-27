# TextView Markdown Rendering — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:executing-plans to implement this plan task-by-task.

**Goal:** Replace the multi-widget markdown renderer with a single `gtk::TextView` per assistant message so text selection works across the entire message.

**Architecture:** A new `MarkdownBufferWriter` struct walks pulldown-cmark events directly and writes plain text into a `gtk::TextBuffer`, applying `TextTag`s for formatting. The existing `render_markdown()` widget-tree renderer is replaced by `render_markdown_to_textview()`. Search highlighting uses a dedicated `TextTag` applied in a second pass over the buffer.

**Tech Stack:** Rust, GTK4 (`gtk::TextView`, `TextBuffer`, `TextTag`), pulldown-cmark 0.13, relm4

**Design doc:** `docs/plans/2026-02-26-textview-markdown-design.md`

---

## Phase 1 — Core (paragraphs, headings, lists, inline formatting)

### Task 0: Make `find_case_insensitive_matches_in_text` public

The search highlighting pass needs to find matches in plain text extracted from the `TextBuffer`. The existing `find_case_insensitive_matches_in_text` function in `highlight.rs` does exactly this but is private. Make it public so the new code can reuse it.

**Files:**
- Modify: `src/ui/highlight.rs:106`

**Step 1: Change visibility**

In `src/ui/highlight.rs`, line 106, change:

```rust
fn find_case_insensitive_matches_in_text(text: &str, query: &str) -> Vec<(usize, usize)> {
```

to:

```rust
pub fn find_case_insensitive_matches_in_text(text: &str, query: &str) -> Vec<(usize, usize)> {
```

**Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: compiles cleanly (no new warnings — the function is not yet called externally)

**Step 3: Commit**

```bash
git add src/ui/highlight.rs
git commit -m "refactor: make find_case_insensitive_matches_in_text public"
```

---

### Task 1: Create `create_tag_table()` and tag catalogue

Create the `TextTagTable` with all formatting tags. This is a pure function with no UI dependencies — easy to validate in isolation.

**Files:**
- Modify: `src/ui/markdown.rs` (add at end, before `#[cfg(test)]`)

**Step 1: Write the tag table factory function**

Add to `src/ui/markdown.rs`, before the `#[cfg(test)]` block:

```rust
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
```

**Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: compiles cleanly (function unused for now, but no warnings because it's private)

**Step 3: Commit**

```bash
git add src/ui/markdown.rs
git commit -m "feat: add TextTag table factory for markdown formatting"
```

---

### Task 2: Implement `MarkdownBufferWriter` — struct and Phase 1 event handling

Create the core writer struct that walks pulldown-cmark events and writes to a `TextBuffer`. Phase 1 handles: paragraphs, headings, lists (ordered, unordered, task, nested), and inline formatting (bold, italic, strikethrough, code, links).

**Files:**
- Modify: `src/ui/markdown.rs` (add after `create_tag_table()`)

**Step 1: Write the writer struct and `process()` method**

Add after `create_tag_table()`:

```rust
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
                Event::End(TagEnd::Emphasis) => { self.tag_stack.retain(|t| *t != "italic"); }
                Event::End(TagEnd::Strong) => { self.tag_stack.retain(|t| *t != "bold"); }
                Event::End(TagEnd::Strikethrough) => { self.tag_stack.retain(|t| *t != "strikethrough"); }

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
                Event::Start(Tag::Heading { .. }) => {
                    self.block_separator();
                }
                Event::End(TagEnd::Heading(level)) => {
                    // The heading text was already inserted with heading tag
                    // via the Text event — but we need to apply the heading tag.
                    // Actually, we handle this by pushing heading tag before text.
                    self.insert_with_tags("\n", &[]);
                    self.has_content = true;
                    // Remove heading tag from stack
                    let heading_tag = match level {
                        pulldown_cmark::HeadingLevel::H1 => "heading-1",
                        pulldown_cmark::HeadingLevel::H2 => "heading-2",
                        pulldown_cmark::HeadingLevel::H3 => "heading-3",
                        _ => "heading-4",
                    };
                    self.tag_stack.retain(|t| *t != heading_tag);
                }
                Event::Start(Tag::Heading { level, .. }) if false => {
                    // This branch is unreachable — heading start is handled above.
                    // Included to satisfy exhaustiveness if needed.
                    let _ = level;
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
                    // Prepare the marker
                    if let Some(frame) = self.list_stack.last_mut() {
                        frame.1 += 1; // increment item index
                    }
                }
                Event::End(TagEnd::Item) => {}

                Event::TaskListMarker(checked) => {
                    if let Some(frame) = self.list_stack.last_mut() {
                        frame.2 = true; // mark as task list
                    }
                    self.current_task_checked = Some(checked);

                    // Insert the task marker
                    let marker = if checked { "[x] " } else { "[ ] " };
                    let tags = self.active_tags();
                    let mut tag_refs: Vec<&str> = tags.iter().copied().collect();
                    tag_refs.push("list-item");
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
        // If inside a heading, push the heading tag
        // (heading tag is on the tag_stack already via Start(Heading))
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
```

**Important note on heading handling:** The `Start(Tag::Heading)` event currently just calls `block_separator()`. The heading tag needs to be pushed onto `tag_stack` so that `emit_text()` applies it. Fix the heading start handler:

Replace the heading Start/End events with this corrected version:

```rust
Event::Start(Tag::Heading { level, .. }) => {
    self.block_separator();
    let heading_tag = match level {
        pulldown_cmark::HeadingLevel::H1 => "heading-1",
        pulldown_cmark::HeadingLevel::H2 => "heading-2",
        pulldown_cmark::HeadingLevel::H3 => "heading-3",
        _ => "heading-4",
    };
    self.tag_stack.push(heading_tag);
}
Event::End(TagEnd::Heading(level)) => {
    self.insert_with_tags("\n", &[]);
    self.has_content = true;
    let heading_tag = match level {
        pulldown_cmark::HeadingLevel::H1 => "heading-1",
        pulldown_cmark::HeadingLevel::H2 => "heading-2",
        pulldown_cmark::HeadingLevel::H3 => "heading-3",
        _ => "heading-4",
    };
    self.tag_stack.retain(|t| *t != heading_tag);
}
```

**Similarly for list items** — the `Start(Tag::Item)` event should insert the marker:

```rust
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
```

**Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -30`
Expected: compiles cleanly

**Step 3: Commit**

```bash
git add src/ui/markdown.rs
git commit -m "feat: add MarkdownBufferWriter for pulldown-cmark to TextBuffer rendering"
```

---

### Task 3: Implement `render_markdown_to_textview()` and search highlighting

Wire the writer into a public function that returns a configured `gtk::TextView`, and add the search highlighting pass.

**Files:**
- Modify: `src/ui/markdown.rs` (add public function after `MarkdownBufferWriter`)

**Step 1: Write the public entry point and search highlighting**

Add after the `MarkdownBufferWriter` impl block:

```rust
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
```

**Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: compiles cleanly

**Step 3: Commit**

```bash
git add src/ui/markdown.rs
git commit -m "feat: add render_markdown_to_textview with search highlighting"
```

---

### Task 4: Integrate into `render_content()` in `transcript_row.rs`

Switch assistant message rendering from the old widget-tree renderer to the new `TextView` renderer.

**Files:**
- Modify: `src/ui/transcript_row.rs:160-163`

**Step 1: Replace the assistant branch**

In `src/ui/transcript_row.rs`, change lines 160–163 from:

```rust
    if role == Role::Assistant {
        let rendered = markdown::render_markdown(content, highlight_query);
        match_count = rendered.1;
        container.append(&rendered.0);
```

to:

```rust
    if role == Role::Assistant {
        let (textview, count) = markdown::render_markdown_to_textview(content, highlight_query);
        match_count = count;
        container.append(&textview);
```

**Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: compiles cleanly

**Step 3: Manual visual test**

Run: `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`

Verify:
1. Assistant messages render with formatting (bold, italic, headings, lists)
2. Text selection works across multiple lines and blocks within a single assistant message
3. User messages still render as before (single label, selectable)
4. Search highlighting works in assistant messages

**Step 4: Commit**

```bash
git add src/ui/transcript_row.rs
git commit -m "feat: switch assistant messages to TextView-based markdown rendering

Fixes cross-block text selection in assistant messages by using a
single gtk::TextView with TextTags instead of multiple gtk::Label
widgets."
```

---

### Task 5: Write unit tests for `render_markdown_to_textview`

Add tests that verify the `TextBuffer` contents and tag application for the new renderer. These tests require GTK initialization, so they use `gtk::init()`.

**Files:**
- Modify: `src/ui/markdown.rs` (add to `#[cfg(test)] mod tests` block)

**Step 1: Add tests**

Add to the existing `mod tests` block in `src/ui/markdown.rs`:

```rust
    // ── render_markdown_to_textview tests ─────────────────────────────

    /// Helper: extract plain text from a rendered textview.
    fn textview_text(content: &str) -> String {
        gtk::init().ok();
        let (view, _) = render_markdown_to_textview(content, None);
        let buf = view.buffer();
        buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string()
    }

    /// Helper: check if a tag is applied at a given char offset.
    fn has_tag_at(content: &str, tag_name: &str, char_offset: i32) -> bool {
        gtk::init().ok();
        let (view, _) = render_markdown_to_textview(content, None);
        let buf = view.buffer();
        let iter = buf.iter_at_offset(char_offset);
        iter.tags().iter().any(|t| {
            t.name().map_or(false, |n| n == tag_name)
        })
    }

    #[test]
    fn textview_plain_paragraph() {
        let text = textview_text("Hello world");
        assert!(text.contains("Hello world"), "got: {text}");
    }

    #[test]
    fn textview_bold_tagged() {
        assert!(has_tag_at("Hello **bold** world", "bold", 6));
    }

    #[test]
    fn textview_italic_tagged() {
        assert!(has_tag_at("Hello *italic* world", "italic", 6));
    }

    #[test]
    fn textview_heading_tagged() {
        assert!(has_tag_at("# Title", "heading-1", 0));
    }

    #[test]
    fn textview_code_inline_tagged() {
        assert!(has_tag_at("Use `code` here", "code-inline", 4));
    }

    #[test]
    fn textview_unordered_list_contains_marker() {
        let text = textview_text("- First\n- Second");
        assert!(text.contains("- First"), "got: {text}");
        assert!(text.contains("- Second"), "got: {text}");
    }

    #[test]
    fn textview_ordered_list_contains_numbers() {
        let text = textview_text("1. Alpha\n2. Beta");
        assert!(text.contains("1. Alpha") || text.contains("1."), "got: {text}");
        assert!(text.contains("2. Beta") || text.contains("2."), "got: {text}");
    }

    #[test]
    fn textview_code_block_tagged() {
        assert!(has_tag_at("```\ncode line\n```", "code-block", 0));
    }

    #[test]
    fn textview_search_highlight_applied() {
        gtk::init().ok();
        let (view, count) = render_markdown_to_textview("Hello world", Some("world"));
        assert_eq!(count, 1);
        let buf = view.buffer();
        // "world" starts at char 6 in "Hello world\n"
        let iter = buf.iter_at_offset(6);
        assert!(iter.tags().iter().any(|t| {
            t.name().map_or(false, |n| n == "search-highlight")
        }));
    }

    #[test]
    fn textview_table_rendered_as_text() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let text = textview_text(md);
        assert!(text.contains("A"), "got: {text}");
        assert!(text.contains("B"), "got: {text}");
        assert!(text.contains("1"), "got: {text}");
        assert!(text.contains("2"), "got: {text}");
        assert!(text.contains("─"), "separator missing, got: {text}");
    }

    #[test]
    fn textview_horizontal_rule() {
        let text = textview_text("Above\n\n---\n\nBelow");
        assert!(text.contains("────"), "got: {text}");
    }
```

**Step 2: Run the tests**

Run: `cargo test --all --no-fail-fast 2>&1 | tail -30`
Expected: all tests pass (new and old)

Note: If GTK init fails in CI (headless), these tests will be skipped gracefully due to `gtk::init().ok()` — the helper returns an empty/invalid view but won't panic. If tests do fail due to missing display, gate them with `#[ignore]` and a comment.

**Step 3: Commit**

```bash
git add src/ui/markdown.rs
git commit -m "test: add unit tests for render_markdown_to_textview"
```

---

### Task 6: Visual validation and polish

Final manual testing pass with fixture data and real session data. Fix any rendering issues found.

**Files:**
- Possibly modify: `src/ui/markdown.rs` (tag properties, spacing)
- Possibly modify: `data/resources/style.css` (TextView-specific styles)

**Step 1: Test with fixtures**

Run: `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`

Check each assistant message for:
- [ ] Bold, italic, strikethrough render correctly
- [ ] Headings have appropriate size/weight
- [ ] Lists show markers and proper indentation
- [ ] Code blocks have monospace font and background
- [ ] Tables are readable with aligned columns
- [ ] Blockquotes are indented and dimmed
- [ ] Horizontal rules are visible
- [ ] Text selection works across ALL block types in a single drag
- [ ] Search highlighting works and match count is correct
- [ ] Expand/collapse still works for long messages

**Step 2: Test with real session data**

Run: `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle`

Verify the same checklist with real Claude session data.

**Step 3: Fix any issues found**

If spacing is off, adjust `pixels-above-lines` / `pixels-below-lines` values in `create_tag_table()`. If colors don't match the theme, use Adwaita CSS variables via `GtkStyleContext` lookup instead of hardcoded hex values.

**Step 4: Commit fixes**

```bash
git add -u
git commit -m "fix: polish TextView markdown rendering spacing and colors"
```

---

### Task 7: Run CI-parity checks

**Step 1: Run full check suite**

```bash
cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast
```

Expected: all three pass cleanly.

**Step 2: Fix any issues**

Address fmt/clippy/test failures.

**Step 3: Final commit if needed**

```bash
git add -u
git commit -m "chore: fix clippy warnings from TextView migration"
```

---

## Phase 2 — Deferred (already handled in Phase 1 writer)

The `MarkdownBufferWriter` in Task 2 already handles all block types (code blocks, tables, blockquotes, horizontal rules) as part of the unified event loop. Phase 2 is about **visual polish** of these block types after Phase 1 is validated:

- Adjust `paragraph-background` color for code blocks to match the current CSS theme
- Fine-tune table column width calculation for wide Unicode content
- Verify blockquote nesting renders correctly with cumulative left-margin
- Test horizontal rules visual weight

These are incremental tweaks to tag properties, not structural changes.

---

## Cleanup (after Phase 2 validation)

### Task 8: Remove old widget-based renderer

Once the `TextView` renderer is confirmed stable:

**Files:**
- Modify: `src/ui/markdown.rs` — remove `render_markdown()`, `render_block()`, `apply_highlight()`
- Verify: `pango_escape()` is still needed by `highlight.rs` (yes, keep it)
- Verify: `markdown_to_blocks()` — keep for existing unit tests, or migrate tests to use `render_markdown_to_textview()` and remove

**Step 1: Remove dead code**

Delete `render_markdown()` (lines 347–356), `apply_highlight()` (lines 360–367), and `render_block()` (lines 371–541).

**Step 2: Verify**

Run: `cargo check && cargo test --all`

**Step 3: Commit**

```bash
git add src/ui/markdown.rs
git commit -m "refactor: remove old widget-based markdown renderer"
```
