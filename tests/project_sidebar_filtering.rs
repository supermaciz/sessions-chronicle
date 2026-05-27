mod helpers;

use helpers::TempDatabase;
use sessions_chronicle::database::{
    count_all_sessions, count_unassigned_sessions, has_unassigned_sessions, load_projects,
    load_sessions_for_filter, search_sessions_for_filter,
};
use sessions_chronicle::models::{AiAssistant, DateFilter, ProjectFilter};

impl TempDatabase {
    /// Seed the broader multi-project fixture used by count and ordering tests.
    ///
    /// Projects: alpha (id=1), beta (id=2), gamma (id=3)
    /// Sessions: claude-alpha-1, claude-alpha-2 (project alpha), claude-unassigned-1, opencode-beta-1 (project beta)
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

#[test]
fn load_projects_orders_by_activity_and_keeps_zero_count_rows() {
    let db = TempDatabase::new("project-sidebar");
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

    let projects = load_projects(&db.path, &[AiAssistant::ClaudeCode], &DateFilter::AnyTime)
        .expect("Load projects failed");
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

    let all_projects = load_projects(&db.path, AiAssistant::ALL, &DateFilter::AnyTime)
        .expect("Load all projects failed");
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

    let empty_projects = load_projects(&db.path, &[], &DateFilter::AnyTime)
        .expect("Load empty tool projects failed");
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
    let db = TempDatabase::new("project-sidebar");
    db.seed();

    let all_count = count_all_sessions(&db.path, &[AiAssistant::ClaudeCode], &DateFilter::AnyTime)
        .expect("Count all sessions failed");
    assert_eq!(all_count, 3);

    let all_tools_count = count_all_sessions(&db.path, AiAssistant::ALL, &DateFilter::AnyTime)
        .expect("Count all sessions with all tools failed");
    assert_eq!(all_tools_count, 4);

    let no_tools_count = count_all_sessions(&db.path, &[], &DateFilter::AnyTime)
        .expect("Count all sessions with empty tools failed");
    assert_eq!(no_tools_count, 0);

    let unassigned_count =
        count_unassigned_sessions(&db.path, &[AiAssistant::ClaudeCode], &DateFilter::AnyTime)
            .expect("Count unassigned sessions failed");
    assert_eq!(unassigned_count, 1);

    let all_tools_unassigned =
        count_unassigned_sessions(&db.path, AiAssistant::ALL, &DateFilter::AnyTime)
            .expect("Count unassigned sessions with all tools failed");
    assert_eq!(all_tools_unassigned, 1);

    let no_tools_unassigned = count_unassigned_sessions(&db.path, &[], &DateFilter::AnyTime)
        .expect("Count unassigned sessions with empty tools failed");
    assert_eq!(no_tools_unassigned, 0);

    let has_unassigned = has_unassigned_sessions(&db.path).expect("Has unassigned check failed");
    assert!(has_unassigned);
}

#[test]
fn load_sessions_for_filter_returns_project_and_tool_intersection() {
    let db = TempDatabase::new("project-sidebar");
    db.seed_project_sidebar_fixture();

    let sessions = load_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::Project(1),
        &DateFilter::AnyTime,
    )
    .expect("load sessions");

    let ids: Vec<&str> = sessions.iter().map(|session| session.id.as_str()).collect();
    assert_eq!(ids, vec!["alpha-claude-new", "alpha-claude-old"]);
}

#[test]
fn search_sessions_for_filter_returns_only_unassigned_matches() {
    let db = TempDatabase::new("project-sidebar");
    db.seed_project_sidebar_fixture();

    let sessions = search_sessions_for_filter(
        &db.path,
        &[AiAssistant::ClaudeCode],
        &ProjectFilter::Unassigned,
        "lonely",
        &DateFilter::AnyTime,
    )
    .expect("search sessions");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "unassigned-claude");
    assert_eq!(sessions[0].project_id, None);
}
