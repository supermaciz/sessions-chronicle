use std::path::{Path, PathBuf};

use crate::models::session::AiAssistant;

/// Known subdirectory names used when resolving an override root.
const CLAUDE_SUBDIR: &str = "claude_sessions";
const OPENCODE_SUBDIR: &str = "opencode_storage";
const CODEX_SUBDIR: &str = "codex_sessions";
const VIBE_SUBDIR: &str = "vibe_sessions";

/// Resolved session source paths for all supported tools.
///
/// In override mode every path derives from a single user-supplied root.
/// In default mode each tool uses its own home-based default.
#[derive(Debug, Clone)]
pub struct SessionSources {
    pub claude_dir: PathBuf,
    pub opencode_storage_root: PathBuf,
    pub opencode_db_paths: Vec<PathBuf>,
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

        let opencode_storage_root = try_subdir(OPENCODE_SUBDIR);
        let opencode_db_paths = resolve_opencode_dbs(&opencode_storage_root);

        Self {
            claude_dir: try_subdir(CLAUDE_SUBDIR),
            opencode_storage_root,
            opencode_db_paths,
            codex_dir: try_subdir(CODEX_SUBDIR),
            vibe_dir: try_subdir(VIBE_SUBDIR),
            override_mode: true,
        }
    }

    fn resolve_defaults() -> Self {
        let opencode_session_dir = PathBuf::from(AiAssistant::OpenCode.session_dir());
        let opencode_storage_root = opencode_session_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(opencode_session_dir);

        let opencode_db_paths = resolve_opencode_dbs(&opencode_storage_root);

        Self {
            claude_dir: PathBuf::from(AiAssistant::ClaudeCode.session_dir()),
            opencode_storage_root,
            opencode_db_paths,
            codex_dir: PathBuf::from(AiAssistant::Codex.session_dir()),
            vibe_dir: PathBuf::from(AiAssistant::MistralVibe.session_dir()),
            override_mode: false,
        }
    }
}

fn collect_opencode_db_candidates(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut candidates: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            name == "opencode.db"
                || (name.starts_with("opencode-") && name.ends_with(".db") && name.len() > 12)
        })
        .collect();

    candidates.sort();
    candidates
}

fn resolve_opencode_dbs(storage_root: &Path) -> Vec<PathBuf> {
    let mut candidates = collect_opencode_db_candidates(storage_root);

    // In override mode (--sessions-dir), storage_root points to a subdirectory
    // like `fixtures/opencode_storage/`, but the real OpenCode layout can place
    // databases one level up alongside the storage directory.
    if let Some(parent) = storage_root.parent() {
        candidates.extend(collect_opencode_db_candidates(parent));
    }

    candidates.sort();
    candidates.dedup();

    candidates
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
            PathBuf::from(AiAssistant::ClaudeCode.session_dir())
        );
        assert_eq!(
            sources.codex_dir,
            PathBuf::from(AiAssistant::Codex.session_dir())
        );
        assert_eq!(
            sources.vibe_dir,
            PathBuf::from(AiAssistant::MistralVibe.session_dir())
        );

        // OpenCode storage root is the parent of the session dir.
        let expected_opencode = PathBuf::from(AiAssistant::OpenCode.session_dir());
        let expected_root = expected_opencode.parent().unwrap();
        assert_eq!(sources.opencode_storage_root, expected_root);
    }

    #[test]
    fn db_filename_changes_in_override_mode() {
        assert_eq!(select_db_filename(false), "sessions.db");
        assert_eq!(select_db_filename(true), "sessions-override.db");
    }

    #[test]
    fn resolve_override_finds_opencode_db() {
        let root = PathBuf::from("tests/fixtures");
        let sources = SessionSources::resolve(Some(&root));
        assert_eq!(sources.opencode_db_paths.len(), 1);
        assert_eq!(
            sources.opencode_db_paths[0],
            root.join("opencode_storage").join("opencode.db")
        );
    }

    #[test]
    fn resolve_override_no_db_returns_none() {
        let root = PathBuf::from("tests/fixtures/claude_sessions");
        let sources = SessionSources::resolve(Some(&root));
        assert!(sources.opencode_db_paths.is_empty());
    }

    #[test]
    fn resolve_opencode_dbs_falls_back_to_parent_directory() {
        let temp = tempfile::tempdir().unwrap();
        let storage_root = temp.path().join("storage");
        std::fs::create_dir_all(&storage_root).unwrap();

        let parent_db = temp.path().join("opencode.db");
        std::fs::write(&parent_db, b"").unwrap();

        assert_eq!(resolve_opencode_dbs(&storage_root), vec![parent_db]);
    }

    #[test]
    fn resolve_opencode_dbs_finds_default_and_channel_variants() {
        let temp = tempfile::tempdir().unwrap();
        let storage_root = temp.path().join("storage");
        std::fs::create_dir_all(&storage_root).unwrap();

        let default_db = storage_root.join("opencode.db");
        let dev_db = storage_root.join("opencode-dev.db");
        let ignored = storage_root.join("other.db");
        std::fs::write(&default_db, b"").unwrap();
        std::fs::write(&dev_db, b"").unwrap();
        std::fs::write(&ignored, b"").unwrap();

        assert_eq!(
            resolve_opencode_dbs(&storage_root),
            vec![dev_db, default_db]
        );
    }
}
