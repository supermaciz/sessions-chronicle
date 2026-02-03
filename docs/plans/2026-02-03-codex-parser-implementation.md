# Codex Parser Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Codex JSONL session parsing, indexing, and startup wiring so Codex sessions appear in Sessions Chronicle.

**Architecture:** Stream JSONL lines into a `CodexParser` that extracts session metadata from the first `session_meta` line and messages from `event_msg` events. Indexer walks `~/.codex/sessions/` for `rollout-*.jsonl` files, inserts sessions/messages into SQLite, and app startup triggers indexing on launch.

**Tech Stack:** Rust 2024, serde_json, chrono, rusqlite, walkdir, relm4.

---

### Task 1: Add Codex fixtures and update fixture docs

**Files:**
- Create: `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl`
- Create: `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-02-00-empty-session.jsonl`
- Create: `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-03-00-malformed.jsonl`
- Modify: `tests/fixtures/README.md`

**Step 1: Add fixtures**

Create the valid session fixture with `session_meta` and two `event_msg` messages:

```json
{"timestamp":"2026-01-18T01:01:28.123Z","type":"session_meta","payload":{"id":"019bce9f-0a40-79e2-8351-8818e8487fb6","timestamp":"2026-01-18T01:01:28.123Z","cwd":"/home/user/project","originator":"codex_cli_rs","cli_version":"0.87.0"}}
{"timestamp":"2026-01-18T01:01:30.000Z","type":"event_msg","payload":{"type":"user_message","message":"Summarize the repo"}}
{"timestamp":"2026-01-18T01:01:31.000Z","type":"event_msg","payload":{"type":"agent_message","message":"Here is the summary"}}
```

Create the empty session fixture with only `session_meta` and no user messages.

Create the malformed fixture with a first line that is an `event_msg` instead of `session_meta` (to verify we reject files missing required metadata).

**Step 2: Update fixture README**

Add a Codex section documenting the new fixtures and their JSONL format.

**Step 3: Commit**

```bash
git add tests/fixtures/codex_sessions tests/fixtures/README.md
git commit -m "test: add codex session fixtures"
```

---

### Task 2: Add Codex parser module and unit tests

**Files:**
- Create: `src/parsers/codex.rs`
- Modify: `src/parsers/mod.rs`

**Step 1: Write failing tests (and stub parser)**

Create `src/parsers/codex.rs` with a `CodexParser` struct and stub `parse` method that returns `anyhow::bail!("not implemented")`. Add tests that use fixtures:

```rust
#[test]
fn parse_valid_session_extracts_messages() {
    let parser = CodexParser;
    let path = PathBuf::from("tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl");
    let (session, messages) = parser.parse(&path).unwrap();
    assert_eq!(session.id, "019bce9f-0a40-79e2-8351-8818e8487fb6");
    assert_eq!(session.project_path.as_deref(), Some("/home/user/project"));
    assert_eq!(session.message_count, 2);
    assert_eq!(messages[0].role, Role::User);
    assert_eq!(messages[0].content, "Summarize the repo");
    assert_eq!(messages[1].role, Role::Assistant);
}

#[test]
fn parse_empty_session_is_rejected() {
    let parser = CodexParser;
    let path = PathBuf::from("tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-02-00-empty-session.jsonl");
    let result = parser.parse(&path);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Session contains no user messages"));
}

#[test]
fn parse_missing_session_meta_is_rejected() {
    let parser = CodexParser;
    let path = PathBuf::from("tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-03-00-malformed.jsonl");
    let result = parser.parse(&path);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("First line must be session_meta"));
}
```

Update `src/parsers/mod.rs` to export `codex` module so tests compile.

**Step 2: Run tests to confirm failure**

Run: `cargo test parsers::codex::tests::parse_valid_session_extracts_messages`

Expected: FAIL with "not implemented".

**Step 3: Implement parser**

Implement streaming JSONL parsing following the design:

- Read the first non-empty line and require `type == "session_meta"`. If not, return `anyhow::bail!("First line must be session_meta")`.
- Extract session id from `payload.id`, timestamp from `payload.timestamp`, project path from `payload.cwd`.
- Stream remaining lines and collect `event_msg` with `payload.type` of `user_message` and `agent_message`.
- Track `last_updated` from the latest valid event `timestamp`.
- Skip malformed JSON lines with `tracing::warn!` and continue.
- If no `user_message` was seen, return `anyhow::bail!("Session contains no user messages")`.

Use `chrono::DateTime::parse_from_rfc3339` to parse timestamps, and default missing event timestamps to `Utc::now()` for message timestamps (matching existing parser behavior).

**Step 4: Run tests to confirm pass**

Run: `cargo test parsers::codex::tests`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/parsers/codex.rs src/parsers/mod.rs
git commit -m "feat: add codex jsonl parser"
```

---

### Task 3: Add Codex indexing support with tests

**Files:**
- Modify: `src/database/indexer.rs`

**Step 1: Write failing indexer test**

Add a new test to the existing `#[cfg(test)] mod tests` block in `indexer.rs` (matching the `opencode_indexing_indexes_sessions` pattern). The test accesses `indexer.db` directly which is allowed within the same module:

```rust
#[test]
fn codex_indexing_indexes_sessions() {
    let temp_db = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
    let sessions_dir = PathBuf::from("tests/fixtures/codex_sessions");

    let count = indexer.index_codex_sessions(&sessions_dir).unwrap();
    assert_eq!(count, 1);

    let sessions: Vec<(String, String)> = indexer
        .db
        .prepare("SELECT id, tool FROM sessions ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].1, "codex");
}
```

Add a second test verifying missing directory returns `0`.

**Step 2: Run tests to confirm failure**

Run: `cargo test database::indexer::tests::codex_indexing_indexes_sessions`

Expected: FAIL (method missing).

**Step 3: Implement indexer method**

Add `index_codex_sessions(&mut self, sessions_dir: &Path) -> Result<usize>` that:

- Returns `Ok(0)` if `sessions_dir` does not exist.
- Walks `sessions_dir` recursively.
- Filters files named `rollout-*.jsonl`.
- Parses with `CodexParser` and inserts with `insert_session_and_messages`.
- Logs and skips parse errors (including missing `session_meta`) without aborting indexing.

**Step 4: Run tests to confirm pass**

Run: `cargo test database::indexer::tests::codex_indexing_indexes_sessions`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs
git commit -m "feat: index codex sessions"
```

---

### Task 4: Wire Codex indexing on app startup

**Files:**
- Modify: `src/app.rs`

**Step 1: Add startup indexing call**

After OpenCode indexing, add a Codex indexing block following the same pattern:

```rust
let codex_sessions_dir = PathBuf::from(Tool::Codex.session_dir());
match idx.index_codex_sessions(&codex_sessions_dir) {
    Ok(count) => {
        tracing::info!(
            "Indexed {} Codex sessions from {}",
            count,
            codex_sessions_dir.display()
        );
    }
    Err(err) => {
        tracing::error!("Failed to index Codex sessions: {}", err);
    }
}
```

Note: Unlike OpenCode which uses `session_dir().parent()`, Codex uses the session directory directly since `~/.codex/sessions/` is the root containing date-nested subdirectories.

**Step 2: Run smoke tests**

Run: `cargo test`

Expected: PASS.

**Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: index codex sessions on startup"
```

---

### Task 5: Final verification

**Files:**
- No changes

**Step 1: Full test suite**

Run: `cargo test`

Expected: PASS.

**Step 2: Summary check**

Confirm Codex sessions appear in the list when fixtures are indexed manually.

---

## Notes

- Missing or malformed `session_meta` should be treated as a skippable parse error (warn and continue indexing).
- Indexer tests access the private `db` field directly because they are defined in the same module (`mod tests` block in `indexer.rs`). This matches the existing OpenCode indexer test pattern.
- `Tool::Codex.session_dir()` already exists and returns `~/.codex/sessions`.
