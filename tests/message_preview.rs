use rusqlite::Connection;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sessions_chronicle::database::load_message_previews_for_session;
use sessions_chronicle::database::schema::initialize_database;

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
            "sessions-chronicle-test-preview-{}-{}.db",
            std::process::id(),
            nanos
        ));
        let connection = Connection::open(&path).expect("Failed to open temp database");
        initialize_database(&connection).expect("Failed to initialize database");

        Self { path, connection }
    }

    fn insert_session(&self, session_id: &str) {
        self.connection
            .execute(
                "INSERT INTO sessions (id, tool, project_path, start_time, message_count, file_path, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    session_id,
                    "claude_code",
                    Some("/projects/test"),
                    1000_i64,
                    3_i64,
                    "/tmp/test-session.jsonl",
                    2000_i64,
                ],
            )
            .expect("Failed to insert session");
    }

    fn insert_message(&self, session_id: &str, index: i64, role: &str, content: &str) {
        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![session_id, index, role, content, 1000_i64 + index * 100],
            )
            .expect("Failed to insert message");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn load_message_previews_returns_numeric_order() {
    let db = TempDatabase::new();
    db.insert_session("test-session");

    // Insert messages in non-numeric order: 2, 10, 1
    db.insert_message("test-session", 2, "assistant", "Message 2");
    db.insert_message("test-session", 10, "assistant", "Message 10");
    db.insert_message("test-session", 1, "user", "Message 1");

    let previews = load_message_previews_for_session(&db.path, "test-session", 100, 0, 2000)
        .expect("Failed to load previews");

    assert_eq!(previews.len(), 3);
    assert_eq!(previews[0].index, 1);
    assert_eq!(previews[1].index, 2);
    assert_eq!(previews[2].index, 10);
}

#[test]
fn load_message_previews_truncates_long_content() {
    let db = TempDatabase::new();
    db.insert_session("test-session");

    // Create a 10,000 character string
    let long_content = "a".repeat(10_000);
    db.insert_message("test-session", 1, "toolresult", &long_content);

    // Request preview with max 2000 chars
    let previews = load_message_previews_for_session(&db.path, "test-session", 100, 0, 2000)
        .expect("Failed to load previews");

    assert_eq!(previews.len(), 1);
    let preview = &previews[0];

    // Preview should be truncated to ~2000 chars (SQLite substr may have slight differences)
    assert!(preview.content_preview.len() <= 2000);
    assert_eq!(preview.content_len, 10_000);
    assert!(preview.is_truncated());
}

#[test]
fn load_message_previews_respects_pagination() {
    let db = TempDatabase::new();
    db.insert_session("test-session");

    // Insert 5 messages
    for i in 0..5 {
        db.insert_message("test-session", i, "user", &format!("Message {}", i));
    }

    // Load first page (limit 2, offset 0)
    let page1 = load_message_previews_for_session(&db.path, "test-session", 2, 0, 2000)
        .expect("Failed to load page 1");

    assert_eq!(page1.len(), 2);
    assert_eq!(page1[0].index, 0);
    assert_eq!(page1[1].index, 1);

    // Load second page (limit 2, offset 2)
    let page2 = load_message_previews_for_session(&db.path, "test-session", 2, 2, 2000)
        .expect("Failed to load page 2");

    assert_eq!(page2.len(), 2);
    assert_eq!(page2[0].index, 2);
    assert_eq!(page2[1].index, 3);

    // Load third page (limit 2, offset 4) - should have only 1 message
    let page3 = load_message_previews_for_session(&db.path, "test-session", 2, 4, 2000)
        .expect("Failed to load page 3");

    assert_eq!(page3.len(), 1);
    assert_eq!(page3[0].index, 4);
}
