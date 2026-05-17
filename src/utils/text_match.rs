//! Case-insensitive substring matching shared by the transcript highlighter
//! and the session-detail search counter.
//!
//! Both must agree on what counts as an occurrence so the `X / Y` match
//! counter stays equal to the number of spans the user sees highlighted.

/// Returns the byte ranges of every non-overlapping, case-insensitive
/// occurrence of `query` in `text`, in document order.
///
/// Matching folds case per Unicode rules (so a single source character may
/// expand to several folded units, e.g. `İ`), and ranges index back into the
/// original, unfolded `text`.
pub fn find_case_insensitive_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    let mut folded_units: Vec<(char, usize, usize)> = Vec::new();
    for (start, ch) in text.char_indices() {
        let end = start + ch.len_utf8();
        for lower in ch.to_lowercase() {
            folded_units.push((lower, start, end));
        }
    }

    let query_chars = fold_query_chars(query);
    find_in_folded_units(&folded_units, &query_chars)
}

/// Number of non-overlapping, case-insensitive occurrences of `query` in
/// `text` — the count of spans [`find_case_insensitive_matches`] would mark.
pub fn count_case_insensitive_matches(text: &str, query: &str) -> usize {
    find_case_insensitive_matches(text, query).len()
}

fn fold_query_chars(query: &str) -> Vec<char> {
    query.chars().flat_map(char::to_lowercase).collect()
}

fn find_in_folded_units(
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
    fn empty_query_matches_nothing() {
        assert!(find_case_insensitive_matches("Hello world", "").is_empty());
        assert_eq!(count_case_insensitive_matches("Hello world", ""), 0);
    }

    #[test]
    fn counts_case_insensitive_occurrences() {
        assert_eq!(
            count_case_insensitive_matches("Hello World WORLD", "world"),
            2
        );
    }

    #[test]
    fn counts_adjacent_non_overlapping_occurrences() {
        assert_eq!(count_case_insensitive_matches("aaa", "a"), 3);
        assert_eq!(count_case_insensitive_matches("aaaa", "aa"), 2);
    }

    #[test]
    fn ranges_index_into_original_text() {
        let matches = find_case_insensitive_matches("a NEEDLE here", "needle");
        assert_eq!(matches, vec![(2, 8)]);
    }

    #[test]
    fn no_match_returns_empty() {
        assert_eq!(count_case_insensitive_matches("Hello world", "missing"), 0);
    }

    #[test]
    fn handles_unicode_case_folding_expansion() {
        assert_eq!(count_case_insensitive_matches("İstanbul", "i"), 1);
    }
}
