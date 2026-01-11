# Repository Guidelines

## Project Structure & Module Organization
- `src/` contains the Rust app: `app.rs` and `main.rs` glue, `ui/` for Relm4 widgets, `database/` for SQLite, `parsers/` for session formats, `models/` for domain types, and `modals/` for dialogs.
- `data/` holds desktop metadata, GSettings schema, icons, CSS, and UI resources in `data/resources/`.
- `tests/fixtures/` provides sample JSONL sessions for development and manual testing.
- `build-aux/` contains Flatpak and Meson manifests.
- `brainstorming/` hosts design notes and planning docs (reference only).

## Build, Test, and Development Commands
- `flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean`: build the GNOME Flatpak bundle.
- `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle`: run the Flatpak build.
- `cargo run -- --sessions-dir tests/fixtures/claude_sessions`: run with fixture sessions for development.
- `cargo run`: run against the default sessions directory (`~/.claude/projects/`).
- `cargo test`: run Rust tests.
- `cargo fmt --all`: enforce rustfmt style (also used by the pre-commit hook).

## Coding Style & Naming Conventions
- Rust 2024 edition; format with rustfmt and keep standard 4-space indentation.
- Naming follows Rust conventions: `snake_case` for functions/modules/vars, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep UI definitions in `data/resources/ui/` and CSS in `data/resources/style.css`.

## Testing Guidelines
- Use fixtures from `tests/fixtures/claude_sessions/` for repeatable manual runs.
- Prefer adding integration tests under `tests/` and running them via `cargo test`.
- Use `RUST_LOG=debug` for troubleshooting (e.g., `RUST_LOG=debug cargo run -- --sessions-dir tests/fixtures/claude_sessions`).

## Commit & Pull Request Guidelines
- Commit messages follow a `type: short summary` pattern (e.g., `feat: ...`, `docs: ...`, `fix: ...`).
- PRs should include a clear description, the key commands run (`cargo test`, `cargo fmt --all`, or Flatpak build if relevant), and screenshots for UI changes.
- Link related issues or notes from `brainstorming/` when applicable.
