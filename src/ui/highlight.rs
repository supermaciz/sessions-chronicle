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

fn fold_query_chars(query: &str) -> Vec<char> {
    query.chars().flat_map(char::to_lowercase).collect()
}

pub fn find_case_insensitive_matches_in_text(text: &str, query: &str) -> Vec<(usize, usize)> {
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn highlight_text_handles_unicode_case_folding_expansion() {
        let (markup, count) = highlight_text("İstanbul", "i");
        assert_eq!(count, 1);
        assert!(markup.contains("<span background="));
    }
}
