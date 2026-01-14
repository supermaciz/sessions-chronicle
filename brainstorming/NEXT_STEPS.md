# Next Steps - Quick Reference

## Immediate Tasks (Priority Order)

### 1. Implement Search
- Connect SearchEntry in `app.rs`
- Query FTS5 messages table
- Display matching sessions
- Highlight search terms

### 2. Add SessionDetail Component
- Create `src/ui/session_detail.rs`
- Display conversation transcript
- Color-code by role
- Add scrolling support

### 3. Session Resumption
- Create `src/utils/terminal.rs`
- Detect available terminal emulator
- Build resume command for tool
- Launch terminal with session

### 4. OpenCode + Codex Indexing
- Add parsers for OpenCode and Codex
- Index sessions into SQLite
- Ensure tool filters show data

---

## Completed Milestones

- ✅ Add `clap` dependency
- ✅ Add CLI args for `--sessions-dir`
- ✅ Wire database indexer in `App`
- ✅ Load sessions from database in `SessionList`
- ✅ Connect sidebar tool filters to SessionList

---

## Testing Workflow

```bash
# Run with test fixtures
cargo run -- --sessions-dir tests/fixtures/claude_sessions

# Run with real sessions
cargo run
```

---

## Current Blockers

1. **Search missing** - Can't find sessions by content yet
2. **Session detail missing** - No transcript view
3. **OpenCode/Codex not indexed** - Filters show empty results
