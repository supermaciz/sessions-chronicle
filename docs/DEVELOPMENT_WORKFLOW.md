# Development Workflow

## Building the Project

```bash
flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean
```

## Running the Project

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

This indexes sessions from all supported tools:
- Claude Code: `~/.claude/projects/`
- OpenCode: `~/.local/share/opencode/storage/`
- Codex: `~/.codex/sessions/`
- Mistral Vibe: `~/.vibe/logs/session/`

## Using Test Fixtures

The `--sessions-dir` flag overrides session source paths **for all tools**. It maps known fixture subdirectories automatically:

```bash
# Override with the full fixture root — maps all four tools
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures
```

This maps to:
| Tool | Resolved path |
|------|--------------|
| Claude Code | `tests/fixtures/claude_sessions/` |
| OpenCode | `tests/fixtures/opencode_storage/` |
| Codex | `tests/fixtures/codex_sessions/` |
| Mistral Vibe | `tests/fixtures/vibe_sessions/` |

If a known subdirectory is missing, the override root itself is used as a fallback for that tool.

You can also point to a single tool's directory:

```bash
# Override with Claude-only data — all tools fall back to this path
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir tests/fixtures/claude_sessions
```

### Override mode and database isolation

When `--sessions-dir` is active, the app uses a separate database file (`sessions-override.db`) instead of the default `sessions.db`. This prevents stale cross-mode data contamination when switching between override and normal mode.

### Resetting the index

The Preferences dialog (menu > Preferences > Advanced) includes a **Reset session index** action that clears and rebuilds the active database from the current session sources. This is useful after modifying fixture files or when the index gets out of sync.

### AI session title behavior

- UI title precedence is `title -> first_prompt -> project`.
- AI title generation is **disabled by default** and runs during indexing when enabled.
- Provider `auto` mode tries `OpenCode` first (default model `opencode/gpt-5-nano`), then falls back to `Claude` (default model `claude-3-5-haiku-latest`).
- You can override the model in Preferences (`AI Session Titles` group).

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

### Unit Tests

Run unit tests (when implemented):

```bash
cargo test
```

These use fixtures automatically via the test harness.

### Integration Testing

Run the full app with test fixtures using the `--sessions-dir` flag shown above.

## Adding Test Fixtures

Create new test session files in the appropriate fixture directory:

```bash
# Claude Code (JSONL format)
cat > tests/fixtures/claude_sessions/another-session.jsonl << 'EOF'
{"type":"user","message":{"role":"user","content":"Test message"},"timestamp":"2025-01-11T10:00:00.000Z","cwd":"/home/user/project","sessionId":"test123","uuid":"msg1","parentUuid":null,"isMeta":false}
{"type":"summary","summary":"Test session title","leafUuid":"msg1","timestamp":"2025-01-11T10:00:05.000Z","cwd":"/home/user/project","sessionId":"test123"}
EOF
```

See `tests/fixtures/README.md` for format details on all supported tools.

## Debugging

Enable trace logging by setting `RUST_LOG`:

```bash
# Debug level
RUST_LOG=debug flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle

# Filter to specific modules
RUST_LOG=sessions_chronicle::parsers=trace flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

## Testing

### Unit Tests

```bash
cargo test
```

### Linting

```bash
cargo clippy
cargo fmt --all
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

### Build manifests

Two Flatpak manifests exist in `build-aux/`:

| Manifest | Purpose |
|----------|---------|
| `io.github.supermaciz.sessionschronicle.Devel.json` | Development builds (used in CI and local dev) |
| `io.github.supermaciz.sessionschronicle.json` | Stable release builds (used by release workflow) |

## Summary

- **Build**: `flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean`
- **Run**: `flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle`
- **Test Data**: Add `--sessions-dir tests/fixtures` flag
- **Unit Tests**: `cargo test`

---

**Last Updated**: 2026-02-22
