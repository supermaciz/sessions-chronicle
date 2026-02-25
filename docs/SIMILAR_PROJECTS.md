# Similar Projects

A list of projects in the same space as Sessions Chronicle — tools for browsing,
searching, and analyzing AI coding agent sessions.

## [AgentsView](https://github.com/wesm/agentsview)

Local web application (Go) that indexes and visualizes AI coding agent sessions.
Supports Claude Code, Codex, Copilot CLI, Gemini CLI, and OpenCode.
Stores everything in a local SQLite database with full-text search, provides an
analytics dashboard with activity heatmaps, and offers live updates via SSE.
Vim-style keyboard navigation and export to HTML/GitHub Gist.

Supersedes the earlier [agent-session-viewer](https://github.com/wesm/agent-session-viewer)
(now archived).

**Why it's interesting:** Very feature-complete web-based approach with strong
search and analytics. Good reference for what power users expect from session
browsing tools.

## [Agent Sessions](https://github.com/jazzyalex/agent-sessions)

Native macOS app (Swift) that serves as a unified browser for AI coding agent
sessions.
Supports Codex CLI, Claude Code, Gemini CLI, GitHub Copilot CLI, Factory Droid,
and OpenCode.
Features unified cross-session search, token usage and rate limit tracking, and
the ability to resume sessions directly in the terminal.

**Why it's interesting:** Local-first, no telemetry, read-only access to session
directories. Shows what a polished native desktop experience looks like for this
problem space — closest in spirit to Sessions Chronicle but macOS-only.

## Copilot Chronicle (GitHub Copilot CLI)

A feature built into [GitHub Copilot CLI](https://github.com/github/copilot-cli)
that indexes all messages and turns locally in SQLite, letting you search your
coding history.
Announced by [Scott Hanselman](https://x.com/shanselman/status/2024527670479114326)
([LinkedIn post](https://www.linkedin.com/posts/shanselman_introducing-copilot-chronicle-this-new-feature-activity-7430293937455656960-KLqS)).
Available via `/experimental on` and `/reindex` commands
([issue #1581](https://github.com/github/copilot-cli/issues/1581)).

**Why it's interesting:** First-party integration from GitHub directly inside
Copilot CLI. The "talk to your coding history" angle — learning how you work and
how you can improve — is a compelling use case that goes beyond simple session
browsing.

## [Claude Code History Viewer](https://github.com/jhlee0409/claude-code-history-viewer)

Cross-platform desktop app (macOS, Windows, Linux) built with Rust, Tauri v2,
React 19, and TypeScript. Monitors Claude Code session files in real time,
stores data in SQLite, and surfaces an analytics dashboard with token usage
statistics. Runs fully offline with no external network calls.

**Why it's interesting:** The closest technical cousin to Sessions Chronicle —
both use Rust and SQLite for local-first storage. The key difference is the UI
stack: Tauri + React instead of GNOME / Relm4. A good reference for cross-platform
desktop tradeoffs in this space.

## [cass (coding\_agent\_session\_search)](https://github.com/Dicklesworthstone/coding_agent_session_search)

CLI / TUI tool written in Rust that indexes sessions from 11+ AI coding agents.
Uses Tantivy for BM25 full-text search combined with FastEmbed (MiniLM) for
local semantic embeddings, delivering hybrid search results in under 60 ms.
Approximately 1 700 commits and no network dependency at search time.

**Why it's interesting:** The most technically ambitious approach in this space.
Combining lexical and semantic search entirely offline, with broad provider
support, makes it a strong reference for what advanced retrieval could look like
in a future Sessions Chronicle search backend.

## [Claude Code Viewer](https://github.com/d-kimuson/claude-code-viewer)

Local web application (Node.js + React) that runs on port 3400 and exposes
Claude Code sessions in the browser. Notable features include a WebSocket-based
terminal integration for "remote-friendly" use, a Git diff viewer, and the
ability to schedule messages via cron. Designed around zero data loss.

**Why it's interesting:** The richest feature set among web-based viewers —
integrated terminal, browser preview, and session scheduling push it well beyond
simple browsing. Useful reference for evaluating which "power user" features are
worth porting to a native desktop context.

## [claude-sessions](https://github.com/iannuttall/claude-sessions)

A set of slash commands (`/project:session-start`, `/project:session-update`,
`/project:session-end`, `/project:session-summary`) for Claude Code that write
structured Markdown notes during a session. Focused on capturing decisions,
context, and progress for cross-session continuity rather than browsing past
history.

**Why it's interesting:** An orthogonal angle: instead of navigating completed
sessions, it helps you *document* them as they happen. Highlights the difference
between session *capture* and session *retrieval* — two complementary problems
in the same space.

## [Codex Sessions Manager](https://github.com/coramba/codex-sessions-manager)

Minimal single-page application (Vue 3 + Vuetify 3) dedicated to OpenAI Codex
sessions. Lists sessions grouped by project, shows per-session stats, and
generates ready-to-copy resume commands.

**Why it's interesting:** A concise, single-tool example of the viewer pattern
before adding multi-agent complexity. Useful as a baseline illustration of the
core problem — pick up a past session quickly — in its simplest form.
