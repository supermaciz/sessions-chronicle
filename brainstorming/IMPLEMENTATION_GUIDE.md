# Sessions Chronicle - Implementation Guide

This guide provides a detailed roadmap and best practices for implementing Sessions Chronicle.

---

## Phase 1: Single Tool Support (Claude Code)

### Step 1: Add Missing Dependencies

Update `Cargo.toml` with the following dependencies:

```toml
[dependencies]
# UI Framework (existing)
relm4 = { version = "0.10.0", features = ["libadwaita", "gnome_48"] }
adw = { version = "0.8.1", package = "libadwaita", features = ["v1_8"] }

# Database
rusqlite = { version = "0.32", features = ["bundled", "fts5"] }

# JSON parsing
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# File system traversal
walkdir = "2.5"

# Date/Time handling
chrono = "0.4"

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Command-line argument parsing
clap = { version = "4.5", features = ["derive"] }

# Logging (existing)
tracing = "0.1.44"
tracing-subscriber = "0.3.22"

# i18n (existing)
gettext-rs = { version = "0.7.7", features = ["gettext-system"] }
```

### Step 2: Create Mock Data

Create `tests/fixtures/` directory with sample session files for testing and development:

```bash
mkdir -p tests/fixtures/claude_sessions
```

**Important**: The application should not check for test fixtures in production code. Instead, use command-line arguments during development:

```bash
# Development with test fixtures
sessions-chronicle --sessions-dir tests/fixtures/claude_sessions

# Production (uses ~/.claude/projects by default)
sessions-chronicle
```

**Design Rationale:**
- ✅ **Clean separation**: Production code doesn't check for test directories
- ✅ **Explicit over magical**: Developers explicitly choose test mode with `--sessions-dir`
- ✅ **Standard practice**: CLI args are the conventional way to override defaults
- ✅ **Flexible**: Easy to test with any directory, not just `tests/fixtures/`
- ✅ **No pollution**: Test-checking logic doesn't bloat production binary

Create `tests/fixtures/claude_sessions/sample-session.jsonl`:

```json
{"type":"user","message":{"role":"user","content":"Help me refactor this code"},"timestamp":"2025-01-10T10:30:00.000Z","cwd":"/home/user/project","sessionId":"abc123","uuid":"msg1","parentUuid":null,"isMeta":false}
{"type":"assistant","message":{"role":"assistant","content":"I'll help you refactor that code. Let me first read the file..."},"timestamp":"2025-01-10T10:30:05.000Z","cwd":"/home/user/project","sessionId":"abc123","uuid":"msg2","parentUuid":"msg1","isMeta":false}
{"type":"system","subtype":"local_command","command":["cat","src/main.rs"],"stdout":"fn main() { println!(\"Hello\"); }","timestamp":"2025-01-10T10:30:10.000Z","cwd":"/home/user/project","sessionId":"abc123","uuid":"msg3","parentUuid":"msg2","isMeta":true}
```

### Step 3: Implement Data Models

Create `src/models/session.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub tool: Tool,
    pub project_path: Option<String>,
    pub start_time: DateTime<Utc>,
    pub message_count: usize,
    pub file_path: String,
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
            Tool::ClaudeCode => "#3584e4",
            Tool::OpenCode => "#26a269",
            Tool::Codex => "#e66100",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude-code-symbolic",
            Tool::OpenCode => "opencode-symbolic",
            Tool::Codex => "codex-symbolic",
        }
    }

    pub fn session_dir(&self) -> String {
        let home = std::env::var("HOME").unwrap();
        match self {
            Tool::ClaudeCode => format!("{}/.claude/projects", home),
            Tool::OpenCode => format!("{}/.local/share/opencode/storage/session", home),
            Tool::Codex => format!("{}/.codex/sessions", home),
        }
    }
}
```

Create `src/models/message.rs`:

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
            Role::User => "#3584e4",
            Role::Assistant => "#26a269",
            Role::ToolCall => "#e66100",
            Role::ToolResult => "#1c71d8",
        }
    }
}
```

Create `src/models/mod.rs`:

```rust
pub mod message;
pub mod session;

pub use message::{Message, Role};
pub use session::{Session, Tool};
```

### Step 4: Build Database Layer

Create `src/database/schema.rs`:

```rust
use anyhow::Result;
use rusqlite::Connection;

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

    // Create indexes
    conn.execute("CREATE INDEX IF NOT EXISTS idx_tool ON sessions(tool)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_project ON sessions(project_path)", [])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_time ON sessions(start_time DESC)", [])?;

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

Create `src/database/indexer.rs`:

```rust
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::models::Session;
use crate::parsers::claude_code::ClaudeCodeParser;

pub struct SessionIndexer {
    db: Connection,
}

impl SessionIndexer {
    pub fn new(db_path: &Path) -> Result<Self> {
        let db = Connection::open(db_path)
            .context("Failed to open database")?;
        crate::database::schema::initialize_database(&db)
            .context("Failed to initialize database schema")?;
        Ok(Self { db })
    }

    pub fn index_claude_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        let parser = ClaudeCodeParser;
        let mut count = 0;

        for entry in walkdir::WalkDir::new(sessions_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "jsonl" {
                        if let Err(e) = self.index_session_file(entry.path(), &parser) {
                            tracing::warn!("Failed to index {}: {}", entry.path().display(), e);
                        } else {
                            count += 1;
                        }
                    }
                }
            }
        }

        Ok(count)
    }

    fn index_session_file(
        &mut self,
        file_path: &Path,
        parser: &ClaudeCodeParser,
    ) -> Result<()> {
        let session = parser.parse_metadata(file_path)?;
        let messages = parser.parse_messages(file_path)?;

        // Insert or update session
        self.db.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, project_path, start_time, message_count, file_path, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                &session.id,
                "claude_code",
                &session.project_path,
                session.start_time.timestamp(),
                session.message_count,
                file_path.to_str(),
                session.last_updated.timestamp(),
            ],
        )?;

        // Delete old messages for this session
        self.db.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            [&session.id],
        )?;

        // Insert new messages
        for msg in messages {
            self.db.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    &msg.session_id,
                    msg.index as i64,
                    format!("{:?}", msg.role).to_lowercase(),
                    &msg.content,
                    msg.timestamp.timestamp(),
                ],
            )?;
        }

        Ok(())
    }
}
```

### Step 5: Implement Claude Code Parser

Create `src/parsers/claude_code.rs`:

```rust
use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::models::{Message, Role, Session, Tool};

pub struct ClaudeCodeParser;

impl ClaudeCodeParser {
    pub fn parse_metadata(&self, file_path: &Path) -> Result<Session> {
        let file = File::open(file_path)
            .context("Failed to open session file")?;

        let reader = BufReader::new(file);
        let mut first_timestamp = None;
        let mut project_path = None;
        let mut session_id = None;
        let mut message_count = 0;

        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(&line)
                .context("Failed to parse JSON")?;

            // Extract session ID from first event
            if session_id.is_none() {
                session_id = event.get("sessionId")
                    .and_then(|v| v.as_str())
                    .or_else(|| file_path.stem().and_then(|s| s.to_str()))
                    .map(|s| s.to_string());
            }

            // Extract project path from cwd
            if project_path.is_none() {
                project_path = event.get("cwd")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }

            // Extract first timestamp
            if first_timestamp.is_none() {
                if let Some(ts) = event.get("timestamp").and_then(|v| v.as_str()) {
                    first_timestamp = Self::parse_timestamp(ts)?;
                }
            }

            message_count += 1;

            // Only process first few events for metadata
            if message_count >= 10 {
                break;
            }
        }

        // Count total messages by reading entire file
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let total_count = reader.lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .count();

        Ok(Session {
            id: session_id.unwrap_or_else(|| {
                file_path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            }),
            tool: Tool::ClaudeCode,
            project_path,
            start_time: first_timestamp.unwrap_or_else(|| Utc::now()),
            message_count: total_count,
            file_path: file_path.to_str().unwrap().to_string(),
            last_updated: Utc::now(),
        })
    }

    pub fn parse_messages(&self, file_path: &Path) -> Result<Vec<Message>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(&line)
                .context("Failed to parse JSON")?;

            if let Some(msg) = Self::parse_event(&event, index) {
                messages.push(msg);
            }
        }

        Ok(messages)
    }

    fn parse_event(event: &Value, index: usize) -> Option<Message> {
        let event_type = event.get("type")?.as_str()?;

        let (role, content) = match event_type {
            "user" => {
                let content = event.get("message")?.get("content")?.as_str()?;
                (Role::User, content.to_string())
            }
            "assistant" => {
                let content = event.get("message")?.get("content")?.as_str()?;
                (Role::Assistant, content.to_string())
            }
            "system" => {
                let subtype = event.get("subtype")?.as_str()?;
                match subtype {
                    "local_command" => {
                        let stdout = event.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
                        let stderr = event.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
                        let cmd = event.get("command").and_then(|v| v.as_array())
                            .map(|arr| arr.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(" "))
                            .unwrap_or_else(|| "command".to_string());
                        (Role::ToolResult, format!("$ {}\n{}", cmd, stdout))
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };

        let timestamp = event.get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| Self::parse_timestamp(s).ok())
            .unwrap_or_else(|| Utc::now());

        let session_id = event.get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        Some(Message {
            session_id,
            index,
            role,
            content,
            timestamp,
        })
    }

    fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .context("Failed to parse timestamp")
    }
}
```

Create `src/parsers/mod.rs`:

```rust
pub mod claude_code;

pub use claude_code::ClaudeCodeParser;
```

Create `src/database/mod.rs`:

```rust
pub mod indexer;
pub mod schema;

pub use indexer::SessionIndexer;
```

### Step 6: Wire Indexer into App

First, add command-line argument parsing to `src/main.rs`:

```rust
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Override sessions directory (for development/testing)
    #[arg(long, value_name = "DIR")]
    sessions_dir: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();

    // ... existing main.rs code ...

    // Pass sessions_dir to App
    let app = RelmApp::new("io.github.supermaciz.sessionschronicle");
    app.run::<App>(args.sessions_dir);
}
```

Update `src/app.rs` to accept sessions directory and initialize database:

```rust
use relm4::{
    actions::{AccelsPlus, RelmAction, RelmActionGroup},
    adw, gtk, main_application, Component, ComponentController, ComponentParts, ComponentSender, SimpleComponent,
};
use adw::prelude::AdwApplicationWindowExt;
use gtk::prelude::{ApplicationExt, ButtonExt, GtkWindowExt, OrientableExt, SettingsExt, ToggleButtonExt, WidgetExt};
use gtk::{gio, glib};
use std::path::PathBuf;
use tracing::{error, info};

use crate::config::{APP_ID, PROFILE};
use crate::database::SessionIndexer;
use crate::modals::{about::AboutDialog, shortcuts::ShortcutsDialog};
use crate::ui::{sidebar::Sidebar, session_list::SessionList};

pub(super) struct App {
    search_visible: bool,
    db_initialized: bool,
    sessions_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) enum AppMsg {
    Quit,
    ToggleSearch,
    InitializeDatabase,
}

relm4::new_action_group!(pub(super) WindowActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, WindowActionGroup, "preferences");
relm4::new_stateless_action!(pub(super) ShortcutsAction, WindowActionGroup, "show-help-overlay");
relm4::new_stateless_action!(AboutAction, WindowActionGroup, "about");
relm4::new_stateless_action!(QuitAction, WindowActionGroup, "quit");

#[relm4::component(pub)]
impl SimpleComponent for App {
    type Init = Option<PathBuf>;  // Sessions directory override
    type Input = AppMsg;
    type Output = ();
    type Widgets = AppWidgets;

    menu! {
        primary_menu: {
            section! {
                "_Preferences" => PreferencesAction,
                "_Keyboard" => ShortcutsAction,
                "_About Sessions Chronicle" => AboutAction,
            }
        }
    }

    view! {
        main_window = adw::ApplicationWindow::new(&main_application()) {
            set_visible: true,

            connect_close_request[sender] => move |_| {
                sender.input(AppMsg::Quit);
                glib::Propagation::Stop
            },

            add_css_class?: if PROFILE == "Devel" {
                    Some("devel")
                } else {
                    None
                },

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_start = &gtk::ToggleButton {
                        set_icon_name: "system-search-symbolic",
                        set_tooltip_text: Some("Search sessions"),
                        #[watch]
                        set_active: model.search_visible,
                        connect_toggled[sender] => move |_| {
                            sender.input(AppMsg::ToggleSearch);
                        },
                    },

                    pack_end = &gtk::MenuButton {
                        set_icon_name: "open-menu-symbolic",
                        set_menu_model: Some(&primary_menu),
                    },
                },

                #[wrap(Some)]
                set_content = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    #[name = "search_bar"]
                    gtk::SearchBar {
                        #[watch]
                        set_search_mode: model.search_visible,

                        #[wrap(Some)]
                        set_child = &gtk::SearchEntry {
                            set_placeholder_text: Some("Search sessions..."),
                            set_hexpand: true,
                        },
                    },

                    adw::NavigationSplitView {
                        set_vexpand: true,

                        #[wrap(Some)]
                        set_sidebar = &adw::NavigationPage::builder()
                            .title("Filters")
                            .child(sidebar.widget())
                            .build(),

                        #[wrap(Some)]
                        set_content = &adw::NavigationPage::builder()
                            .title("Sessions")
                            .child(session_list.widget())
                            .build(),
                    },
                },
            },
        }
    }

    fn init(
        sessions_dir: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let sidebar = Sidebar::builder().launch(()).detach();
        let session_list = SessionList::builder().launch(()).detach();

        let model = Self {
            search_visible: false,
            db_initialized: false,
            sessions_dir,
        };
        let widgets = view_output!();

        let app = root.application().unwrap();
        let mut actions = RelmActionGroup::<WindowActionGroup>::new();

        let shortcuts_action = {
            RelmAction::<ShortcutsAction>::new_stateless(move |_| {
                ShortcutsDialog::builder().launch(()).detach();
            })
        };

        let about_action = {
            RelmAction::<AboutAction>::new_stateless(move |_| {
                AboutDialog::builder().launch(()).detach();
            })
        };

        let quit_action = {
            RelmAction::<QuitAction>::new_stateless(move |_| {
                sender.input(AppMsg::Quit);
            })
        };

        app.set_accelerators_for_action::<QuitAction>(&["<Control>q"]);

        actions.add_action(shortcuts_action);
        actions.add_action(about_action);
        actions.add_action(quit_action);
        actions.register_for_widget(&widgets.main_window);

        widgets.load_window_size();

        // Initialize database in background
        let sender_clone = sender.clone();
        glib::idle_add_local_once(move || {
            sender_clone.input(AppMsg::InitializeDatabase);
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            AppMsg::Quit => main_application().quit(),
            AppMsg::ToggleSearch => {
                self.search_visible = !self.search_visible;
            }
            AppMsg::InitializeDatabase => {
                if !self.db_initialized {
                    info!("Initializing database...");

                    let data_dir = glib::user_data_dir();
                    let db_path = data_dir.join("sessions-chronicle").join("sessions.db");

                    if let Err(e) = std::fs::create_dir_all(db_path.parent().unwrap()) {
                        error!("Failed to create data directory: {}", e);
                        return;
                    }

                    let mut indexer = match SessionIndexer::new(&db_path) {
                        Ok(i) => i,
                        Err(e) => {
                            error!("Failed to initialize database: {}", e);
                            return;
                        }
                    };

                    // Get sessions directory from command-line args or use default
                    let sessions_dir = self.sessions_dir.clone().unwrap_or_else(|| {
                        let home = std::env::var("HOME").unwrap();
                        std::path::PathBuf::from(format!("{}/.claude/projects", home))
                    });

                    match indexer.index_claude_sessions(&sessions_dir) {
                        Ok(count) => {
                            info!("Indexed {} Claude Code sessions", count);
                            self.db_initialized = true;
                        }
                        Err(e) => {
                            error!("Failed to index sessions: {}", e);
                        }
                    }
                }
            }
        }
    }

    fn shutdown(&mut self, widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        let _ = widgets.save_window_size();
    }
}

impl AppWidgets {
    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = gio::Settings::new(APP_ID);
        let (width, height) = self.main_window.default_size();

        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;
        settings.set_boolean("is-maximized", self.main_window.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let settings = gio::Settings::new(APP_ID);

        let width = settings.int("window-width");
        let height = settings.int("window-height");
        let is_maximized = settings.boolean("is-maximized");

        self.main_window.set_default_size(width, height);

        if is_maximized {
            self.main_window.maximize();
        }
    }
}
```

---

## Phase 2: UI Polish & Filtering

### Step 7: Connect Sidebar Checkboxes

Update `src/ui/sidebar.rs` to emit filter messages:

```rust
use relm4::{gtk, ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};
use gtk::prelude::*;

#[derive(Debug)]
pub struct Sidebar {
    claude_code_active: bool,
    opencode_active: bool,
    codex_active: bool,
}

#[derive(Debug)]
pub enum SidebarMsg {
    ToggleClaudeCode(bool),
    ToggleOpenCode(bool),
    ToggleCodex(bool),
}

#[derive(Debug)]
pub enum SidebarOutput {
    FilterChanged {
        claude_code: bool,
        opencode: bool,
        codex: bool,
    },
}

#[relm4::component(pub)]
impl SimpleComponent for Sidebar {
    type Init = ();
    type Input = SidebarMsg;
    type Output = SidebarOutput;
    type Widgets = SidebarWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 12,
            set_margin_all: 12,
            set_width_request: 200,

            gtk::Label {
                set_label: "Filters",
                set_halign: gtk::Align::Start,
                add_css_class: "title-4",
                set_margin_bottom: 6,
            },

            gtk::Separator {
                set_margin_bottom: 12,
            },

            gtk::Label {
                set_label: "Tools",
                set_halign: gtk::Align::Start,
                add_css_class: "heading",
                set_margin_bottom: 6,
            },

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 6,

                gtk::CheckButton {
                    set_label: Some("Claude Code"),
                    #[watch]
                    set_active: model.claude_code_active,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::ToggleClaudeCode(btn.is_active()));
                    },
                },

                gtk::CheckButton {
                    set_label: Some("OpenCode"),
                    #[watch]
                    set_active: model.opencode_active,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::ToggleOpenCode(btn.is_active()));
                    },
                },

                gtk::CheckButton {
                    set_label: Some("Codex"),
                    #[watch]
                    set_active: model.codex_active,
                    connect_toggled[sender] => move |btn| {
                        sender.input(SidebarMsg::ToggleCodex(btn.is_active()));
                    },
                },
            },

            gtk::Separator {
                set_margin_top: 12,
                set_margin_bottom: 12,
            },

            gtk::Label {
                set_label: "Projects",
                set_halign: gtk::Align::Start,
                add_css_class: "heading",
                set_margin_bottom: 6,
            },

            gtk::ScrolledWindow {
                set_vexpand: true,
                set_hscrollbar_policy: gtk::PolicyType::Never,

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_spacing: 6,

                    gtk::Label {
                        set_label: "No projects yet",
                        set_halign: gtk::Align::Start,
                        add_css_class: "dim-label",
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        _root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self {
            claude_code_active: true,
            opencode_active: true,
            codex_active: true,
        };
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            SidebarMsg::ToggleClaudeCode(active) => {
                self.claude_code_active = active;
                self.emit_filters(sender);
            }
            SidebarMsg::ToggleOpenCode(active) => {
                self.opencode_active = active;
                self.emit_filters(sender);
            }
            SidebarMsg::ToggleCodex(active) => {
                self.codex_active = active;
                self.emit_filters(sender);
            }
        }
    }
}

impl Sidebar {
    fn emit_filters(&self, sender: ComponentSender<Self>) {
        let _ = sender.output(SidebarOutput::FilterChanged {
            claude_code: self.claude_code_active,
            opencode: self.opencode_active,
            codex: self.codex_active,
        });
    }
}
```

### Step 8: Connect SessionList to Database

Update `src/ui/session_list.rs` to load and display sessions:

```rust
use relm4::{adw, gtk, ComponentParts, ComponentSender, SimpleComponent};
use gtk::prelude::*;
use rusqlite::Connection;

#[derive(Debug)]
pub struct SessionList {
    sessions: Vec<SessionData>,
    db_path: std::path::PathBuf,
}

#[derive(Debug, Clone)]
struct SessionData {
    id: String,
    tool: String,
    project_path: Option<String>,
    start_time: i64,
    message_count: i64,
}

#[derive(Debug)]
pub enum SessionListMsg {
    LoadSessions,
    FilterSessions {
        claude_code: bool,
        opencode: bool,
        codex: bool,
    },
}

#[derive(Debug)]
pub enum SessionListOutput {
    SessionSelected(String),
}

#[relm4::component(pub)]
impl SimpleComponent for SessionList {
    type Init = ();
    type Input = SessionListMsg;
    type Output = SessionListOutput;
    type Widgets = SessionListWidgets;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 0,

            #[name = "list_box"]
            gtk::ListBox {
                set_vexpand: true,
                add_css_class: "boxed-list",

                connect_row_activated[sender] => move |_, row| {
                    if let Some(index) = row.index() {
                        if let Some(session) = model.sessions.get(index as usize) {
                            let _ = sender.output(SessionListOutput::SessionSelected(session.id.clone()));
                        }
                    }
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let data_dir = glib::user_data_dir();
        let db_path = data_dir.join("sessions-chronicle").join("sessions.db");

        let model = Self {
            sessions: Vec::new(),
            db_path,
        };
        let widgets = view_output!();

        // Load sessions after initialization
        let sender_clone = sender.clone();
        glib::idle_add_local_once(move || {
            sender_clone.input(SessionListMsg::LoadSessions);
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            SessionListMsg::LoadSessions => {
                self.load_from_db();
            }
            SessionListMsg::FilterSessions { .. } => {
                // TODO: Implement filtering
            }
        }
    }

    fn post_view(&self, widgets: &mut Self::Widgets) {
        // Clear existing rows
        while let Some(row) = widgets.list_box.row_at_index(0) {
            widgets.list_box.remove(&row);
        }

        // Add session rows
        for session in &self.sessions {
            let row = adw::ActionRow::builder()
                .title(&format_session_time(session.start_time))
                .subtitle(session.project_path.as_deref().unwrap_or("Unknown project"))
                .build();

            widgets.list_box.append(&row);
        }

        if self.sessions.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No sessions")
                .subtitle("Connect to Claude Code to see sessions")
                .build();
            widgets.list_box.append(&row);
        }
    }
}

impl SessionList {
    fn load_from_db(&mut self) {
        let conn = match Connection::open(&self.db_path) {
            Ok(c) => c,
            Err(_) => {
                tracing::warn!("Database not initialized yet");
                return;
            }
        };

        let mut stmt = match conn.prepare(
            "SELECT id, tool, project_path, start_time, message_count FROM sessions ORDER BY start_time DESC"
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to prepare query: {}", e);
                return;
            }
        };

        let sessions_result = stmt.query_map([], |row| {
            Ok(SessionData {
                id: row.get(0)?,
                tool: row.get(1)?,
                project_path: row.get(2)?,
                start_time: row.get(3)?,
                message_count: row.get(4)?,
            })
        });

        match sessions_result {
            Ok(sessions) => {
                self.sessions = sessions.filter_map(|s| s.ok()).collect();
                tracing::info!("Loaded {} sessions", self.sessions.len());
            }
            Err(e) => {
                tracing::error!("Failed to query sessions: {}", e);
            }
        }
    }
}

fn format_session_time(timestamp: i64) -> String {
    use chrono::{DateTime, Local, TimeZone};

    let dt = Local.timestamp_opt(timestamp, 0).single()
        .unwrap_or_else(|| Local::now());

    let now = Local::now();
    let duration = now.signed_duration_since(dt);

    if duration.num_days() > 7 {
        dt.format("%Y-%m-%d").to_string()
    } else if duration.num_days() > 0 {
        format!("{} days ago", duration.num_days())
    } else if duration.num_hours() > 0 {
        format!("{} hours ago", duration.num_hours())
    } else if duration.num_minutes() > 0 {
        format!("{} minutes ago", duration.num_minutes())
    } else {
        "Just now".to_string()
    }
}
```

---

## Phase 3: Testing & Error Handling

### Step 9: Add Integration Test

Create `tests/integration_test.rs`:

```rust
use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::parsers::ClaudeCodeParser;
use std::path::PathBuf;

#[test]
fn test_parse_and_index_session() {
    let test_data = PathBuf::from("tests/fixtures/claude_sessions/sample-session.jsonl");
    assert!(test_data.exists(), "Test data file not found");

    let parser = ClaudeCodeParser;
    let session = parser.parse_metadata(&test_data).unwrap();

    assert_eq!(session.tool, sessions_chronicle::models::Tool::ClaudeCode);
    assert!(!session.id.is_empty());
    assert!(session.message_count > 0);
}
```

---

## Common Pitfalls & Best Practices

### Pitfall 1: Loading Entire JSONL into Memory

**Wrong:**
```rust
let content = std::fs::read_to_string(file_path)?;
let lines: Vec<&str> = content.lines().collect();
for line in lines { /* parse */ }
```

**Right:**
```rust
let file = std::fs::File::open(file_path)?;
let reader = std::io::BufReader::new(file);
for line in reader.lines() {
    /* parse line by line */
}
```

### Pitfall 2: Hardcoded Paths

**Wrong:**
```rust
let db_path = "/home/user/.local/share/sessions-chronicle/sessions.db";
```

**Right:**
```rust
let data_dir = glib::user_data_dir();
let db_path = data_dir.join("sessions-chronicle").join("sessions.db");
```

### Pitfall 3: Panicking on Errors

**Wrong:**
```rust
let json: Value = serde_json::from_str(&line).unwrap();
```

**Right:**
```rust
let json: Value = serde_json::from_str(&line)
    .context("Failed to parse JSON line")?;
```

### Best Practice 1: Contextual Errors with `anyhow`

```rust
use anyhow::{Context, Result};

fn parse_session(path: &Path) -> Result<Session> {
    let content = std::fs::read_to_string(path)
        .context("Failed to read session file")?;
    let json: Value = serde_json::from_str(&content)
        .context("Failed to parse session JSON")?;
    // ...
}
```

### Best Practice 2: Structured Logging with `tracing`

```rust
tracing::info!("Indexing sessions from {}", sessions_dir.display());
tracing::warn!("Failed to index {}: {}", path, error);
tracing::error!("Database initialization failed: {}", error);
```

### Best Practice 3: Use CSS Custom Properties for Theming

In `resources/style.css`:
```css
:root {
    --claude-code-color: #3584e4;
    --opencode-color: #26a269;
    --codex-color: #e66100;
}

.session-claude-code {
    color: var(--claude-code-color);
}
```

---

## Phase 4: Multi-Tool Support (v2)

Deferred to v2:
- OpenCode parser (complex multi-file structure)
- Codex parser (streaming chunks, encrypted reasoning)
- Hierarchical session display for OpenCode subagents
- Real-time file watching with `notify` crate

---

## Development Checklist

### Core Functionality
- [ ] Add dependencies to Cargo.toml
- [ ] Create mock data directory
- [ ] Implement data models (Session, Message)
- [ ] Create database schema
- [ ] Implement Claude Code parser (metadata)
- [ ] Implement Claude Code parser (messages)
- [ ] Build database indexer
- [ ] Wire indexer into App init
- [ ] Connect SessionList to database
- [ ] Display sessions in list view

### UI & UX
- [ ] Connect Sidebar checkboxes to filters
- [ ] Implement search functionality
- [ ] Add SessionDetail component
- [ ] Implement keyboard shortcuts
- [ ] Add loading states (progress bars)
- [ ] Handle empty states
- [ ] Add session resumption
- [ ] Implement fallback (copy to clipboard)

### Testing & Polish
- [ ] Write integration tests
- [ ] Add error handling throughout
- [ ] Add structured logging
- [ ] Polish UI with proper spacing and icons
- [ ] Write user documentation (README.md)
- [ ] Add Flatpak packaging verification

### v2 Features
- [ ] OpenCode parser
- [ ] Codex parser
- [ ] File watching for real-time updates
- [ ] Session export functionality
- [ ] Analytics and usage charts
