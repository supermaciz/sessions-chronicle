# Sessions Chronicle - Project Status

**A GNOME app for browsing, searching, and resuming AI coding sessions**

---

### Shipped Features

- Browse and filter sessions across 4 tools (Claude Code, OpenCode, Codex, Mistral Vibe)
- Full-text search (SQLite FTS5) + in-detail search term highlighting
- Rich markdown rendering for assistant messages
- Resume sessions in a configurable terminal emulator + failure toasts
- Consistent `--sessions-dir` override across tools + isolated override index DB + reset/reindex in Preferences (PR #24)
- Utility pane with filters/session-context modes and in-pane resume action (PR #27)
- Session row redesign: first prompt title, project-aware subtitle, relative timestamps, row context menu resume (PR #30)
- Inline expand/collapse for truncated messages with on-demand full content loading (PR #35)
- Tool calls & subagents inspector: inline expander rows, ToolInspector utility pane, subagent drill-down (PR #36)
- OpenCode SQLite dual-read indexing: SQLite-first with JSON fallback, supports post-migration installs (PR #37)
- Keyboard navigation polish + LLM model tracking and display per assistant message (PR #38, #39, #41)


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
│   │   ├── message_preview.rs # MessagePreview for UI
│   │   ├── tool_call.rs       # ToolCall model
│   │   ├── subagent.rs        # Subagent model
│   │   └── transcript_item.rs # TranscriptItem (ordered view)
│   ├── parsers/          # Session file parsers
│   │   ├── claude_code.rs   # Claude Code JSONL parser
│   │   ├── codex.rs         # Codex JSONL parser
│   │   ├── mistral_vibe.rs  # Mistral Vibe parser
│   │   └── opencode/         # OpenCode parser module (JSON + SQLite backends)
│   ├── database/         # SQLite operations
│   │   ├── schema.rs     # DB schema + FTS5
│   │   ├── indexer.rs    # Index sessions
│   │   └── mod.rs        # load_session, search_sessions
│   ├── ui/               # UI components (Relm4)
│   │   ├── tool_inspector_pane.rs # ToolInspector utility pane (tool calls + subagents)
│   │   ├── transcript_row.rs      # Transcript row component (messages, tool calls, subagents)
│   │   ├── highlight.rs  # Search term highlighting helpers
│   │   ├── markdown.rs   # Markdown parser and GTK renderer
│   │   ├── format.rs     # Shared formatting helpers
│   │   ├── session_list.rs  # Session list view
│   │   ├── session_detail.rs # Session detail/transcript view
│   │   ├── session_row.rs # Session list row component
│   │   ├── sidebar.rs    # Tool/project filters
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
│   ├── io.github.supermaciz.sessionschronicle.Devel.json   # Dev Flatpak
│   └── io.github.supermaciz.sessionschronicle.json         # Stable Flatpak
└── docs/                 # Design docs
```

### Database Schema

**sessions** table (v1):
```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    tool TEXT NOT NULL,
    project_path TEXT,
    start_time INTEGER NOT NULL,
    message_count INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    last_updated INTEGER NOT NULL,
    first_prompt TEXT,
    parent_session_id TEXT,           -- added in v1
    is_subagent INTEGER NOT NULL DEFAULT 0  -- added in v1
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

**transcript_items** table (v1):
```sql
CREATE TABLE transcript_items (
    session_id TEXT NOT NULL,
    item_index INTEGER NOT NULL,
    kind TEXT NOT NULL,
    message_index INTEGER,
    tool_call_id TEXT,
    subagent_id TEXT,
    PRIMARY KEY (session_id, item_index)
);
```

**tool_calls** table (v1):
```sql
CREATE TABLE tool_calls (
    id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    subagent_id TEXT,
    tool_name TEXT NOT NULL,
    status TEXT NOT NULL,
    title TEXT,
    summary TEXT,
    input_json TEXT,
    output_text TEXT,
    error_text TEXT,
    started_at INTEGER,
    ended_at INTEGER,
    duration_ms INTEGER,
    parser_call_id TEXT,
    PRIMARY KEY (session_id, id)
);
```

**subagents** table (v1):
```sql
CREATE TABLE subagents (
    id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    title TEXT NOT NULL,
    prompt TEXT,
    result_summary TEXT,
    child_session_id TEXT,
    parser_ref TEXT,
    PRIMARY KEY (session_id, id)
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

### CI/CD

GitHub Actions workflows handle:
- Automated `cargo test`, `cargo clippy`, and `cargo fmt --check` on every push/PR
- Flatpak build verification (dev and stable manifests)

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

**Last Updated**: 2026-02-28
**Current Phase**: Phase 8 — Complete
