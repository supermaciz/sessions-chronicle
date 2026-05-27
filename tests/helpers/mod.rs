use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::schema::initialize_database;

pub struct TempDatabase {
    pub path: PathBuf,
    pub connection: Connection,
}

impl TempDatabase {
    pub fn new(name_prefix: &str) -> Self {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "sessions-chronicle-{}-{}-{}.db",
            name_prefix,
            std::process::id(),
            nanos
        ));

        let connection = Connection::open(&path).expect("Failed to open temp database");
        initialize_database(&connection).expect("Failed to initialize database");

        Self { path, connection }
    }

    /// Seed a deterministic two-project dataset for sidebar filtering tests.
    ///
    /// Inserts:
    /// - Projects: alpha (id=1), beta (id=2)
    /// - Sessions: 3 in alpha, 1 in beta, and 1 unassigned session
    /// - Messages:
    ///   - "this session is lonely" on unassigned-claude
    ///   - "alpha topic" on alpha-claude-new
    ///
    /// This fixture is used to validate project-scoped filtering,
    /// unassigned-session behavior, and search behavior across projects.
    #[allow(dead_code)]
    pub fn seed_project_sidebar_fixture(&self) {
        self.connection
            .execute(
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![1_i64, "/projects/alpha", "alpha"],
            )
            .expect("Failed to insert project alpha");

        self.connection
            .execute(
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![2_i64, "/projects/beta", "beta"],
            )
            .expect("Failed to insert project beta");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "alpha-claude-old",
                    "claude_code",
                    Some("/projects/alpha"),
                    Some(1_i64),
                    10_i64,
                    2_i64,
                    "/tmp/alpha-claude-old.jsonl",
                    100_i64,
                ],
            )
            .expect("Failed to insert alpha old claude session");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "alpha-claude-new",
                    "claude_code",
                    Some("/projects/alpha"),
                    Some(1_i64),
                    20_i64,
                    3_i64,
                    "/tmp/alpha-claude-new.jsonl",
                    200_i64,
                ],
            )
            .expect("Failed to insert alpha new claude session");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "alpha-opencode",
                    "opencode",
                    Some("/projects/alpha"),
                    Some(1_i64),
                    30_i64,
                    2_i64,
                    "/tmp/alpha-opencode.jsonl",
                    300_i64,
                ],
            )
            .expect("Failed to insert alpha opencode session");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "unassigned-claude",
                    "claude_code",
                    Option::<String>::None,
                    Option::<i64>::None,
                    40_i64,
                    1_i64,
                    "/tmp/unassigned-claude.jsonl",
                    400_i64,
                ],
            )
            .expect("Failed to insert unassigned claude session");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "beta-claude",
                    "claude_code",
                    Some("/projects/beta"),
                    Some(2_i64),
                    50_i64,
                    1_i64,
                    "/tmp/beta-claude.jsonl",
                    500_i64,
                ],
            )
            .expect("Failed to insert beta claude session");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "unassigned-claude",
                    0_i64,
                    "user",
                    "this session is lonely",
                    1_i64,
                    Option::<String>::None,
                ],
            )
            .expect("Failed to insert message for unassigned claude session");

        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "alpha-claude-new",
                    0_i64,
                    "user",
                    "alpha topic",
                    2_i64,
                    Option::<String>::None,
                ],
            )
            .expect("Failed to insert message for alpha claude session");
    }

    #[allow(dead_code)]
    pub fn insert_project(&self, id: i64, path: &str, name: &str) {
        self.connection
            .execute(
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, path, name],
            )
            .expect("Failed to insert project");
    }

    #[allow(dead_code)]
    pub fn insert_session(
        &self,
        id: &str,
        tool: &str,
        project_path: Option<&str>,
        project_id: Option<i64>,
        start_time: i64,
        last_updated: i64,
    ) {
        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    id,
                    tool,
                    project_path,
                    project_id,
                    start_time,
                    1_i64,
                    format!("/tmp/{id}.jsonl"),
                    last_updated,
                ],
            )
            .expect("Failed to insert session");
    }

    #[allow(dead_code)]
    pub fn insert_message(
        &self,
        session_id: &str,
        message_index: i64,
        content: &str,
        timestamp: i64,
    ) {
        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    session_id,
                    message_index,
                    "user",
                    content,
                    timestamp,
                    Option::<String>::None,
                ],
            )
            .expect("Failed to insert message");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
