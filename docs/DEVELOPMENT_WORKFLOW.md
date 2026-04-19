# Development Workflow

## Building and running the Project

### Via Flatpak

```bash
flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean
```

Run with:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

Use this path when you want the closest match to the packaged app environment.

### Via Meson (faster inner loop)

```bash
meson setup builddir -Dprofile=development --prefix="$HOME/.local"
```

If the build directory already exists and you need to change options, rerun:

```bash
meson setup builddir --reconfigure -Dprofile=development --prefix="$HOME/.local"
```

Compile incrementally:

```bash
meson compile -C builddir
```

Install the rebuilt binary and desktop resources into `~/.local`:

```bash
meson install -C builddir
```

Run the locally installed build:

```bash
"$HOME/.local/bin/sessions-chronicle"
```

Meson is the faster day-to-day loop because it reuses the local build tree instead of rebuilding the full Flatpak dependency stack each time. Flatpak remains the right choice when you need to verify packaging behavior or reproduce the release-like runtime.

### Sessions locations

This indexes sessions from all supported AI assistants:
- Claude Code: `~/.claude/projects/`
- OpenCode: `~/.local/share/opencode/storage/`
- Codex: `~/.codex/sessions/`
- Mistral Vibe: `~/.vibe/logs/session/`

## Using Test Fixtures

The `--sessions-dir` flag overrides session source paths **for all assistants**. It maps known fixture subdirectories automatically:

```bash
# Override with the full fixture root — maps all four assistants
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

Meson-installed build equivalent:

```bash
"$HOME/.local/bin/sessions-chronicle" --sessions-dir tests/fixtures
```

This maps to:
| AI assistant | Resolved path |
|--------------|---------------|
| Claude Code | `tests/fixtures/claude_sessions/` |
| OpenCode | `tests/fixtures/opencode_storage/` |
| Codex | `tests/fixtures/codex_sessions/` |
| Mistral Vibe | `tests/fixtures/vibe_sessions/` |

OpenCode database resolution in override mode:
- Checks `tests/fixtures/opencode_storage/opencode.db` first
- Falls back to `tests/fixtures/opencode.db` (parent directory) when present
- Uses JSON storage fallback if no SQLite database is found

If a known subdirectory is missing, the override root itself is used as a fallback for that assistant.

You can also point to a single assistant's directory:

```bash
# Override with Claude-only data — all assistants fall back to this path
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures/claude_sessions
```

### Override mode and database isolation

When `--sessions-dir` is active, the app uses a separate database file (`sessions-override.db`) instead of the default `sessions.db`. This prevents stale cross-mode data contamination when switching between override and normal mode.

### Resetting the index

The Preferences dialog (menu > Preferences > Advanced) includes a **Reset session index** action that clears and rebuilds the active database from the current session sources. This is useful after modifying fixture files or when the index gets out of sync.

## Terminology

- `AI assistant` refers to a session source such as Claude Code, OpenCode, Codex, or Mistral Vibe.
- `tool call` refers to an action invoked within a transcript.
- When docs mention a literal field, API key, or legacy storage name called `tool`, that wording is kept intentionally.

## Why This Approach?

### ✅ Advantages

1. **Clean Separation** - Production code doesn't check for test directories
2. **Explicit Over Magical** - Developers explicitly choose test mode
3. **Standard Practice** - CLI args are the conventional way to override defaults
4. **Flexible** - Easy to test with any directory, not just `tests/fixtures/`
5. **No Pollution** - Test-checking logic doesn't bloat production binary
6. **Build Artifacts** - Can install release build without dev dependencies

### ❌ What We Avoid

```rust
// BAD: Don't do this
let sessions_dir = if std::path::Path::new("tests/fixtures/claude_sessions").exists() {
    std::path::PathBuf::from("tests/fixtures/claude_sessions")
} else {
    std::path::PathBuf::from("~/.claude/projects")
};
```

**Problems with this approach:**
- Mixes production and test concerns
- "Magical" behavior that's hard to discover
- Hardcoded paths in production binary
- Tests can accidentally pass due to wrong data source
- Violates single responsibility principle

## Testing Workflow

### Integration Testing

Run the full app with test fixtures using the `--sessions-dir` flag shown above.

## Startup Indexing Behavior

- Startup uses background incremental indexing based on file fingerprints.
- The header spinner indicates indexing is running.
- Assistant rows in the sidebar gain source-health dots after the first indexing pass.
- Partial indexing problems reveal a persistent Sessions-only banner; clean runs hide it again.
- The banner action opens the Indexing Status dialog with per-source details and recent errors.
- The global empty state shows resolved source paths and assistant health once indexing has completed.
- Preferences -> Advanced -> Reset session index triggers a full reindex.

## Adding Test Fixtures

Create new test session files in the appropriate fixture directory:

```bash
# Claude Code (JSONL format)
cat > tests/fixtures/claude_sessions/another-session.jsonl << 'EOF'
{"type":"user","message":{"role":"user","content":"Test message"},"timestamp":"2025-01-11T10:00:00.000Z","cwd":"/home/user/project","sessionId":"test123","uuid":"msg1","parentUuid":null,"isMeta":false}
{"type":"summary","summary":"Test session title","leafUuid":"msg1","timestamp":"2025-01-11T10:00:05.000Z","cwd":"/home/user/project","sessionId":"test123"}
EOF
```

See `tests/fixtures/README.md` for format details on all supported AI assistants.

## Debugging

Enable trace logging by setting `RUST_LOG`:

```bash
# Debug level
RUST_LOG=debug flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle

# Filter to specific modules
RUST_LOG=sessions_chronicle::parsers=trace flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

Meson-installed build equivalent:

```bash
RUST_LOG=debug "$HOME/.local/bin/sessions-chronicle"
RUST_LOG=sessions_chronicle::parsers=trace "$HOME/.local/bin/sessions-chronicle"
```

## Testing

```bash
cargo test --all --no-fail-fast
```

### Linting

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
```

## IDE Configuration

### VS Code (launch.json)

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug with test fixtures",
      "cargo": {
        "args": ["build", "--bin=sessions-chronicle"]
      },
      "args": ["--sessions-dir", "tests/fixtures/claude_sessions"],
      "cwd": "${workspaceFolder}"
    },
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug with real sessions",
      "cargo": {
        "args": ["build", "--bin=sessions-chronicle"]
      },
      "args": [],
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

### IntelliJ IDEA / RustRover

Create run configurations:
1. **Debug (test fixtures)**
   - Program arguments: `--sessions-dir tests/fixtures/claude_sessions`
2. **Debug (production)**
   - Program arguments: (empty)

## CI/CD

The project uses GitHub Actions for continuous integration and releases. Workflows are defined in `.github/workflows/`.

### CI (`ci.yml`) — runs on every push to `main` and on PRs

| Job | What it does |
|-----|-------------|
| **Tests** | `cargo test` (under Xvfb for GTK) |
| **Clippy** | `cargo clippy -- -D warnings` |
| **Rustfmt** | `cargo fmt --all -- --check` |
| **Coverage** | `cargo llvm-cov` → LCOV report uploaded to Codecov |
| **Flatpak** | Builds the dev Flatpak bundle |

### Release (`release.yml`) — runs when a GitHub release is published

Builds the stable Flatpak bundle using the `build-aux/io.github.supermaciz.sessionschronicle.json` manifest, generates a SHA256 checksum, and uploads both to the release.

### Flatpak Repository (`flatpak-repository.yml`) — runs when a GitHub release is published or manually

Builds a signed Flatpak repository from the stable manifest, stores the payload in a dedicated branch, and deploys it under `https://sessions-chronicle.maciz.dev/flatpak/` as part of the GitHub Pages website. See `docs/FLATPAK_REPOSITORY.md` for signing-key setup, publishing details, and install commands.

### Build manifests

Two Flatpak manifests exist in `build-aux/`:

| Manifest | Purpose |
|----------|---------|
| `io.github.supermaciz.sessionschronicle.Devel.json` | Development builds (used in CI and local dev) |
| `io.github.supermaciz.sessionschronicle.json` | Stable release builds (used by release workflow) |

## Summary

- **Flatpak build**: `flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean`
- **Flatpak run**: `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle`
- **Meson setup**: `meson setup builddir -Dprofile=development --prefix="$HOME/.local"`
- **Meson rebuild**: `meson compile -C builddir && meson install -C builddir`
- **Meson run**: `"$HOME/.local/bin/sessions-chronicle"`
- **Test Data**: Add `--sessions-dir tests/fixtures` flag
- **CI parity**: `cargo fmt --all -- --check && cargo clippy --all -- -D warnings && cargo test --all --no-fail-fast`

---

**Last Updated**: 2026-03-25
