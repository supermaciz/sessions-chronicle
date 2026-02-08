use crate::ui::markdown::pango_escape;

/// Highlight background and foreground colors (Tango yellow).
const HIGHLIGHT_BG: &str = "#fce94f";
const HIGHLIGHT_FG: &str = "#1e1e1e";

/// Highlight every case-insensitive occurrence of `query` in `text`.
///
/// Escapes `text` for Pango markup and wraps each match with a `<span>` tag.
/// Returns the resulting Pango markup string and the number of matches found.
pub fn highlight_text(text: &str, query: &str) -> (String, usize) {
    if query.is_empty() {
        return (pango_escape(text), 0);
    }

    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    let mut result = String::with_capacity(text.len() * 2);
    let mut count = 0usize;
    let mut pos = 0usize;

    while let Some(match_start) = lower_text[pos..].find(&lower_query) {
        let abs_start = pos + match_start;
        let abs_end = abs_start + query.len();

        // Append escaped non-match segment
        result.push_str(&pango_escape(&text[pos..abs_start]));
        // Append highlighted match (using original case)
        result.push_str(&format!(
            "<span background=\"{}\" foreground=\"{}\">",
            HIGHLIGHT_BG, HIGHLIGHT_FG
        ));
        result.push_str(&pango_escape(&text[abs_start..abs_end]));
        result.push_str("</span>");
        count += 1;
        pos = abs_end;
    }

    // Append remaining text
    result.push_str(&pango_escape(&text[pos..]));

    (result, count)
}

/// Highlight every case-insensitive occurrence of `query` inside existing Pango
/// markup, skipping `<…>` tags and `&…;` entities so they are not altered.
///
/// Returns the highlighted markup and the number of matches found.
pub fn highlight_in_markup(markup: &str, query: &str) -> (String, usize) {
    if query.is_empty() {
        return (markup.to_string(), 0);
    }

    // Extract visible text segments with their byte ranges in the markup.
    let segments = extract_text_segments(markup);

    // Collect all visible text into one string so we can match across segments.
    let visible: String = segments.iter().map(|(_, text)| text.as_str()).collect();
    let lower_visible = visible.to_lowercase();
    let lower_query = query.to_lowercase();

    // Find match positions in the visible-text string.
    let mut match_positions: Vec<(usize, usize)> = Vec::new();
    let mut search_pos = 0;
    while let Some(start) = lower_visible[search_pos..].find(&lower_query) {
        let abs = search_pos + start;
        match_positions.push((abs, abs + query.len()));
        search_pos = abs + query.len();
    }

    let count = match_positions.len();
    if count == 0 {
        return (markup.to_string(), 0);
    }

    // Map visible-text offsets back to markup byte ranges.
    // Build a map: for each visible-text offset, what markup byte offset is it?
    let mut vis_to_markup: Vec<usize> = Vec::with_capacity(visible.len() + 1);
    for (markup_start, text) in &segments {
        for (i, _) in text.char_indices() {
            vis_to_markup.push(markup_start + i);
        }
        // End sentinel for the segment
    }
    // Final sentinel: end of all visible text maps to the position after the last segment
    if let Some((start, text)) = segments.last() {
        vis_to_markup.push(start + text.len());
    }

    // Build highlighted markup by walking the original markup and inserting spans.
    // We need: for each match in visible-text coords, find the corresponding
    // markup byte positions.
    let mut result = String::with_capacity(markup.len() * 2);
    let mut markup_pos = 0usize;

    for &(vis_start, vis_end) in &match_positions {
        let m_start = vis_to_markup[vis_start];
        let m_end = vis_to_markup[vis_end];

        // Copy markup from current position to match start verbatim
        result.push_str(&markup[markup_pos..m_start]);
        // Insert highlight span around the matched region
        // The matched region may contain tags — we must highlight only text parts.
        result.push_str(&wrap_text_in_markup(
            &markup[m_start..m_end],
            HIGHLIGHT_BG,
            HIGHLIGHT_FG,
        ));
        markup_pos = m_end;
    }

    // Append remaining markup
    result.push_str(&markup[markup_pos..]);

    (result, count)
}

/// Extract visible text segments from Pango markup.
/// Returns `(byte_offset_in_markup, visible_text)` pairs.
fn extract_text_segments(markup: &str) -> Vec<(usize, String)> {
    let mut segments = Vec::new();
    let bytes = markup.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'<' {
            // Skip tag
            while i < len && bytes[i] != b'>' {
                i += 1;
            }
            if i < len {
                i += 1; // skip '>'
            }
        } else {
            // Text segment
            let start = i;
            let mut text = String::new();
            while i < len && bytes[i] != b'<' {
                if bytes[i] == b'&' {
                    // Entity — copy verbatim but don't count as separate chars
                    // for matching purposes. Decode for visible text.
                    let entity_start = i;
                    while i < len && bytes[i] != b';' {
                        i += 1;
                    }
                    if i < len {
                        i += 1; // skip ';'
                    }
                    let entity = &markup[entity_start..i];
                    text.push_str(&decode_entity(entity));
                } else {
                    text.push(bytes[i] as char);
                    i += 1;
                }
            }
            if !text.is_empty() {
                segments.push((start, text));
            }
        }
    }

    segments
}

/// Decode a Pango/HTML entity to its character representation.
fn decode_entity(entity: &str) -> String {
    match entity {
        "&amp;" => "&".to_string(),
        "&lt;" => "<".to_string(),
        "&gt;" => ">".to_string(),
        "&quot;" => "\"".to_string(),
        "&apos;" => "'".to_string(),
        _ => entity.to_string(),
    }
}

/// Wrap only the visible text portions within a markup fragment in highlight spans.
/// Tags are passed through unchanged.
fn wrap_text_in_markup(fragment: &str, bg: &str, fg: &str) -> String {
    let mut result = String::new();
    let bytes = fragment.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'<' {
            // Copy tag verbatim
            while i < len && bytes[i] != b'>' {
                result.push(bytes[i] as char);
                i += 1;
            }
            if i < len {
                result.push(bytes[i] as char);
                i += 1;
            }
        } else {
            // Text segment — wrap in highlight span
            result.push_str(&format!(
                "<span background=\"{}\" foreground=\"{}\">",
                bg, fg
            ));
            while i < len && bytes[i] != b'<' {
                result.push(bytes[i] as char);
                i += 1;
            }
            result.push_str("</span>");
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── highlight_text ──────────────────────────────────────────────────

    #[test]
    fn highlight_text_empty_query_returns_escaped() {
        let (markup, count) = highlight_text("Hello <world>", "");
        assert_eq!(count, 0);
        assert_eq!(markup, "Hello &lt;world&gt;");
    }

    #[test]
    fn highlight_text_single_match() {
        let (markup, count) = highlight_text("Hello world", "world");
        assert_eq!(count, 1);
        assert!(markup.contains("<span background="));
        assert!(markup.contains("world</span>"));
    }

    #[test]
    fn highlight_text_case_insensitive() {
        let (_markup, count) = highlight_text("Hello World WORLD", "world");
        assert_eq!(count, 2);
    }

    #[test]
    fn highlight_text_no_match() {
        let (markup, count) = highlight_text("Hello world", "missing");
        assert_eq!(count, 0);
        assert_eq!(markup, "Hello world");
    }

    #[test]
    fn highlight_text_escapes_special_chars() {
        let (markup, count) = highlight_text("a < b & c", "<");
        assert_eq!(count, 1);
        assert!(markup.contains("&lt;</span>"));
        assert!(markup.contains("&amp;"));
    }

    #[test]
    fn highlight_text_adjacent_matches() {
        let (_markup, count) = highlight_text("aaa", "a");
        assert_eq!(count, 3);
    }

    // ── highlight_in_markup ─────────────────────────────────────────────

    #[test]
    fn highlight_in_markup_empty_query() {
        let (result, count) = highlight_in_markup("<b>Hello</b>", "");
        assert_eq!(count, 0);
        assert_eq!(result, "<b>Hello</b>");
    }

    #[test]
    fn highlight_in_markup_simple() {
        let (result, count) = highlight_in_markup("<b>Hello</b> world", "world");
        assert_eq!(count, 1);
        assert!(result.contains("<span background="));
        assert!(result.contains("world</span>"));
        // Tags preserved
        assert!(result.contains("<b>Hello</b>"));
    }

    #[test]
    fn highlight_in_markup_inside_tag() {
        let (result, count) = highlight_in_markup("<b>Hello world</b>", "world");
        assert_eq!(count, 1);
        assert!(result.contains("<span background="));
    }

    #[test]
    fn highlight_in_markup_no_match() {
        let (result, count) = highlight_in_markup("<b>Hello</b>", "missing");
        assert_eq!(count, 0);
        assert_eq!(result, "<b>Hello</b>");
    }

    #[test]
    fn highlight_in_markup_preserves_entities() {
        let (result, count) = highlight_in_markup("a &amp; b", "& b");
        assert_eq!(count, 1);
        assert!(result.contains("<span background="));
    }

    #[test]
    fn highlight_in_markup_case_insensitive() {
        let (_result, count) = highlight_in_markup("<i>Hello WORLD</i>", "world");
        assert_eq!(count, 1);
    }

    #[test]
    fn highlight_in_markup_match_across_entity() {
        // "a&b" in visible text, query "a&b"
        let (result, count) = highlight_in_markup("a&amp;b", "a&b");
        assert_eq!(count, 1);
        assert!(result.contains("<span background="));
    }
}
