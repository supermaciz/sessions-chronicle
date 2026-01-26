use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

use crate::parsers::claude_code::ClaudeCodeParser;
use crate::parsers::opencode::{OpenCodeParser, ParseError};

pub struct SessionIndexer {
    db: Connection,
}

fn is_skippable_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<ParseError>().is_some()
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
                    Ok(indexed) => {
                        if indexed {
                            count += 1;
                        }
                    }
                    Err(err) => {
                        if is_skippable_error(&err) {
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

    fn index_session_file(&mut self, file_path: &Path, parser: &ClaudeCodeParser) -> Result<()> {
        let (session, messages) = parser.parse(file_path)?;
        self.insert_session_and_messages(&session, &messages, file_path)?;
        Ok(())
    }

    fn index_opencode_session_file(
        &mut self,
        file_path: &Path,
        parser: &OpenCodeParser,
    ) -> Result<bool> {
        let (session, messages) = parser.parse(file_path)?;
        self.insert_session_and_messages(&session, &messages, file_path)?;
        Ok(true)
    }

    fn insert_session_and_messages(
        &mut self,
        session: &crate::models::Session,
        messages: &[crate::models::Message],
        file_path: &Path,
    ) -> Result<()> {
        let tx = self.db.transaction()?;

        tx.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, project_path, start_time, message_count, file_path, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                &session.id,
                session.tool.to_storage(),
                &session.project_path,
                session.start_time.timestamp(),
                session.message_count as i64,
                file_path.to_str(),
                session.last_updated.timestamp(),
            ],
        )?;

        tx.execute("DELETE FROM messages WHERE session_id = ?1", [&session.id])?;

        for msg in messages {
            tx.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    &msg.session_id,
                    msg.index as i64,
                    format!("{:?}", msg.role).to_lowercase(),
                    &msg.content,
                    msg.timestamp.timestamp(),
                ],
            )?;
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

    fn remove_session_for_file(&mut self, file_path: &Path) -> Result<()> {
        let Some(file_path_str) = file_path.to_str() else {
            tracing::warn!("Cannot prune session with non-UTF8 path: {:?}", file_path);
            return Ok(());
        };

        let tx = self.db.transaction()?;

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
    fn opencode_indexing_indexes_sessions_and_prunes_subagents() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

        let count = indexer.index_opencode_sessions(&storage_root).unwrap();
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
        assert_eq!(sessions[0].0, "session-001");
        assert_eq!(sessions[0].1, "opencode");
        assert_eq!(sessions[1].0, "session-003");
        assert_eq!(sessions[1].1, "opencode");

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
}
