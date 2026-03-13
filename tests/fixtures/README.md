# Test Fixtures

This directory contains sample session files for testing and development.

## Structure

```
tests/fixtures/
├── claude_sessions/        # Claude Code session samples (JSONL format)
│   └── sample-session.jsonl
├── codex_sessions/         # Codex CLI session samples (JSONL format)
│   └── 2026/01/18/...
├── vibe_sessions/          # Mistral Vibe session samples (meta.json + JSONL)
│   └── session_20260203_191451_b9383361/
├── opencode.db             # OpenCode SQLite fixture (preferred backend)
└── opencode_storage/       # OpenCode legacy JSON storage fixture (fallback backend)
    ├── session/
    ├── message/
    └── part/
```

## Purpose

- **Development**: Test parsers without requiring actual Claude Code/OpenCode/Codex installations
- **Testing**: Integration tests use these fixtures to verify parsing and database indexing
- **CI/CD**: Consistent test data across different environments

## Claude Code Session Format

Files are in JSONL (JSON Lines) format, with one JSON object per line:

- **User messages**: `type: "user"`
- **Assistant messages**: `type: "assistant"`
- **System events**: `type: "system"` with `subtype` (e.g., `local_command`)
- **Summary**: `type: "summary"` containing session title

See `docs/SESSION_FORMAT_ANALYSIS.md` for detailed format documentation.

## Codex Session Format

Files are in JSONL (JSON Lines) format, with one JSON object per line:

- **Session metadata**: first line must be `type: "session_meta"`
- **Event messages**: `type: "event_msg"` with `payload.type` values such as `user_message` and `agent_message`

Fixtures added for Codex parsing coverage:

- `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` (valid 3-line session)
- `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-02-00-empty-session.jsonl` (session_meta only)
- `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-03-00-malformed.jsonl` (event_msg first line, missing session_meta)

## Vibe Session Format

Each session directory includes:

- `meta.json` with `session_id`, `start_time`, `end_time`, and `environment.working_directory`
- `messages.jsonl` containing `system`, `user`, `assistant` (with optional `tool_calls`), and `tool` messages

## OpenCode Session Format

OpenCode parsing is SQLite-first, with legacy JSON storage fallback:

- Preferred: `tests/fixtures/opencode.db` (SQLite backend)
- Fallback: `tests/fixtures/opencode_storage/` (legacy JSON storage backend)

This mirrors runtime behavior where `opencode.db` is used when available and legacy JSON storage is still supported for older data layouts.

## Adding Fixtures

To add more test data:

1. Create new `.jsonl` files following the Claude Code format
2. Update integration tests in `tests/` to use the new fixtures
3. Keep fixtures minimal - only include what's needed for testing specific features
