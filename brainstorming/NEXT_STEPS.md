# Next Steps - Quick Reference

## Immediate Tasks (Priority Order)

### 1. Session Resumption Improvements (High Priority)
- Add visual feedback during terminal launch (toasts + button states)
- Implement Claude CLI installation verification
- Enhance tooltips and accessibility for resume buttons
- Add comprehensive unit tests for terminal utilities

### 2. OpenCode + Codex Indexing
- Add parsers for OpenCode and Codex
- Index sessions into SQLite
- Ensure tool filters show data

### 3. Search Term Highlighting
- Highlight matching terms in SessionDetail
- Use markup tags with highlighting class

---

## Completed Milestones

- ✅ Add `clap` dependency
- ✅ Add CLI args for `--sessions-dir`
- ✅ Fix session date/sort semantics (Date column = session end time)
- ✅ Wire database indexer in `App`
- ✅ Load sessions from database in `SessionList`
- ✅ Connect sidebar tool filters to SessionList
- ✅ Implement search UI (SearchBar + SearchEntry in `app.rs`)
- ✅ Implement FTS5 search queries in `database/mod.rs`
- ✅ Connect search to SessionList filtering
- ✅ Add SessionDetail component with conversation transcript view
- ✅ Color-code messages by role in SessionDetail
- ✅ Add scrolling support to SessionDetail
- ✅ Implement navigation between list and detail views
- ✅ Add session resumption with terminal emulator integration (basic)
- ✅ Add terminal preferences dialog for emulator selection

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

1. **Session resumption lacks user feedback** - No visual indication during terminal launch
2. **No Claude CLI verification** - Silent failures if Claude not installed
3. **OpenCode/Codex not indexed** - Filters show empty results for those tools
4. **Search term highlighting missing** - Search works but doesn't highlight matches

---

**Last Updated**: 2026-01-19

**See Also**: `SESSION_RESUMPTION_IMPROVEMENTS.md` for detailed implementation plan
