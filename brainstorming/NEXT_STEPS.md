# Next Steps - Quick Reference

## Immediate Tasks (Priority Order)

### 1. Fix Dependencies
```toml
# Add to Cargo.toml
rusqlite = { version = "0.38.0", features = ["bundled", "fts5"] }  # Add fts5!
clap = { version = "4.5", features = ["derive"] }
```

### 2. Add CLI Arguments
Update `src/main.rs`:
```rust
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[arg(long, value_name = "DIR")]
    sessions_dir: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();
    // Pass args.sessions_dir to App
}
```

### 3. Wire Database Indexer
Update `src/app.rs`:
- Add `InitializeDatabase` message
- Create indexer on startup
- Index sessions from directory
- Show progress/count to user

### 4. Load Sessions in SessionList
Update `src/ui/session_list.rs`:
- Query database on component init
- Display sessions with metadata
- Format timestamps ("2 hours ago")
- Handle empty state

### 5. Connect Sidebar Filters
Update `src/ui/sidebar.rs`:
- Add `SidebarOutput::FilterChanged` message
- Send to parent when checkboxes toggle
- Update SessionList query based on filters

### 6. Implement Search
- Connect SearchEntry in `app.rs`
- Query FTS5 messages table
- Display matching sessions
- Highlight search terms

### 7. Add SessionDetail Component
- Create `src/ui/session_detail.rs`
- Display conversation transcript
- Color-code by role
- Add scrolling support

### 8. Session Resumption
- Create `src/utils/terminal.rs`
- Detect available terminal emulator
- Build resume command for tool
- Launch terminal with session

---

## Testing Workflow

```bash
# Run with test fixtures
cargo run -- --sessions-dir tests/fixtures/claude_sessions

# Run with real sessions
cargo run
```

---

## Current Blockers

1. **FTS5 missing** - Add `fts5` feature to rusqlite
2. **No CLI args** - Can't override sessions directory for testing
3. **Indexer not wired** - Database stays empty
4. **SessionList static** - Not loading from DB

---

**Fix these 4 blockers first, then the app will start working!**
