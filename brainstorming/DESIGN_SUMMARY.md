# Sessions Chronicle - Design Summary

## Project Overview

**Sessions Chronicle** is a GNOME application for browsing, searching, and resuming AI coding sessions from multiple CLI tools (Claude Code, OpenCode, Codex).

Inspired by: [agent-sessions](https://github.com/jazzyalex/agent-sessions) (macOS)

---

## Technical Stack (v1)

- **Language**: Rust
- **UI Framework**: GTK4 + Libadwaita (GNOME HIG)
- **Architecture**: Relm4 (Elm-inspired reactive UI)
- **Database**: SQLite with FTS5 (Full-Text Search)
- **Supported Tools**: Claude Code, OpenCode, Codex

---

## Core Features (v1 Scope)

### 1. Session Browsing
- Browse all sessions from supported AI tools
- Filter by tool (Claude Code / OpenCode / Codex)
- Filter by project path
- Sort by date (newest first)

### 2. Full-Text Search
- Search across all message content
- SQLite FTS5 for fast searching
- Highlights matching terms
- Filter search results by tool/project

### 3. Session Detail View
- View complete conversation transcript
- Message types: User, Assistant, Tool Call, Tool Result
- Metadata: timestamp, project path, message count
- Session ID for reference

### 4. Session Resumption
- "Resume Session" button launches terminal
- Opens new terminal window with session resumed
- Uses session-specific CLI commands (e.g., `claude code resume <session-id>`)

---

## UI Design

See mockups:
- `mockup-list-view.svg` - **PRIMARY**: Compact list of sessions
- `mockup-cards-view.svg` - Alternative: Card-based layout
- `mockup-session-detail.svg` - Session conversation view

### Layout Structure

```
┌─────────────────────────────────────────┐
│ Sessions Chronicle         [Resume] ⋮ ⚙ │ ← HeaderBar
├─────────────────────────────────────────┤
│ 🔍 Search sessions...                   │ ← Search entry
├──────────┬──────────────────────────────┤
│          │                              │
│ Sidebar  │  Session List / Detail       │
│          │                              │
│ • Tools  │  (Main content area)         │
│ • Proj   │                              │
│          │                              │
└──────────┴──────────────────────────────┘
```

### Visual Design Principles

**GNOME HIG Compliance**:
- Libadwaita styling (adaptive, modern)
- Standard header bar with split buttons
- System font (Cantarell)
- Standard spacing (6px, 12px, 18px)

**Color Coding**:
- Claude Code: Blue `#3584e4`
- OpenCode: Green `#26a269`
- Codex: Orange `#e66100`

**Accessibility**:
- High contrast text
- Keyboard navigation (arrows, Enter, Ctrl+R to resume)
- Screen reader support (GTK built-in)

---

## Data Architecture

### Session Storage Locations

```
~/.claude/sessions/           ← Claude Code
~/.local/share/opencode/storage/session/  ← OpenCode
~/.codex/sessions/            ← Codex
```

### Database Schema (SQLite)

**sessions** table:
```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,              -- session-20260105-143022
    tool TEXT NOT NULL,               -- claude_code, opencode, codex
    project_path TEXT,                -- ~/projects/my-app
    start_time INTEGER,               -- Unix timestamp
    message_count INTEGER,
    file_path TEXT NOT NULL,          -- full path to session file
    last_updated INTEGER              -- for cache invalidation
);

CREATE INDEX idx_tool ON sessions(tool);
CREATE INDEX idx_project ON sessions(project_path);
CREATE INDEX idx_time ON sessions(start_time DESC);
```

**messages** table (FTS5):
```sql
CREATE VIRTUAL TABLE messages USING fts5(
    session_id,                       -- FK to sessions.id
    message_index,                    -- 0, 1, 2...
    role,                             -- user, assistant, tool_call, tool_result
    content,                          -- actual text content
    timestamp                         -- when message was sent
);
```

### Indexing Strategy

**On First Run**:
1. Scan session directories for all JSON files
2. Parse each session file
3. Extract metadata → `sessions` table
4. Extract message content → `messages` FTS5 table
5. Show progress bar to user

**On Subsequent Runs**:
1. Check for new/modified session files (compare `last_updated`)
2. Incrementally update database
3. Background refresh every 30 seconds (optional)

**Search Process**:
1. User types query in search bar
2. Query FTS5 `messages` table: `SELECT * FROM messages WHERE content MATCH 'query'`
3. Get matching `session_id`s
4. Join with `sessions` table for metadata
5. Display results in list view

---

## Session Resumption

### How It Works

When user clicks "Resume Session":

1. **Get session info** from database
2. **Determine tool** (Claude Code / OpenCode / Codex)
3. **Launch terminal** with appropriate command:

```bash
# Claude Code
gnome-terminal -- bash -c "claude code resume <session-id>; exec bash"

# OpenCode
gnome-terminal -- bash -c "opencode resume <session-id>; exec bash"

# Codex (if supported)
gnome-terminal -- bash -c "codex resume <session-id>; exec bash"
```

4. **Terminal opens**, session resumes
5. User can continue conversation

### Terminal Detection

Prefer in order:
1. `gnome-terminal` (GNOME default)
2. `tilix`
3. `konsole`
4. `xterm` (fallback)

User can override in Settings.

---

## Future Enhancements (Post-v1)

- **View toggle**: Switch between list/cards view
- **Favorites/Stars**: Mark important sessions
- **Analytics**: Usage charts, time-of-day heatmaps
- **Git integration**: Show repo state during session
- **Session export**: Export to Markdown/HTML
- **Session deletion**: Remove old sessions
- **Watch mode**: Auto-refresh when new sessions appear
- **More tools**: GitHub Copilot CLI, Gemini CLI, custom tools

---

## Open Questions

1. **Relm4 vs pure gtk-rs?**
   - Relm4: Better state management, more boilerplate
   - Decision: Start with Relm4, can remove later if too complex

2. **Session file format?**
   - Need to inspect actual session files from each tool
   - Assumption: JSON format, need to validate

3. **Resume command format?**
   - Need to verify each tool supports `resume <session-id>`
   - Fallback: Open in working directory + copy command to clipboard

4. **Real-time updates?**
   - Watch filesystem with `inotify` for new sessions?
   - Poll every N seconds?
   - Decision: Manual refresh for v1, auto-refresh later

---

## Implementation Advice

### Immediate Priorities

1. **Start with a single tool** - Don't try to support all 3 tools at once. Pick Claude Code first (it's the most popular), get it working end-to-end, then add others. The complexity of parsing 3 different formats simultaneously will slow you down.

2. **Build the data layer before more UI** - You have UI structure but no models, database, or parsers yet. Implement in this order:
   - `src/models/` - Session/Message structs
   - `src/database/` - SQLite + FTS5 setup
   - `src/parsers/` - Start with just `claude_code.rs`
   - Then wire into existing UI

3. **Add missing dependencies to Cargo.toml** - Your design docs mention SQLite with FTS5 but it's not in `Cargo.toml` yet. Add:
   ```toml
   rusqlite = { version = "0.32", features = ["bundled", "fts5"] }
   serde = { version = "1.0", features = ["derive"] }
   serde_json = "1.0"
   walkdir = "2.5"
   chrono = "0.4"
   anyhow = "1.0"
   ```

### Architecture Suggestions

4. **Connect Sidebar checkboxes to filters** - Currently they don't emit messages. Add `connect_toggled` handlers to send filter change messages to the parent component.

5. **Add SessionDetail component early** - You have `SessionList` but no detail view. Implement this early to test your parser outputs. It doesn't need to be polished - just display raw data to verify parsing works.

6. **Error handling strategy** - Decide on error propagation. Use `anyhow` for app-level errors and `thiserror` for parser-specific errors.

### Technical Considerations

7. **JSONL streaming for large files** - Don't load entire JSONL files into memory. Use `BufReader::lines()` to process line by line. This is critical for sessions with thousands of messages.

8. **Database location** - Use `glib::user_data_dir()` with your APP_ID, not hardcoded paths. This makes the app portable across different Linux distributions.

9. **File watching vs polling** - Design docs mention both. For v1, just do a "Refresh" button. The `notify` crate adds complexity you don't need yet.

10. **Session resumption fallback** - Don't assume `resume <id>` works for all tools. If terminal command fails, fall back to: copy session ID to clipboard + show notification with manual command.

11. **SQLite thread safety** - If you add background indexing later, use a connection pool (like `r2d2`) or `OnceLock` for the single connection. Don't share a single Connection across threads.

12. **Tool color consistency** - Your design has hex codes - use CSS custom properties in `style.css` instead of hardcoding in Rust. This makes theming easier and more maintainable.

### Testing Strategy

13. **Mock data early** - Create sample session files in `tests/fixtures/` directory for development. Don't rely on having actual Claude Code sessions locally. This ensures you can test without real data. Use command-line arguments (`--sessions-dir`) to specify test fixtures during development, rather than checking for test directories in production code.

14. **Integration test** - Write a test that: parses → stores in DB → retrieves → displays. This validates the full pipeline and catches data transformation bugs early.

15. **Loading states** - Add a spinner/progress bar during initial indexing. First run could take minutes if user has many sessions. Give users feedback that something is happening.

### UX Polish

16. **Empty states** - You have "No Sessions Yet" - good. Also need empty states for:
   - Search with no results
   - All filters unchecked
   - No sessions for selected tool

17. **Keyboard shortcuts** - Add to ShortcutsDialog:
   - `Ctrl+F` - Toggle search
   - `Ctrl+R` - Resume selected session
   - `Escape` - Clear search

18. **Session metadata display** - Show useful info in list view: tool icon/color, project name (extracted from path), date, message count. This helps users quickly find what they need.

### OpenCode Complexity

19. **Defer OpenCode's multi-file structure** - It's the most complex (session metadata + message dirs + parts + diffs). Save for last or consider v2.

20. **Parent-child session display** - OpenCode has subagent sessions with `parentID`. For v1, just show them as flat list. Hierarchical display is a v2 feature.

### Documentation

21. **Write actual README.md** - Currently empty. Include:
   - Installation instructions
   - First run setup
   - Screenshots/mockups
   - Requirements (Claude Code, OpenCode, Codex need to be installed separately)

22. **Document dev setup** - How to build, run, test. Use your existing `relm4_template_README.md` as template but customize for this project.

### Potential Issues

23. **Relm4 macro complexity** - The `view!` macro is great but debugging can be hard. If you hit issues, fall back to traditional GTK4 builder pattern temporarily.

24. **Codex encrypted reasoning** - Never decrypt locally. Persist encrypted content unchanged. If user wants to view reasoning, they need to use the Codex CLI itself.

---

## Next Steps

1. ✅ Review design mockups
2. ✅ Confirm technical approach
3. ✅ Create project skeleton (Cargo, GTK4, Relm4)
4. ⏭️ Add missing dependencies to Cargo.toml
5. ⏭️ Create mock data directory (tests/fixtures/) with sample session files
6. ⏭️ Implement data models (Session, Message, Tool, Role)
7. ⏭️ Create database schema and indexer (SQLite + FTS5)
8. ⏭️ Implement session parser for one tool (Claude Code)
9. ⏭️ Wire indexer into App init phase with progress bar
10. ⏭️ Connect SessionList to display real data from DB
11. ⏭️ Add SessionDetail view with raw data display
12. ⏭️ Connect Sidebar checkboxes to filter logic
13. ⏭️ Implement search with FTS5
14. ⏭️ Polish SessionDetail with proper message display
15. ⏭️ Implement session resumption with fallback
16. ⏭️ Add keyboard shortcuts and polish UI
17. ⏭️ Write integration tests
18. ⏭️ Add OpenCode parser (v2)
19. ⏭️ Add Codex parser (v2)

