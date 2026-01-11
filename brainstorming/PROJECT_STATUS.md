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
- ✅ Basic UI structure (Sidebar, SessionList)

**Dependencies**
- ✅ Relm4 (reactive UI framework)
- ✅ Libadwaita (GNOME styling)
- ✅ rusqlite (SQLite database)
- ✅ serde/serde_json (JSON parsing)
- ✅ chrono (date/time handling)
- ✅ anyhow/thiserror (error handling)

### 🚧 In Progress / Next Steps

**Missing Features**
- ⬜ CLI arguments (`clap`) for `--sessions-dir` override
- ⬜ Database indexer wired into App initialization
- ⬜ SessionList loading from database
- ⬜ Search functionality (FTS5 queries)
- ⬜ SessionDetail component (conversation view)
- ⬜ Session resumption (terminal launch)
- ⬜ Sidebar filters connected to SessionList

**Missing Dependencies**
- ⬜ Add `clap` for CLI argument parsing

### 📋 Roadmap

**Phase 1: Single Tool (Claude Code)** - Current
1. Add missing dependencies
2. Implement CLI args with `--sessions-dir`
3. Wire database indexer into App
4. Load sessions in SessionList from DB
5. Add SessionDetail component
6. Implement search with FTS5
7. Add session resumption (terminal launch)

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
│   ├── app.rs            # Main App component
│   ├── models/           # Data models
│   │   ├── session.rs    # Session, Tool
│   │   └── message.rs    # Message, Role
│   ├── parsers/          # Session file parsers
│   │   └── claude_code.rs
│   ├── database/         # SQLite operations
│   │   ├── schema.rs     # DB schema + FTS5
│   │   └── indexer.rs    # Index sessions
│   ├── ui/               # UI components (Relm4)
│   │   ├── sidebar.rs
│   │   └── session_list.rs
│   └── modals/           # Dialogs
│       ├── about.rs
│       └── shortcuts.rs
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

1. **Update Cargo.toml:**
   ```toml
   rusqlite = { version = "0.38.0", features = ["bundled"] }  # FTS5 is built-in
   clap = { version = "4.5", features = ["derive"] }
   ```

2. **Add CLI argument parsing** in `main.rs`:
   ```rust
   use clap::Parser;

   #[derive(Parser)]
   struct Args {
       #[arg(long)]
       sessions_dir: Option<PathBuf>,
   }
   ```

3. **Wire database indexer** in `app.rs`:
   - Initialize DB on app startup
   - Index sessions from directory
   - Show progress bar during indexing

4. **Load sessions in SessionList**:
   - Query DB for sessions
   - Display in list with metadata
   - Format timestamps ("2 hours ago")

5. **Connect Sidebar filters**:
   - Emit filter change messages
   - Update SessionList query
   - Handle "all unchecked" case

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

**Last Updated**: 2026-01-11
**Current Phase**: Phase 1 - Single Tool Support (Claude Code)
**Next Milestone**: Database indexing + session display
