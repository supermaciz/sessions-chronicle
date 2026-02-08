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

    let matches = find_case_insensitive_matches_in_text(text, query);
    let mut result = String::with_capacity(text.len() * 2);
    let mut pos = 0usize;

    for (abs_start, abs_end) in &matches {
        let abs_start = *abs_start;
        let abs_end = *abs_end;

        // Append escaped non-match segment
        result.push_str(&pango_escape(&text[pos..abs_start]));
        // Append highlighted match (using original case)
        result.push_str(&format!(
            "<span background=\"{}\" foreground=\"{}\">",
            HIGHLIGHT_BG, HIGHLIGHT_FG
        ));
        result.push_str(&pango_escape(&text[abs_start..abs_end]));
        result.push_str("</span>");
        pos = abs_end;
    }

    // Append remaining text
    result.push_str(&pango_escape(&text[pos..]));

    (result, matches.len())
}

/// Highlight every case-insensitive occurrence of `query` inside existing Pango
/// markup, skipping `<…>` tags and `&…;` entities so they are not altered.
///
/// Returns the highlighted markup and the number of matches found.
pub fn highlight_in_markup(markup: &str, query: &str) -> (String, usize) {
    if query.is_empty() {
        return (markup.to_string(), 0);
    }

    let visible_units = extract_visible_units(markup);
    let match_positions = find_case_insensitive_matches_in_units(&visible_units, query);
    let count = match_positions.len();
    if count == 0 {
        return (markup.to_string(), 0);
    }

    // Build highlighted markup by walking the original markup and inserting spans.
    // We need: for each match in visible-text coords, find the corresponding
    // markup byte positions.
    let mut result = String::with_capacity(markup.len() * 2);
    let mut markup_pos = 0usize;

    for &(m_start, m_end) in &match_positions {
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

#[derive(Clone, Debug)]
struct VisibleUnit {
    ch: char,
    markup_start: usize,
    markup_end: usize,
}

fn fold_query_chars(query: &str) -> Vec<char> {
    query.chars().flat_map(char::to_lowercase).collect()
}

fn find_case_insensitive_matches_in_text(text: &str, query: &str) -> Vec<(usize, usize)> {
    let mut folded_units: Vec<(char, usize, usize)> = Vec::new();
    for (start, ch) in text.char_indices() {
        let end = start + ch.len_utf8();
        for lower in ch.to_lowercase() {
            folded_units.push((lower, start, end));
        }
    }

    let query_chars = fold_query_chars(query);
    find_case_insensitive_matches_in_folded_units(&folded_units, &query_chars)
}

fn find_case_insensitive_matches_in_units(
    units: &[VisibleUnit],
    query: &str,
) -> Vec<(usize, usize)> {
    let mut folded_units: Vec<(char, usize, usize)> = Vec::new();
    for unit in units {
        for lower in unit.ch.to_lowercase() {
            folded_units.push((lower, unit.markup_start, unit.markup_end));
        }
    }

    let query_chars = fold_query_chars(query);
    find_case_insensitive_matches_in_folded_units(&folded_units, &query_chars)
}

fn find_case_insensitive_matches_in_folded_units(
    folded_units: &[(char, usize, usize)],
    query_chars: &[char],
) -> Vec<(usize, usize)> {
    if query_chars.is_empty() || folded_units.is_empty() || query_chars.len() > folded_units.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut i = 0usize;
    while i + query_chars.len() <= folded_units.len() {
        let is_match = folded_units[i..i + query_chars.len()]
            .iter()
            .zip(query_chars.iter())
            .all(|((ch, _, _), q)| ch == q);

        if is_match {
            let start = folded_units[i].1;
            let end = folded_units[i + query_chars.len() - 1].2;
            matches.push((start, end));
            i += query_chars.len();
        } else {
            i += 1;
        }
    }

    matches
}

/// Extract visible characters from Pango markup and map each displayed character
/// back to a byte range in the original markup string.
fn extract_visible_units(markup: &str) -> Vec<VisibleUnit> {
    let mut units = Vec::new();
    let mut i = 0usize;

    while i < markup.len() {
        let rest = &markup[i..];

        if rest.starts_with('<') {
            if let Some(tag_end) = rest.find('>') {
                i += tag_end + 1;
            } else {
                break;
            }
            continue;
        }

        if rest.starts_with('&')
            && let Some(entity_end_rel) = rest.find(';')
        {
            let entity_end = i + entity_end_rel + 1;
            let entity = &markup[i..entity_end];
            let decoded = decode_entity(entity);
            for ch in decoded.chars() {
                units.push(VisibleUnit {
                    ch,
                    markup_start: i,
                    markup_end: entity_end,
                });
            }
            i = entity_end;
            continue;
        }

        let ch = rest
            .chars()
            .next()
            .expect("non-empty rest must have at least one char");
        let end = i + ch.len_utf8();
        units.push(VisibleUnit {
            ch,
            markup_start: i,
            markup_end: end,
        });
        i = end;
    }

    units
}

/// Wrap only the visible text portions within a markup fragment in highlight spans.
/// Tags are passed through unchanged.
fn wrap_text_in_markup(fragment: &str, bg: &str, fg: &str) -> String {
    let mut result = String::new();
    let mut i = 0;

    while i < fragment.len() {
        let rest = &fragment[i..];

        if rest.starts_with('<') {
            // Copy tag verbatim
            if let Some(tag_end) = rest.find('>') {
                let end = i + tag_end + 1;
                result.push_str(&fragment[i..end]);
                i = end;
            } else {
                result.push_str(rest);
                break;
            }
        } else {
            // Text segment — wrap in highlight span
            result.push_str(&format!(
                "<span background=\"{}\" foreground=\"{}\">",
                bg, fg
            ));
            if let Some(next_tag_rel) = rest.find('<') {
                let end = i + next_tag_rel;
                result.push_str(&fragment[i..end]);
                i = end;
            } else {
                result.push_str(rest);
                i = fragment.len();
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

    #[test]
    fn highlight_text_handles_unicode_case_folding_expansion() {
        let (markup, count) = highlight_text("İstanbul", "i");
        assert_eq!(count, 1);
        assert!(markup.contains("<span background="));
    }

    #[test]
    fn highlight_in_markup_preserves_entity_bytes_when_highlighting() {
        let (result, count) = highlight_in_markup("a&amp;b", "&b");
        assert_eq!(count, 1);
        assert!(
            result.contains("<span background=\"#fce94f\" foreground=\"#1e1e1e\">&amp;b</span>")
        );
    }

    #[test]
    fn highlight_in_markup_handles_utf8_visible_text() {
        let (result, count) = highlight_in_markup("<b>café 漢字</b>", "漢字");
        assert_eq!(count, 1);
        assert!(result.contains("<span background="));
    }
}
