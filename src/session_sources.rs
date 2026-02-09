use std::path::{Path, PathBuf};

use crate::models::session::Tool;

/// Known subdirectory names used when resolving an override root.
const CLAUDE_SUBDIR: &str = "claude_sessions";
const OPENCODE_SUBDIR: &str = "opencode_storage";
const CODEX_SUBDIR: &str = "codex_sessions";
const VIBE_SUBDIR: &str = "vibe_sessions";

/// Resolved session source paths for all supported tools.
///
/// In override mode every path derives from a single user-supplied root.
/// In default mode each tool uses its own home-based default.
pub struct SessionSources {
    pub claude_dir: PathBuf,
    pub opencode_storage_root: PathBuf,
    pub codex_dir: PathBuf,
    pub vibe_dir: PathBuf,
    pub override_mode: bool,
}

impl SessionSources {
    /// Resolve session source paths from an optional override root.
    ///
    /// Override mode: prefer known subdirectories under `root`; fall back to
    /// `root` itself when a subdirectory is missing.
    ///
    /// Default mode: derive paths from `Tool::session_dir()`.
    pub fn resolve(override_root: Option<&Path>) -> Self {
        match override_root {
            Some(root) => Self::resolve_override(root),
            None => Self::resolve_defaults(),
        }
    }

    fn resolve_override(root: &Path) -> Self {
        let try_subdir = |subdir: &str| -> PathBuf {
            let candidate = root.join(subdir);
            if candidate.exists() {
                candidate
            } else {
                root.to_path_buf()
            }
        };

        Self {
            claude_dir: try_subdir(CLAUDE_SUBDIR),
            opencode_storage_root: try_subdir(OPENCODE_SUBDIR),
            codex_dir: try_subdir(CODEX_SUBDIR),
            vibe_dir: try_subdir(VIBE_SUBDIR),
            override_mode: true,
        }
    }

    fn resolve_defaults() -> Self {
        let opencode_session_dir = PathBuf::from(Tool::OpenCode.session_dir());
        let opencode_storage_root = opencode_session_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(opencode_session_dir);

        Self {
            claude_dir: PathBuf::from(Tool::ClaudeCode.session_dir()),
            opencode_storage_root,
            codex_dir: PathBuf::from(Tool::Codex.session_dir()),
            vibe_dir: PathBuf::from(Tool::MistralVibe.session_dir()),
            override_mode: false,
        }
    }
}

/// Select the database filename based on override mode.
pub fn select_db_filename(override_mode: bool) -> &'static str {
    if override_mode {
        "sessions-override.db"
    } else {
        "sessions.db"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolve_override_prefers_known_subdirectories() {
        // tests/fixtures contains claude_sessions/, opencode_storage/,
        // codex_sessions/, vibe_sessions/
        let root = PathBuf::from("tests/fixtures");
        let sources = SessionSources::resolve(Some(&root));

        assert!(sources.override_mode);
        assert_eq!(sources.claude_dir, root.join("claude_sessions"));
        assert_eq!(sources.opencode_storage_root, root.join("opencode_storage"));
        assert_eq!(sources.codex_dir, root.join("codex_sessions"));
        assert_eq!(sources.vibe_dir, root.join("vibe_sessions"));
    }

    #[test]
    fn resolve_override_falls_back_to_root_when_subdirs_missing() {
        // Use a directory that exists but has no known subdirectories.
        let root = PathBuf::from("tests/fixtures/claude_sessions");
        let sources = SessionSources::resolve(Some(&root));

        assert!(sources.override_mode);
        // All paths should fall back to the root itself.
        assert_eq!(sources.claude_dir, root);
        assert_eq!(sources.opencode_storage_root, root);
        assert_eq!(sources.codex_dir, root);
        assert_eq!(sources.vibe_dir, root);
    }

    #[test]
    fn resolve_default_uses_tool_defaults() {
        let sources = SessionSources::resolve(None);

        assert!(!sources.override_mode);
        assert_eq!(
            sources.claude_dir,
            PathBuf::from(Tool::ClaudeCode.session_dir())
        );
        assert_eq!(sources.codex_dir, PathBuf::from(Tool::Codex.session_dir()));
        assert_eq!(
            sources.vibe_dir,
            PathBuf::from(Tool::MistralVibe.session_dir())
        );

        // OpenCode storage root is the parent of the session dir.
        let expected_opencode = PathBuf::from(Tool::OpenCode.session_dir());
        let expected_root = expected_opencode.parent().unwrap();
        assert_eq!(sources.opencode_storage_root, expected_root);
    }

    #[test]
    fn db_filename_changes_in_override_mode() {
        assert_eq!(select_db_filename(false), "sessions.db");
        assert_eq!(select_db_filename(true), "sessions-override.db");
    }
}
