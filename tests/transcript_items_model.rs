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

    fn insert_tool_call(
        &self,
        session_id: &str,
        tool_call_id: &str,
        input_json: &str,
        output_text: &str,
    ) {
        self.connection
            .execute(
                "INSERT INTO tool_calls (id, session_id, tool_name, status, input_json, output_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    tool_call_id,
                    session_id,
                    "Bash",
                    "completed",
                    input_json,
                    output_text,
                ],
            )
            .expect("Failed to insert tool call");
    }

    fn insert_tool_call_transcript_item(
        &self,
        session_id: &str,
        item_index: i64,
        tool_call_id: &str,
    ) {
        self.connection
            .execute(
                "INSERT INTO transcript_items (session_id, item_index, kind, tool_call_id)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![session_id, item_index, "tool_call", tool_call_id],
            )
            .expect("Failed to insert tool call transcript item");
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

#[test]
fn load_transcript_items_includes_tool_input_and_output() {
    let db = TempDatabase::new();
    let sid = "test-tool-payload-session";
    db.insert_session(sid);

    let input_json = r#"{"command":"cargo test"}"#;
    let output_text = "Process completed";
    db.insert_tool_call(sid, "tool_1", input_json, output_text);
    db.insert_tool_call_transcript_item(sid, 0, "tool_1");

    let items = load_transcript_items(&db.path, sid, 100, 0, 2000)
        .expect("Failed to load transcript items");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].tool_input_json.as_deref(), Some(input_json));
    assert_eq!(items[0].tool_output_text.as_deref(), Some(output_text));
}

#[test]
fn load_transcript_items_includes_reasoning_flags() {
    let db = TempDatabase::new();
    let sid = "test-reasoning-preview-session";
    db.insert_session(sid);
    db.insert_message(sid, 0, "assistant", "Visible answer", Some("o3-mini"));
    db.insert_transcript_item(sid, 0, "message", Some(0));

    db.connection
        .execute(
            "INSERT INTO reasoning_attachments
             (session_id, transcript_item_index, visible_text, encrypted_content)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![sid, 0_i64, Some("chain of thought"), Some("cipher")],
        )
        .unwrap();

    let items = load_transcript_items(&db.path, sid, 100, 0, 2000).unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].reasoning_preview.has_reasoning);
    assert!(items[0].reasoning_preview.has_visible_reasoning);
    assert!(!items[0].reasoning_preview.encrypted_only);
}
