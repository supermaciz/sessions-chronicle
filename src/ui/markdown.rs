use html2pango::html_escape;
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

/// Parse markdown into intermediate blocks with Pango-markup strings.
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
    let mut list_ordered: Option<bool> = None;
    let mut list_items: Vec<String> = Vec::new();
    let mut task_list_items: Vec<(bool, String)> = Vec::new();
    let mut is_task_list = false;
    let mut current_task_checked: Option<bool> = None;
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
                list_ordered = Some(start.is_some());
                list_items.clear();
                task_list_items.clear();
                is_task_list = false;
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
                        html_escape(&url)
                    ));
                }
            }
            Event::Text(text) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&text);
                } else {
                    inline_buf.push_str(&html_escape(&text));
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&html);
                } else {
                    inline_buf.push_str(&html_escape(&html));
                }
            }
            Event::Code(code) => {
                inline_buf.push_str(&format!("<tt>{}</tt>", html_escape(&code)));
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
                is_task_list = true;
                current_task_checked = Some(checked);
            }
            Event::End(TagEnd::Paragraph) => {
                let text = std::mem::take(&mut inline_buf);
                if !text.is_empty() {
                    if in_blockquote {
                        blockquote_blocks.push(MarkdownBlock::Paragraph(text));
                    } else {
                        blocks.push(MarkdownBlock::Paragraph(text));
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
                if is_task_list {
                    let checked = current_task_checked.take().unwrap_or(false);
                    task_list_items.push((checked, text));
                } else {
                    list_items.push(text);
                }
            }
            Event::End(TagEnd::List(_)) => {
                let block = if is_task_list {
                    MarkdownBlock::TaskList(std::mem::take(&mut task_list_items))
                } else {
                    MarkdownBlock::List {
                        ordered: list_ordered.unwrap_or(false),
                        items: std::mem::take(&mut list_items),
                    }
                };
                if in_blockquote {
                    blockquote_blocks.push(block);
                } else {
                    blocks.push(block);
                }
                list_ordered = None;
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
pub fn render_markdown(content: &str) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 4);

    for block in markdown_to_blocks(content) {
        render_block(&container, block);
    }

    container
}

/// Render a single `MarkdownBlock` as GTK widgets appended to `container`.
/// Called recursively for blockquotes.
fn render_block(container: &gtk::Box, block: MarkdownBlock) {
    match block {
        MarkdownBlock::Paragraph(markup) => {
            let label = gtk::Label::new(None);
            label.set_markup(&markup);
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_halign(gtk::Align::Start);
            label.set_xalign(0.0);
            label.set_selectable(true);
            container.append(&label);
        }
        MarkdownBlock::Heading { level, content } => {
            let label = gtk::Label::new(None);
            label.set_markup(&content);
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

                let text_label = gtk::Label::new(None);
                text_label.set_markup(item_markup);
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

                let text_label = gtk::Label::new(None);
                text_label.set_markup(&item_markup);
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
                render_block(&quote_box, inner);
            }

            container.append(&quote_box);
        }
        MarkdownBlock::Table { headers, rows } => {
            let grid = gtk::Grid::new();
            grid.add_css_class("markdown-table");
            grid.set_column_spacing(12);
            grid.set_row_spacing(4);

            for (col, header) in headers.iter().enumerate() {
                let label = gtk::Label::new(None);
                label.set_markup(header);
                label.add_css_class("markdown-table-header");
                label.set_halign(gtk::Align::Start);
                label.set_hexpand(true);
                grid.attach(&label, col as i32, 0, 1, 1);
            }

            for (row_index, row) in rows.iter().enumerate() {
                for (col, cell) in row.iter().enumerate() {
                    let label = gtk::Label::new(None);
                    label.set_markup(cell);
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
}
