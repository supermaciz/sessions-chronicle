# Sessions Chronicle - Project Status

Last updated: 2026-04-05
Branch snapshot: `main` (`v0.4.1`)

## Current Product State

Sessions Chronicle is a GNOME desktop app that indexes local AI coding assistant sessions and provides:

- Cross-assistant session browsing and filtering (Claude Code, OpenCode, Codex, Mistral Vibe)
- Project sidebar filtering with cross-filtered session queries
- Full-text search via SQLite FTS5 with in-transcript highlighting
- Session detail views with markdown rendering, inline tool calls, and subagent inspection
- Resume-in-terminal flows from list and detail views
- Keyboard navigation and search shortcuts aligned with GNOME patterns
- Token usage display in session detail (input/output, optional reasoning, optional cache read/write)
- Incremental indexing with file fingerprints and startup background indexing feedback
- Indexing diagnostics with assistant health dots, persistent issue banner, and empty-state source visibility
- Session rows show message count, activity count, and ending status for at-a-glance context
- Structured summary header in session detail view (model, timestamps, token totals, project)
- Favorite sessions pinning: toggle pin from list or detail header bar; `pinned_at` stored in `sessions` table (schema `user_version = 8`)
- Pin Filter in sidebar: dedicated "Pinned" entry with live badge count; filters session list to pinned-only; compatible with AI assistant toggles and search
- Consecutive tool calls in session detail grouped into collapsible bursts (`DisplayToolBurst`), reducing visual clutter and preserving page-boundary correctness

## Terminology

- `AI assistant` refers to a session source such as Claude Code, OpenCode, Codex, or Mistral Vibe.
- `tool call` refers to an action invoked within a transcript.
- `tool` may still appear as a literal historical storage or schema name, such as the `sessions.tool` column.

## Recently Landed Work

- Favorite sessions pinning and Pin Filter: pin toggle in session list and detail header bar; "Pinned" sidebar filter with live count; `sessions.pinned_at` column (schema v8) (#115)
- Tool call grouping in session detail: consecutive tool calls collapsed into expandable bursts; page-boundary regrouping handles splits across pagination (#110)
- Parser correctness fixes: closed drift regressions across Claude Code, OpenCode, Codex, and Mistral Vibe parsers (8f4d0c9)
- Structured session summary header in session detail view: model slug, start/end timestamps, token totals, and project path (#105)
- Session rows show message count, activity count, and ending status; duration replaced by message count (#98, #104)
- AI assistant filter rows in sidebar streamlined for a cleaner layout (#103)
- Indexing status dialog: detailed per-source diagnostics with source summaries, recent errors, and direct re-index action (#96)
- Indexing diagnostics: persistent issue banner, assistant sidebar status dots, and empty-state source results (`PerSourceResult`, `SourceStatus`) (#95)
- Project detection and indexing: git-root resolution, `projects` table, and `project_id` FK on sessions (schema `user_version = 6`)
- Project sidebar filtering with cross-filtered session queries; sidebar shows project list alongside AI assistant filters (#81)
- App init extracted into `src/app/init.rs`; `analytics_worker.rs` and `project_resolver.rs` added as dedicated modules
- App update logic refactored into modular handlers under `src/app/handlers/`
- Background indexing worker (`src/indexing_worker.rs`) used for incremental and full reindex runs
- Schema migration to `user_version = 5` with fingerprint-based incremental indexing support and a one-time fingerprint reset
- OpenCode indexing stability improvements (including WAL-aware reindex behavior)
- Analytics workspace with overview cards, activity heatmap, session span buckets, and token usage breakdowns

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
|  |- lib.rs                     # crate root (re-exports for tests)
|  |- config.rs / config.rs.in   # build-time config constants
|  |- app/                       # top-level app component + update handlers
|  |  |- mod.rs
|  |  |- init.rs                 # app initialization logic
|  |  |- handlers/
|  |  |- helpers.rs
|  |  `- types.rs
|  |- indexing_worker.rs         # background indexing worker
|  |- analytics_worker.rs        # background analytics computation worker
|  |- project_resolver.rs        # git-root and worktree-aware project detection
|  |- session_sources.rs         # source path resolution + --sessions-dir behavior
|  |- database/                  # schema, search, indexing, analytics queries
|  |- parsers/                   # per-assistant parsers
|  |- models/                    # sessions/messages/tool calls/subagents/token usage
|  |- ui/                        # list/detail/sidebar/inspector/analytics components
|  |  |- tool_renderers/         # per-tool-call type renderers
|  |  `- modals/                 # dialogs (about, preferences, shortcuts)
|  `- utils/terminal.rs          # terminal detection/spawn for resume
|- data/resources/               # UI templates and CSS
|- tests/                        # integration and behavior tests
`- docs/                         # architecture notes and plans
```

## Database Snapshot

Current migration level is `PRAGMA user_version = 8`.

### Core Tables

- `sessions`
  - identity and metadata per session (`id`, `tool`, `project_path`, timestamps); here `tool` is the historical storage column name for the assistant
  - hierarchy fields (`parent_session_id`, `is_subagent`)
  - token usage aggregates (`input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`)
  - `project_id` FK → `projects(id)` (added in v6)
  - activity counts (`edit_count`, `read_count`, `command_count`) and `ending_status` (added in v7)
  - `pinned_at` nullable timestamp for favorite pinning (added in v8)
- `projects`
  - canonical project records (`id`, `path`, `name`); path is the git root, name is the directory basename
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
flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
flatpak-builder --run flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

CI-parity checks:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

## Known Gaps / Active Exploration

- Markdown rendering still has practical GTK constraints (for example, link interactivity remains limited)
- Indexing diagnostics now include per-source details and recent errors via the Indexing Status dialog; richer remediation actions remain follow-up work
- Ongoing UX refinements continue under newer plans in `docs/explorations/`

## Reference Docs

- `docs/DEVELOPMENT_WORKFLOW.md`
- `docs/SESSION_FORMAT_ANALYSIS.md`
- `docs/PARSER_DESIGN.md`
- `docs/SEARCH_ARCHITECTURE.md`
- `docs/explorations/` for exploration/design history
