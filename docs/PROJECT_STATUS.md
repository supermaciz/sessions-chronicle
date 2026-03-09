# Sessions Chronicle - Project Status

Last updated: 2026-03-04  
Branch snapshot: `main` (`v0.3.2` lineage)

## Current Product State

Sessions Chronicle is a GNOME desktop app that indexes local AI coding assistant sessions and provides:

- Cross-assistant session browsing and filtering (Claude Code, OpenCode, Codex, Mistral Vibe)
- Full-text search via SQLite FTS5 with in-transcript highlighting
- Session detail views with markdown rendering, inline tool calls, and subagent inspection
- Resume-in-terminal flows from list and detail views
- Keyboard navigation and search shortcuts aligned with GNOME patterns
- Token usage display in session detail (input/output, optional reasoning, optional cache read/write)
- Incremental indexing with file fingerprints and startup background indexing feedback

## Terminology

- `AI assistant` refers to a session source such as Claude Code, OpenCode, Codex, or Mistral Vibe.
- `tool call` refers to an action invoked within a transcript.
- `tool` may still appear as a literal historical storage or schema name, such as the `sessions.tool` column.

## Recently Landed Work

- App update logic refactored into modular handlers under `src/app/handlers/`
- Background indexing worker (`src/indexing_worker.rs`) used for incremental and full reindex runs
- Schema migration to `user_version = 4` with fingerprint-based incremental indexing support
- OpenCode indexing stability improvements (including WAL-aware reindex behavior)
- Documentation and skills updates for planning/review workflows

## Technical Architecture

### Stack

- Language: Rust 2024
- UI: GTK4 + Libadwaita + Relm4
- Storage: SQLite (`rusqlite`) + FTS5 virtual table for message search
- CLI parsing: `clap`
- Markdown parsing: `pulldown-cmark`

### Source Layout

```text
sessions-chronicle/
|- src/
|  |- main.rs                    # startup, args, Relm4 app launch
|  |- app/                       # top-level app component + update handlers
|  |  |- mod.rs
|  |  |- handlers/
|  |  |- helpers.rs
|  |  `- types.rs
|  |- indexing_worker.rs         # background indexing worker
|  |- session_sources.rs         # source path resolution + --sessions-dir behavior
|  |- database/                  # schema, search, indexing logic
|  |- parsers/                   # per-assistant parsers
|  |- models/                    # sessions/messages/tool calls/subagents/token usage
|  |- ui/                        # list/detail/sidebar/inspector components
|  `- utils/terminal.rs          # terminal detection/spawn for resume
|- data/resources/               # UI templates and CSS
|- tests/                        # integration and behavior tests
`- docs/                         # architecture notes and plans
```

## Database Snapshot

Current migration level is `PRAGMA user_version = 4`.

### Core Tables

- `sessions`
  - identity and metadata per session (`id`, `tool`, `project_path`, timestamps); here `tool` is the historical storage column name for the assistant
  - hierarchy fields (`parent_session_id`, `is_subagent`)
  - token usage aggregates (`input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`)
- `messages` (FTS5 virtual table)
  - searchable message content and unindexed metadata (`session_id`, `message_index`, `role`, `timestamp`, `model`)
- `transcript_items`
  - ordered mixed transcript timeline (messages, tool calls, subagents)
- `tool_calls`
  - normalized tool call records plus payload/result metadata
- `subagents`
  - subagent metadata and optional child session linkage
- `file_fingerprints`
  - incremental indexing state (`file_path`, `mtime_ns`, `size`)

## Parsing and Data Safety Guardrails

- Session logs are treated as untrusted input
- JSONL parsing is streamed with `BufReader` line iteration (no whole-file loads)
- Malformed records are skipped with warning logs where possible
- Source paths are resolved via platform APIs and shared helpers (no hardcoded user paths)

## Development Workflow

Primary local loop:

```bash
flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

CI-parity checks:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

## Known Gaps / Active Exploration

- Analytics dashboard is in exploration/design phase (`docs/plans/2026-03-02-basic-analytics-exploration.md`)
- Markdown rendering still has practical GTK constraints (for example, link interactivity remains limited)

## Reference Docs

- `docs/DEVELOPMENT_WORKFLOW.md`
- `docs/SESSION_FORMAT_ANALYSIS.md`
- `docs/PARSER_DESIGN.md`
- `docs/SEARCH_ARCHITECTURE.md`
- `docs/plans/` for exploration/design history
