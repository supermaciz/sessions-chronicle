# Test Fixtures

This directory contains sample session files for testing and development.

## Structure

```
tests/fixtures/
├── claude_sessions/        # Claude Code session samples (JSONL format)
│   └── sample-session.jsonl
├── claude_recent_events/   # Synthetic Claude Code v2.1.300+ events (skip/title/search coverage)
│   └── 6e10c428-58df-4ad1-9ef0-73fb3ef741b2.jsonl
├── codex_sessions/         # Codex CLI session samples (JSONL format)
│   └── 2026/01/18/...
├── vibe_sessions/          # Mistral Vibe session samples (meta.json + JSONL)
│   ├── unified/              # Mistral Vibe 3.0.x Unified stores (minor 9)
│   └── session_20260203_191451_b9383361/ # Legacy meta.json + JSONL
├── vibe_unified_variants/    # Minimal/corrupt/race stores for reader tests
├── kimi_home/              # Synthetic current ~/.kimi-code session home
├── opencode.db             # OpenCode SQLite fixture (preferred backend)
└── opencode_storage/       # OpenCode legacy JSON storage fixture (fallback backend)
    ├── session/
    ├── message/
    └── part/
```

## Purpose

- **Development**: Test parsers for all five supported AI assistants without requiring their CLIs
- **Testing**: Integration tests use these fixtures to verify parsing and database indexing
- **CI/CD**: Consistent test data across different environments

## Claude Code Session Format

Files are in JSONL (JSON Lines) format, with one JSON object per line:

- **User messages**: `type: "user"`
- **Assistant messages**: `type: "assistant"`
- **System events**: `type: "system"` with `subtype` (e.g., `local_command`)
- **Summary**: `type: "summary"` containing session title

See `docs/SESSION_FORMAT_ANALYSIS.md` for detailed format documentation.

Recent-event fixture (`tests/fixtures/claude_recent_events/`, **synthetic**, kept separate from `claude_sessions/` so session counts stay valid):

- Covers event types the parser skips without error (`permission-mode`, `mode`, `atis-latch`, `bridge-session`, `agent-name`, `attachment`, `api-request-shape`, `api-request`, `api-request-blob`, `cost-state`), including `api-request-blob` lines whose `message.role` must not become messages
- Covers title selection: an `isMeta` skill body and a `task-notification` turn must not become the first prompt (no `ai-title`, so the fallback is exercised)
- Covers search: the non-human turns stay searchable; blob-only tokens do not
- Exercised by `tests/claude_recent_events.rs`

## Codex Session Format

Files are in JSONL (JSON Lines) format, with one JSON object per line:

- **Session metadata**: first line must be `type: "session_meta"`
- **Event messages**: `type: "event_msg"` with `payload.type` values such as `user_message` and `agent_message`

Fixtures added for Codex parsing coverage:

- `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` (valid 3-line session)
- `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-02-00-empty-session.jsonl` (session_meta only)
- `tests/fixtures/codex_sessions/2026/01/18/rollout-2026-01-18T02-03-00-malformed.jsonl` (event_msg first line, missing session_meta)
- `tests/fixtures/codex_sessions/2026/05/19/rollout-2026-05-19T09-00-00-collab-resume.jsonl` (**synthetic** `collab_resume_end` coverage, derived from the upstream `CollabResumeEndEvent` struct — no real `collab_resume_*` event was found in captured rollouts)

Subagent linkage fixtures live under `tests/fixtures/codex_subagent_linkage/`:

- `2026/04/18/` (parent + child pair using `event_msg` `collab_agent_spawn_end`)
- `2026/05/18/` (parent + child pair using the response-item form: `response_item` `function_call` / `function_call_output` for `spawn_agent` / `wait_agent`, anonymized from a real Codex `0.130.0` rollout)
- `2026/05/20/` (**synthetic** parent-only rollout where `spawn_agent`'s `function_call_output` omits `agent_id` — a rejected spawn — guarding that the spawn still degrades to an unlinked subagent row instead of vanishing)

## Vibe Session Format

Each session directory includes:

- `meta.json` with `session_id`, `start_time`, `end_time`, and `environment.working_directory`
- `messages.jsonl` containing `system`, `user`, `assistant` (with optional `tool_calls`), and `tool` messages

Unified fixtures are under `vibe_sessions/unified/` and cover Vibe 3.0.0,
3.0.1 and 3.0.2 store shapes (all store minor 9), including parent/child
sessions. `vibe_unified_variants/` contains isolated edge-case stores copied
into temporary locations by tests. Their independent stdlib generator and
fixture inventory are documented in `vibe_unified_generator/README.md`.
Regenerate the stores from the repository root with:

```bash
python3 -I tests/fixtures/vibe_unified_generator/generate.py
```

## OpenCode Session Format

OpenCode parsing is SQLite-first, with legacy JSON storage fallback:

- Preferred: `tests/fixtures/opencode.db` (SQLite backend)
- Fallback: `tests/fixtures/opencode_storage/` (legacy JSON storage backend)

This mirrors runtime behavior where `opencode.db` is used when available and legacy JSON storage is still supported for older data layouts.

## Kimi Code Session Format

`kimi_home/` represents only the current `$KIMI_CODE_HOME` layout (default
`~/.kimi-code`). It contains synthetic IDs, harmless content, no credentials,
and no real paths. Custom homes are supported when visible in the Flatpak
sandbox; legacy `~/.kimi` sessions are not parsed and are intentionally
excluded.

## Adding Fixtures

To add more test data:

1. Create new `.jsonl` files following the Claude Code format
2. Update integration tests in `tests/` to use the new fixtures
3. Keep fixtures minimal - only include what's needed for testing specific features
