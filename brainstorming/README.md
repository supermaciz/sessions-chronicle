# Sessions Chronicle - Brainstorming & Design

This directory contains all design documentation, mockups, and architectural decisions for **Sessions Chronicle**, a GNOME app for browsing AI coding sessions.

---

## 📋 Documentation Index

### Core Design Documents

1. **[DESIGN_SUMMARY.md](DESIGN_SUMMARY.md)** ⭐ **START HERE**
   - Complete project overview
   - Feature scope for v1
   - Technical decisions
   - Next steps

2. **[UI_DESIGN_COMPARISON.md](UI_DESIGN_COMPARISON.md)**
   - List view vs Cards view analysis
   - Pros/cons of each approach
   - Recommendation: Start with List View

3. **[SEARCH_ARCHITECTURE.md](SEARCH_ARCHITECTURE.md)**
   - How agent-sessions implements search
   - Two-phase progressive search explained
   - Recommendation for Sessions Chronicle: SQLite FTS5

4. **[RUST_ARCHITECTURE.md](RUST_ARCHITECTURE.md)** 🔧
   - Complete Rust code structure
   - Dependencies (Relm4, GTK4, rusqlite)
   - Data models, parsers, database layer
   - Code examples for each module

---

## 🎨 Visual Mockups

All mockups are SVG files (open in browser or image viewer):

1. **[mockup-list-view.svg](mockup-list-view.svg)** ⭐ **PRIMARY DESIGN**
   - Compact list of sessions
   - Sidebar with filters
   - Search bar
   - Information-dense layout

2. **[mockup-cards-view.svg](mockup-cards-view.svg)**
   - Alternative: Card-based layout
   - More visual, less dense
   - Could be added as view toggle later

3. **[mockup-session-detail.svg](mockup-session-detail.svg)**
   - Session conversation view
   - Message types (User, Assistant, Tool Call, Tool Result)
   - Resume button in header
   - Scrollable transcript

4. **[architecture-diagram.svg](architecture-diagram.svg)** 📐
   - Visual architecture diagram
   - Data flow from session files → UI
   - Shows all layers: Parsers, Indexer, Database, UI, Terminal

---

## 🎯 Quick Summary

**What**: GNOME app for browsing/searching/resuming AI coding sessions

**Tech Stack**:
- Rust + Relm4 (reactive UI)
- GTK4 + Libadwaita (GNOME HIG)
- SQLite + FTS5 (full-text search)

**Supported Tools** (v1):
- Claude Code
- OpenCode
- Codex

**Core Features** (v1):
- Browse all sessions with filtering
- Full-text search across messages
- Session detail view
- Resume sessions in terminal

**UI Choice**: List view (compact, information-dense)

---

## 📁 Session Data Locations

```
~/.claude/sessions/                           ← Claude Code
~/.local/share/opencode/storage/session/      ← OpenCode
~/.codex/sessions/                            ← Codex
```

All stored as JSON files (format TBD - need to inspect actual files).

---

## ✅ Next Steps

1. **Inspect session file formats**
   - Look at actual JSON from each tool
   - Design parsers accordingly

2. **Create Rust project skeleton**
   - `cargo new sessions-chronicle`
   - Add dependencies to Cargo.toml

3. **Implement first parser** (Claude Code)
   - Parse metadata
   - Parse messages
   - Unit tests

4. **Build database indexer**
   - SQLite schema
   - FTS5 setup
   - Index one tool's sessions

5. **Create basic UI**
   - Relm4 main window
   - Session list view
   - Search bar

6. **Implement search**
   - FTS5 queries
   - Filter by tool/project
   - Display results

7. **Add session detail view**
   - Message rendering
   - Syntax highlighting for code (future)

8. **Implement session resumption**
   - Terminal detection
   - Launch command
   - Test with each tool

---

## 🔍 Open Questions

1. **Session file format**: Need to inspect actual files
2. **Resume command**: Verify each tool supports `resume <session-id>`
3. **Real-time updates**: Manual refresh (v1) or auto-watch (v2)?
4. **Relm4 complexity**: Worth it? (Yes, for this app's state management)

---

## 📝 Notes

- **No code yet** - pure design phase
- Focus on v1 MVP: browse, search, resume
- Future: analytics, git integration, more tools
- GNOME HIG compliance is critical
- Accessibility built-in via GTK4

---

## 🎨 Design Principles

1. **Simple & focused** - Don't over-engineer
2. **GNOME HIG** - Follow platform conventions
3. **Performance** - Fast search, responsive UI
4. **Privacy** - All local, no telemetry
5. **Extensible** - Easy to add more AI tools later

---

**Last Updated**: 2026-01-05
**Status**: Design phase complete, ready for implementation
