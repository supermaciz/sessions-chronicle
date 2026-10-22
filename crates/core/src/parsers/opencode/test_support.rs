//! Test-only helpers shared with the indexer tests (Task 5).

use rusqlite::Connection;
use std::path::Path;

use crate::parsers::ParsedSession;

use super::v2::V2Reader;

/// Creates a synthetic OpenCode V2 SQLite database at `path` by executing the
/// fixture `schema.sql` and `sessions.sql` files. The returned connection is
/// the caller's to keep (e.g. as a WAL writer in the snapshot test) or drop.
pub(crate) fn create_v2_db(path: &Path) -> Connection {
    let conn = Connection::open(path).expect("open synthetic V2 DB");
    let schema = std::fs::read_to_string(crate::fixture_path("opencode_v2/schema.sql"))
        .expect("read opencode_v2 schema.sql");
    let sessions = std::fs::read_to_string(crate::fixture_path("opencode_v2/sessions.sql"))
        .expect("read opencode_v2 sessions.sql");
    conn.execute_batch(&schema)
        .expect("apply opencode_v2 schema.sql");
    conn.execute_batch(&sessions)
        .expect("apply opencode_v2 sessions.sql");
    conn
}

/// Builds the synthetic fixture in a temporary database and parses `ses_v2`.
pub(crate) fn parse_fixture() -> ParsedSession {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("fixture.db");
    let conn = create_v2_db(&db_path);
    drop(conn);

    let reader = V2Reader::open(&db_path).expect("open V2 reader");
    reader.parse_session("ses_v2").expect("parse ses_v2")
}
