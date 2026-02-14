# Sessions Chronicle - Project Status

**A GNOME app for browsing, searching, and resuming AI coding sessions**

---

## Current Status: Phase 5 In Progress - Consolidating Foundations

### ✅ Completed

**Core Architecture**
- ✅ Project structure with Rust + GTK4 + Relm4
- ✅ Data models (`Session`, `Message`, `Tool`)
- ✅ Database layer with SQLite + FTS5
- ✅ Claude Code parser (JSONL format, streaming)
- ✅ OpenCode parser (multi-file format with message parts)
- ✅ Codex parser (JSONL format, streaming)
- ✅ Test fixtures in `tests/fixtures/claude_sessions/`, `tests/fixtures/opencode_storage/`, `tests/fixtures/codex_sessions/`, `tests/fixtures/vibe_sessions/`
- ✅ Basic UI structure (Sidebar, SessionList, SessionDetail)

**Key Shipped Features (high-level)**
- ✅ Browse and filter sessions across 4 tools (Claude Code, OpenCode, Codex, Mistral Vibe)
- ✅ Full-text search (SQLite FTS5) + in-detail search term highlighting
- ✅ Rich markdown rendering for assistant messages
- ✅ Resume sessions in a configurable terminal emulator + failure toasts
- ✅ Consistent `--sessions-dir` override across tools + isolated override index DB + reset/reindex in Preferences (PR #24)
- ✅ Utility pane with filters/session-context modes and in-pane resume action (PR #27)
- ✅ Session row redesign: first prompt title, project-aware subtitle, relative timestamps, row context menu resume (PR #30)

**Dependencies**
- ✅ Relm4 (reactive UI framework)
- ✅ Libadwaita (GNOME styling)
- ✅ rusqlite (SQLite database)
- ✅ serde/serde_json (JSON parsing)
- ✅ chrono (date/time handling)
- ✅ anyhow/thiserror (error handling)
- ✅ clap (CLI args)
- ✅ pulldown-cmark (markdown parsing)
- ✅ In-tree `pango_escape` helper (Pango markup escaping)



### 📋 Roadmap

**Phase 1: Single Tool (Claude Code)** - Complete
1. ✅ Add missing dependencies
2. ✅ Implement CLI args with `--sessions-dir`
3. ✅ Wire database indexer into App
4. ✅ Load sessions in SessionList from DB
5. ✅ Connect sidebar tool filters to SessionList
6. ✅ Implement search with FTS5 queries
7. ✅ Add SessionDetail component
8. ✅ Add session resumption (terminal launch)

**Phase 2: Multi-Tool Support** - Complete
- ✅ OpenCode parser (multi-file format)
- ✅ Codex parser (JSONL streaming, encrypted reasoning support)
- ✅ Filter sessions with no user messages
- ✅ Message preview model
- ✅ Mistral Vibe parser (directory-based logs with `meta.json` + `messages.jsonl`)
- ✅ Tool filters in UI (sidebar checkboxes)

**Phase 3: Markdown Rendering** - Complete ([design](plans/2026-01-30-markdown-rendering-design.md))
- ✅ Markdown rendering for assistant messages (pulldown-cmark + Pango markup)
- ✅ Support for headings, code blocks, lists, task lists, blockquotes, tables, horizontal rules
- ✅ Inline formatting (bold, italic, strikethrough, inline code, links)
- ✅ Comprehensive test suite (19 unit tests)

**Phase 4: Search Highlighting** - Complete ([design](plans/2026-02-07-search-highlighting-design.md))
- ✅ Search term highlighting in SessionDetail view
- ✅ Highlight matching terms when viewing search results
- ✅ Visual distinction for search matches

**Phase 5: Consolidating Foundations** - In Progress

- ✅ Unified `--sessions-dir` behavior across all tools (isolated DB + fixture mapping + single resolver) ([design](plans/2026-02-07-sessions-dir-unified-behavior-design.md), PR #24)
- ✅ UI refinement
  * ✅ Utility pane + session detail ([design](plans/2026-02-08-session-detail-utility-pane-design.md)) PR #27
  * ✅ Improve session row with first-prompt preview + safer markup handling ([design](plans/2026-02-11-session-row-prompt-preview-design.md)) PR #30
  * ✅ Improve keyboard shortcuts ([design](plans/2026-02-13-keyboard-shortcuts-hig-conformity-design.md))
  * ✅ Fix "About" modal
- ✅ Basic CI/CD setup with GitHub Actions (automated testing, formatting checks, linting, Flatpak builds)
- ⬜ Releases [design](plans/2026-02-14-release-flatpak-workflow-design.md)

**Phase 6: Tool Calls & Subagents** - Future ([design](plans/2026-01-30-tool-calls-and-subagents-design.md))
- ⬜ Enrich Message model (tool_name, tool_input, parent_message_index)
- ⬜ Enrich Session model (parent_session_id)
- ⬜ Parse tool_use/tool_result in Claude Code & OpenCode
- ⬜ Inline tool badges in transcript
- ⬜ Tool detail panel (lateral, input/output display)
- ⬜ Subagent tree view & navigation

**Next Features?** - Future
- Syntax highlighting for code blocks (syntect)
- Real-time session monitoring (file watching)
- Session export (Markdown/HTML)
- Analytics and usage charts
- Git integration
- Git-ai integration
- Display reasoning/thinking blocks
- Semantic search
- Session summaries (grouped by project or other criteria)

---

## Technical Architecture

### Tech Stack

- **Language**: Rust 2024
- **UI**: GTK4 + Libadwaita (GNOME HIG compliant)
- **Reactive UI**: Relm4 (Elm-inspired architecture)
- **Database**: SQLite with FTS5 (full-text search)
- **Supported Tools**: Claude Code, OpenCode, Codex, Mistral Vibe
- **License**: `MIT`

### Project Structure

```
sessions-chronicle/
├── src/
│   ├── main.rs           # Entry point, Relm4 app setup
│   ├── lib.rs            # Library exports
│   ├── config.rs         # App constants (APP_ID, VERSION)
│   ├── app.rs            # Main App component (search, window, navigation)
│   ├── session_sources.rs # Unified session source resolver
│   ├── models/           # Data models
│   │   ├── session.rs         # Session, Tool
│   │   ├── message.rs         # Message, Role
│   │   └── message_preview.rs # MessagePreview for UI
│   ├── parsers/          # Session file parsers
│   │   ├── claude_code.rs   # Claude Code JSONL parser
│   │   ├── codex.rs         # Codex JSONL parser
│   │   ├── mistral_vibe.rs  # Mistral Vibe parser
│   │   └── opencode.rs      # OpenCode multi-file parser
│   ├── database/         # SQLite operations
│   │   ├── schema.rs     # DB schema + FTS5
│   │   ├── indexer.rs    # Index sessions
│   │   └── mod.rs        # load_session, search_sessions
│   ├── ui/               # UI components (Relm4)
│   │   ├── detail_context_pane.rs # Session context utility pane
│   │   ├── highlight.rs  # Search term highlighting helpers
│   │   ├── markdown.rs   # Markdown parser and GTK renderer
│   │   ├── message_row.rs # Message row component
│   │   ├── sidebar.rs    # Tool/project filters
│   │   ├── session_list.rs  # Session list view
│   │   ├── session_detail.rs # Session detail/transcript view
│   │   ├── session_row.rs # Session list row component
│   │   ├── modals/
│   │   │   ├── about.rs      # About dialog
│   │   │   ├── preferences.rs # Preferences dialog (terminal settings, index reset)
│   │   │   └── shortcuts.rs  # Keyboard shortcuts
│   │   └── mod.rs
│   └── utils/            # Utilities
│       ├── terminal.rs   # Terminal emulator detection and spawning
│       └── mod.rs
├── data/                 # Desktop integration
│   ├── icons/            # App icons
│   ├── resources/        # UI resources (CSS, .ui files)
│   └── *.xml.in          # GSettings schema, desktop entry, metainfo
├── tests/fixtures/       # Test data
│   ├── claude_sessions/  # Sample Claude Code sessions
│   ├── codex_sessions/   # Sample Codex sessions
│   ├── opencode_storage/ # Sample OpenCode sessions
│   └── vibe_sessions/    # Sample Mistral Vibe sessions
├── build-aux/            # Build manifests
│   └── io.github.supermaciz.sessionschronicle.Devel.json
└── docs/                 # Design docs
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

**Mistral Vibe**: `~/.vibe/logs/session/` (v2)
- Format: Directory per session with `meta.json` + JSONL `messages.jsonl`
- Special: No per-message timestamps; session-level metadata with tool stats

---

## Development Workflow

### Building and Running

```bash
# Build
flatpak-builder --user flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json --force-clean

# Run
flatpak-builder --run flatpak_app build-aux/io.github.supermaciz.sessionschronicle.Devel.json sessions-chronicle
```

See `DEVELOPMENT_WORKFLOW.md` for test fixtures and development workflow.

### Key Design Decisions

1. **CLI args for test data** - No hardcoded checks for test directories
2. **Streaming JSONL parsing** - Use `BufReader::lines()`, never load entire file
3. **SQLite FTS5 for search** - Simple, fast, built-in full-text search
4. **Unified source resolution** - One override mechanism for all tools (`session_sources.rs`)
5. **Utility-pane navigation model** - Filters in list mode, session context in detail mode

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

## Known Limitations

### Markdown Rendering

**Nested blockquotes are not fully supported** (`src/ui/markdown.rs`)
- When a blockquote contains another blockquote (`> outer\n>\n> > inner`), only the innermost quote content is preserved
- This is due to the single-level `in_blockquote` flag and `blockquote_blocks` buffer being cleared on each new quote start
- **Impact**: Low — Claude sessions rarely contain nested blockquotes
- **Status**: Documented limitation, not prioritized for fixing
- **Reference**: [PR #12 review comment](https://github.com/supermaciz/sessions-chronicle/pull/12#discussion_r2774898364)

**Markdown parsing performance** (`src/ui/message_row.rs:73`)
- Markdown parsing happens on every `MessageRow` widget initialization
- Each assistant message is parsed from scratch when the row is created
- **Impact**: Low for typical session sizes (<100 messages), but could become noticeable for very large sessions
- **Status**: Monitor performance; consider caching parsed `MarkdownBlock` structures if needed
- **Mitigation strategy**: Could cache parsed blocks in `MessagePreview` or lazily render on scroll

**Links are not clickable** (`src/ui/markdown.rs:1182-1186`)
- Links render as text followed by the URL in parentheses: `[text](url)` → "text (url)"
- URLs are shown but not clickable due to GTK Label limitations
- **Impact**: Low — users can copy/paste URLs, most Claude sessions don't have many links
- **Status**: Acceptable for v1
- **Future enhancement**: Could use `gtk::LinkButton` or handle click events to make links interactive

---

## Implementation Notes

### Immediate Tasks

1. **Consolidating foundations** (partially complete):
    - ✅ `src/session_sources.rs` module with fixture subdirectory mapping
    - ✅ Isolated override database (`sessions-override.db`)
    - ✅ Preferences reset action (controller-with-outputs pattern)
    - ✅ Unified source resolver wired into App startup
    - ⬜ UI polish (utility pane + session detail)

2. **Tool calls and subagents support**:
    - Enrich Message model with tool call data
    - Parse tool_use/tool_result events from session files
    - Display tool badges and detail panels

### Testing Strategy

**Unit tests**:
```bash
cargo test
```

**Integration testing**: Use the `--sessions-dir` flag to test with fixtures (see `DEVELOPMENT_WORKFLOW.md`)

### Error Handling

- Use `anyhow` for app-level errors with context
- Use `thiserror` for parser-specific errors
- Log warnings for malformed files, continue indexing
- Never panic on user data

---

## References

### Design Documents

- **SESSION_FORMAT_ANALYSIS.md** - Detailed format specs for all 4 tools
- **SEARCH_ARCHITECTURE.md** - Why we chose SQLite FTS5
- **UI_DESIGN_COMPARISON.md** - List view vs cards view analysis
- **DEVELOPMENT_WORKFLOW.md** - CLI args and testing workflow

### External Resources

- [Claude Code Session Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/claude-code-session-format.md)
- [Codex Storage Format](https://github.com/jazzyalex/agent-sessions/blob/main/docs/session-storage-format.md)
- [Agent Sessions (inspiration)](https://github.com/jazzyalex/agent-sessions)

---

**Last Updated**: 2026-02-13
**Current Phase**: Phase 5 - Consolidating Foundations (In Progress)
**Next Milestone**: Phase 5 completion - keyboard shortcuts polish, About dialog follow-up, and release readiness
