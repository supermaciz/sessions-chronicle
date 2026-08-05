# Sessions Chronicle - Project Status

Last updated: 2026-06-21
Branch snapshot: `main` (`v0.7.1`)

## Current Product State

Sessions Chronicle is a GNOME desktop app that indexes local AI coding assistant sessions and provides:

- Cross-assistant session browsing and filtering (Claude Code, OpenCode, Codex, Mistral Vibe, Kimi Code)
- Project sidebar filtering with cross-filtered session queries
- Full-text search via SQLite FTS5 with in-transcript highlighting and pagination-aware navigation
- Session detail views with markdown rendering, inline tool calls, and subagent inspection
- Resume-in-terminal flows from list and detail views
- Keyboard navigation and search shortcuts aligned with GNOME patterns
- Token usage display in session detail (input/output, optional reasoning, optional cache read/write)
- Incremental indexing with file fingerprints and startup background indexing feedback
- Indexing diagnostics with assistant health dots, persistent issue banner, empty-state source visibility, and dedicated Indexing Status dialog
- Session rows show message count, activity count, and ending status for at-a-glance context
- Structured summary header in session detail view (model, timestamps, token totals, project)
- Favorite sessions pinning: toggle pin from list or detail header bar; `pinned_at` stored in `sessions` table (schema `user_version = 8`)
- Pin Filter in sidebar: dedicated "Pinned" entry with live badge count; filters session list to pinned-only; compatible with AI assistant toggles and search
- Consecutive tool calls in session detail grouped into collapsible bursts (`DisplayToolBurst`), reducing visual clutter and preserving page-boundary correctness
- Explicit `id:` search filter in session list for direct session ID lookup
- Responsiveness instrumentation for session detail rendering with row-build breakdown metrics
- Session detail transcript rendered with a typed `gtk::ListView` for recycled, fluid scrolling; prose rendered with composable `GtkLabel` segments instead of `GtkTextView`
- Date filter pill in the session list (Today / Yesterday / custom range) alongside assistant and project filters
- Streamlined sidebar split into two unlabeled filter blocks (assistants + pins, projects)
- Session summary moved into a header-bar popover (`speaker-notes` button) instead of an inline header block
- Subagent support for Mistral Vibe (child sessions indexed from `<session>/agents/`) and refreshed Codex subagent parsing (`collab_resume_end` + response-item subagents)
- Expanded tool call classification: Plan, Skill, and UserInput categories plus snake_case and assistant-specific tool name variants
- Current Kimi Code sessions are indexed from `$KIMI_CODE_HOME` (default `~/.kimi-code`) when visible in the Flatpak sandbox; legacy `~/.kimi` sessions are not parsed

## Terminology

- `AI assistant` refers to a session source such as Claude Code, OpenCode, Codex, Mistral Vibe, or Kimi Code.
- `tool call` refers to an action invoked within a transcript.
- `tool` may still appear as a literal historical storage or schema name, such as the `sessions.tool` column.

## Recently Landed Work

### Since `v0.4.8` (`v0.5.0` → `v0.7.1`)

- Prose rendering rewrite: replaced `GtkTextView` markdown prose with composable `GtkLabel` segments, with correct rendering of block content after list item text (#173, #174)
- Session detail transcript migrated to a typed `gtk::ListView` for recycled rows and fluid scrolling; transcript made directly scrollable and batching tuned (#152, #151)
- Session detail module restructured for readability and reorganized into a `ui/session_detail/` directory with per-row modules (#161, #169)
- Date filter pill for session browsing with Today/Yesterday/custom range options (#157, plus Yesterday follow-up)
- Sidebar simplified into two unlabeled filter blocks (#158)
- Session summary moved into a header-bar popover with width constraints and a `speaker-notes` icon (#164)
- Mistral Vibe subagent support and refreshed session formats; child sessions indexed from `<session>/agents/` (#166)
- Codex subagent parser update for `collab_resume_end` and response-item subagents (#153)
- Tool call classification expanded with Plan, Skill, and UserInput categories and snake_case / assistant-specific name variants
- Tool inspector fixes: pinned sidebar so navigation doesn't open an empty pane, reset scroll on selection change, and removal of the always-empty Inner Tools section (#165)
- FTS5 storage reworked: `messages` is now a b-tree source table backed by an external-content `messages_fts` index (schema v13)
- Performance: batch post-indexing session list inserts and preserve list scroll position after indexing (#148)
- DRY refactors: shared SQL filter helpers, shared tool call row header, shared reasoning pill builder, and a `local_today_midday_utc` test helper

### Earlier (through `v0.4.8`)

- Self-hosted Flatpak repository workflow: signed repository published via GitHub Pages at `https://sessions-chronicle.maciz.dev/flatpak/`; stable App ID `dev.maciz.sessionschronicle` (#123)
- Astro landing page at `https://sessions-chronicle.maciz.dev` with screenshots and feature overview (#125)
- FTS5-backed session detail search with pagination-aware navigation; matches across all loaded transcript content (#129)
- Performance: FTS5 external content table for messages to speed up large session detail loading (#126)
- Performance: improved session detail render fluidity with deferred page load and paced render batches (#128)
- Explicit `id:` session ID search filter in session list for direct lookup (#136)
- Session detail responsiveness instrumentation with row-build breakdown metrics for issue #127 (#137)
- Full-app metrics capture for issue #140 with row construction timing in real application path (#141)
- Favorite sessions pinning and Pin Filter: pin toggle in session list and detail header bar; "Pinned" sidebar filter with live count; `sessions.pinned_at` column (schema v8) (#115)
- Tool call grouping in session detail: consecutive tool calls collapsed into expandable bursts; page-boundary regrouping handles splits across pagination (#110)
- Indexing status dialog: detailed per-source diagnostics with source summaries, recent errors, and direct re-index action (#96)
- Indexing diagnostics: persistent issue banner, assistant sidebar status dots, and empty-state source results (`PerSourceResult`, `SourceStatus`) (#95)
- Structured session summary header in session detail view: model slug, start/end timestamps, token totals, and project path (#105)
- Session rows show message count, activity count, and ending status; duration replaced by message count (#98, #104)
- AI assistant filter rows in sidebar streamlined for a cleaner layout (#103)
- Parser correctness fixes: closed drift regressions across Claude Code, OpenCode, Codex, and Mistral Vibe parsers (8f4d0c9)
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
|  |- parsers/                   # per-assistant parsers (Claude Code, Codex, OpenCode, Mistral Vibe, Kimi Code)
|  |- models/                    # sessions/messages/tool calls/subagents/token usage
|  |- ui/                        # list/detail/sidebar/inspector/analytics components
|  |  |- session_detail/         # typed-ListView transcript + per-row modules
|  |  |- tool_renderers/         # per-tool-call type renderers
|  |  |- date_pill.rs            # session-list date filter pill
|  |  |- activity_bar.rs         # session activity indicators
|  |  `- modals/                 # dialogs (about, preferences, shortcuts)
|  `- utils/terminal.rs          # terminal detection/spawn for resume
|- data/resources/               # UI templates and CSS
|- tests/                        # integration and behavior tests
`- docs/                         # architecture notes and plans
```

## Database Snapshot

Current migration level is `PRAGMA user_version = 14`.

Migrations since v8:

- v9 — `reasoning_attachments` side table; clears `file_fingerprints` to re-index
- v10 — clears `file_fingerprints` to rebuild transcripts after parser changes
- v11 — `subagents.agent_id` (nullable); clears `file_fingerprints`
- v12 — session-list ordering indexes for faster startup/filter reloads
- v13 — replaces the FTS5-virtual `messages` table with a b-tree source table backed by an external-content `messages_fts` index; clears `file_fingerprints`
- v14 — clears `file_fingerprints` to re-index after Mistral Vibe subagent support; adds an index on `sessions.file_path` for efficient Vibe subtree pruning

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
- `messages` + `messages_fts`
  - `messages` is a b-tree source table with message content and metadata (`session_id`, `message_index`, `role`, `timestamp`, `model`); `messages_fts` is an external-content FTS5 index over it (changed in v13)
- `transcript_items`
  - ordered mixed transcript timeline (messages, tool calls, subagents)
- `tool_calls`
  - normalized tool call records plus payload/result metadata
- `subagents`
  - subagent metadata and optional child session linkage; `agent_id` nullable (added in v11)
- `reasoning_attachments`
  - per-message reasoning payloads kept out of the FTS path (added in v9)
- `file_fingerprints`
  - incremental indexing state (`file_path`, `mtime_ns`, `size`)

## Parsing and Data Safety Guardrails

- Session logs are treated as untrusted input
- JSONL parsing is streamed with `BufReader` line iteration (no whole-file loads)
- Malformed records are skipped with warning logs where possible
- Source paths are resolved via platform APIs and shared helpers (no hardcoded user paths)

## Development Workflow

Primary local loop (Meson, faster inner loop):

```bash
meson setup builddir -Dprofile=development --prefix="$HOME/.local"
meson compile -C builddir && meson install -C builddir
"$HOME/.local/bin/sessions-chronicle" --sessions-dir tests/fixtures
```

Flatpak loop (closest to the packaged runtime; use to verify packaging):

```bash
flatpak-builder --user flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json --force-clean
flatpak-builder --run flatpak_app build-aux/dev.maciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

CI-parity checks:

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast
```

## Known Gaps / Active Exploration

- Markdown prose now renders via composable `GtkLabel` segments; links are shown with a dimmed URL suffix rather than clickable hyperlinks
- Indexing diagnostics now include per-source details and recent errors via the Indexing Status dialog; richer remediation actions remain follow-up work
- Ongoing UX refinements continue under newer plans in `docs/explorations/`

## Reference Docs

- `docs/DEVELOPMENT_WORKFLOW.md`
- `docs/SESSION_FORMAT_ANALYSIS.md`
- `docs/PARSER_DESIGN.md`
- `docs/SEARCH_ARCHITECTURE.md`
- `docs/explorations/` for exploration/design history
