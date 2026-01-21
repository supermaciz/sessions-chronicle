# Development Workflow

## Running with Test Data

Sessions Chronicle uses command-line arguments to specify session directories, making development and testing straightforward without polluting production code.

### Development Mode

Use the `--sessions-dir` flag to point to test fixtures:

```bash
cargo run -- --sessions-dir tests/fixtures/claude_sessions
```

### Production Mode

Run without arguments to use default Claude Code sessions directory:

```bash
cargo run
```

This defaults to `~/.claude/projects/`.

### Custom Sessions Directory

Point to any directory containing session files:

```bash
cargo run -- --sessions-dir /path/to/custom/sessions
```

### GTK Options Passthrough

Pass GTK options after `--` so clap ignores them:

```bash
cargo run -- --sessions-dir tests/fixtures/claude_sessions -- --help-all
```

### Flatpak Development

Run the Flatpak build with a custom sessions directory:

```bash
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle --sessions-dir /path/to/sessions
```

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

Run the full app with test data:

```bash
# Terminal 1: Run with test fixtures
cargo run -- --sessions-dir tests/fixtures/claude_sessions

# Terminal 2: Make changes to code
# Save file, app auto-reloads (if using cargo watch)

# Or with cargo watch:
cargo watch -x 'run -- --sessions-dir tests/fixtures/claude_sessions'
```

### Testing with Real Sessions

Test with your actual Claude Code sessions:

```bash
# Use default directory
cargo run

# Or explicitly specify
cargo run -- --sessions-dir ~/.claude/projects
```

## Adding Test Fixtures

Create new test session files in `tests/fixtures/claude_sessions/`:

```bash
# Create a new test fixture
cat > tests/fixtures/claude_sessions/another-session.jsonl << 'EOF'
{"type":"user","message":{"role":"user","content":"Test message"},"timestamp":"2025-01-11T10:00:00.000Z","cwd":"/home/user/project","sessionId":"test123","uuid":"msg1","parentUuid":null,"isMeta":false}
{"type":"summary","summary":"Test session title","leafUuid":"msg1","timestamp":"2025-01-11T10:00:05.000Z","cwd":"/home/user/project","sessionId":"test123"}
EOF

# Run to verify
cargo run -- --sessions-dir tests/fixtures/claude_sessions
```

## Debugging

Enable trace logging:

```bash
RUST_LOG=debug cargo run -- --sessions-dir tests/fixtures/claude_sessions
```

Filter to specific modules:

```bash
RUST_LOG=sessions_chronicle::parsers=trace cargo run -- --sessions-dir tests/fixtures/claude_sessions
```

## Build Profiles

### Development Build

```bash
cargo build
cargo run -- --sessions-dir tests/fixtures/claude_sessions
```

### Release Build

```bash
cargo build --release
./target/release/sessions-chronicle --sessions-dir tests/fixtures/claude_sessions
```

### Production Installation

```bash
cargo install --path .
sessions-chronicle  # Uses default ~/.claude/projects
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

## Continuous Integration

In CI/CD pipelines, use test fixtures:

```yaml
# .github/workflows/test.yml
- name: Run tests
  run: cargo test

- name: Run integration test
  run: cargo run -- --sessions-dir tests/fixtures/claude_sessions
```

## Summary

**Development**: `cargo run -- --sessions-dir tests/fixtures/claude_sessions`
**Production**: `cargo run` or installed binary
**Custom**: `--sessions-dir /path/to/sessions`

This keeps code clean, explicit, and follows Rust CLI best practices.

---

**Last Updated**: 2026-01-19
