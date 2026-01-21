# Test Fixtures

This directory contains sample session files for testing and development.

## Structure

```
fixtures/
└── claude_sessions/     # Claude Code session samples (JSONL format)
    └── sample-session.jsonl
```

## Purpose

- **Development**: Test parsers without requiring actual Claude Code/OpenCode/Codex installations
- **Testing**: Integration tests use these fixtures to verify parsing and database indexing
- **CI/CD**: Consistent test data across different environments

## Claude Code Session Format

Files are in JSONL (JSON Lines) format, with one JSON object per line:

- **User messages**: `type: "user"`
- **Assistant messages**: `type: "assistant"`
- **System events**: `type: "system"` with `subtype` (e.g., `local_command`)
- **Summary**: `type: "summary"` containing session title

See `docs/SESSION_FORMAT_ANALYSIS.md` for detailed format documentation.

## Adding Fixtures

To add more test data:

1. Create new `.jsonl` files following the Claude Code format
2. Update integration tests in `tests/` to use the new fixtures
3. Keep fixtures minimal - only include what's needed for testing specific features
