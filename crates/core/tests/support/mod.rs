use rusqlite::Connection;
use sessions_chronicle_core::database::schema::initialize_database;
use std::path::PathBuf;
use tempfile::TempDir;

pub struct TempDatabase {
    _directory: TempDir,
    path: PathBuf,
}

impl TempDatabase {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sessions.db");
        let connection = Connection::open(&path).unwrap();
        initialize_database(&connection).unwrap();
        drop(connection);
        Self {
            _directory: directory,
            path,
        }
    }

    pub fn seed_session(&self, id: &str, last_updated: i64, is_subagent: bool, messages: &[&str]) {
        let connection = Connection::open(&self.path).unwrap();
        connection
            .execute(
                "INSERT INTO sessions (
                    id, tool, start_time, message_count, file_path, last_updated,
                    is_subagent
                ) VALUES (?1, 'claude_code', ?2, ?3, ?4, ?2, ?5)",
                rusqlite::params![
                    id,
                    last_updated,
                    messages.len() as i64,
                    format!("/{id}.jsonl"),
                    is_subagent as i64,
                ],
            )
            .unwrap();
        for (message_index, content) in messages.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO messages (
                        session_id, message_index, role, content, timestamp
                    ) VALUES (?1, ?2, 'user', ?3, ?4)",
                    rusqlite::params![id, message_index as i64, content, last_updated],
                )
                .unwrap();
        }
    }

    pub fn search_connection(
        &self,
    ) -> (
        sessions_chronicle_core::database::shell_search::ShellSearchConnection,
        sessions_chronicle_core::database::shell_search::ShellSearchInterrupt,
    ) {
        sessions_chronicle_core::database::shell_search::ShellSearchConnection::open_read_only(
            &self.path,
        )
        .unwrap()
        .unwrap()
    }
}

impl Default for TempDatabase {
    fn default() -> Self {
        Self::new()
    }
}
