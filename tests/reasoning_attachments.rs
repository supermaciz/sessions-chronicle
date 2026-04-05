use rusqlite::Connection;
use sessions_chronicle::database::{load_reasoning_attachment, schema::initialize_database};
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
            "sessions-chronicle-reasoning-{}-{}.db",
            std::process::id(),
            nanos
        ));
        let connection = Connection::open(&path).unwrap();
        initialize_database(&connection).unwrap();
        Self { path, connection }
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn load_reasoning_attachment_returns_full_payload() {
    let db = TempDatabase::new();
    db.connection
        .execute(
            "INSERT INTO reasoning_attachments
             (session_id, transcript_item_index, visible_text, summary_text, encrypted_content, source_model, source_timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "s1",
                4_i64,
                Some("full reasoning"),
                Some("summary"),
                Some("ciphertext"),
                Some("o3-mini"),
                Some(1_700_000_000_i64),
            ],
        )
        .unwrap();

    let attachment = load_reasoning_attachment(&db.path, "s1", 4)
        .unwrap()
        .expect("attachment should exist");

    assert_eq!(attachment.visible_text.as_deref(), Some("full reasoning"));
    assert_eq!(attachment.summary_text.as_deref(), Some("summary"));
    assert_eq!(attachment.encrypted_content.as_deref(), Some("ciphertext"));
    assert_eq!(attachment.source_model.as_deref(), Some("o3-mini"));
    assert_eq!(
        attachment.source_timestamp.unwrap().timestamp(),
        1_700_000_000
    );
}
