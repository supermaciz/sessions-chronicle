# Sessions Chronicle - Documentation Index

This directory contains project documentation, architecture notes, and implementation plans for **Sessions Chronicle**, a GNOME app for browsing AI coding assistant sessions.

---

## 📋 Documentation Index

### Core Documents

1. **[PROJECT_STATUS.md](PROJECT_STATUS.md)** ⭐ **START HERE**
   - Current implementation status
   - What's completed, what's next
   - Technical architecture overview
   - Development workflow and best practices

2. **[SESSION_FORMAT_ANALYSIS.md](SESSION_FORMAT_ANALYSIS.md)** 📄
   - Cross-assistant comparison tables (storage, file format, event structure, model metadata)
   - Implementation status, open questions, next steps
   - Per-assistant format details: [session-formats/](session-formats/)
     - [claude-code.md](session-formats/claude-code.md), [codex.md](session-formats/codex.md), [opencode.md](session-formats/opencode.md), [mistral-vibe.md](session-formats/mistral-vibe.md), [kimi-code.md](session-formats/kimi-code.md)

3. **[PARSER_DESIGN.md](PARSER_DESIGN.md)** 🦀
   - Trait-based parser architecture and factory pattern
   - Title extraction, timestamp parsing, content extraction per assistant
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
~/.codex/sessions/                            ← Codex active rollouts
~/.codex/archived_sessions/                   ← Codex archived rollouts (not yet indexed; can be .jsonl.zst)
~/.vibe/logs/session/                         ← Mistral Vibe (v2)
$KIMI_CODE_HOME/sessions/                     ← Kimi Code (defaults to ~/.kimi-code/sessions/)
```

Custom Kimi Code homes are supported when visible in the Flatpak sandbox.
Legacy Kimi sessions under `~/.kimi` are not parsed.

---

## 🎨 Design Principles

1. **Simple & focused** - Don't over-engineer
2. **GNOME HIG** - Follow platform conventions
3. **Performance** - Fast search, responsive UI
4. **Privacy** - All local, no telemetry
5. **Extensible** - Easy to add more AI assistants later

---

**Last Updated**: 2026-06-21

**Current Status**: `v0.7.1`. Analytics workspace, project filtering, token usage display, incremental indexing, and indexing diagnostics are implemented. Recent work: typed-ListView transcript with `GtkLabel` prose rendering, date filter pill, header-bar session summary popover, Mistral Vibe subagents, and an external-content FTS5 search index (schema `user_version = 14`).  
**Next Milestone**: Ongoing UX/documentation polish and follow-up refinements tracked in newer files under `docs/explorations/`.
