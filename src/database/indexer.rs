use anyhow::{Context, Result};
use rusqlite::Connection;
use std::ffi::OsStr;
use std::path::Path;

use crate::parsers::claude_code::ClaudeCodeParser;

pub struct SessionIndexer {
    db: Connection,
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
                if Self::is_sidechain_file(path) {
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

    fn index_session_file(&mut self, file_path: &Path, parser: &ClaudeCodeParser) -> Result<()> {
        let session = parser.parse_metadata(file_path)?;
        let messages = parser.parse_messages(file_path)?;

        // Insert or update session
        self.db.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, project_path, start_time, message_count, file_path, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                &session.id,
                "claude_code",
                &session.project_path,
                session.start_time.timestamp(),
                session.message_count as i64,
                file_path.to_str(),
                session.last_updated.timestamp(),
            ],
        )?;

        // Delete old messages for this session
        self.db
            .execute("DELETE FROM messages WHERE session_id = ?1", [&session.id])?;

        // Insert new messages
        for msg in messages {
            self.db.execute(
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

        Ok(())
    }

    fn is_sidechain_file(file_path: &Path) -> bool {
        let is_agent_file = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.starts_with("agent-"));
        let is_subagent = file_path
            .components()
            .any(|component| component.as_os_str() == OsStr::new("subagents"));

        is_agent_file || is_subagent
    }

    fn remove_session_for_file(&mut self, file_path: &Path) -> Result<()> {
        let Some(file_path_str) = file_path.to_str() else {
            tracing::warn!("Cannot prune session with non-UTF8 path: {:?}", file_path);
            return Ok(());
        };
        self.db.execute(
            "DELETE FROM messages WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        self.db
            .execute("DELETE FROM sessions WHERE file_path = ?1", [file_path_str])?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn is_sidechain_file_detects_agent_prefix() {
        let path = PathBuf::from("/home/user/.claude/sessions/agent-abc123.jsonl");
        assert!(SessionIndexer::is_sidechain_file(&path));
    }

    #[test]
    fn is_sidechain_file_detects_subagents_directory() {
        let path = PathBuf::from("/home/user/.claude/sessions/subagents/some-session.jsonl");
        assert!(SessionIndexer::is_sidechain_file(&path));
    }

    #[test]
    fn is_sidechain_file_allows_regular_sessions() {
        let path = PathBuf::from("/home/user/.claude/sessions/abc123.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path));
    }

    #[test]
    fn is_sidechain_file_allows_agent_in_middle_of_name() {
        // "agent-" prefix is required, not just containing "agent"
        let path = PathBuf::from("/home/user/.claude/sessions/my-agent-session.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path));
    }
}
