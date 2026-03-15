use rusqlite::Connection;
use sessions_chronicle::database::{SessionIndexer, load_session};
use sessions_chronicle::project_resolver::resolve_project_path;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git command failed: {:?}\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_claude_session(path: &Path, session_id: &str, cwd: &Path) {
    let line = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": "hello" },
        "timestamp": "2026-03-15T12:00:00.000Z",
        "cwd": cwd.to_str().unwrap(),
        "sessionId": session_id,
        "uuid": format!("{}-msg-1", session_id),
        "parentUuid": serde_json::Value::Null,
        "isMeta": false
    });
    fs::write(path, format!("{}\n", line)).unwrap();
}

#[test]
fn fixture_paths_store_resolved_project_path_and_keep_raw_session_path() {
    let raw_cwd = "/home/user/project";
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

    indexer
        .index_claude_sessions(Path::new("tests/fixtures/claude_sessions"))
        .unwrap();

    let session = load_session(temp_db.path(), "abc123")
        .unwrap()
        .expect("fixture session should exist");
    let project_id = session
        .project_id
        .expect("fixture session should have project_id");

    let conn = Connection::open(temp_db.path()).unwrap();
    let stored_path: String = conn
        .query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(session.project_path.as_deref(), Some(raw_cwd));
    assert_eq!(stored_path, resolve_project_path(raw_cwd));
}

#[test]
fn reindexing_same_fixture_does_not_duplicate_project_rows() {
    let raw_cwd = "/home/user/project";
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let fixtures = Path::new("tests/fixtures/claude_sessions");

    indexer.index_claude_sessions(fixtures).unwrap();
    indexer.index_claude_sessions(fixtures).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let resolved_path = resolve_project_path(raw_cwd);
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE path = ?1",
            [resolved_path],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1);
}

#[test]
fn main_repo_and_worktree_sessions_share_project_id() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let temp = tempfile::tempdir().unwrap();

    let repo = temp.path().join("repo");
    let worktree = temp.path().join("repo-worktree");
    let sessions_dir = temp.path().join("sessions");

    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&sessions_dir).unwrap();
    git(temp.path(), &["init", "repo"]);
    fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-m",
            "init",
        ],
    );
    git(&repo, &["worktree", "add", worktree.to_str().unwrap()]);

    write_claude_session(&sessions_dir.join("main.jsonl"), "main-session", &repo);
    write_claude_session(
        &sessions_dir.join("worktree.jsonl"),
        "worktree-session",
        &worktree.join("src"),
    );

    indexer.index_claude_sessions(&sessions_dir).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let project_ids: Vec<i64> = conn
        .prepare(
            "SELECT project_id FROM sessions WHERE id IN ('main-session', 'worktree-session') ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(project_ids.len(), 2);
    assert_eq!(project_ids[0], project_ids[1]);
}

#[test]
fn subdirectory_session_resolves_to_repo_root() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let temp = tempfile::tempdir().unwrap();

    let repo = temp.path().join("repo");
    let sessions_dir = temp.path().join("sessions");
    let nested = repo.join("src/deep/module");

    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::create_dir_all(&nested).unwrap();
    git(temp.path(), &["init", "repo"]);
    fs::write(repo.join("README.md"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-m",
            "init",
        ],
    );

    write_claude_session(
        &sessions_dir.join("nested.jsonl"),
        "nested-session",
        &nested,
    );

    indexer.index_claude_sessions(&sessions_dir).unwrap();

    let conn = Connection::open(temp_db.path()).unwrap();
    let project_path: String = conn
        .query_row(
            "SELECT p.path
             FROM sessions s
             JOIN projects p ON p.id = s.project_id
             WHERE s.id = 'nested-session'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(project_path, repo.canonicalize().unwrap().to_string_lossy());
}
