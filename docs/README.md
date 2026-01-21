# Sessions Chronicle - Brainstorming & Design

This directory contains all design documentation, mockups, and architectural decisions for **Sessions Chronicle**, a GNOME app for browsing AI coding sessions.

---

## 📋 Documentation Index

### Core Documents

1. **[PROJECT_STATUS.md](PROJECT_STATUS.md)** ⭐ **START HERE**
   - Current implementation status
   - What's completed, what's next
   - Technical architecture overview
   - Development workflow and best practices

2. **[SESSION_FORMAT_ANALYSIS.md](SESSION_FORMAT_ANALYSIS.md)** 📄
   - Detailed file format specs for Claude Code, Codex, OpenCode
   - Parser implementation guidance
   - Event structure comparisons

3. **[DEVELOPMENT_WORKFLOW.md](DEVELOPMENT_WORKFLOW.md)** 🛠️
   - Running with test data vs production
   - Command-line arguments for development
   - Testing workflow and IDE configuration
   - Why we use CLI args instead of hardcoded checks

### Design Decisions

4. **[UI_DESIGN_COMPARISON.md](UI_DESIGN_COMPARISON.md)**
   - List view vs Cards view analysis
   - Pros/cons of each approach
   - Recommendation: Start with List View

5. **[SEARCH_ARCHITECTURE.md](SEARCH_ARCHITECTURE.md)**
   - How agent-sessions implements search
   - Two-phase progressive search explained
   - Recommendation for Sessions Chronicle: SQLite FTS5

---

## 🎨 Visual Mockups

All mockups are SVG files in the `mockups/` subfolder (open in browser or image viewer):

1. **[mockups/list-view.svg](mockups/list-view.svg)** ⭐ **PRIMARY DESIGN**
   - Compact list of sessions
   - Sidebar with filters
   - Search bar
   - Information-dense layout

2. **[mockups/cards-view.svg](mockups/cards-view.svg)**
   - Alternative: Card-based layout
   - More visual, less dense
   - Could be added as view toggle later

3. **[mockups/session-detail.svg](mockups/session-detail.svg)**
   - Session conversation view
   - Message types (User, Assistant, Tool Call, Tool Result)
   - Resume button in header
   - Scrollable transcript

4. **[mockups/architecture-diagram.svg](mockups/architecture-diagram.svg)** 📐
   - Visual architecture diagram
   - Data flow from session files → UI
   - Shows all layers: Parsers, Indexer, Database, UI, Terminal

---

## 🎯 Current Status

**Phase**: Phase 1 - Core Implementation (Claude Code only)

**Completed**:
- ✅ Project structure with Rust + GTK4 + Relm4
- ✅ Data models (Session, Message, Tool, Role)
- ✅ Database schema (SQLite + FTS5)
- ✅ Claude Code parser (JSONL streaming)
- ✅ Test fixtures
- ✅ Basic UI components (Sidebar, SessionList, SessionDetail)
- ✅ CLI args (`clap`) for `--sessions-dir`
- ✅ Database indexer wired into App
- ✅ SessionList loading from DB
- ✅ Sidebar tool filters wired to SessionList (Claude data only)
- ✅ Search functionality with FTS5 full-text search
- ✅ Session detail view with conversation transcript
- ✅ Session resumption with terminal emulator integration
- ✅ Terminal preferences dialog for emulator selection
- ✅ Session resumption failure notifications with toast feedback

**Next Tasks**:
- ⬜ OpenCode/Codex parsers + indexing (populate filters)
- ⬜ Search term highlighting in SessionDetail

---

## 📁 Session Data Locations

```
~/.claude/projects/                           ← Claude Code (v1)
~/.local/share/opencode/storage/session/      ← OpenCode (v2)
~/.codex/sessions/                            ← Codex (v2)
```

---

## 🎨 Design Principles

1. **Simple & focused** - Don't over-engineer
2. **GNOME HIG** - Follow platform conventions
3. **Performance** - Fast search, responsive UI
4. **Privacy** - All local, no telemetry
5. **Extensible** - Easy to add more AI tools later

---

**Last Updated**: 2026-01-22
**Status**: Phase 1 core features complete (Claude Code support)
