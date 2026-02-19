use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

use crate::parsers::ParsedSession;
use crate::parsers::claude_code::ClaudeCodeParser;
use crate::parsers::codex::{CodexParser, ParseError as CodexParseError};
use crate::parsers::mistral_vibe::{MistralVibeParser, ParseError as MistralVibeParseError};
use crate::parsers::opencode::{OpenCodeParser, ParseError as OpenCodeParseError};

pub struct SessionIndexer {
    db: Connection,
}

fn is_opencode_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<OpenCodeParseError>().is_some()
}

fn is_codex_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<CodexParseError>().is_some()
}

impl SessionIndexer {
    pub fn new(db_path: &Path) -> Result<Self> {
        let db = Connection::open(db_path).context("Failed to open database")?;
        crate::database::schema::initialize_database(&db)
            .context("Failed to initialize database schema")?;
        Ok(Self { db })
    }

    pub fn index_claude_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        let parser = ClaudeCodeParser;
        let mut count = 0;

        for entry in walkdir::WalkDir::new(sessions_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if entry.file_type().is_file()
                && let Some(ext) = path.extension()
                && ext == "jsonl"
            {
                if Self::is_sidechain_file(path, sessions_dir) {
                    if let Err(err) = self.remove_session_for_file(path) {
                        tracing::warn!(
                            "Failed to prune sidechain session {}: {}",
                            path.display(),
                            err
                        );
                    }
                    continue;
                }
                if let Err(e) = self.index_session_file(path, &parser) {
                    tracing::warn!("Failed to index {}: {}", path.display(), e);
                } else {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    pub fn index_opencode_sessions(&mut self, storage_root: &Path) -> Result<usize> {
        let sessions_dir = storage_root.join("session");

        if !sessions_dir.exists() {
            return Ok(0);
        }

        let parser = OpenCodeParser::new(storage_root);
        let mut count = 0;

        for entry in walkdir::WalkDir::new(&sessions_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if entry.file_type().is_file()
                && let Some(ext) = path.extension()
                && ext == "json"
            {
                match self.index_opencode_session_file(path, &parser) {
                    Ok(()) => {
                        count += 1;
                    }
                    Err(err) => {
                        if is_opencode_error(&err) {
                            tracing::debug!("Skipped OpenCode session {}: {}", path.display(), err);
                            if let Err(remove_err) = self.remove_session_for_file(path) {
                                tracing::warn!(
                                    "Failed to prune session {}: {}",
                                    path.display(),
                                    remove_err
                                );
                            }
                        } else {
                            tracing::warn!("Failed to index {}: {}", path.display(), err);
                        }
                    }
                }
            }
        }

        Ok(count)
    }

    pub fn index_codex_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let parser = CodexParser;
        let mut count = 0;

        for entry in walkdir::WalkDir::new(sessions_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if entry.file_type().is_file()
                && let Some(file_name) = path.file_name().and_then(|name| name.to_str())
                && file_name.starts_with("rollout-")
                && file_name.ends_with(".jsonl")
            {
                match self.index_codex_session_file(path, &parser) {
                    Ok(()) => {
                        count += 1;
                    }
                    Err(err) => {
                        if is_codex_error(&err) {
                            tracing::warn!("Skipped Codex session {}: {}", path.display(), err);
                            if let Err(remove_err) = self.remove_session_for_file(path) {
                                tracing::warn!(
                                    "Failed to prune session {}: {}",
                                    path.display(),
                                    remove_err
                                );
                            }
                        } else {
                            tracing::warn!("Failed to index {}: {}", path.display(), err);
                        }
                    }
                }
            }
        }

        Ok(count)
    }

    pub fn index_vibe_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let parser = MistralVibeParser;
        let mut count = 0;

        let entries = std::fs::read_dir(sessions_dir)
            .with_context(|| format!("Failed to read {}", sessions_dir.display()))?;

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!("Failed to read Mistral Vibe session entry: {}", err);
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if !path.join("meta.json").exists() || !path.join("messages.jsonl").exists() {
                continue;
            }

            match parser.parse(&path) {
                Ok(parsed) => {
                    self.insert_parsed_session(&parsed, &path)?;
                    count += 1;
                }
                Err(err) => {
                    if matches!(
                        err.downcast_ref::<MistralVibeParseError>(),
                        Some(MistralVibeParseError::NoUserMessages)
                    ) {
                        if let Err(remove_err) = self.remove_session_for_file(&path) {
                            tracing::warn!(
                                "Failed to prune session {}: {}",
                                path.display(),
                                remove_err
                            );
                        }
                    } else {
                        tracing::warn!("Failed to index {}: {}", path.display(), err);
                    }
                }
            }
        }

        Ok(count)
    }

    fn index_session_file(&mut self, file_path: &Path, parser: &ClaudeCodeParser) -> Result<()> {
        let parsed = parser.parse(file_path)?;
        self.insert_parsed_session(&parsed, file_path)?;
        Ok(())
    }

    fn index_opencode_session_file(
        &mut self,
        file_path: &Path,
        parser: &OpenCodeParser,
    ) -> Result<()> {
        let parsed = parser.parse(file_path)?;
        self.insert_parsed_session(&parsed, file_path)?;
        Ok(())
    }

    fn index_codex_session_file(&mut self, file_path: &Path, parser: &CodexParser) -> Result<()> {
        let parsed = parser.parse(file_path)?;
        self.insert_parsed_session(&parsed, file_path)?;
        Ok(())
    }

    fn insert_parsed_session(&mut self, parsed: &ParsedSession, file_path: &Path) -> Result<()> {
        let session = &parsed.session;
        let tx = self.db.transaction()?;

        tx.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, project_path, start_time, message_count, file_path, last_updated,
              first_prompt, parent_session_id, is_subagent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                &session.id,
                session.tool.to_storage(),
                &session.project_path,
                session.start_time.timestamp(),
                session.message_count as i64,
                file_path.to_str(),
                session.last_updated.timestamp(),
                &session.first_prompt,
                &session.parent_session_id,
                session.is_subagent as i64,
            ],
        )?;

        tx.execute("DELETE FROM messages WHERE session_id = ?1", [&session.id])?;
        tx.execute(
            "DELETE FROM transcript_items WHERE session_id = ?1",
            [&session.id],
        )?;
        tx.execute(
            "DELETE FROM tool_calls WHERE session_id = ?1",
            [&session.id],
        )?;
        tx.execute("DELETE FROM subagents WHERE session_id = ?1", [&session.id])?;

        for msg in &parsed.messages {
            tx.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    &session.id,
                    msg.index as i64,
                    format!("{:?}", msg.role).to_lowercase(),
                    &msg.content,
                    msg.timestamp.timestamp(),
                ],
            )?;
        }

        for tc in &parsed.tool_calls {
            crate::database::insert_tool_call(&tx, tc, &session.id)?;
        }

        for sa in &parsed.subagents {
            crate::database::insert_subagent(&tx, sa, &session.id)?;
        }

        for item in &parsed.transcript_items {
            crate::database::insert_transcript_item(&tx, item, &session.id)?;
        }

        tx.commit()?;

        Ok(())
    }

    fn is_sidechain_file(file_path: &Path, sessions_dir: &Path) -> bool {
        let is_agent_file = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.starts_with("agent-"));

        // Check if path is under sessions_dir/subagents/
        let is_subagent = file_path
            .strip_prefix(sessions_dir)
            .ok()
            .and_then(|rel| rel.components().next())
            .is_some_and(|first| first.as_os_str() == "subagents");

        is_agent_file || is_subagent
    }

    /// Clear all indexed sessions and messages.
    ///
    /// Note: `messages` is an FTS5 virtual table. Standard `DELETE FROM` works
    /// correctly on FTS5 tables and participates in transactions normally.
    pub fn clear_all_sessions(&mut self) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM transcript_items", [])?;
        tx.execute("DELETE FROM tool_calls", [])?;
        tx.execute("DELETE FROM subagents", [])?;
        tx.execute("DELETE FROM messages", [])?;
        tx.execute("DELETE FROM sessions", [])?;
        tx.commit()?;
        Ok(())
    }

    fn remove_session_for_file(&mut self, file_path: &Path) -> Result<()> {
        let Some(file_path_str) = file_path.to_str() else {
            tracing::warn!("Cannot prune session with non-UTF8 path: {:?}", file_path);
            return Ok(());
        };

        let tx = self.db.transaction()?;

        tx.execute(
            "DELETE FROM transcript_items WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute(
            "DELETE FROM tool_calls WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute(
            "DELETE FROM subagents WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute(
            "DELETE FROM messages WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute("DELETE FROM sessions WHERE file_path = ?1", [file_path_str])?;

        tx.commit()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[test]
    fn is_sidechain_file_detects_agent_prefix() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/agent-abc123.jsonl");
        assert!(SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_detects_subagents_directory() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/subagents/some-session.jsonl");
        assert!(SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_allows_regular_sessions() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/abc123.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_allows_agent_in_middle_of_name() {
        // "agent-" prefix is required, not just containing "agent"
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/my-agent-session.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_allows_subagents_in_project_name() {
        // "subagents" in an encoded project path should not trigger filtering
        let sessions_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/-home-user-subagents/session.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn opencode_indexing_indexes_all_sessions_including_subagents() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

        let count = indexer.index_opencode_sessions(&storage_root).unwrap();
        // session-002 has parentID but no messages → NoUserMessages → pruned
        // session-tools-child has parentID and messages → indexed as is_subagent=1
        assert_eq!(count, 4);

        let all_sessions: Vec<(String, String, i64)> = indexer
            .db
            .prepare("SELECT id, tool, is_subagent FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(all_sessions.len(), 4);
        assert!(all_sessions.iter().all(|(_, tool, _)| tool == "opencode"));

        let ids: Vec<&str> = all_sessions.iter().map(|(id, _, _)| id.as_str()).collect();
        assert!(ids.contains(&"session-001"));
        assert!(ids.contains(&"session-003"));
        assert!(ids.contains(&"session-tools-001"));
        assert!(ids.contains(&"session-tools-child"));

        // Verify subagent flag
        let child = all_sessions
            .iter()
            .find(|(id, _, _)| id == "session-tools-child")
            .unwrap();
        assert_eq!(
            child.2, 1,
            "session-tools-child should be marked as subagent"
        );

        let normal = all_sessions
            .iter()
            .find(|(id, _, _)| id == "session-001")
            .unwrap();
        assert_eq!(normal.2, 0, "session-001 should not be marked as subagent");

        let msg_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert!(msg_count > 0, "Should have messages for indexed sessions");
    }

    #[test]
    fn opencode_indexing_returns_zero_for_missing_storage_root() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let nonexistent_root = PathBuf::from("tests/fixtures/nonexistent_opencode_storage");

        let count = indexer.index_opencode_sessions(&nonexistent_root).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn codex_indexing_indexes_sessions() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/codex_sessions");

        let count = indexer.index_codex_sessions(&sessions_dir).unwrap();
        assert_eq!(count, 2);

        let sessions: Vec<(String, String)> = indexer
            .db
            .prepare("SELECT id, tool FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|(_, tool)| tool == "codex"));
    }

    #[test]
    fn codex_indexing_returns_zero_for_missing_sessions_dir() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let nonexistent_dir = PathBuf::from("tests/fixtures/nonexistent_codex_sessions");

        let count = indexer.index_codex_sessions(&nonexistent_dir).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn mistral_vibe_indexing_indexes_sessions() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/vibe_sessions");

        let count = indexer.index_vibe_sessions(&sessions_dir).unwrap();
        assert_eq!(count, 2);

        let sessions: Vec<(String, String)> = indexer
            .db
            .prepare("SELECT id, tool FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|(_, tool)| tool == "mistral_vibe"));
    }

    #[test]
    fn mistral_vibe_indexing_returns_zero_for_missing_sessions_dir() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let nonexistent_dir = PathBuf::from("tests/fixtures/nonexistent_vibe_sessions");

        let count = indexer.index_vibe_sessions(&nonexistent_dir).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn clear_all_sessions_removes_sessions_and_messages() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

        // Seed with real fixture data
        let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");
        let count = indexer.index_claude_sessions(&sessions_dir).unwrap();
        assert!(count > 0, "Should have indexed at least one session");

        let msg_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert!(msg_count > 0, "Should have messages before clear");

        // Clear everything
        indexer.clear_all_sessions().unwrap();

        let session_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(session_count, 0, "Sessions should be empty after clear");

        let msg_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(msg_count, 0, "Messages should be empty after clear");
    }
}
