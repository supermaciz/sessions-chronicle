use rusqlite::Connection;
use sessions_chronicle::database::{load_transcript_items, schema::initialize_database};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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
            "sessions-chronicle-test-ti-model-{}-{}.db",
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
                    2_i64,
                    "/tmp/test-session.jsonl",
                    2000_i64,
                ],
            )
            .expect("Failed to insert session");
    }

    fn insert_message(
        &self,
        session_id: &str,
        index: i64,
        role: &str,
        content: &str,
        model: Option<&str>,
    ) {
        self.connection
            .execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    session_id,
                    index,
                    role,
                    content,
                    1000_i64 + index * 100,
                    model,
                ],
            )
            .expect("Failed to insert message");
    }

    fn insert_transcript_item(
        &self,
        session_id: &str,
        item_index: i64,
        kind: &str,
        message_index: Option<i64>,
    ) {
        self.connection
            .execute(
                "INSERT INTO transcript_items (session_id, item_index, kind, message_index)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![session_id, item_index, kind, message_index,],
            )
            .expect("Failed to insert transcript item");
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn load_transcript_items_exposes_model_for_assistant_only() {
    let db = TempDatabase::new();
    let sid = "test-model-session";
    db.insert_session(sid);

    // Assistant message with model
    db.insert_message(
        sid,
        0,
        "assistant",
        "Hello from the model",
        Some("claude-sonnet-4-5-20250514"),
    );
    // User message with no model
    db.insert_message(sid, 1, "user", "A user message", None);

    // Transcript items referencing these messages
    db.insert_transcript_item(sid, 0, "message", Some(0));
    db.insert_transcript_item(sid, 1, "message", Some(1));

    let items = load_transcript_items(&db.path, sid, 100, 0, 2000)
        .expect("Failed to load transcript items");

    assert_eq!(items.len(), 2);

    // Assistant row should carry model
    assert_eq!(
        items[0].model.as_deref(),
        Some("claude-sonnet-4-5-20250514"),
        "assistant message should expose model slug"
    );

    // User row should have no model
    assert_eq!(
        items[1].model, None,
        "user message should have no model value"
    );
}
