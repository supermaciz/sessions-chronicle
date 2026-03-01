# AI Session Titles (Issue #48) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a true AI-generated `title` field (distinct from `first_prompt`) for sessions, with native titles preserved (OpenCode metadata, Claude summary events), and an optional AI fallback generator using auto-detected host CLI tools.

**Architecture:** Extend the `Session` model and SQLite schema with nullable `title`. Fill `title` from native source data first, then optionally apply a configurable AI fallback generator. The fallback runs synchronously inside the existing Relm4 `IndexingWorker` thread, is guarded by settings (`enabled`, `CLI provider`, `model`), calls host CLI binaries via `std::process::Command` (with `flatpak-spawn --host` when sandboxed), and never blocks indexing on failure.

**Tech Stack:** Rust 2024, rusqlite/SQLite migrations, Relm4 `Worker` trait, libadwaita Preferences dialog, GSettings schema keys, `std::process::Command` with 30s timeout.

---

## Scope

- Implement issue #48 as "real title" behavior (AI-generated title), not only first-prompt preview.
- **V1 providers:** Claude CLI and OpenCode CLI only.
- Auto-detect available CLI providers on the host system.
- Add global settings to control AI title generation:
  - disabled by default (auto-detection proposes activation when a CLI is found),
  - selectable CLI provider (`auto`, `claude`, `opencode`),
  - optional model override string.
- Preserve native titles where available (OpenCode metadata, Claude summary events).
- Keep `first_prompt` for search and fallback display.
- Title generation runs at indexation time only, inside the existing `IndexingWorker`.

## Non-goals

- No per-tool/per-workspace title generation policy (global setting only).
- No manual "regenerate title" action.
- No tokio runtime or async — synchronous `std::process::Command` in worker thread.
- No Codex or Vibe providers (future V2).
- No Ollama / local model provider (future).
- No background queue or retry worker beyond the existing indexing worker.

## Configuration Behavior

When `ai-title-generation-enabled == false` (default):
- No CLI title generation runs.
- Titles come only from native sources (OpenCode metadata, Claude summary) or UI fallback to `first_prompt`.

When `ai-title-generation-enabled == true`:
- If session has no native title, app calls selected CLI provider to generate one.
- If provider is `"auto"`, resolve to first available CLI in order: `claude`, `opencode`.
- If `ai-title-generation-model` is non-empty, pass it to selected CLI with provider-appropriate flag.
- Any CLI/auth/network failure is non-fatal: leave `title` empty and fallback to existing UI behavior.

## Auto-detection

At app startup or when settings are opened:
- Test CLI availability via `which claude` / `which opencode` (or `flatpak-spawn --host which ...` in sandbox).
- When provider is `"auto"` (default), resolve to first available CLI in preference order: `claude` > `opencode`.
- In Preferences UI, show the resolved provider name when in auto mode (e.g., "Auto (Claude detected)").
- If no CLI is found and feature is enabled, log a warning and skip generation silently.

## CLI Option Verification (2026-03-01)

Verified locally in this environment:

- `claude 2.1.63 (Claude Code)`
  - Non-interactive: `-p` / `--print`
  - Model override: `--model <model>`
  - Output control: `--output-format text|json|stream-json`
  - Safer mode: `--permission-mode plan`, optional `--tools ""`

- `opencode 1.2.15`
  - Non-interactive: `opencode run [message..]`
  - Model override: `-m, --model provider/model`
  - Output control: `--format default|json`
  - Agent selection: `--agent <name>` (can use plan agent if configured)

## Execution Model

Title generation runs **synchronously inside the existing `IndexingWorker`** (Relm4 `Worker` trait), which already operates on a dedicated background thread:

1. `IndexingWorker` receives `StartIncremental` or `StartFullReindex` message.
2. Indexer parses and persists sessions as before.
3. After indexing, if AI title generation is enabled, iterate over newly indexed sessions that have no `title`.
4. For each, call the resolved CLI provider via `std::process::Command` with a 30s timeout.
5. On success, update the `title` column in SQLite.
6. On failure (timeout, non-zero exit, empty output), log and skip — indexing continues.
7. Worker sends `IndexingWorkerOutput::Completed` as before.

Sequential processing provides natural throttling — no rate limiting needed.

The `TitleGenerationConfig` is passed to the worker via its `Init` payload (extend existing `PathBuf` init to a struct).

## Title Prompt Specification (OpenCode-aligned)

Prompt design follows OpenCode title-generation behavior (`packages/opencode/src/agent/prompt/title.txt`).

Required rules for our generated-title prompt:
- Same language as the user request.
- Single line output only.
- 50-char target (hard post-guard still applies in app sanitization).
- No tool names.
- Focus on retrievability: what user wants to achieve.
- Keep exact technical terms, numbers, filenames, HTTP codes.
- Do not output meta text (no "summarizing", no explanations).
- Always output a meaningful title, even for short conversational input.

Canonical prompt template for CLI providers:

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

Context fed to prompt:
- Default: first real user message text.
- Optional enrichment: first assistant response snippet if available, capped to keep prompt short.
- If context is empty/invalid: skip generation and fallback.

---

### Task 1: Add GSettings keys and defaults (feature flag + provider + model)

**Files:**
- Modify: `data/io.github.supermaciz.sessionschronicle.gschema.xml.in`
- Test: `src/app.rs` (or a focused settings helper test module if added)

**Step 1: Write the failing test**

Add a test that validates settings defaults and accepted values:
- `ai-title-generation-enabled` defaults to `false`.
- `ai-title-generation-provider` defaults to `"auto"`.
- `ai-title-generation-model` defaults to empty string.

**Step 2: Run test to verify it fails**

Run: `cargo test settings_ -- --nocapture`
Expected: FAIL because keys are not defined.

**Step 3: Write minimal implementation**

Add keys in `data/io.github.supermaciz.sessionschronicle.gschema.xml.in`:
- `ai-title-generation-enabled` (`b`, default `false`)
- `ai-title-generation-provider` (`s`, default `"auto"`)
- `ai-title-generation-model` (`s`, default `""`)

**Step 4: Run test to verify it passes**

Run: `cargo test settings_ -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add data/io.github.supermaciz.sessionschronicle.gschema.xml.in
git commit -m "feat: add settings for configurable ai title generation"
```

---

### Task 2: Expose configuration in Preferences UI

**Files:**
- Modify: `src/ui/modals/preferences.rs`
- Modify: `src/app.rs` (if output wiring is needed)
- Test: `src/ui/modals/preferences.rs` (or existing UI test module)

**Step 1: Write the failing test**

Add test coverage for:
- toggle persists `ai-title-generation-enabled`,
- provider combo persists valid provider string,
- model entry persists free-form model string.

**Step 2: Run test to verify it fails**

Run: `cargo test preferences -- --nocapture`
Expected: FAIL because controls and wiring do not exist.

**Step 3: Write minimal implementation**

In `src/ui/modals/preferences.rs`, under a new group (e.g. "AI Session Titles"):
- `SwitchRow`: enable/disable feature.
- `ComboRow`: provider selection (`Auto`, `Claude`, `OpenCode`).
- `EntryRow` or `ActionRow + Entry`: model override.
- When provider is `Auto`, show subtitle with detected provider (e.g., "Claude detected" or "No CLI found").

Persist values with `gio::Settings::set_boolean` and `set_string`.

**Step 4: Run test to verify it passes**

Run: `cargo test preferences -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/modals/preferences.rs src/app.rs
git commit -m "feat: add preferences controls for ai title provider and model"
```

---

### Task 3: Add `title` to model and database schema (v5)

**Files:**
- Modify: `src/models/session.rs`
- Modify: `src/database/schema.rs`
- Test: `src/database/schema.rs`

**Step 1: Write the failing test**

Add migration test asserting:
- v4 DB migrates to v5,
- `sessions` table has `title` column.

**Step 2: Run test to verify it fails**

Run: `cargo test v4_to_v5_migration_adds_session_title_column -- --nocapture`
Expected: FAIL.

**Step 3: Write minimal implementation**

- Add `title: Option<String>` to `Session` with `#[serde(default)]`.
- Add `apply_v5_migration` and bump latest schema version.
- Ensure fresh `sessions` table includes `title`.

**Step 4: Run test to verify it passes**

Run: `cargo test v4_to_v5_migration_adds_session_title_column -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/models/session.rs src/database/schema.rs
git commit -m "feat: add session title field and v5 migration"
```

---

### Task 4: Persist/load `title` in DB queries and index writes

**Files:**
- Modify: `src/database/indexer.rs`
- Modify: `src/database/mod.rs`
- Test: `tests/load_session.rs`
- Test: `src/database/indexer.rs`

**Step 1: Write the failing tests**

1) Extend `tests/load_session.rs` to seed and assert `session.title` roundtrip.
2) Extend indexer tests to query `title` from `sessions` and assert persistence.

**Step 2: Run tests to verify they fail**

Run:
- `cargo test load_session_returns_existing_session -- --nocapture`
- `cargo test opencode_dual_read_prefers_sqlite_over_json -- --nocapture`

Expected: FAIL due to missing mapping.

**Step 3: Write minimal implementation**

- Add `title` to insert SQL in `src/database/indexer.rs`.
- Add `title` to all relevant `SELECT` projections in `src/database/mod.rs`.
- Map `title` in `session_from_row`.

**Step 4: Run tests to verify they pass**

Run:
- `cargo test load_session_returns_existing_session -- --nocapture`
- `cargo test opencode_dual_read_prefers_sqlite_over_json -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/database/indexer.rs src/database/mod.rs tests/load_session.rs
git commit -m "feat: persist and read session titles in sqlite layer"
```

---

### Task 5: Extract native titles from source formats

**Files:**
- Modify: `src/parsers/claude_code.rs`
- Modify: `src/parsers/opencode/mod.rs`
- Test: `src/parsers/claude_code.rs`
- Test: `src/parsers/opencode/mod.rs`

**Step 1: Write the failing tests**

- Claude: `type == "summary"` must map to `session.title`.
- OpenCode: metadata title maps to `session.title`, while `first_prompt` remains first user message.

**Step 2: Run tests to verify they fail**

Run:
- `cargo test claude -- --nocapture`
- `cargo test opencode -- --nocapture`

Expected: FAIL.

**Step 3: Write minimal implementation**

- Parse and capture Claude summary event as native title.
- Keep OpenCode title/first_prompt separated.

**Step 4: Run tests to verify they pass**

Run:
- `cargo test claude -- --nocapture`
- `cargo test opencode -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/parsers/claude_code.rs src/parsers/opencode/mod.rs
git commit -m "feat: map native source titles to session title"
```

---

### Task 6: Add CLI auto-detection and title generator module

**Files:**
- Create: `src/utils/title_generator.rs`
- Modify: `src/utils/mod.rs`
- Test: `src/utils/title_generator.rs`

**Step 1: Write the failing tests**

Add tests for:
- CLI auto-detection logic (mock `which` results),
- provider command building for `claude` and `opencode`,
- model override flag mapping per provider,
- prompt template generation matches OpenCode-aligned constraints,
- output sanitization (single line, max 50 chars, trim whitespace),
- flatpak-spawn --host wrapping behavior,
- disabled-feature short-circuit returns `None`,
- 30s timeout configuration.

**Step 2: Run tests to verify they fail**

Run: `cargo test title_generator -- --nocapture`
Expected: FAIL (module absent).

**Step 3: Write minimal implementation**

```rust
pub struct TitleGenerationConfig {
    pub enabled: bool,
    pub provider: TitleProvider,
    pub model_override: Option<String>,
}

pub enum TitleProvider {
    Auto,
    Claude,
    OpenCode,
}
```

Auto-detection helper:
- `detect_available_provider()` -> `Option<TitleProvider>` via `which` / `flatpak-spawn --host which`.
- Preference order: `claude` > `opencode`.

Provider-specific command builders:
- Claude: `claude -p "<prompt>" --output-format text --model <override?> --permission-mode plan --tools ""`
- OpenCode: `opencode run "<prompt>" --format json --model <override?>`

Execution:
- `std::process::Command` with `.timeout(Duration::from_secs(30))` (via `wait_timeout` or `kill` after spawn).
- Returns `Option<String>` — `None` on any failure.

Output sanitization:
- Take first non-empty line.
- Trim whitespace.
- Truncate to 50 chars at word boundary.
- Strip surrounding quotes if present.

**Step 4: Run tests to verify they pass**

Run: `cargo test title_generator -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/utils/title_generator.rs src/utils/mod.rs
git commit -m "feat: add cli auto-detection and ai title generator"
```

---

### Task 7: Integrate generator into IndexingWorker

**Files:**
- Modify: `src/indexing_worker.rs`
- Modify: `src/database/indexer.rs`
- Modify: `src/app.rs` (extend worker init payload)
- Test: `src/database/indexer.rs`

**Step 1: Write the failing test**

Add indexer test cases:
- disabled setting: no generation call,
- enabled setting + missing native title: generation attempted,
- generation failure: indexing still succeeds with `title = None`.

Use injected fake generator (trait or closure) to avoid real CLI execution in tests.

**Step 2: Run test to verify it fails**

Run: `cargo test indexer_generates_ai_title_when_enabled -- --nocapture`
Expected: FAIL.

**Step 3: Write minimal implementation**

- Extend `IndexingWorker` init from `PathBuf` to a struct including `TitleGenerationConfig`.
- In `app.rs`, read GSettings and build config before worker init.
- In worker `update()`, after `indexer.index_all_*()`, iterate newly indexed sessions without titles.
- For each, call `title_generator::generate()` synchronously (already on worker thread).
- On success, update title in DB via `indexer.update_session_title(session_id, title)`.
- On failure, log and continue.

**Step 4: Run test to verify it passes**

Run: `cargo test indexer_generates_ai_title_when_enabled -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/indexing_worker.rs src/database/indexer.rs src/app.rs
git commit -m "feat: generate ai titles during indexation in worker thread"
```

---

### Task 8: Prioritize `title` in session list display

**Files:**
- Modify: `src/ui/session_row.rs`
- Modify: `src/ui/session_list.rs`
- Test: `src/ui/session_row.rs`

**Step 1: Write the failing tests**

Add precedence assertions:
- `title` present -> display `title`.
- no `title`, `first_prompt` present -> display `first_prompt`.
- neither -> project fallback.

**Step 2: Run tests to verify they fail**

Run: `cargo test session_title_ -- --nocapture`
Expected: FAIL.

**Step 3: Write minimal implementation**

Update `SessionRow::session_title` precedence logic and test fixtures with `title` field.

**Step 4: Run tests to verify they pass**

Run: `cargo test session_title_ -- --nocapture`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/session_row.rs src/ui/session_list.rs
git commit -m "feat: display session title with fallback to first prompt"
```

---

### Task 9: Update documentation for configurable behavior

**Files:**
- Modify: `README.md`
- Modify: `docs/session-formats/claude-code.md`
- Modify: `docs/session-formats/opencode.md`
- Modify: `docs/PROJECT_STATUS.md` (if feature tracking section should be updated)

**Step 1: Write doc assertions/checklist**

Define explicit documentation checklist:
- default disabled behavior,
- auto-detection mechanism,
- provider/model selection,
- fallback order and failure behavior.

**Step 2: Validate existing docs are outdated**

Run: `rg "first_prompt|title" docs README.md`
Expected: old wording needs updates.

**Step 3: Write minimal doc updates**

Document precedence:
1. Native source title (OpenCode metadata, Claude summary event),
2. AI-generated title (if enabled and CLI available),
3. `first_prompt` fallback.

Document:
- Auto-detection of CLI providers (claude > opencode).
- Selected CLI must be installed/authenticated on host.
- 30s timeout per generation, non-fatal failures.

**Step 4: Verify consistency**

Run: `rg "AI title|auto-detect|provider|model" docs README.md`
Expected: consistent messaging.

**Step 5: Commit**

```bash
git add README.md docs/session-formats/claude-code.md docs/session-formats/opencode.md docs/PROJECT_STATUS.md
git commit -m "docs: describe configurable ai session title generation"
```

---

### Task 10: Final verification before PR

**Files:**
- Verify only

**Step 1: Formatting**

Run: `cargo fmt --all -- --check`
Expected: PASS.

**Step 2: Lints**

Run: `cargo clippy --all -- -D warnings`
Expected: PASS.

**Step 3: Tests**

Run: `cargo test --all --no-fail-fast`
Expected: PASS.

**Step 4: Flatpak verification**

Run:
- `flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean`
- `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`

Expected: app launches, settings visible, auto-detection works, behavior matches defaults and toggles.

**Step 5: Commit (if needed for final fixes)**

```bash
git add -A
git commit -m "test: finalize configurable ai session title feature verification"
```

---

## Acceptance Criteria (Issue #48)

- `Session` has a dedicated `title` field in model + DB.
- AI title generation is **disabled by default**.
- Auto-detection identifies available CLI providers (`claude`, `opencode`).
- User can enable/disable feature in Preferences.
- User can choose provider (`Auto`, `Claude`, `OpenCode`).
- User can set model override string.
- Native titles (OpenCode metadata, Claude summary) are preferred over AI-generated titles.
- Session-row display precedence is `title` -> `first_prompt` -> project fallback.
- Title generation runs synchronously in `IndexingWorker` thread with 30s timeout.
- CLI generation failures never break indexing.
- Full verification commands pass.

## Future Enhancements (out of scope)

- **Ollama provider**: local model generation without API keys.
- **Codex / Vibe providers**: extend `TitleProvider` enum and command builders.
- **Manual re-generation**: button in session detail view to regenerate a title on demand.
- **Batch re-generation**: regenerate titles for all sessions missing one.

## Risks and Mitigations

- **No CLI found on host** -> auto-detection returns `None`, feature stays inactive, user informed in Preferences UI.
- **Provider CLI not authenticated** -> non-fatal fallback to existing behavior, logged as warning.
- **Provider output format drifts over time** -> sanitize output aggressively and keep test fixtures for parser logic.
- **Slow CLI response** -> 30s timeout per call, sequential processing prevents resource exhaustion.
- **Migration regressions** -> explicit v4->v5 migration and idempotence tests.
