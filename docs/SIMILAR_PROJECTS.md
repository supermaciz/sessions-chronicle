# Similar Projects

Last reviewed: 2026-04-25

A shortlist of projects in the same product space as Sessions Chronicle:
tools that index, browse, search, or analyze local AI assistant sessions.

## Selection criteria

Projects listed here should be:

- meaningfully similar to Sessions Chronicle's core problem space
- publicly available (open source or source-available)
- actively maintained as of 2026-04-25
- not archived

For this document, "actively maintained" means there was visible recent repository
activity on GitHub such as pushes or release work in March or April 2026.

## Closest matches

### [AgentsView](https://github.com/wesm/agentsview)

Local-first desktop and web application written primarily in Go + Svelte (552
stars). It indexes AI assistant sessions into SQLite with FTS, exposes full-text
search, analytics dashboards, live SSE updates, and export/publish flows.
Supports Claude Code, Codex, Copilot, Gemini, and 10+ other assistants.
Optional PostgreSQL team-sync mode.

- **Why it's similar:** Strong overlap on local indexing, session browsing,
  search, analytics, and multi-assistant support.
- **How it differs from Sessions Chronicle:** Browser/webview-first product with
  a local server architecture rather than a native GNOME/GTK app.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-17 (v0.15.0) and
  active release/install documentation.

### [Agent Sessions](https://github.com/jazzyalex/agent-sessions)

Native macOS app in Swift (390 stars) for browsing, searching, and resuming AI
assistant sessions across Codex CLI, Claude Code, Gemini CLI, GitHub Copilot
CLI, Factory Droid, and OpenCode. Includes a live "Agent Cockpit" HUD for
active sessions, cross-session search, transcript reading, and resume flows.

- **Why it's similar:** Very close product shape: local-first desktop app,
  multi-assistant session browser, transcript reading, search, and resume.
- **How it differs from Sessions Chronicle:** macOS-only, with a stronger focus
  on live-session monitoring and iTerm2 integration.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-20 (v3.3.1) and
  current release notes. 41 releases, 1,016 commits.

### [Claude Code History Viewer](https://github.com/jhlee0409/claude-code-history-viewer)

Desktop app and optional headless server built with Rust, Tauri, and React (675
stars). Supports Claude Code, Codex CLI, and OpenCode. Focuses on unified
browsing, global search, analytics, live file watching, archive management, and
a browser-accessible server mode.

- **Why it's similar:** Local-first session viewer with Rust + SQLite
  architecture, multi-assistant support, transcript search, and analytics.
- **How it differs from Sessions Chronicle:** Cross-platform Tauri stack and
  optional Web UI/server deployment rather than GTK/Libadwaita packaging.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-21 (v1.6.0) with
  688 commits and active issue tracker.

### [cass (coding_agent_session_search)](https://github.com/Dicklesworthstone/coding_agent_session_search)

Rust CLI/TUI (612 stars) that indexes local coding-agent session history across
11+ providers. Uses a custom search stack with sub-60ms lexical search, optional
local semantic search via MiniLM/FastEmbed, BM25 ranking, HTML export with
AES-256 encryption, JSON/robot output modes, and automation-oriented workflows.

- **Why it's similar:** Same underlying problem: unify fragmented local AI
  assistant history into one searchable system.
- **How it differs from Sessions Chronicle:** Much more CLI/TUI and
  automation-agent oriented; broadest provider coverage of any project here;
  less focused on native desktop reading UX.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-21 and current
  install/release documentation.

### [Claude Code Viewer](https://github.com/d-kimuson/claude-code-viewer)

Web application and local server in TypeScript/Vue (984 stars) focused on
Claude Code, with full session history browsing, real-time monitoring, search,
project creation, scheduling, Git diff review, terminal integration, and
remote-friendly access.

- **Why it's similar:** Strong overlap on session browsing, search, transcript
  inspection, and local-first usage of Claude Code history.
- **How it differs from Sessions Chronicle:** Primarily a Claude Code client and
  control surface, not a cross-assistant desktop browser first.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-03 and an active
  release/CI setup.

### [claude-view](https://github.com/tombelieber/claude-view)

Real-time monitoring dashboard for Claude Code sessions built with Rust and
React (30 stars). Provides live cost tracking, context window gauges, full-text
search, sub-agent visualization, and session analytics. Designed for power users
running many simultaneous Claude sessions.

- **Why it's similar:** Local-first, Rust-based session search and analytics
  with a desktop-oriented UI.
- **How it differs from Sessions Chronicle:** Claude Code-only; emphasis is on
  live monitoring of active sessions rather than browsing past history.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-20.

### [Claudoscope](https://github.com/cordwainersmith/Claudoscope)

Native macOS menu bar app in Swift (121 stars) exclusively for Claude Code
(`~/.claude/projects/`). Combines a compact menu bar popover with a full
dashboard covering session browsing, analytics, cost estimation, plan and
timeline views, hooks, skills, MCPs, memory, and a secret-scanning feature that
alerts on leaked credentials in session history. Installable via Homebrew cask.

- **Why it's adjacent:** Rich native-desktop session browsing and analytics
  with a local-first, privacy-first approach closely mirrors Sessions
  Chronicle's core purpose.
- **Why it is not a direct match:** Claude Code-only, macOS-exclusive (Apple
  Silicon, 14+), and menu-bar-first rather than a standalone multi-assistant
  session browser.
- **Maintenance signal:** GitHub repo shows pushes on 2026-04-13 (v0.6.0).

### [Chronicle / claude-history-manager](https://github.com/JosephYaduvanshi/claude-history-manager)

Native macOS app in SwiftUI (8 stars) for browsing Claude Code session history
from `~/.claude/projects/`. Indexes sessions into local SQLite with FTS5 and
provides search filters, transcript reading, tags, pins, archive/soft-delete,
smart folders, stats dashboards, menu bar access, Quick Look, metadata-only
iCloud sync, and resume/open actions for terminals and editors.

- **Why it's similar:** Local-first native desktop session browser with SQLite
  indexing, full-text search, transcript reading, analytics, and resume flows.
- **How it differs from Sessions Chronicle:** Claude Code-only and macOS-only,
  with SwiftUI/AppKit packaging rather than GNOME/GTK.
- **Maintenance signal:** GitHub repo was created and pushed on 2026-04-25,
  with release/install documentation.

### [CodMate](https://github.com/loocor/codmate)

Native macOS app in Swift (625 stars) for managing CLI AI sessions. Supports
Codex (`~/.codex/sessions`), Claude Code (`~/.claude/projects`), and Gemini CLI
(`~/.gemini/tmp`). Features a 3-column interface, fast indexing, Git review with
AI-generated commit messages, and one-click session resume.

- **Why it's similar:** Multi-assistant native desktop app, local-first, session
  browse/search/resume.
- **How it differs from Sessions Chronicle:** macOS Swift rather than GNOME/GTK;
  narrower assistant support.
- **Status note:** The maintainer has announced the project for archival as of
  January 2026, citing ecosystem evolution.
- **Maintenance signal:** Last push 2026-01-05 (v0.5.9); archival is imminent.

## Adjacent projects

These are worth tracking, but they are not as directly comparable to Sessions
Chronicle's current scope.

### [CC Switch](https://github.com/farion1231/cc-switch)

Cross-platform Tauri 2 (Rust/TypeScript) desktop manager for Claude Code,
Codex, Gemini CLI, OpenCode, and OpenClaw (31,100 stars). Includes a Session
Manager component with browse/search/restore across all five tools alongside
provider config switching, MCP sync, and a cost dashboard.

- **Why it's adjacent:** Multi-assistant desktop app with session browsing as
  one of its features.
- **Why it is not a direct match:** Its primary purpose is provider and config
  management; session history browsing is a secondary feature.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-21.

### [SpecStory](https://github.com/specstoryai/getspecstory)

Go CLI (1,100 stars) that captures and indexes AI coding conversations locally
from IDEs (Cursor, VSCode) and CLI tools (Claude Code, Codex). Saves to
`.specstory/history/`, enables local search, and offers optional cloud sync for
cross-project full-text search.

- **Why it's adjacent:** Multi-assistant local session capture and full-text
  search.
- **Why it is not a direct match:** IDE extension + CLI delivery; cloud sync
  upsell; no native desktop browser UI.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-21 (v1.12.0).

### [claude-historian-mcp](https://github.com/Vvkmnn/claude-historian-mcp)

TypeScript MCP server (217 stars) for searching Claude Code conversation history
from within Claude itself. Zero external dependencies, 11 search scopes, fuzzy
matching. Feeds history back as context rather than rendering it in a UI.

- **Why it's adjacent:** Active project solving Claude Code session search,
  shares the local-first ethos.
- **Why it is not a direct match:** Delivered as an MCP tool with no UI; Claude
  Code-only.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-18.

### [session-graph](https://github.com/robertoshimizu/session-graph)

Python tool (94 stars) that converts Claude Code, DeepSeek, Grok, and Warp
sessions into a W3C-ontology RDF knowledge graph with SPARQL queries and
Wikidata entity linking.

- **Why it's adjacent:** Multi-assistant session analysis with a unique
  knowledge-graph angle.
- **Why it is not a direct match:** Batch transformation pipeline rather than an
  interactive session browser.
- **Maintenance signal:** GitHub repo shows pushes on 2026-02-25.

### [ccpeek](https://github.com/ahmedelgabri/ccpeek)

Go local web app (localhost:3000, 28 stars) that indexes Claude Code
conversations into SQLite and exposes a rich browser view covering sessions,
plans, todos, shell snapshots, file history, paste cache, usage data, memories,
and commands.

- **Why it's adjacent:** Active local session indexer sharing the SQLite + FTS
  approach.
- **Why it is not a direct match:** Claude Code-only; broader scope than session
  browsing (shell history, secrets scanning, todos).
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-17.

### [Entropic](https://github.com/Dimension-AI-Technologies/Entropic)

GUI and CLI environment for Claude Code, Codex, and Gemini with repository
discovery, TODO tracking, chat history, and Git history views.

- **Why it's adjacent:** Overlaps with cross-repo agent history and workspace
  visibility.
- **Why it is not a direct match:** Broader "agent workspace" product, the
  repository is currently a fork rather than a clean standalone viewer product,
  and adoption remains low (9 stars).
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-04.

## Projects removed from the main list

These appeared in earlier versions of this document but are no longer in the
main shortlist because they are either less directly comparable or no longer
show clear ongoing maintenance.

### Copilot Chronicle

Interesting first-party feature direction inside GitHub Copilot CLI, but not a
standalone open-source project page comparable to Sessions Chronicle.

### [claude-sessions](https://github.com/iannuttall/claude-sessions)

Useful slash-command pack for documenting Claude Code sessions, but it solves
session capture/documentation rather than browsing and analysis. No push
activity since January 2025.

### [Codex Sessions Manager](https://github.com/coramba/codex-sessions-manager)

A small single-assistant Codex browser with a useful narrow scope, but current
repository activity appears limited compared with the actively maintained tools
listed above.
