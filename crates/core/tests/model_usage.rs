use rusqlite::Connection;
use sessions_chronicle_core::database::{
    SessionModels, load_all_transcript_items, load_session_models, schema::initialize_database,
};
use tempfile::TempDir;

fn seeded() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    let db = Connection::open(&path).unwrap();
    initialize_database(&db).unwrap();
    db.execute_batch(
        "INSERT INTO sessions (id, tool, start_time, message_count, file_path, last_updated) VALUES
         ('parent','claude',0,0,'parent',0), ('child','claude',0,0,'child',0),
         ('user-only','claude',0,0,'user',0), ('null-delegation','claude',0,0,'null',0);
         INSERT INTO messages (session_id,message_index,role,content,timestamp,model) VALUES
         ('parent',0,'user','hello',1,'ignored-user'),
         ('parent',4,'assistant','b',1700000050,'B'),
         ('parent',9,'assistant','d',1700000010,'D'),
         ('parent',10,'assistant','null',0,NULL),
         ('child',0,'assistant','child',1,'child-model'),
         ('user-only',0,'user','hello',1,'ignored-user');
         INSERT INTO tool_calls (session_id,id,tool_name,status,started_at,model) VALUES
         ('parent','a','call','completed',NULL,'A'),
         ('parent','a2','call','completed',1700000060,'A'),
         ('parent','c','call','completed',1700000000,'C'),
         ('parent','empty','call','completed',NULL,'');
         INSERT INTO subagents (session_id,id,title,child_session_id,source_model) VALUES
         ('parent','b','delegate','child','B'),
         ('parent','blank','delegate',NULL,' \t '),
         ('null-delegation','null','delegate',NULL,NULL);
         INSERT INTO context_entries (session_id,id,kind,text,timestamp,item_index) VALUES
         ('parent','ctx','file','x',0,1);
         INSERT INTO transcript_items (session_id,item_index,kind,message_index,tool_call_id,subagent_id,context_id) VALUES
         ('parent',0,'message',0,NULL,NULL,NULL), ('parent',1,'context',NULL,NULL,NULL,'ctx'),
         ('parent',2,'subagent',NULL,NULL,'b',NULL), ('parent',3,'tool_call',NULL,'a',NULL,NULL),
         ('parent',4,'message',4,NULL,NULL,NULL), ('parent',5,'tool_call',NULL,'a2',NULL,NULL),
         ('parent',8,'tool_call',NULL,'c',NULL,NULL), ('parent',9,'message',9,NULL,NULL,NULL),
         ('parent',10,'message',10,NULL,NULL,NULL), ('parent',11,'tool_call',NULL,'empty',NULL,NULL),
         ('parent',12,'subagent',NULL,NULL,'blank',NULL),
         ('child',0,'message',0,NULL,NULL,NULL), ('user-only',0,'message',0,NULL,NULL,NULL),
         ('null-delegation',0,'subagent',NULL,NULL,'null',NULL);"
    ).unwrap();
    (dir, path)
}

#[test]
fn model_usage_order_and_first_timestamps() {
    let (_dir, path) = seeded();
    let parent = load_session_models(&path, "parent").unwrap();
    let actual: Vec<_> = parent
        .models
        .iter()
        .map(|m| {
            (
                m.model.as_str(),
                m.entry_count,
                m.first_item_index,
                m.first_used_at,
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            ("B", 2, 2, None),
            ("A", 2, 3, None),
            ("C", 1, 8, Some(1700000000)),
            ("D", 1, 9, Some(1700000010))
        ]
    );
    assert_eq!(parent.unattributed_count, 3);
    assert_eq!(
        load_session_models(&path, "missing").unwrap(),
        SessionModels::default()
    );
    assert_eq!(
        load_session_models(&path, "user-only").unwrap(),
        SessionModels::default()
    );
    assert_eq!(
        load_session_models(&path, "null-delegation")
            .unwrap()
            .unattributed_count,
        1
    );
    assert_eq!(
        load_session_models(&path, "child").unwrap().models[0].model,
        "child-model"
    );
}

#[test]
fn model_usage_projection() {
    let (_dir, path) = seeded();
    Connection::open(&path).unwrap().execute(
        "UPDATE subagents SET source_model = 'source-model' WHERE session_id = 'parent' AND id = 'b'",
        [],
    ).unwrap();
    let rows = load_all_transcript_items(&path, "parent", 100).unwrap();
    let row = |index| rows.iter().find(|r| r.item_index == index).unwrap();
    assert_eq!(row(4).model.as_deref(), Some("B"));
    assert_eq!(row(4).tool_call_model, None);
    assert_eq!(row(4).subagent_source_model, None);
    assert_eq!(row(4).subagent_child_session_id, None);
    assert_eq!(row(3).model, None);
    assert_eq!(row(3).tool_call_model.as_deref(), Some("A"));
    assert_eq!(row(3).subagent_source_model, None);
    assert_eq!(row(3).subagent_child_session_id, None);
    assert_eq!(row(2).model, None);
    assert_eq!(row(2).tool_call_model, None);
    assert_eq!(
        row(2).subagent_source_model.as_deref(),
        Some("source-model")
    );
    assert_eq!(row(2).subagent_child_session_id.as_deref(), Some("child"));
}

#[test]
fn model_usage_milliseconds_are_unknown_without_changing_order_or_counts() {
    let (_dir, path) = seeded();
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "UPDATE tool_calls SET started_at = 1712000201100 WHERE session_id = 'parent' AND id = 'a';",
    ).unwrap();
    let usage = load_session_models(&path, "parent").unwrap();
    let actual: Vec<_> = usage
        .models
        .iter()
        .map(|m| {
            (
                m.model.as_str(),
                m.entry_count,
                m.first_item_index,
                m.first_used_at,
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            ("B", 2, 2, None),
            ("A", 2, 3, None),
            ("C", 1, 8, Some(1700000000)),
            ("D", 1, 9, Some(1700000010))
        ]
    );
}

#[test]
fn model_usage_preserves_nonblank_padded_slugs() {
    let (_dir, path) = seeded();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE tool_calls SET model = ' A ' WHERE session_id = 'parent' AND id = 'a'",
        [],
    )
    .unwrap();
    let usage = load_session_models(&path, "parent").unwrap();
    let actual: Vec<_> = usage
        .models
        .iter()
        .map(|m| (m.model.as_str(), m.entry_count, m.first_item_index))
        .collect();
    assert_eq!(
        actual,
        [
            ("B", 2, 2),
            (" A ", 1, 3),
            ("A", 1, 5),
            ("C", 1, 8),
            ("D", 1, 9)
        ]
    );
}
