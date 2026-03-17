use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::schema::initialize_database;
use sessions_chronicle::database::{
    count_all_sessions, count_unassigned_sessions, has_unassigned_sessions, load_projects,
};
use sessions_chronicle::models::AiAssistant;

struct TempDatabase {
    path: PathBuf,
    connection: Connection,
}

impl TempDatabase {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "sessions-chronicle-project-sidebar-test-{}-{}.db",
            std::process::id(),
            nanos
        ));

        let connection = Connection::open(&path).expect("Failed to open temp database");
        initialize_database(&connection).expect("Failed to initialize database");

        Self { path, connection }
    }

    fn seed(&self) {
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
                "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
                rusqlite::params![3_i64, "/projects/gamma", "gamma"],
            )
            .expect("Failed to insert project gamma");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "claude-alpha-1",
                    "claude_code",
                    Some("/projects/alpha"),
                    Some(1_i64),
                    10_i64,
                    3_i64,
                    "/tmp/claude-alpha-1.jsonl",
                    100_i64,
                ],
            )
            .expect("Failed to insert claude alpha session 1");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "claude-alpha-2",
                    "claude_code",
                    Some("/projects/alpha"),
                    Some(1_i64),
                    20_i64,
                    5_i64,
                    "/tmp/claude-alpha-2.jsonl",
                    200_i64,
                ],
            )
            .expect("Failed to insert claude alpha session 2");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "claude-unassigned-1",
                    "claude_code",
                    Option::<String>::None,
                    Option::<i64>::None,
                    30_i64,
                    4_i64,
                    "/tmp/claude-unassigned-1.jsonl",
                    300_i64,
                ],
            )
            .expect("Failed to insert unassigned claude session");

        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    "opencode-beta-1",
                    "opencode",
                    Some("/projects/beta"),
                    Some(2_i64),
                    40_i64,
                    2_i64,
                    "/tmp/opencode-beta-1.jsonl",
                    400_i64,
                ],
            )
            .expect("Failed to insert opencode beta session");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn load_projects_orders_by_activity_and_keeps_zero_count_rows() {
    let db = TempDatabase::new();
    db.seed();

    db.connection
        .execute(
            "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
            rusqlite::params![4_i64, "/projects/Delta", "Delta"],
        )
        .expect("Failed to insert project Delta");

    db.connection
        .execute(
            "INSERT INTO projects (id, path, name) VALUES (?1, ?2, ?3)",
            rusqlite::params![5_i64, "/projects/aardvark", "aardvark"],
        )
        .expect("Failed to insert project aardvark");

    let projects =
        load_projects(&db.path, &[AiAssistant::ClaudeCode]).expect("Load projects failed");
    let names: Vec<&str> = projects
        .iter()
        .map(|project| project.name.as_str())
        .collect();
    let counts: Vec<usize> = projects
        .iter()
        .map(|project| project.session_count)
        .collect();

    assert_eq!(names, vec!["alpha", "aardvark", "beta", "Delta", "gamma"]);
    assert_eq!(counts, vec![2, 0, 0, 0, 0]);

    let all_projects = load_projects(&db.path, AiAssistant::ALL).expect("Load all projects failed");
    let all_names: Vec<&str> = all_projects
        .iter()
        .map(|project| project.name.as_str())
        .collect();
    let all_counts: Vec<usize> = all_projects
        .iter()
        .map(|project| project.session_count)
        .collect();

    assert_eq!(
        all_names,
        vec!["beta", "alpha", "aardvark", "Delta", "gamma"]
    );
    assert_eq!(all_counts, vec![1, 2, 0, 0, 0]);

    let empty_projects = load_projects(&db.path, &[]).expect("Load empty tool projects failed");
    let empty_names: Vec<&str> = empty_projects
        .iter()
        .map(|project| project.name.as_str())
        .collect();
    let empty_counts: Vec<usize> = empty_projects
        .iter()
        .map(|project| project.session_count)
        .collect();

    assert_eq!(
        empty_names,
        vec!["aardvark", "alpha", "beta", "Delta", "gamma"]
    );
    assert_eq!(empty_counts, vec![0, 0, 0, 0, 0]);
}

#[test]
fn project_sidebar_counts_include_unassigned_visibility_flag() {
    let db = TempDatabase::new();
    db.seed();

    let all_count = count_all_sessions(&db.path, &[AiAssistant::ClaudeCode])
        .expect("Count all sessions failed");
    assert_eq!(all_count, 3);

    let all_tools_count = count_all_sessions(&db.path, AiAssistant::ALL)
        .expect("Count all sessions with all tools failed");
    assert_eq!(all_tools_count, 4);

    let no_tools_count =
        count_all_sessions(&db.path, &[]).expect("Count all sessions with empty tools failed");
    assert_eq!(no_tools_count, 0);

    let unassigned_count = count_unassigned_sessions(&db.path, &[AiAssistant::ClaudeCode])
        .expect("Count unassigned sessions failed");
    assert_eq!(unassigned_count, 1);

    let all_tools_unassigned = count_unassigned_sessions(&db.path, AiAssistant::ALL)
        .expect("Count unassigned sessions with all tools failed");
    assert_eq!(all_tools_unassigned, 1);

    let no_tools_unassigned = count_unassigned_sessions(&db.path, &[])
        .expect("Count unassigned sessions with empty tools failed");
    assert_eq!(no_tools_unassigned, 0);

    let has_unassigned = has_unassigned_sessions(&db.path).expect("Has unassigned check failed");
    assert!(has_unassigned);
}
