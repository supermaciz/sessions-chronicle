# Mistral Vibe v2 Parser Design

**Status**: Implemented  
**Date**: 2026-02-04  

Design document for adding Mistral Vibe v2 session support to Sessions Chronicle.

## Overview

Mistral Vibe v2 stores each session as a directory containing `meta.json` and `messages.jsonl`. The parser streams `messages.jsonl` and extracts user and assistant messages. Tool calls (`tool_calls` on assistant messages) and tool outputs (`role: tool`) are intentionally ignored — tool call support is deferred to Phase 4 alongside the other parsers.

## Storage Structure

```
~/.vibe/logs/session/
└── session_YYYYMMDD_HHMMSS_<shortid>/
    ├── meta.json
    └── messages.jsonl
```

`VIBE_HOME` overrides the base directory (use `$VIBE_HOME/logs/session/`).

## File Formats

### meta.json

Session-level metadata, minimal required fields:

```json
{
  "session_id": "session_20260203_191451_b9383361",
  "start_time": "2026-02-03T19:14:51Z",
  "end_time": "2026-02-03T19:16:05Z",
  "environment": { "working_directory": "/home/user/project" }
}
```

### messages.jsonl

OpenAI-style messages, one JSON object per line. Supported roles:
- `system` (ignored)
- `user` (indexed)
- `assistant` (indexed)
- `tool` (ignored)

Example (simplified):

```json
{"role":"user","content":"List files"}
{"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"list_files","arguments":"{\"path\":\".\"}"}}]}
{"role":"tool","tool_call_id":"call_1","content":"README.md\nsrc\n"}
{"role":"assistant","content":"The root contains README.md and src."}
```

## Mapping to Sessions Chronicle Model

### Session

| Session Field | Source |
|--------------|--------|
| `id` | `meta.session_id` |
| `tool` | `Tool::MistralVibe` |
| `project_path` | `meta.environment.working_directory` |
| `start_time` | `meta.start_time` |
| `last_updated` | `meta.end_time` (fallback: `start_time`) |
| `file_path` | session directory path |
| `message_count` | number of emitted messages |

### Messages

Rules:
- Ignore `role: system` and `role: tool`.
- Emit `Role::User` for `role: user` with non-empty `content`.
- Emit `Role::Assistant` for `role: assistant` with non-empty `content`.
- Ignore `tool_calls` on assistant messages (deferred to Phase 4).
- Synthetic timestamps: `session.start_time + index seconds`.
- If no user message exists, reject the session (skip/prune).

## Indexing Strategy

`index_vibe_sessions(sessions_dir)`:
- Return `Ok(0)` if the directory does not exist.
- Scan immediate subdirectories for `meta.json` + `messages.jsonl`.
- Parse each directory; on `NoUserMessages`, prune DB rows for that path.
- Insert sessions and messages in a transaction.

## UI + Resume

- Add `Tool::MistralVibe` with storage key `mistral_vibe`.
- Include Mistral Vibe in sidebar filters and default tool selection.
- Resume command: `vibe --resume "$2"`.

## Error Handling

- Missing files: skip directory.
- Malformed JSONL lines: log and skip line.
- Missing required metadata: return parse error.
- No user messages: treat as non-session; prune any prior DB entry.

## Tests

- Fixture-based parser test using `tests/fixtures/vibe_sessions/`.
- Parser rejection for sessions without user messages.
- Indexer test that counts 1 session and stores tool `mistral_vibe`.
- Indexer test for missing sessions directory.
