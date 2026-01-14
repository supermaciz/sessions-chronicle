# Search Architecture Analysis (from agent-sessions)

## How agent-sessions Implements Search

### Two-Phase Progressive Search (Not Traditional Indexing)

Instead of building a full-text search index upfront, agent-sessions uses **lazy parsing**:

1. **Lightweight Sessions** (~50KB metadata)
   - Fast to load
   - Contains: file path, timestamps, estimated message counts
   - No conversation content

2. **Fully Parsed Sessions** (variable size)
   - Parsed on-demand during search
   - Contains actual conversation transcript
   - **Transcript cache** stores rendered text for faster subsequent searches

### Search Process

**Phase 1**: Batch-process small/medium sessions (<10MB)
**Phase 2**: Parse large sessions (≥10MB) sequentially

### What Gets Searched

The FilterEngine checks in priority order:

1. **Transcript cache** (preferred) - Rendered conversation text (what user sees)
2. **Raw event fields** (fallback) - `event.text`, `event.toolInput`, `event.toolOutput`
3. **Metadata** - Model type, date range, session kind (user/assistant/tool)

### Filters Available

- Text query (full-text search in transcripts)
- Date range
- AI tool (Codex/Claude/etc.)
- Model type
- Session kind (user/assistant/tool)
- Project path

---

## For Sessions Chronicle (Rust + SQLite)

We have options:

### Option A: Follow agent-sessions approach
- Lazy parsing with transcript caching
- Progressive search phases
- Good for very large sessions
- More complex implementation

### Option B: SQLite FTS5 (Full-Text Search)
- Pre-index all session content
- Use SQLite's built-in FTS5 for searching
- Simpler Rust implementation (rusqlite + FTS5)
- Excellent performance for most use cases

### Option C: Hybrid
- SQLite for metadata + basic search
- Lazy transcript parsing for large sessions
- Cache rendered transcripts in SQLite

**Recommendation for v1**: Start with **Option B (SQLite FTS5)**
- Simpler to implement
- Good performance
- Rust has excellent SQLite support (rusqlite)
- Can optimize later if needed

---

**Status**: Search is not implemented yet; plan remains SQLite FTS5.
**Last Updated**: 2026-01-14
