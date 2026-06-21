# Similar Projects

Last reviewed: 2026-06-21

A shortlist of projects in the same product space as Sessions Chronicle:
tools that index, browse, search, analyze, resume, or operationalize local AI
assistant sessions.

## Selection criteria

Projects listed here should be:

- meaningfully similar to Sessions Chronicle's core problem space
- publicly available (open source, source-available, or public marketplace)
- visibly active or still strategically relevant as of 2026-06-21
- not archived

For this document, "actively maintained" means there was visible recent
repository or marketplace activity on GitHub, GitHub Releases, a product site,
or a public marketplace listing during May or June 2026.

Stats below are a snapshot from GitHub, project websites, or marketplace pages
on 2026-06-21. They are directional, not permanent product truth.

## Closest matches

### [AgentsView](https://github.com/kenn-io/agentsview)

Local-first desktop and web application written primarily in Go. It indexes AI
assistant sessions into SQLite with full-text search, analytics, insights, token
use statistics, live workflows, and broad assistant coverage. Supports Claude
Code, Codex, and more than 20 other agents.

- **Why it's similar:** Strong overlap on local indexing, cross-assistant
  session browsing, search, analytics, and usage insight.
- **How it differs from Sessions Chronicle:** Browser/webview and local-server
  oriented rather than native GNOME/GTK. Broader assistant coverage and faster
  feature expansion.
- **Maintenance signal:** 3,034 stars, v0.34.0 released on 2026-06-20, pushed
  on 2026-06-21.
- **Product implication:** This is now the strongest direct benchmark for
  breadth. Sessions Chronicle should not try to win by matching provider count
  alone.

### [Claude Code History Viewer](https://github.com/jhlee0409/claude-code-history-viewer)

Desktop app and optional headless server built with Tauri, React, and Rust.
It positions itself as a unified history viewer for AI coding assistants and
supports Claude Code, Gemini CLI, Antigravity, Codex CLI, Cline, Cursor, Aider,
OpenCode, ForgeCode, and CodeBuddy Code. It emphasizes offline browsing,
search, analytics, live watching, archive management, and server mode.

- **Why it's similar:** Local-first session viewer with Rust-backed desktop
  packaging, multi-assistant support, transcript search, and analytics.
- **How it differs from Sessions Chronicle:** Cross-platform Tauri stack,
  optional server/headless deployment, and broader assistant coverage.
- **Maintenance signal:** 1,653 stars, v1.15.0 released on 2026-06-20, pushed
  on 2026-06-20.
- **Product implication:** This is the closest cross-platform "default viewer"
  threat. Sessions Chronicle's defensible angle is native Linux/GNOME quality,
  not generic feature parity.

### [Agent Sessions](https://github.com/jazzyalex/agent-sessions)

Native macOS app in Swift for browsing, searching, analyzing, and resuming AI
assistant session history. Supports Codex, Claude Code, OpenCode, Cursor Agent,
Hermes, OpenClaw, Copilot CLI, and more. Includes usage tracking and an active
session-management posture.

- **Why it's similar:** Very close product shape: local-first desktop app,
  multi-assistant session browser, search, analysis, and resume.
- **How it differs from Sessions Chronicle:** macOS-only with a stronger
  active-session and Apple-platform integration story.
- **Maintenance signal:** 651 stars, v3.9.3 released on 2026-06-13, pushed on
  2026-06-21.
- **Product implication:** Validates the native desktop category. Sessions
  Chronicle can credibly own the Linux/GNOME side if the UX is polished enough.

### [cass / coding_agent_session_search](https://github.com/Dicklesworthstone/coding_agent_session_search)

Rust CLI/TUI that indexes local coding-agent session history across 11+
providers including Codex, Claude, Gemini, Cursor, and Aider. It focuses on fast
search, TUI/CLI workflows, optional structured output, and automation-friendly
usage.

- **Why it's similar:** Same underlying problem: unify fragmented local AI
  assistant history into one searchable system.
- **How it differs from Sessions Chronicle:** CLI/TUI and automation-first;
  less focused on calm native reading, transcript ergonomics, and GNOME desktop
  workflows.
- **Maintenance signal:** 918 stars, v0.6.16 released on 2026-06-15, pushed on
  2026-06-20.
- **Product implication:** cass is the benchmark for power-user search.
  Sessions Chronicle should win on structured reading, review, and visual
  comprehension.

### [Claude Code and Codex Assist](https://marketplace.visualstudio.com/items?itemName=agsoft.claude-history-viewer)

VS Code extension for Claude Code, Codex CLI, and OpenCode. The marketplace
listing describes unified chat history, file diffs, full-text search, token
usage/cost tracking, pin/rename/delete actions, and resume flows inside the
editor.

- **Why it's similar:** Directly targets browsing, search, diffs, usage
  analytics, and resume for the same local AI assistant histories.
- **How it differs from Sessions Chronicle:** IDE extension rather than
  standalone desktop app; includes optional Pro features and lives inside VS
  Code workflows.
- **Maintenance signal:** 7,846 installs and 12 ratings on the Visual Studio
  Marketplace snapshot reviewed on 2026-06-21.
- **Product implication:** This is a serious distribution threat: users may
  prefer history inside the editor where they already review code.

### [Nimbalyst](https://github.com/nimbalyst/nimbalyst)

Local interactive visual editor and session manager for building with Codex,
Claude Code, OpenCode alpha, and Copilot alpha. It combines sessions, tasks,
visual editors, file collaboration, and diff review into a broader agent
workspace.

- **Why it's similar:** It includes session management and review around the
  same terminal-first AI assistant ecosystem.
- **How it differs from Sessions Chronicle:** Active workspace and visual
  editor rather than passive session archive; Electron/React/Monaco stack
  rather than GTK/Libadwaita.
- **Maintenance signal:** 890 stars, v0.65.4 released on 2026-06-15, pushed on
  2026-06-19.
- **Product implication:** This is a category-shift threat. It suggests user
  demand may move from "history viewer" toward "agent workbench with history
  included."

### [Claude Code Viewer](https://github.com/d-kimuson/claude-code-viewer)

Web application and local server in TypeScript/Vue focused on Claude Code. It
provides session history browsing, real-time monitoring, search, project
creation, scheduling, Git diff review, terminal integration, and remote-friendly
access.

- **Why it's similar:** Strong overlap on session browsing, search, transcript
  inspection, and local-first usage of Claude Code history.
- **How it differs from Sessions Chronicle:** Primarily a Claude Code client
  and control surface, not a cross-assistant native Linux browser first.
- **Maintenance signal:** 1,224 stars, v0.7.5 released on 2026-05-10, pushed
  on 2026-05-10.
- **Product implication:** Shows that single-assistant tools can still gain
  adoption when they own active workflow control.

### [CCManager](https://github.com/kbwo/ccmanager)

CLI session manager for Claude Code, Gemini CLI, Codex CLI, Cursor Agent,
Copilot CLI, Cline CLI, OpenCode, and Kimi CLI. It focuses on managing sessions
across tools and projects rather than rendering historical transcripts as a
desktop reading experience.

- **Why it's similar:** Multi-assistant session management for the same coding
  agent ecosystem.
- **How it differs from Sessions Chronicle:** CLI manager, operational control,
  and worktree/session workflows rather than native transcript browsing.
- **Maintenance signal:** 1,158 stars, v4.1.20 released on 2026-06-19, pushed
  on 2026-06-19.
- **Product implication:** Reinforces that session continuity and operational
  control are high-demand adjacent jobs.

## Adjacent projects

These are worth tracking, but they are not as directly comparable to Sessions
Chronicle's current scope.

### [CC Switch](https://github.com/farion1231/cc-switch)

Cross-platform desktop all-in-one assistant for Claude Code, Codex, OpenCode,
OpenClaw, Gemini CLI, Hermes Agent, and provider/MCP/skills management.

- **Why it's adjacent:** Includes session management in a broad multi-assistant
  control plane.
- **Why it is not a direct match:** Provider switching, MCP sync, skills, WSL
  support, and operational management are the core surface; history browsing is
  one feature inside a much larger product.
- **Maintenance signal:** 105,258 stars, v3.16.3 released on 2026-06-14, pushed
  on 2026-06-19.
- **Product implication:** Massive attention signal for "control plane" tools.
  Sessions Chronicle should borrow lessons from the workflow story, not chase
  the whole surface area.

### [claude-tap](https://github.com/liaohch3/claude-tap)

Python local proxy and trace viewer for coding-agent API traffic from Claude
Code, Codex CLI, Gemini CLI, Cursor CLI, OpenCode, Kimi/Kimi Code, Pi, and
Hermes. It shows real API traffic, agent context, tool schemas, tool calls,
streaming responses, token usage, request diffs, and exports traces without
uploading private data to a hosted dashboard.

- **Why it's adjacent:** Strong overlap with the forensics and observability
  side of Sessions Chronicle's future direction.
- **Why it is not a direct match:** Trace/proxy viewer rather than historical
  local transcript browser. It inspects API-level runs more than it organizes a
  user's durable project/session history.
- **Maintenance signal:** 1,883 stars, v0.1.120 released on 2026-06-18, pushed
  on 2026-06-20.
- **Product implication:** This raises the bar for any "what did the agent do?"
  feature. Sessions Chronicle should focus on work-history comprehension, while
  claude-tap is stronger for raw trace debugging.

### [SpecStory](https://github.com/specstoryai/getspecstory)

Go CLI and extension ecosystem that captures and indexes AI coding
conversations locally from IDEs and CLI tools, then can sync conversations to
the cloud and process histories into reusable skills.

- **Why it's adjacent:** Multi-assistant local session capture, search, and
  reuse of history.
- **Why it is not a direct match:** IDE extension plus CLI delivery, optional
  cloud sync, and "history into skills" workflow rather than native desktop
  reading.
- **Maintenance signal:** 1,252 stars, v1.13.0 released on 2026-05-18, pushed
  on 2026-06-19.

### [cli-continues](https://github.com/yigitkonur/cli-continues)

TypeScript CLI for resuming any AI coding session in another tool, including
Claude Code, Copilot, Gemini, Codex, and Cursor.

- **Why it's adjacent:** Attacks session continuity and cross-tool handoff.
- **Why it is not a direct match:** Not a history browser or analytics surface.
- **Maintenance signal:** 1,284 stars, pushed on 2026-05-07.

### [claude-historian-mcp](https://github.com/Vvkmnn/claude-historian-mcp)

TypeScript MCP server for searching and retrieving Claude Code conversation
history from within Claude itself.

- **Why it's adjacent:** Solves local session search and retrieval by feeding
  history back into the AI assistant.
- **Why it is not a direct match:** Delivered as an MCP server with no human
  reading UI; Claude Code-only.
- **Maintenance signal:** 178 stars, pushed on 2026-06-15.
- **Product implication:** This is a threat to "search old history manually"
  use cases, but not to human review, forensics, or visual comprehension.

### [Claudoscope](https://github.com/cordwainersmith/Claudoscope)

Native macOS app for Claude Code and Cowork sessions, with real-time dashboard,
analytics, conversation history, security hardening, secrets detection, and
project insights.

- **Why it's adjacent:** Native desktop session browsing and analytics with a
  local-first, privacy-first angle.
- **Why it is not a direct match:** Claude-focused, macOS-only, and real-time
  dashboard oriented.
- **Maintenance signal:** 197 stars, v0.8.0 released on 2026-06-11, pushed on
  2026-06-17.

### [claude-view](https://github.com/tombelieber/claude-view)

Real-time monitoring dashboard for Claude Code sessions built with Rust and
React. Provides cost tracking, search, sub-agent visibility, reports, heatmaps,
and live monitoring.

- **Why it's adjacent:** Local-first Rust-based session search and analytics.
- **Why it is not a direct match:** Claude Code-only and live monitoring first,
  not a multi-assistant native desktop history browser.
- **Maintenance signal:** 87 stars, v0.44.0 released on 2026-06-03, pushed on
  2026-06-15.

### [ccpeek](https://github.com/ahmedelgabri/ccpeek)

Go local web app for exploring Claude Code history locally.

- **Why it's adjacent:** Active local session indexer and browser.
- **Why it is not a direct match:** Claude Code-only and local web app rather
  than native GTK/Libadwaita desktop.
- **Maintenance signal:** 30 stars, v1.10.0 released on 2026-03-27, pushed on
  2026-06-13.

### [Chronicle / claude-history-manager](https://github.com/josephyaduvanshi/claude-history-manager)

Native macOS browser for Claude Code session history with search, pins, tags,
resume, and local transcripts.

- **Why it's adjacent:** Native desktop session browser with local storage,
  search, and resume.
- **Why it is not a direct match:** Claude Code-only and macOS-only.
- **Maintenance signal:** 43 stars, v0.3.0 released on 2026-04-26, pushed on
  2026-04-26.

### [CodMate](https://github.com/loocor/codmate)

Native macOS SwiftUI app for managing CLI AI sessions across Codex, Claude
Code, and Gemini CLI, with browse/search/resume and Git review workflows.

- **Why it's adjacent:** Multi-assistant native desktop app with session
  browse/search/resume.
- **Why it is not a direct match:** macOS-only, narrower assistant coverage,
  and appears less active than the leading tools.
- **Maintenance signal:** 666 stars, v0.5.9 released on 2026-01-05, pushed on
  2026-03-31.

### [session-graph](https://github.com/robertoshimizu/session-graph)

Python tool that converts scattered AI coding sessions into a queryable
knowledge graph using RDF/SPARQL and entity linking.

- **Why it's adjacent:** Turns session history into structured analysis data.
- **Why it is not a direct match:** Batch transformation pipeline rather than an
  interactive desktop session browser.
- **Maintenance signal:** 106 stars, v0.6.0 released on 2026-02-21, pushed on
  2026-02-25.

### [Entropic](https://github.com/Dimension-AI-Technologies/Entropic)

GUI and CLI environment for Claude Code, Codex, and Gemini with repository
discovery, TODO tracking, chat history, and Git history views.

- **Why it's adjacent:** Overlaps with cross-repo agent history and workspace
  visibility.
- **Why it is not a direct match:** Broader agent workspace product with low
  adoption and less evidence of focused session-browser momentum.
- **Maintenance signal:** 9 stars, pushed on 2026-04-17.

### [opcode](https://github.com/winfunc/opcode)

Tauri/TypeScript GUI app and toolkit for Claude Code. It supports custom
agents, interactive session management, and secure background agents.

- **Why it's adjacent:** It shows large developer interest in GUI/control-plane
  surfaces around Claude Code.
- **Why it is not a direct match:** Claude Code-focused control plane rather
  than multi-assistant local history browser.
- **Maintenance signal:** 22,072 stars, v0.2.0 released on 2025-08-31, last
  pushed on 2025-10-16. High attention, but not a current active-maintenance
  signal by this document's criteria.

## Projects removed from the main list

These appeared in earlier versions of this document but are no longer in the
main shortlist because they are either less directly comparable, lower signal,
or no longer show clear current momentum.

### Copilot Chronicle

Interesting first-party feature direction inside GitHub Copilot CLI, but not a
standalone open-source project page comparable to Sessions Chronicle.

### [claude-sessions](https://github.com/iannuttall/claude-sessions)

Useful slash-command pack for documenting Claude Code sessions, but it solves
session capture/documentation rather than browsing and analysis. No clear
current maintenance signal.

### [Codex Sessions Manager](https://github.com/coramba/codex-sessions-manager)

A small single-assistant Codex browser with a useful narrow scope, but current
repository activity appears limited compared with the actively maintained tools
listed above.

## Market read

The product space has become much more crowded since the March and April
assessments. The key shift is that "history viewer" is no longer a sufficient
category by itself. Competitors now cluster into four shapes:

- **Broad local viewers:** AgentsView and Claude Code History Viewer.
- **Native platform viewers:** Agent Sessions on macOS and Sessions Chronicle
  on GNOME/Linux.
- **Operational control planes:** CC Switch, Nimbalyst, CCManager, CodMate.
- **Trace/observability viewers:** claude-tap and similar proxy/logging tools.
- **Context feeders:** claude-historian-mcp, SpecStory, and other tools that
  turn history into assistant-readable context.

Sessions Chronicle remains meaningfully differentiated only if it leans into
what those clusters do not fully cover: a native Linux/GNOME, local-first,
human-readable work memory for understanding what AI assistants did across
projects.
