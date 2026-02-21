# Sessions Chronicle - Documentation Index

This directory contains project documentation, architecture notes, and implementation plans for **Sessions Chronicle**, a GNOME app for browsing AI coding sessions.

---

## 📋 Documentation Index

### Core Documents

1. **[PROJECT_STATUS.md](PROJECT_STATUS.md)** ⭐ **START HERE**
   - Current implementation status
   - What's completed, what's next
   - Technical architecture overview
   - Development workflow and best practices

2. **[SESSION_FORMAT_ANALYSIS.md](SESSION_FORMAT_ANALYSIS.md)** 📄
   - Cross-tool comparison tables (storage, file format, event structure, model metadata)
   - Implementation status, open questions, next steps
   - Per-tool format details: [session-formats/](session-formats/)
     - [claude-code.md](session-formats/claude-code.md), [codex.md](session-formats/codex.md), [opencode.md](session-formats/opencode.md), [mistral-vibe.md](session-formats/mistral-vibe.md)

3. **[PARSER_DESIGN.md](PARSER_DESIGN.md)** 🦀
   - Trait-based parser architecture and factory pattern
   - Title extraction, timestamp parsing, content extraction per tool
   - JSONL streaming, tool call handling, error handling recommendations
   - OpenCode multi-file (legacy) and SQLite read paths

4. **[DEVELOPMENT_WORKFLOW.md](DEVELOPMENT_WORKFLOW.md)** 🛠️
   - Running with test data vs production
   - Command-line arguments for development
   - Testing workflow and IDE configuration
   - Why we use CLI args instead of hardcoded checks

### Design Decisions

5. **[UI_DESIGN_COMPARISON.md](UI_DESIGN_COMPARISON.md)**
   - List view vs Cards view analysis
   - Pros/cons of each approach
   - Recommendation: Start with List View

6. **[SEARCH_ARCHITECTURE.md](SEARCH_ARCHITECTURE.md)**
   - How agent-sessions implements search
   - Two-phase progressive search explained
   - Recommendation for Sessions Chronicle: SQLite FTS5

### Plans

7. **[plans/2026-01-26-opencode-parser-design.md](plans/2026-01-26-opencode-parser-design.md)** ✅
   - OpenCode session parser implementation (completed)
   - Multi-file structure handling

8. **[plans/2026-02-03-codex-parser-design.md](plans/2026-02-03-codex-parser-design.md)** ✅
   - Codex CLI session parser implementation (completed)
   - JSONL event streaming and message extraction

9. **[plans/2026-02-04-mistral-vibe-v2-design.md](plans/2026-02-04-mistral-vibe-v2-design.md)** ✅
   - Mistral Vibe v2 parser implementation (completed)
   - Directory-based sessions with meta.json + messages.jsonl

10. **[plans/2026-01-30-tool-calls-and-subagents-design.md](plans/2026-01-30-tool-calls-and-subagents-design.md)**
    - Tool calls display with inline badges and detail panel
    - Subagent tree view and navigation

11. **[plans/2026-01-30-markdown-rendering-design.md](plans/2026-01-30-markdown-rendering-design.md)** ✅
    - Markdown rendering for assistant messages (pulldown-cmark + Pango markup)
    - Native GTK4 widgets per block type

12. **[plans/2026-02-07-search-highlighting-exploration.md](plans/2026-02-07-search-highlighting-exploration.md)** ✅
    - UX exploration for search highlighting behavior in SessionDetail
    - Tradeoffs between inline and filtered-match approaches

13. **[plans/2026-02-07-search-highlighting-design.md](plans/2026-02-07-search-highlighting-design.md)** ✅
    - Chosen implementation direction for search highlighting
    - Detailed UI and integration notes for implemented feature

14. **[plans/2026-02-07-sessions-dir-unified-behavior-design.md](plans/2026-02-07-sessions-dir-unified-behavior-design.md)** ✅
    - Unified sessions directory behavior
    - Isolated database and fixture subdirectory mapping
    - Preferences reset action for index management

15. **[plans/2026-02-08-session-detail-utility-pane-design.md](plans/2026-02-08-session-detail-utility-pane-design.md)** ✅
    - Utility pane behavior and session detail integration
    - Filters/session-context pane mode switching

16. **[plans/2026-02-11-session-row-prompt-preview-design.md](plans/2026-02-11-session-row-prompt-preview-design.md)** ✅
    - Session row prompt preview and subtitle behavior
    - Markup-safe title/subtitle rendering guidance

17. **[plans/2026-02-13-keyboard-shortcuts-hig-conformity-design.md](plans/2026-02-13-keyboard-shortcuts-hig-conformity-design.md)** ✅
    - Keyboard shortcuts aligned with GNOME HIG
    - Type-to-search integration

18. **[plans/2026-02-14-release-flatpak-workflow-design.md](plans/2026-02-14-release-flatpak-workflow-design.md)** ✅
    - Stable Flatpak manifest and release workflow
    - GitHub Actions CI/CD for release builds

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

## 📁 Session Data Locations

```
~/.claude/projects/                           ← Claude Code (v1)
~/.local/share/opencode/opencode.db           ← OpenCode ≥ 2026-02-14 (SQLite)
~/.local/share/opencode/storage/session/      ← OpenCode legacy (JSON, pre-migration)
~/.codex/sessions/                            ← Codex (v2)
~/.vibe/logs/session/                         ← Mistral Vibe (v2)
```

---

## 🎨 Design Principles

1. **Simple & focused** - Don't over-engineer
2. **GNOME HIG** - Follow platform conventions
3. **Performance** - Fast search, responsive UI
4. **Privacy** - All local, no telemetry
5. **Extensible** - Easy to add more AI tools later

---

**Last Updated**: 2026-02-21

**Current Status**: Phase 5 Complete — Phase 6 (Tool Calls & Subagents) is next
**Next Milestone**: Phase 6 — Enrich Message model, parse tool events, inline tool badges
