# Repository Guidelines

## Where to Look First
- `README.md`: setup, project overview, and basic build/run commands.
- `docs/DEVELOPMENT_WORKFLOW.md`: fixture-driven runs (`--sessions-dir`), debugging, and CI details.
- `docs/PROJECT_STATUS.md`: current roadmap, phase status, and design references.

## Terminology
- Use `AI assistant` for Claude Code, OpenCode, Codex, and Mistral Vibe when they are session sources.
- Use `tool call` for actions invoked inside transcripts.
- Avoid `tool` alone in prose unless you are referring to a literal historical field/schema name or an external format that uses that term.

## Project Structure & Module Organization
- `src/` contains the Rust app:
  - `main.rs` and `app.rs` wire app startup and top-level Relm4 flow.
  - `session_sources.rs` resolves per-assistant session paths and `--sessions-dir` overrides.
  - `ui/` holds Relm4 widgets, with dialogs under `ui/modals/`.
  - `database/` owns SQLite schema, indexing, and search.
  - `parsers/` handles assistant-specific session formats.
  - `models/` defines domain types.
  - `utils/` contains shared helpers (for example terminal integration).
- `data/` holds desktop metadata, GSettings schema, icons, CSS, and UI resources in `data/resources/`.
- `tests/` contains integration tests; `tests/fixtures/` contains sample sessions for Claude Code, OpenCode, Codex, and Mistral Vibe.
- `build-aux/` contains Flatpak manifests (dev and stable) and the vendor script for offline builds.
- `docs/` hosts architecture notes plus exploration, design, and implementation plans.
  - `docs/plans/` contains plan files following these naming conventions: `YYYY-MM-DD-feature-name-exploration.md`, `YYYY-MM-DD-feature-name-design.md`, and implementation plans as `YYYY-MM-DD-feature-name.md` (preferred) or `YYYY-MM-DD-feature-name-implementation.md` (optional).
- `flatpak_app/` is generated build output; do not edit it directly.

## Plan Types in `docs/plans/`

There are three plan types in `docs/plans/`:

### `-exploration.md` - Design Exploration
Created when multiple implementation approaches exist and a decision must be recorded.
- Compares 2+ alternative designs with trade-offs
- Includes visual mockups when relevant
- Ends with a decision and rationale
- Example: `2026-02-10-session-row-prompt-preview-exploration.md`

### `-design.md` - Implementation Design
The single source of truth after a decision is made.
- Problem statement and scope
- Schema changes (SQL migrations)
- API signatures and data structures
- Step-by-step implementation flow
- UI/UX behavior specifications
- Test and verification plan
- Produced via the `brainstorming` skill
- Example: `2026-03-01-startup-performance-design.md`

### `.md` (preferred) or `-implementation.md` (optional) - Implementation Plan
Task-by-task execution plan used to implement a validated design.
- Produced via the `writing-plans` skill
- Prefer no suffix for new plans; `-implementation` is acceptable when extra clarity is useful
- Typically not committed to git
- Example: `2026-03-04-startup-performance.md`

**Process**: Exploration -> Decision -> Design -> Implementation Plan -> Implementation

## Fast Dev Loop
- `flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean`: build the GNOME Flatpak bundle.
- `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle`: run with local session data.
- `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures`: run with fixture data.
- `cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast`: run CI-parity checks locally.

## Coding Style & Naming Conventions
- Rust 2024 edition; format with rustfmt and keep standard 4-space indentation.
- Naming follows Rust conventions: `snake_case` for functions/modules/vars, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep UI definitions in `data/resources/ui/` and CSS in `data/resources/style.css`.

## Parsing, Paths, and Data Safety Guardrails
- Stream JSONL data with `BufReader` and line iteration; do not load large session logs fully into memory.
- Do not hardcode user/system paths; use platform APIs and existing path-resolution helpers.
- Treat session files as untrusted input: handle malformed entries gracefully and continue indexing where possible.

## Testing Guidelines
- Use fixtures from `tests/fixtures/` for repeatable manual runs; prefer `--sessions-dir tests/fixtures` for end-to-end checks.
- Prefer adding integration tests under `tests/` and running them via `cargo test --all --no-fail-fast`.
- Run `cargo clippy --all -- -D warnings` and `cargo fmt --all -- --check` before opening a PR.

## Commit & Pull Request Guidelines
- Commit messages follow a `type: short summary` pattern (e.g., `feat: ...`, `docs: ...`, `fix: ...`).
- PRs should include a concise problem/solution description, key verification commands run, and screenshots for UI changes.
- Link related issues or notes from `docs/` when applicable.

## Definition of Done (Before PR)
- `cargo fmt --all -- --check` passes.
- `cargo clippy --all -- -D warnings` passes.
- `cargo test --all --no-fail-fast` passes.
- UI changes include updated screenshots.
- Packaging/build changes include a Flatpak build verification run.

## Markdown Style
- All documentation uses GitHub Flavored Markdown (GFM).
- To create a line break within a paragraph (soft break), add **two trailing spaces** at the end of the line. Without them, consecutive lines render as a single paragraph.

## Documentation & Resources
- Relm4 docs are not available via Context7; use zread or the direct links below.
- Relm4 crate docs: https://docs.rs/crate/relm4/0.10.0
- Relm4 book: https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/SUMMARY.md
- Relm4 macros: https://docs.rs/relm4-macros/0.10.1/relm4_macros/
- Relm4 icons: https://crates.io/crates/relm4-icons
