# Sessions Chronicle - Project Status

**A GNOME app for browsing, searching, and resuming AI coding sessions**

---

## Current Status: Phase 1 - Core Implementation

### ✅ Completed

**Core Architecture**
- ✅ Project structure with Rust + GTK4 + Relm4
- ✅ Data models (`Session`, `Message`, `Tool`, `Role`)
- ✅ Database layer with SQLite + FTS5
- ✅ Claude Code parser (JSONL format, streaming)
- ✅ Test fixtures in `tests/fixtures/claude_sessions/`
- ✅ Basic UI structure (Sidebar, SessionList, SessionDetail)

**Implemented Core Features**
- ✅ CLI arguments (`clap`) for `--sessions-dir` override
- ✅ Relm4 CLI passthrough (`with_args`) + GTK arg split
- ✅ Database indexer wired into App initialization
- ✅ SessionList loading from database
- ✅ Sidebar tool filters wired to SessionList (Claude data only)
- ✅ Search functionality with FTS5 full-text search
- ✅ Search UI with SearchBar and SearchEntry in `app.rs`
- ✅ SessionDetail component with conversation transcript view
- ✅ Navigation between list and detail views using NavigationView

**Dependencies**
- ✅ Relm4 (reactive UI framework)
- ✅ Libadwaita (GNOME styling)
- ✅ rusqlite (SQLite database)
- ✅ serde/serde_json (JSON parsing)
- ✅ chrono (date/time handling)
- ✅ anyhow/thiserror (error handling)
- ✅ clap (CLI args)

### 🚧 In Progress / Next Steps

**Missing Features**
- ⬜ Session resumption (terminal launch with tool resume command)
- ⬜ OpenCode/Codex parsers + indexing (filters show empty for those tools)
- ⬜ Search term highlighting in SessionDetail

### 📋 Roadmap

**Phase 1: Single Tool (Claude Code)** - Current
1. ✅ Add missing dependencies
2. ✅ Implement CLI args with `--sessions-dir`
3. ✅ Wire database indexer into App
4. ✅ Load sessions in SessionList from DB
5. ✅ Connect sidebar tool filters to SessionList
6. ✅ Implement search with FTS5 queries
7. ✅ Add SessionDetail component
8. Add session resumption (terminal launch)

**Phase 2: Multi-Tool Support** - Future
- OpenCode parser (multi-file format)
- Codex parser (streaming, encrypted reasoning)
- Tool switching in UI

**Phase 3: Advanced Features** - Future
- Real-time session monitoring (file watching)
- Session export (Markdown/HTML)
- Analytics and usage charts
- Git integration

---

## Technical Architecture

### Tech Stack

- **Language**: Rust 2024
- **UI**: GTK4 + Libadwaita (GNOME HIG compliant)
- **Reactive UI**: Relm4 (Elm-inspired architecture)
- **Database**: SQLite with FTS5 (full-text search)
- **Supported Tools**: Claude Code (v1), OpenCode (v2), Codex (v2)

### Project Structure

```
sessions-chronicle/
├── src/
│   ├── main.rs           # Entry point, Relm4 app setup
│   ├── lib.rs            # Library exports
│   ├── config.rs         # App constants (APP_ID, VERSION)
│   ├── app.rs            # Main App component (search, window)
│   ├── models/           # Data models
│   │   ├── session.rs    # Session, Tool
│   │   └── message.rs    # Message, Role
│   ├── parsers/          # Session file parsers
│   │   └── claude_code.rs
│   ├── database/         # SQLite operations
│   │   ├── schema.rs     # DB schema + FTS5
│   │   ├── indexer.rs    # Index sessions
│   │   └── mod.rs        # load_sessions, search_sessions
│   ├── ui/               # UI components (Relm4)
│   │   ├── sidebar.rs    # Tool/project filters
│   │   ├── session_list.rs  # Session list view
│   │   └── modals/
│   │       ├── about.rs      # About dialog
│   │       └── shortcuts.rs  # Keyboard shortcuts
│   └── models/mod.rs      # Model exports
├── tests/fixtures/       # Test data
└── brainstorming/        # Design docs
```

### Database Schema

**sessions** table:
```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    tool TEXT NOT NULL,
    project_path TEXT,
    start_time INTEGER NOT NULL,
    message_count INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    last_updated INTEGER NOT NULL
);
```

**messages** table (FTS5):
```sql
CREATE VIRTUAL TABLE messages USING fts5(
    session_id UNINDEXED,
    message_index UNINDEXED,
    role UNINDEXED,
    content,              -- searchable
    timestamp UNINDEXED
);
```

### Session File Formats

**Claude Code**: `~/.claude/projects/`
- Format: JSONL (one JSON object per line)
- Event types: `user`, `assistant`, `system`, `summary`
- Streaming: Line-by-line with `BufReader` (never load full file)

**OpenCode**: `~/.local/share/opencode/storage/` (v2)
- Format: Multi-file structure (session metadata + message dirs)
- Complex: Parent-child sessions, message parts, diffs

**Codex**: `~/.codex/sessions/` (v2)
- Format: JSONL with streaming chunks
- Special: Encrypted reasoning blocks (never decrypt locally)

---

## Development Workflow

### Running with Test Data

```bash
# Development with test fixtures
cargo run -- --sessions-dir tests/fixtures/claude_sessions

# Production (uses ~/.claude/projects by default)
cargo run
```

See `DEVELOPMENT_WORKFLOW.md` for details.

### Key Design Decisions

1. **CLI args for test data** - No hardcoded checks for test directories
2. **Streaming JSONL parsing** - Use `BufReader::lines()`, never load entire file
3. **SQLite FTS5 for search** - Simple, fast, built-in full-text search
4. **Single tool first** - Claude Code only for v1, others in v2
5. **List view UI** - More information density than cards view

### Common Pitfalls

**❌ Don't load JSONL into memory:**
```rust
let content = std::fs::read_to_string(file_path)?;  // BAD
```

**✅ Stream line by line:**
```rust
let file = File::open(file_path)?;
let reader = BufReader::new(file);
for line in reader.lines() { /* ... */ }
```

**❌ Don't hardcode paths:**
```rust
let db_path = "/home/user/.local/share/...";  // BAD
```

**✅ Use platform APIs:**
```rust
let data_dir = glib::user_data_dir();
let db_path = data_dir.join("sessions-chronicle").join("sessions.db");
```

---

## Implementation Notes

### Immediate Tasks

1. **Add SessionDetail component**:
   - Display transcript for selected session
   - Include tool icon + timestamps
   - Color-code messages by role (user/assistant/system)

2. **Session resumption**:
   - Create `src/utils/terminal.rs`
   - Detect available terminal emulator
   - Build and launch tool-specific resume commands

3. **OpenCode + Codex indexing**:
   - Add parsers for OpenCode and Codex
   - Index sessions into SQLite so filters show data

4. **Search term highlighting**:
   - Highlight matching terms in SessionDetail when viewing search results

### Testing Strategy

**Unit tests**: Test parsers with fixtures
```bash
cargo test
```

**Integration testing**: Run with test data
```bash
cargo run -- --sessions-dir tests/fixtures/claude_sessions
```

**With real sessions**: Test with actual Claude Code sessions
```bash
cargo run  # Uses ~/.claude/projects
```

### Error Handling

- Use `anyhow` for app-level errors with context
- Use `thiserror` for parser-specific errors
- Log warnings for malformed files, continue indexing
- Never panic on user data

---

## References

### Design Documents

- **SESSION_FORMAT_ANALYSIS.md** - Detailed format specs for all 3 tools
- **SEARCH_ARCHITECTURE.md** - Why we chose SQLite FTS5
- **UI_DESIGN_COMPARISON.md** - List view vs cards view analysis
- **DEVELOPMENT_WORKFLOW.md** - CLI args and testing workflow

### External Resources

- [Claude Code Session Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/claude-code-session-format.md)
- [Codex Storage Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/session-storage-format.md)
- [Agent Sessions (inspiration)](https://github.com/jazzyalex/agent-sessions)

---

**Last Updated**: 2026-01-15
**Current Phase**: Phase 1 - Single Tool Support (Claude Code)
**Next Milestone**: Session resumption + OpenCode/Codex support
