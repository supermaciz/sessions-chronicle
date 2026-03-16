# Similar Projects

Last reviewed: 2026-03-16  

A shortlist of projects in the same product space as Sessions Chronicle:
tools that index, browse, search, or analyze local AI assistant sessions.

## Selection criteria

Projects listed here should be:

- meaningfully similar to Sessions Chronicle's core problem space
- publicly available
- actively maintained as of 2026-03-16
- not archived

For this document, "actively maintained" means there was visible recent repository
activity on GitHub such as pushes or release work in March 2026.

## Closest matches

### [AgentsView](https://github.com/wesm/agentsview)

Local-first desktop and web application written primarily in Go. It indexes AI
assistant sessions into SQLite, exposes full-text search, analytics, live
updates, and export/publish flows. Supports Claude Code, Codex, OpenCode, and
several other assistants.

- **Why it's similar:** Strong overlap on local indexing, session browsing,
  search, analytics, and multi-assistant support.
- **How it differs from Sessions Chronicle:** Browser/webview-first product with
  a local server architecture rather than a native GNOME/GTK app.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-16 and active
  release/install documentation.

### [Agent Sessions](https://github.com/jazzyalex/agent-sessions)

Native macOS app in Swift for browsing, searching, and resuming AI assistant
sessions across Codex CLI, Claude Code, Gemini CLI, GitHub Copilot CLI, Factory
Droid, and OpenCode. Includes a live "Agent Cockpit" view for active sessions,
cross-session search, transcript reading, and resume flows.

- **Why it's similar:** Very close product shape: local-first desktop app,
  multi-assistant session browser, transcript reading, search, and resume.
- **How it differs from Sessions Chronicle:** macOS-only, with a stronger focus
  on live-session monitoring and iTerm2 integration.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-15 and current
  release notes for version 3.2.

### [Claude Code History Viewer](https://github.com/jhlee0409/claude-code-history-viewer)

Desktop app and optional headless server built with Rust, Tauri, and React.
Supports Claude Code, Codex CLI, and OpenCode. Focuses on unified browsing,
global search, analytics, live file watching, archive management, and a
browser-accessible server mode.

- **Why it's similar:** Local-first session viewer with Rust + SQLite-style
  architecture, multi-assistant support, transcript search, and analytics.
- **How it differs from Sessions Chronicle:** Cross-platform Tauri stack and
  optional Web UI/server deployment rather than GTK/Libadwaita packaging.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-16 and a recent
  v1.6.0 feature set in the README.

### [cass (coding_agent_session_search)](https://github.com/Dicklesworthstone/coding_agent_session_search)

Rust CLI/TUI that indexes local coding-agent session history across 11+
providers. Uses a custom search stack with lexical search, optional local
semantic search, JSON/robot output modes, and automation-oriented workflows.

- **Why it's similar:** Same underlying problem: unify fragmented local AI
  assistant history into one searchable system.
- **How it differs from Sessions Chronicle:** Much more CLI/TUI and
  automation-agent oriented; broader provider coverage; less focused on native
  desktop reading UX.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-15 and current
  install/release documentation.

### [Claude Code Viewer](https://github.com/d-kimuson/claude-code-viewer)

Web application and local server focused on Claude Code, with full session
history browsing, real-time monitoring, search, project creation, scheduling,
Git diff review, terminal integration, and remote-friendly access.

- **Why it's similar:** Strong overlap on session browsing, search, transcript
  inspection, and local-first usage of Claude Code history.
- **How it differs from Sessions Chronicle:** Primarily a Claude Code client and
  control surface, not a cross-assistant desktop browser first.
- **Maintenance signal:** GitHub repo shows pushes on 2026-03-03 and an active
  release/CI setup.

## Adjacent projects

These are worth tracking, but they are not as directly comparable to Sessions
Chronicle's current scope.

### [Entropic](https://github.com/Dimension-AI-Technologies/Entropic)

GUI and CLI environment for Claude Code, Codex, and Gemini with repository
discovery, TODO tracking, chat history, and Git history views.

- **Why it's adjacent:** Overlaps with cross-repo agent history and workspace
  visibility.
- **Why it is not a direct match:** Broader "agent workspace" product, and the
  referenced repository is currently a fork rather than a clean standalone
  viewer product.
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
session capture/documentation rather than browsing and analysis. Repository
metadata also does not show recent push activity relative to the projects above.

### [Codex Sessions Manager](https://github.com/coramba/codex-sessions-manager)

A small single-assistant Codex browser with a useful narrow scope, but current
repository activity appears limited compared with the actively maintained tools
listed above.
