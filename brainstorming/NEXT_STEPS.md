# Next Steps - Quick Reference

## Immediate Tasks (Priority Order)

### 1. Add SessionDetail Component
- Create `src/ui/session_detail.rs`
- Display conversation transcript
- Color-code by role
- Add scrolling support

### 2. Session Resumption
- Create `src/utils/terminal.rs`
- Detect available terminal emulator
- Build resume command for tool
- Launch terminal with session

### 3. OpenCode + Codex Indexing
- Add parsers for OpenCode and Codex
- Index sessions into SQLite
- Ensure tool filters show data

### 4. Search Term Highlighting
- Highlight matching terms in SessionDetail
- Use markup tags with highlighting class

---

## Completed Milestones

- ✅ Add `clap` dependency
- ✅ Add CLI args for `--sessions-dir`
- ✅ Wire database indexer in `App`
- ✅ Load sessions from database in `SessionList`
- ✅ Connect sidebar tool filters to SessionList
- ✅ Implement search UI (SearchBar + SearchEntry in `app.rs`)
- ✅ Implement FTS5 search queries in `database/mod.rs`
- ✅ Connect search to SessionList filtering

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

1. **Session detail missing** - No transcript view for selected session
2. **Session resumption missing** - Can't resume sessions from the app
3. **OpenCode/Codex not indexed** - Filters show empty results for those tools
4. **Search term highlighting missing** - Search works but doesn't highlight matches
