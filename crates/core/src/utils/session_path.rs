//! Neutral session-path predicates shared by models and parsers.
//!
//! The predicates live here — not in a parser module — so
//! `models::session` can apply resume rules without importing parser
//! internals.  The Unified parser re-exports the predicate it shares.

use std::path::Path;

/// A Unified Vibe session directory lives directly inside a directory
/// named `unified`.
pub fn is_unified_session_dir(path: &Path) -> bool {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "unified")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn matches_only_direct_children_of_a_unified_directory() {
        assert!(is_unified_session_dir(Path::new(
            "/home/u/.vibe/logs/session/unified/aaaaaaaa-0000-4000-8000-000000000302"
        )));
        // A legacy Vibe session keeps its historical layout.
        assert!(!is_unified_session_dir(Path::new(
            "/home/u/.vibe/logs/session/session_20261020_100000_bbbbbbbb"
        )));
        // The `unified` directory itself and deeper paths are not
        // session directories.
        assert!(!is_unified_session_dir(Path::new(
            "/home/u/.vibe/logs/session/unified"
        )));
        assert!(!is_unified_session_dir(Path::new(
            "/home/u/.vibe/logs/session/unified/id/deeper"
        )));
        // The predicate is purely lexical: any path whose parent's
        // final component is `unified` matches, relative or not.
        assert!(is_unified_session_dir(Path::new("unified/id")));
    }
}
