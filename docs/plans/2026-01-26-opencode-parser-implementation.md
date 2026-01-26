# OpenCode Parser Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add OpenCode session support with a unified Claude parser API and tool-aware resume behavior.

**Architecture:** Refactor Claude parsing into a single-pass `parse()` returning `(Session, Vec<Message>)`, then implement a new OpenCode parser that reads session/message/part JSON files from the OpenCode storage layout. Extend the indexer and app startup to ingest OpenCode sessions, skipping subagent sessions and empty sessions, and update resume command building to be tool-aware.

**Tech Stack:** Rust 2024, serde_json, chrono, rusqlite, Relm4

---

### Task 1: Refactor Claude parser into unified `parse()`

**Files:**
- Modify: `src/parsers/claude_code.rs`
- Modify: `src/database/indexer.rs`

**Step 1: Write failing test for new API shape**

```rust
#[test]
fn parse_returns_session_and_messages() {
    let file = create_temp_session(&[
        r#"{\"type\":\"user\",\"timestamp\":\"2024-01-01T00:00:00Z\",\"message\":{\"content\":\"Hello\"}}"#,
        r#"{\"type\":\"assistant\",\"timestamp\":\"2024-01-01T00:00:01Z\",\"message\":{\"content\":\"Hi!\"}}"#,
    ]);
    let parser = ClaudeCodeParser;
    let result = parser.parse(file.path()).unwrap();
    assert_eq!(result.1.len(), 2);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test parsers::claude_code::tests::parse_returns_session_and_messages -v`
Expected: FAIL, `parse` not found.

**Step 3: Implement minimal combined parse**

- Add `pub fn parse(&self, file_path: &Path) -> Result<(Session, Vec<Message>)>`
- Use a single `BufReader` pass, collecting both metadata and messages.
- Preserve current semantics: skip empty sessions, skip sessions without user messages.

**Step 4: Update indexer to use new API**

- In `index_session_file`, replace `parse_metadata/parse_messages` with `parse`.

**Step 5: Run targeted tests**

Run: `cargo test parsers::claude_code::tests -v`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/parsers/claude_code.rs src/database/indexer.rs
git commit -m "refactor: unify claude parser into parse()"
```

---

### Task 2: Add OpenCode parser module

**Files:**
- Create: `src/parsers/opencode.rs`
- Modify: `src/parsers/mod.rs`

**Step 1: Write unit tests for OpenCode parsing**

Add tests for:

- `parse_session_metadata_extracts_fields`
- `parse_skips_subagent_sessions`
- `parse_skips_sessions_without_user_messages`
- `load_parts_handles_missing_files`
- `message_reconstruction_orders_correctly`

Example test snippet:

```rust
#[test]
fn parse_skips_subagent_sessions() {
    let parser = OpenCodeParser::new(Path::new("tests/fixtures/opencode_storage"));
    let session_path = Path::new("tests/fixtures/opencode_storage/session/project-a/session-002.json");
    let result = parser.parse(session_path);
    assert!(result.is_err());
}
```

**Step 2: Run tests to confirm failures**

Run: `cargo test opencode::tests -v`
Expected: FAIL, module missing.

**Step 3: Implement `OpenCodeParser`**

- `pub fn new(storage_root: &Path) -> Self`
- `pub fn parse(&self, session_path: &Path) -> Result<(Session, Vec<Message>)>`
- Helpers:
  - `parse_session_metadata()`: read `session/.../<sessionID>.json`
  - `load_messages()`: read `message/<sessionID>/*.json`, sort by `time.created`
  - `load_parts()`: read `part/<messageID>/*.json` best-effort (missing file => warn)
- Map part types:
  - `text` → `Role::User`/`Role::Assistant` content from `content.text`
  - `tool-invocation` → `Role::ToolCall` content `[Tool: name]` + input summary
  - `tool-result` → `Role::ToolResult` content from `content.result` or placeholder
  - `reasoning` → skip
  - unknown → warn and skip
- Flatten to `Vec<Message>` with sequential `index`.

**Step 4: Run tests**

Run: `cargo test opencode::tests -v`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/parsers/opencode.rs src/parsers/mod.rs
git commit -m "feat: add opencode parser core"
```

---

### Task 3: Add OpenCode fixtures

**Files:**
- Create: `tests/fixtures/opencode_storage/...` (see layout below)

**Fixture layout:**

```
tests/fixtures/opencode_storage/
├── session/project-a/session-001.json
├── session/project-a/session-002.json
├── session/global/session-003.json
├── message/session-001/msg-001.json
├── message/session-001/msg-002.json
├── message/session-003/msg-001.json
└── part/msg-001/part-001.json
└── part/msg-002/part-001.json
└── part/msg-002/part-002.json
```

**Example session JSON:**

```json
{
  "id": "session-001",
  "directory": "/projects/alpha",
  "time": { "created": 1704067200000, "updated": 1704067260000 }
}
```

**Example message JSON:**

```json
{ "id": "msg-001", "sessionID": "session-001", "role": "user", "time": { "created": 1704067200000 } }
```

**Example part JSON:**

```json
{ "id": "part-001", "type": "text", "content": { "text": "Hello OpenCode" } }
```

**Step 1: Add files**

Create the fixture tree and JSON contents.

**Step 2: Run unit tests**

Run: `cargo test opencode::tests -v`
Expected: PASS.

**Step 3: Commit**

```bash
git add tests/fixtures/opencode_storage
git commit -m "test: add opencode storage fixtures"
```

---

### Task 4: Add OpenCode indexing to SessionIndexer

**Files:**
- Modify: `src/database/indexer.rs`

**Step 1: Write integration test for OpenCode indexing**

Create a new test module in `src/database/indexer.rs` or new file `tests/opencode_indexer.rs`:

- Use a temp DB.
- Call `index_opencode_sessions()` pointing to `tests/fixtures/opencode_storage`.
- Verify session count and that subagent session is pruned.

**Step 2: Run test to see failure**

Run: `cargo test opencode_indexer -v`
Expected: FAIL, indexer method missing.

**Step 3: Implement `index_opencode_sessions()`**

- Walk `storage_root/session/` for `*.json`.
- For each session file:
  - Parse metadata; if parentID exists → prune and continue.
  - Parse messages; if no user messages → prune and continue.
  - Insert/replace session with `tool = "opencode"`.
  - Delete old messages and insert new messages.

**Step 4: Run test**

Run: `cargo test opencode_indexer -v`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs tests/opencode_indexer.rs
git commit -m "feat: index opencode sessions"
```

---

### Task 5: Wire OpenCode indexing into app startup

**Files:**
- Modify: `src/app.rs`

**Step 1: Write minimal test or smoke check note**

No unit test; add a manual validation note.

**Step 2: Implement**

- Resolve OpenCode storage root from `Tool::OpenCode.session_dir()` by calling `.parent()` to get `.../storage`.
- Call `indexer.index_opencode_sessions(&opencode_root)` after Claude indexing.
- If OpenCode dir is missing, treat as zero sessions.

**Step 3: Manual smoke**

Run app (Flatpak or dev), verify OpenCode sessions appear when present.

**Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: index opencode sessions at startup"
```

---

### Task 6: Make resume command tool-aware

**Files:**
- Modify: `src/utils/terminal.rs`
- Modify: `src/ui/session_list.rs`
- Modify: `src/ui/session_detail.rs`
- Modify: `src/app.rs`
- Modify: `src/models/session.rs` (if needed for helper)

**Step 1: Write failing test for tool-aware resume**

```rust
#[test]
fn test_build_resume_command_opencode() {
    let temp_dir = std::env::temp_dir();
    let cmd = build_resume_command(Tool::OpenCode, "session-123", &temp_dir).unwrap();
    assert!(cmd[2].contains("opencode --session"));
}
```

**Step 2: Run test to confirm failure**

Run: `cargo test utils::terminal::tests::test_build_resume_command_opencode -v`
Expected: FAIL.

**Step 3: Implement tool-aware command**

- Change signature to `build_resume_command(tool: Tool, session_id: &str, workdir: &Path)`
- Use:
  - `claude -r <id>`
  - `opencode --session <id>`
  - `codex <id>`
- Keep current `bash -lc` wrapping.

**Step 4: Pass tool when requesting resume**

- Update `SessionListOutput::ResumeRequested` and `SessionDetailOutput::ResumeRequested` to include `Tool`.
- Emit tool from both list and detail.
- Update `AppMsg::ResumeSession` to accept `(String, Tool)` and pass tool into `build_resume_command`.

**Step 5: Run tests**

Run: `cargo test utils::terminal::tests -v`
Expected: PASS.

**Step 6: Commit**

```bash
git add src/utils/terminal.rs src/ui/session_list.rs src/ui/session_detail.rs src/app.rs
git commit -m "feat: build resume command per tool"
```

---

### Task 7: Integration test for search/FTS with OpenCode messages

**Files:**
- Create: `tests/opencode_search.rs`

**Step 1: Write integration test**

- Create temp DB.
- Index fixtures via `index_opencode_sessions`.
- Search for a known phrase from OpenCode text part and assert hit.

**Step 2: Run test**

Run: `cargo test opencode_search -v`
Expected: PASS.

**Step 3: Commit**

```bash
git add tests/opencode_search.rs
git commit -m "test: opencode sessions searchable"
```

---

### Final Validation

Run:
- `cargo fmt --all`
- `cargo clippy`
- `cargo test`

Manual checks:
- OpenCode filter shows sessions
- Session detail includes tool call/results
- Search finds OpenCode content
- Resume runs `opencode --session <id>` for OpenCode sessions
