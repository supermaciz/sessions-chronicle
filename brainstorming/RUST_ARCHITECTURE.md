# Rust Architecture for Sessions Chronicle

## Project Structure

```
sessions-chronicle/
├── Cargo.toml
├── src/
│   ├── main.rs                 # Entry point, Relm4 app setup
│   ├── app.rs                  # Main App component (Relm4)
│   ├── models/                 # Data models
│   │   ├── mod.rs
│   │   ├── session.rs          # Session struct
│   │   └── message.rs          # Message struct
│   ├── parsers/                # Session file parsers
│   │   ├── mod.rs
│   │   ├── claude_code.rs      # Claude Code session parser
│   │   ├── opencode.rs         # OpenCode session parser
│   │   └── codex.rs            # Codex session parser
│   ├── database/               # SQLite operations
│   │   ├── mod.rs
│   │   ├── schema.rs           # DB schema + migrations
│   │   ├── indexer.rs          # Index sessions into DB
│   │   └── search.rs           # Search queries
│   ├── ui/                     # UI components (Relm4)
│   │   ├── mod.rs
│   │   ├── window.rs           # Main window
│   │   ├── sidebar.rs          # Sidebar filters
│   │   ├── session_list.rs     # Session list view
│   │   ├── session_detail.rs   # Session detail view
│   │   └── search_bar.rs       # Search entry
│   └── utils/                  # Utilities
│       ├── mod.rs
│       ├── terminal.rs         # Terminal launching
│       └── config.rs           # App configuration
├── resources/                  # GTK resources
│   ├── style.css
│   └── icons/
└── tests/
    └── parser_tests.rs
```

---

## Key Dependencies (Cargo.toml)

```toml
[package]
name = "sessions-chronicle"
version = "0.1.0"
edition = "2021"

[dependencies]
# UI Framework
relm4 = { version = "0.10.0", features = ["libadwaita", "gnome_48"] }
adw = { version = "0.8.1", package = "libadwaita", features = ["v1_8"] }

# Database
rusqlite = { version = "0.32", features = ["bundled", "fts5"] }

# JSON parsing
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# File system
walkdir = "2.5"

# Date/Time
chrono = "0.4"

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Logging (existing in project)
tracing = "0.1.44"
tracing-subscriber = "0.3.22"
```

---

## Core Data Models

### Session Model (`src/models/session.rs`)

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,                    // session-20260105-143022
    pub tool: Tool,
    pub project_path: Option<String>,
    pub start_time: DateTime<Utc>,
    pub message_count: usize,
    pub file_path: String,             // Absolute path to session file
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    OpenCode,
    Codex,
}

impl Tool {
    pub fn color(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "#3584e4",  // Blue
            Tool::OpenCode => "#26a269",    // Green
            Tool::Codex => "#e66100",       // Orange
        }
    }

    pub fn session_dir(&self) -> String {
        let home = std::env::var("HOME").unwrap();
        match self {
            Tool::ClaudeCode => format!("{}/.claude/sessions", home),
            Tool::OpenCode => format!("{}/.local/share/opencode/storage/session", home),
            Tool::Codex => format!("{}/.codex/sessions", home),
        }
    }
}
```

### Message Model (`src/models/message.rs`)

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub session_id: String,
    pub index: usize,
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    ToolCall,
    ToolResult,
}

impl Role {
    pub fn color(&self) -> &'static str {
        match self {
            Role::User => "#3584e4",       // Blue
            Role::Assistant => "#26a269",  // Green
            Role::ToolCall => "#e66100",   // Orange
            Role::ToolResult => "#1c71d8", // Darker blue
        }
    }
}
```

---

## Parser Architecture

### Parser Trait (`src/parsers/mod.rs`)

```rust
use anyhow::Result;
use crate::models::{Session, Message};

pub trait SessionParser {
    /// Parse session metadata from file
    fn parse_metadata(&self, file_path: &str) -> Result<Session>;

    /// Parse all messages from session file
    fn parse_messages(&self, file_path: &str) -> Result<Vec<Message>>;
}
```

### Claude Code Parser Example (`src/parsers/claude_code.rs`)

```rust
use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use crate::models::{Session, Message, Tool, Role};
use crate::parsers::SessionParser;

pub struct ClaudeCodeParser;

impl SessionParser for ClaudeCodeParser {
    fn parse_metadata(&self, file_path: &str) -> Result<Session> {
        let content = fs::read_to_string(file_path)
            .context("Failed to read session file")?;

        let json: Value = serde_json::from_str(&content)
            .context("Failed to parse JSON")?;

        // Extract metadata from Claude Code session format
        // (Need to inspect actual format)
        let id = extract_session_id(file_path)?;
        let start_time = extract_timestamp(&json)?;
        let message_count = count_messages(&json)?;
        let project_path = extract_project_path(&json);

        Ok(Session {
            id,
            tool: Tool::ClaudeCode,
            project_path,
            start_time,
            message_count,
            file_path: file_path.to_string(),
            last_updated: chrono::Utc::now(),
        })
    }

    fn parse_messages(&self, file_path: &str) -> Result<Vec<Message>> {
        // Parse full conversation
        // Return Vec<Message>
        todo!("Implement based on actual session format")
    }
}

// Helper functions
fn extract_session_id(file_path: &str) -> Result<String> {
    // Extract from filename or JSON
    todo!()
}

fn extract_timestamp(json: &Value) -> Result<chrono::DateTime<chrono::Utc>> {
    // Parse timestamp field
    todo!()
}

fn count_messages(json: &Value) -> Result<usize> {
    // Count conversation events
    todo!()
}

fn extract_project_path(json: &Value) -> Option<String> {
    // Try to extract working directory
    None
}
```

---

## Database Layer

### Schema (`src/database/schema.rs`)

```rust
use rusqlite::{Connection, Result};

pub fn initialize_database(conn: &Connection) -> Result<()> {
    // Create sessions table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            tool TEXT NOT NULL,
            project_path TEXT,
            start_time INTEGER NOT NULL,
            message_count INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            last_updated INTEGER NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tool ON sessions(tool)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_time ON sessions(start_time DESC)",
        [],
    )?;

    // Create FTS5 messages table
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS messages USING fts5(
            session_id UNINDEXED,
            message_index UNINDEXED,
            role UNINDEXED,
            content,
            timestamp UNINDEXED
        )",
        [],
    )?;

    Ok(())
}
```

### Indexer (`src/database/indexer.rs`)

```rust
use rusqlite::Connection;
use anyhow::Result;
use walkdir::WalkDir;
use crate::models::Tool;
use crate::parsers::{SessionParser, ClaudeCodeParser, OpenCodeParser, CodexParser};

pub struct SessionIndexer {
    db: Connection,
}

impl SessionIndexer {
    pub fn new(db_path: &str) -> Result<Self> {
        let db = Connection::open(db_path)?;
        crate::database::schema::initialize_database(&db)?;
        Ok(Self { db })
    }

    pub fn index_all_sessions(&mut self) -> Result<usize> {
        let mut count = 0;

        for tool in [Tool::ClaudeCode, Tool::OpenCode, Tool::Codex] {
            count += self.index_tool_sessions(tool)?;
        }

        Ok(count)
    }

    fn index_tool_sessions(&mut self, tool: Tool) -> Result<usize> {
        let session_dir = tool.session_dir();
        let parser = get_parser_for_tool(tool);

        let mut count = 0;
        for entry in WalkDir::new(&session_dir).max_depth(2) {
            let entry = entry?;
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "json" {  // Assuming JSON format
                        self.index_session_file(entry.path().to_str().unwrap(), &*parser)?;
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    fn index_session_file(&mut self, file_path: &str, parser: &dyn SessionParser) -> Result<()> {
        let session = parser.parse_metadata(file_path)?;
        let messages = parser.parse_messages(file_path)?;

        // Insert into sessions table
        self.db.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, project_path, start_time, message_count, file_path, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                &session.id,
                format!("{:?}", session.tool),
                &session.project_path,
                session.start_time.timestamp(),
                session.message_count,
                &session.file_path,
                session.last_updated.timestamp(),
            ],
        )?;

        // Insert messages into FTS5 table
        for msg in messages {
            self.db.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    &msg.session_id,
                    msg.index,
                    format!("{:?}", msg.role),
                    &msg.content,
                    msg.timestamp.timestamp(),
                ],
            )?;
        }

        Ok(())
    }
}

fn get_parser_for_tool(tool: Tool) -> Box<dyn SessionParser> {
    match tool {
        Tool::ClaudeCode => Box::new(ClaudeCodeParser),
        Tool::OpenCode => Box::new(OpenCodeParser),
        Tool::Codex => Box::new(CodexParser),
    }
}
```

---

## Relm4 UI Architecture

### Main App Component (`src/app.rs`)

```rust
use relm4::prelude::*;
use gtk4::prelude::*;

pub struct App {
    sessions: Vec<Session>,
    filtered_sessions: Vec<Session>,
    selected_session: Option<Session>,
    search_query: String,
    filter_tool: Option<Tool>,
}

#[derive(Debug)]
pub enum AppMsg {
    Search(String),
    FilterByTool(Option<Tool>),
    SelectSession(String),
    ResumeSession(String),
    Refresh,
}

impl SimpleComponent for App {
    type Input = AppMsg;
    type Output = ();
    type Init = ();
    type Root = adw::ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::new(&app)
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = App {
            sessions: load_sessions_from_db(),
            filtered_sessions: vec![],
            selected_session: None,
            search_query: String::new(),
            filter_tool: None,
        };

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            AppMsg::Search(query) => {
                self.search_query = query;
                self.apply_filters();
            }
            AppMsg::FilterByTool(tool) => {
                self.filter_tool = tool;
                self.apply_filters();
            }
            AppMsg::SelectSession(id) => {
                self.selected_session = self.sessions.iter()
                    .find(|s| s.id == id)
                    .cloned();
            }
            AppMsg::ResumeSession(id) => {
                if let Some(session) = self.sessions.iter().find(|s| s.id == id) {
                    resume_session_in_terminal(session);
                }
            }
            AppMsg::Refresh => {
                self.sessions = load_sessions_from_db();
                self.apply_filters();
            }
        }
    }
}
```

---

## Terminal Integration (`src/utils/terminal.rs`)

```rust
use std::process::Command;
use crate::models::{Session, Tool};

pub fn resume_session_in_terminal(session: &Session) {
    let command = build_resume_command(session);

    let terminal = detect_terminal();

    let _ = Command::new(&terminal)
        .args(&["--", "bash", "-c", &format!("{}; exec bash", command)])
        .spawn();
}

fn build_resume_command(session: &Session) -> String {
    match session.tool {
        Tool::ClaudeCode => format!("claude code resume {}", session.id),
        Tool::OpenCode => format!("opencode resume {}", session.id),
        Tool::Codex => format!("codex resume {}", session.id),
    }
}

fn detect_terminal() -> String {
    for terminal in ["gnome-terminal", "tilix", "konsole", "xterm"] {
        if which::which(terminal).is_ok() {
            return terminal.to_string();
        }
    }
    "xterm".to_string()  // Fallback
}
```

### Session Resumption with Fallback

```rust
use gtk::prelude::*;
use gtk::gdk::Display;

pub fn resume_session_in_terminal(session: &Session) {
    let command = build_resume_command(session);
    let terminal = detect_terminal();

    // Try to launch terminal with resume command
    let result = Command::new(&terminal)
        .args(&["--", "bash", "-c", &format!("{}; exec bash", command)])
        .spawn();

    if result.is_err() {
        // Fallback: copy command to clipboard and show notification
        copy_to_clipboard(&command);
        show_resume_fallback_notification(&command);
    }
}

fn copy_to_clipboard(text: &str) {
    if let Some(display) = Display::default() {
        if let Some(clipboard) = display.clipboard() {
            clipboard.set_text(text);
        }
    }
}

fn show_resume_fallback_notification(command: &str) {
    let notification = gio::Notification::new("Session Resume");
    notification.set_body(Some(&format!(
        "Terminal launch failed. Command copied to clipboard:\n{}",
        command
    )));
    notification.set_default_action_and_target_value(
        Some("app.show-notification"),
        None,
    );

    if let Some(app) = gio::Application::default() {
        app.send_notification(None, &notification);
    }
}
```

---

## Next: Implementation Steps

### Phase 1: Data Layer & Single Tool Support

1. **Create mock data directory** (`tests/fixtures/`) with sample Claude Code session files before inspecting actual files
2. **Implement data models** in `src/models/`:
   - `session.rs` - Session struct with Tool enum
   - `message.rs` - Message struct with Role enum
3. **Build database layer** in `src/database/`:
   - `schema.rs` - Initialize SQLite + FTS5 tables
   - `indexer.rs` - Index sessions from JSONL files (streaming, not loading into memory)
   - `search.rs` - FTS5 query implementation
4. **Implement Claude Code parser** (`src/parsers/claude_code.rs`):
   - Parse metadata from JSONL file
   - Parse messages line-by-line with `BufReader::lines()`
   - Handle streaming chunks and encrypted reasoning
   - Extract title: first `type == "user"` where `isMeta == false` or `type == "summary"`
5. **Wire indexer into App init**:
   - Add progress bar during initial indexing
   - Show session count after indexing completes
   - Handle errors gracefully (skip malformed files, log errors)
6. **Connect SessionList to database**:
   - Load sessions from DB on startup
   - Display session metadata (tool, project, date, message count)
   - Add loading state during initial load
7. **Add SessionDetail component**:
   - Display raw message data to verify parsing
   - Color-code by role (user, assistant, tool_call, tool_result)
   - Add scrolling for long conversations

### Phase 2: UI Polish & Filtering

8. **Connect Sidebar checkboxes**:
   - Wire up Claude Code, OpenCode, Codex checkboxes to filter messages
   - Update SessionList when filters change
   - Handle "all unchecked" case (show empty state)
9. **Implement search with FTS5**:
   - Connect SearchEntry to FTS5 query
   - Highlight matching terms in results
   - Debounce search input (don't query on every keystroke)
10. **Polish SessionDetail view**:
    - Format timestamps nicely (relative time: "2 hours ago")
    - Truncate very long messages with "Show more" button
    - Add syntax highlighting for code blocks (if feasible)
11. **Add keyboard shortcuts**:
    - `Ctrl+F` - Toggle search bar
    - `Ctrl+R` - Resume selected session
    - `Escape` - Clear search
    - Update ShortcutsDialog with new shortcuts
12. **Implement session resumption**:
    - Detect available terminal
    - Launch with resume command
    - Add fallback: copy to clipboard + notification
    - Add resume button to SessionDetail view

### Phase 3: Testing & Error Handling

13. **Add loading states**:
    - Progress bar during initial indexing
    - Spinner when searching
    - Skeleton screens while loading session detail
14. **Write integration tests**:
    - Parse mock session file → Store in DB → Retrieve → Verify data integrity
    - Test FTS5 search with various queries
    - Test filter combinations
15. **Error handling**:
    - Use `anyhow` for app-level errors with context
    - Use `thiserror` for parser-specific errors
    - Skip malformed JSONL files, log errors, continue indexing
    - Show user-friendly error messages

### Phase 4: Multi-Tool Support (v2)

16. **Add OpenCode parser** (defer - most complex):
    - Parse session metadata JSON file
    - Read messages from separate directory structure
    - Handle parent-child session relationships (flat display for v1)
    - Consider storing sessions separately due to multi-file complexity
17. **Add Codex parser**:
    - Parse JSONL format with streaming chunks
    - Coalesce chunks by message_id
    - Handle encrypted reasoning (never decrypt locally)
    - Support multimodal content (images as base64 or URLs)

### Technical Notes

- **Database location**: Use `glib::user_data_dir()` with `APP_ID` instead of hardcoded paths
- **JSONL streaming**: Use `BufReader::lines()` to avoid loading large files into memory
- **CSS theming**: Define tool colors as CSS custom properties in `style.css`
- **SQLite threading**: If adding background indexing later, use `r2d2` connection pool or `OnceLock`
