# OpenCode Support Plan

This document describes the planned work to add **OpenCode** session ingestion to Sessions Chronicle.

## Goals (MVP)

- Index OpenCode sessions into SQLite (`sessions` table + FTS5 `messages`) so the **OpenCode** filter is no longer empty.
- Display a readable transcript in the session detail view (user/assistant text + tool calls/results).
- Ensure "Resume in Terminal" does not run the Claude command for OpenCode sessions.

## Non-goals (for this iteration)

- Show or index OpenCode **subagent/child sessions** (sessions with `parentID`). We skip them for now to keep the list clean.
- Diff/patch rendering beyond storing/searching text.
- Perfect parity with the OpenCode UI; we only need reliable indexing + viewing.

## Reference: OpenCode storage layout

OpenCode persists sessions as JSON on disk (XDG data dir).

Default Linux location:

- `~/.local/share/opencode/storage/`

High-level structure (relevant to us):

- `session/<projectID>/<sessionID>.json` (session metadata)
- `message/<sessionID>/<messageID>.json` (message info)
- `part/<messageID>/<partID>.json` (message parts: text/tool/reasoning/...)  
- `session_diff/<sessionID>.json` (diffs; optional)

Sources:

- OpenCode CLI docs: `--session` and `--continue` flags
  - https://opencode.ai/docs/cli/
- OpenCode storage implementation (reference):
  - https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/storage/storage.ts
  - https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/session/message-v2.ts

## Design decisions

1) Skip subagent sessions

- If a session metadata JSON contains `parentID`, treat it as a child/subagent session and **ignore it**.
- Additionally, ensure we **prune** any previously-indexed rows for these sessions (so they don't linger in the DB).

2) Message reconstruction strategy (readable transcript)

- OpenCode message storage is split across "message" and "part" JSONs.
- Reconstruct a linear list of `crate::models::Message` records in chronological order:
  - `Role::User`: join all `text` parts for a user message (ignore `ignored:true` parts)
  - `Role::Assistant`: join all assistant `text` parts
  - `Role::ToolCall`: for each tool part, store tool name + input
  - `Role::ToolResult`: store tool output (or error)

Notes:

- If OpenCode indicates a tool output has been compacted (older output cleared), store a placeholder like `[Old tool result content cleared]`.
- If we encounter both v2 and legacy message formats, implement a best-effort fallback:
  - Preferred: read parts from `part/<messageID>/` (v2)
  - Fallback: if parts are inline in the message JSON, use them

3) Resume behavior

- Claude sessions keep the current behavior (`claude -r <id>`).
- OpenCode sessions should use the OpenCode CLI:
  - `opencode --session <id>`
- If OpenCode is not installed, fail with a clear error (or disable the resume action).

## Implementation plan

### 1. Parser module

- Add `src/parsers/opencode.rs` and export it in `src/parsers/mod.rs`.
- Parse OpenCode session metadata from `session/<projectID>/<sessionID>.json`:
  - `id`
  - `directory` (project path)
  - `time.created`, `time.updated` (epoch ms)
  - `parentID` (used only for skipping)

### 2. Message extraction

- For each eligible session:
  - Read `message/<sessionID>/*.json`
  - For each message:
    - Determine `role` and `time.created`
    - Load parts from `part/<messageID>/*.json`
    - Convert to a sequence of app messages (user/assistant/tool call/tool result)
- Sort messages deterministically (timestamp, then ID) and assign incremental `message_index`.

### 3. Database indexing

- Extend `src/database/indexer.rs` with `index_opencode_sessions(...)`.
- For each eligible session:
  - `INSERT OR REPLACE` into `sessions`:
    - `tool = "opencode"`
    - `project_path = directory`
    - `file_path = path to the session JSON`
    - `start_time`, `last_updated`
    - `message_count`
  - Refresh FTS rows:
    - `DELETE FROM messages WHERE session_id = ?`
    - Insert reconstructed transcript messages into FTS5

### 4. App wiring

- Call the OpenCode indexer during app startup (alongside the existing Claude indexer).
- If the OpenCode storage directory does not exist, treat it as "no sessions" (not an error).

### 5. Resume handling

- Update resume command building to be tool-aware:
  - Claude: `claude -r <id>`
  - OpenCode: `opencode --session <id>`

### 6. Fixtures & tests

- Add minimal OpenCode fixture data under `tests/fixtures/` following the `storage/` layout:
  - `storage/session/...`
  - `storage/message/...`
  - `storage/part/...`
- Add tests:
  - Parser unit tests: metadata parsing, subagent skipping, transcript reconstruction
  - Indexer integration test: index fixture into a temp DB, verify sessions + FTS search

## Validation checklist

- `cargo fmt --all`
- `cargo test`
- Manual:
  - OpenCode sessions appear in the list under the OpenCode filter
  - Session detail shows user/assistant text and tool call/result entries
  - Search returns hits from OpenCode sessions
  - Resume uses `opencode --session <id>` for OpenCode sessions (no Claude command)

## Risks / unknowns

- OpenCode schemas may evolve; parsing should be tolerant (best-effort, skip on hard failures).
- Some sessions may be "global" (projectID = `global`); we should still index them.
- Parts can include richer types (files, snapshots, etc.). MVP focuses on text + tool IO.
