# AI Session Titles (Issue #48) Revised Implementation Plan

## Summary

Add a dedicated nullable `Session.title` field, preserve native titles from source data, and optionally generate missing titles via host CLI tools (`claude`, `opencode`) during indexing.

Title precedence for display and persistence is:
1. Native parsed title
2. AI-generated title (when enabled)
3. Existing `first_prompt` fallback (UI only)

The feature is disabled by default, non-fatal on any generation failure, and runs only inside the existing background indexing worker.

## Decision-Locked Scope

In scope:
- Add `title` to model + SQLite schema (v5 migration)
- Parse native titles for OpenCode and Claude Code
- Add global settings (enabled/provider/model)
- Auto-detect available host CLI providers (`claude` then `opencode`)
- Generate titles synchronously in `IndexingWorker` after indexing
- UI title precedence: `title -> first_prompt -> project fallback`

Out of scope:
- Per-tool or per-workspace policies
- Manual “regenerate title” actions
- Async runtime introduction
- New providers (Codex, Vibe, Ollama)

## Important Interfaces and Type Changes

### Data model
- `src/models/session.rs`
  - Add field: `pub title: Option<String>` with `#[serde(default)]`

### Database schema
- `src/database/schema.rs`
  - Add migration `apply_v5_migration`
  - `sessions` table gains nullable column `title TEXT`
  - Bump `PRAGMA user_version` latest migration target to 5

### Indexing outputs (required for deterministic candidate selection)
- `src/database/indexer.rs`
  - Extend indexing return type to carry IDs of sessions indexed in current run
  - New type:
    - `IndexingOutcome { stats: IndexingStats, indexed_session_ids: Vec<String> }`
  - `index_all_incremental` and `index_all_full_reindex` return `IndexingOutcome`

### Worker init
- `src/indexing_worker.rs`
  - Replace `type Init = PathBuf` with:
    - `IndexingWorkerInit { db_path: PathBuf, title_generation: TitleGenerationConfig }`

### Title generation module
- Create `src/utils/title_generator.rs`
  - `TitleGenerationConfig { enabled, provider, model_override }`
  - `TitleProvider { Auto, Claude, OpenCode }`
  - `detect_available_provider()`
  - `generate_title(context, config)`

### Database write helper
- `src/database/indexer.rs`
  - Add `update_session_title(session_id: &str, title: &str) -> Result<()>`

## Configuration and Defaults

Add keys in `data/io.github.supermaciz.sessionschronicle.gschema.xml.in`:
- `ai-title-generation-enabled` (`b`) default `false`
- `ai-title-generation-provider` (`s`) default `"auto"`
- `ai-title-generation-model` (`s`) default `""`

Accepted provider values are exactly: `auto`, `claude`, `opencode`.

Behavior:
- If disabled: never invoke any CLI generator
- If enabled and provider is `auto`: resolve provider by detection order `claude > opencode`
- If enabled and no provider detected: skip generation, log debug/warn
- If model override is empty: do not pass model flag

## Auto-Detection and Host Execution Rules

Detection must work both native and Flatpak sandboxed:
- Native: execute `which <bin>` via `std::process::Command`
- Flatpak: execute `flatpak-spawn --host which <bin>`

Flatpak detection follows existing project rule used in terminal utils:
- `/.flatpak-info` exists OR `FLATPAK_ID` env var is set

Provider resolution order for `auto`:
1. `claude`
2. `opencode`

## CLI Invocation Contract

### Claude
Command:
- `claude -p <prompt> --output-format text --permission-mode plan --tools ""`
- Add `--model <model>` only when configured

### OpenCode
Command:
- `opencode run <prompt> --format default`
- Add `--model <model>` only when configured

Both providers:
- hard timeout: 30 seconds per session
- on timeout: kill process, return `None`
- on non-zero exit or empty output: return `None`

Implementation note:
- Add dependency `wait-timeout` to implement robust timeout without async runtime

### CLI flags verified locally (2026-03-01)

- `claude 2.1.63`: `-p` (non-interactive), `--output-format text`, `--model <m>`, `--permission-mode plan`, `--tools ""`
- `opencode 1.2.15`: `run [msg]` (non-interactive), `--format default|json`, `-m/--model provider/model`, `--agent <name>`

## Prompt and Sanitization Contract

Input context for generation is only `session.first_prompt` (trimmed).  
If missing or empty, skip generation.

Prompt template (single source for all providers):

```text
You are a title generator. Output ONLY a session title.

Generate a brief title that helps the user find this conversation later.

Rules:
- Same language as the user message
- Single line
- <= 50 characters
- No tool names
- Keep technical terms/numbers/filenames/HTTP codes exact
- Focus on the main user intent
- Do not explain, do not answer questions

Conversation context:
{{context_text}}

Output only the title.
```

Sanitization rules in app (always applied):
1. Take first non-empty line
2. Trim whitespace and surrounding quotes
3. Collapse internal whitespace to single spaces
4. Enforce max 50 chars with safe char-boundary truncation
5. Reject if resulting title is empty

## Indexing Integration (Decision Complete)

1. Indexing run starts in `IndexingWorker` as today.
2. Indexer returns `IndexingOutcome` containing `indexed_session_ids`.
3. If AI generation disabled: finish immediately with existing completion output.
4. If enabled: iterate only `indexed_session_ids`.
5. For each session ID:
   - Load minimal fields (`id`, `title`, `first_prompt`, `is_subagent`) from DB
   - Skip if `title` already present
   - Skip subagent sessions (`is_subagent = 1`)
   - Skip if no valid `first_prompt`
   - Generate title via resolved provider
   - Persist via `update_session_title`
6. Generation failures are logged and ignored.
7. Worker always emits `Completed` unless indexing itself fails.

Performance guardrail:
- Hard cap generation attempts per run with constant `MAX_TITLE_GENERATIONS_PER_RUN = 25`
- Remaining sessions are left for future indexing runs

## Native Title Extraction Rules

### OpenCode
- Preserve source metadata title in `session.title`
- Do not populate `first_prompt` from metadata title anymore
- `first_prompt` continues to be extracted from first user message

### Claude Code
- Parse `type == "summary"` events and map summary text to `session.title`
- If multiple summary events exist, keep latest non-empty summary by event order
- If no summary event, `session.title` remains `None`

## UI and Settings Changes

### Preferences dialog
`src/ui/modals/preferences.rs` adds a new group "AI Session Titles":
- Switch row: enable/disable generation
- Combo row: provider (`Auto`, `Claude`, `OpenCode`)
- Entry row: optional model override
- Auto mode subtitle shows detected provider status:
  - `Auto (Claude detected)`
  - `Auto (OpenCode detected)`
  - `Auto (No CLI detected)`

### Session title display
`src/ui/session_row.rs`:
- Display precedence becomes:
  1. non-empty `session.title`
  2. non-empty `session.first_prompt`
  3. project-name fallback

## Implementation Steps

1. Schema and model foundation
- Add `Session.title` and v5 migration
- Update all SQL projections/inserts to include `title`

2. Parser alignment
- OpenCode: split metadata title from first prompt
- Claude: parse summary events into title

3. Indexing plumbing
- Introduce `IndexingOutcome` and session ID collection
- Add DB helper to update session title

4. Title generator module
- Add config/provider types
- Add provider detection and command builders
- Add prompt creation, timeout handling, and sanitization

5. Worker and app wiring
- Extend worker init payload
- Read settings in `app.rs`, pass `TitleGenerationConfig` to worker
- Apply capped generation pass after indexing

6. Preferences UI
- Add rows and settings persistence
- Add auto-detection status subtitle logic

7. Session row display update
- Switch to `title -> first_prompt -> fallback`

8. Documentation update
- Update README and format docs to reflect precedence and settings behavior

## Test Plan

### Unit tests
- `src/database/schema.rs`
  - v4 -> v5 migration adds `title` column
- `src/database/mod.rs` / `src/database/indexer.rs`
  - session title persists and round-trips
  - `update_session_title` updates only target session
- `src/parsers/opencode/mod.rs`
  - metadata title maps to `session.title`
  - `first_prompt` still extracted from user message
- `src/parsers/claude_code.rs`
  - summary events set `session.title`
- `src/utils/title_generator.rs`
  - provider auto-detection order
  - flatpak host wrapping behavior
  - command flag mapping (provider/model)
  - sanitization and 50-char truncation
  - timeout path returns `None`

### Integration tests
- `tests/load_session.rs`
  - loading sessions returns `title` when present
- `src/ui/session_row.rs` tests
  - title precedence behavior

### Validation commands
- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

Flatpak runtime verification (manual):
- Build and run devel manifest
- Confirm Preferences controls
- Confirm auto-detection messaging
- Confirm default disabled behavior

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| No CLI found on host | Auto-detection returns `None`, feature stays inactive, Preferences subtitle shows "No CLI detected" |
| Provider CLI not authenticated | Non-fatal: generation returns `None`, logged as warning, indexing continues |
| CLI output format drifts across versions | Aggressive sanitization (first line, trim, 50-char cap) + test fixtures for parser logic |
| Slow or hanging CLI response | 30s hard timeout per call + `MAX_TITLE_GENERATIONS_PER_RUN = 25` cap |
| v5 migration regression | Explicit v4->v5 migration test + idempotence guard in schema code |

## Assumptions and Defaults

- Title generation runs only at indexing time, never on session open.
- Existing sessions are not backfilled immediately unless they are re-indexed.
- AI-generated titles are skipped for subagent sessions.
- Failure modes (missing CLI, auth errors, network failures, malformed output) are non-fatal and do not fail indexing.
