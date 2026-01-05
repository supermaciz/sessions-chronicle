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

## Next Steps

1. ✅ Review design mockups
2. ✅ Confirm technical approach
3. ⏭️ Inspect actual session file formats (Claude Code, OpenCode, Codex)
4. ⏭️ Create project skeleton (Cargo, GTK4, Relm4)
5. ⏭️ Implement session parser for one tool (Claude Code)
6. ⏭️ Build database indexer
7. ⏭️ Build basic UI (list view)
8. ⏭️ Implement search
9. ⏭️ Implement session detail view
10. ⏭️ Implement session resumption

