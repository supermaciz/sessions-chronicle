# Similar Projects

Last reviewed: 2026-11-21

A shortlist of projects in the same product space as Sessions Chronicle:
tools that index, browse, search, analyze, resume, or operationalize local AI
assistant sessions.

## Selection criteria

Projects listed here should be:

- meaningfully similar to Sessions Chronicle's core problem space
- publicly available (open source, source-available, or public marketplace)
- visibly active or still strategically relevant as of 2026-11-21
- not archived
- with a commit or release in the six months before the review (projects
  that fall short move to "Projects removed from the main list")

For this document, "actively maintained" means there was visible recent
repository or marketplace activity on GitHub, GitHub Releases, a product site,
or a public marketplace listing during October or November 2026. Entries that
fall short of this are kept only when they are still strategically relevant,
and their maintenance signal says so.

Stats below are a snapshot from GitHub, project websites, or marketplace pages
on 2026-11-21. Where an entry changed noticeably since the previous snapshot
(2026-06-21, or 2026-07-17 for the visualization-first section), the earlier
figure is given in parentheses. They are directional, not permanent product
truth.

## Closest matches

### [AgentsView](https://github.com/kenn-io/agentsview)

Local-first desktop and web application written primarily in Go. It indexes AI
assistant sessions into SQLite with full-text search, analytics, insights, token
use statistics, live workflows, and broad assistant coverage. Supports Claude
Code, Codex, and more than 50 other agents.

- **Why it's similar:** Strong overlap on local indexing, cross-assistant
  session browsing, search, analytics, and usage insight.
- **How it differs from Sessions Chronicle:** Browser/webview and local-server
  oriented rather than native GNOME/GTK. Broader assistant coverage and faster
  feature expansion.
- **Maintenance signal:** 6,491 stars, v0.47.0 released on 2026-11-09, pushed on
  2026-11-21 (was 3,034 stars, v0.34.0, and "more than 20 other agents" in
  June).
- **Product implication:** This is now the strongest direct benchmark for
  breadth. Sessions Chronicle should not try to win by matching provider count
  alone.

### [Omnilogue (formerly Claude Code History Viewer)](https://github.com/jhlee0409/omnilogue)

Desktop app and optional headless server built with Tauri, React, and Rust.
Renamed from Claude Code History Viewer (`jhlee0409/claude-code-history-viewer`
now redirects here); v2.0.0 shipped on 2026-11-19. It positions itself as an
offline history viewer for AI coding assistants and now lists 33 history
sources, including Claude Code, Codex CLI, GitHub Copilot, Gemini CLI, Cursor,
OpenCode, Aider, Cline, Goose, Kimi, Mistral Vibe, Qwen Code, and Zed. It
emphasizes offline browsing, search, conversation branches, tool-call and
file-edit inspection, token and cost analytics, and an optional server mode.
Documents a Flatpak for Linux.

- **Why it's similar:** Local-first session viewer with Rust-backed desktop
  packaging, multi-assistant support, transcript search, and analytics.
- **How it differs from Sessions Chronicle:** Cross-platform Tauri stack,
  optional server/headless deployment, and broader assistant coverage.
- **Maintenance signal:** 2,333 stars, v2.0.0 released on 2026-11-19, pushed on
  2026-11-20 (was 1,653 stars and v1.15.0 in June).
- **Product implication:** This is the closest cross-platform "default viewer"
  threat. Sessions Chronicle's defensible angle is native Linux/GNOME quality,
  not generic feature parity.

### [Agent Sessions](https://github.com/jazzyalex/agent-sessions)

Native macOS app in Swift for browsing, searching, analyzing, and resuming AI
assistant session history. Supports Codex, Claude Code, OpenCode, Cursor Agent,
Antigravity, Hermes, OpenClaw, Copilot CLI, and more. Includes usage tracking
and an active session-management posture.

- **Why it's similar:** Very close product shape: local-first desktop app,
  multi-assistant session browser, search, analysis, and resume.
- **How it differs from Sessions Chronicle:** macOS-only with a stronger
  active-session and Apple-platform integration story.
- **Maintenance signal:** 953 stars, v6.6 released on 2026-11-18, pushed on
  2026-11-19 (was 651 stars and v3.9.3 in June).
- **Product implication:** Validates the native desktop category. Sessions
  Chronicle can credibly own the Linux/GNOME side if the UX is polished enough.

### [cass / coding_agent_session_search](https://github.com/Dicklesworthstone/coding_agent_session_search)

Rust CLI/TUI that indexes local coding-agent session history across 30+
providers including Codex, Claude, Gemini, Cursor, and Aider. It focuses on fast
search, TUI/CLI workflows, optional structured output, and automation-friendly
usage.

- **Why it's similar:** Same underlying problem: unify fragmented local AI
  assistant history into one searchable system.
- **How it differs from Sessions Chronicle:** CLI/TUI and automation-first;
  less focused on calm native reading, transcript ergonomics, and GNOME desktop
  workflows.
- **Maintenance signal:** 1,221 stars, v0.11.0 released on 2026-11-05, pushed on
  2026-11-21. Now advertises 30+ providers (was 11+).
- **Product implication:** cass is the benchmark for power-user search.
  Sessions Chronicle should win on structured reading, review, and visual
  comprehension.

### [Claude Code and Codex Assist](https://marketplace.visualstudio.com/items?itemName=agsoft.claude-history-viewer)

VS Code extension for Claude Code, Codex, OpenCode, Grok, and Copilot. The
marketplace listing describes unified chat history, file diffs, full-text
search, token usage/cost/quota tracking, pin/rename/delete actions, resume
flows inside the editor, and an MCP server that gives agents memory of past
chats.

- **Why it's similar:** Directly targets browsing, search, diffs, usage
  analytics, and resume for the same local AI assistant histories.
- **How it differs from Sessions Chronicle:** IDE extension rather than
  standalone desktop app; includes optional Pro features and lives inside VS
  Code workflows.
- **Maintenance signal:** 12,077 installs and 15 ratings (3.8 average), v0.8.3
  updated on 2026-11-20 on the Visual Studio Marketplace snapshot reviewed on
  2026-11-21 (was 7,846 installs in June).
- **Product implication:** This is a serious distribution threat: users may
  prefer history inside the editor where they already review code.

### [Nimbalyst](https://github.com/nimbalyst/nimbalyst)

Open-source visual workspace for Claude Code, Codex, and OpenCode. It runs
several agents in parallel and combines sessions, tasks, visual editors
(markdown, mockups, diagrams), and diff review into a broader agent workspace.
Desktop app for macOS, Windows, and Linux, with an iOS/Android companion.

- **Why it's similar:** It includes session management and review around the
  same terminal-first AI assistant ecosystem.
- **How it differs from Sessions Chronicle:** Active workspace and visual
  editor rather than passive session archive; Electron/React/Monaco stack
  rather than GTK/Libadwaita.
- **Maintenance signal:** 2,095 stars, v0.83.0 released on 2026-11-10, pushed on
  2026-11-21 (was 890 stars in June).
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
- **Maintenance signal:** 1,307 stars, v0.8.2 released on 2026-08-18, last
  commit on 2026-08-18. No activity in the last three months; still listed for
  adoption, but momentum has slowed.
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
- **Maintenance signal:** 1,280 stars, v4.4.11 released on 2026-10-27, pushed on
  2026-10-27.
- **Product implication:** Reinforces that session continuity and operational
  control are high-demand adjacent jobs.

### [Obelisk](https://github.com/tommy0103/obelisk)

Indexes local coding-agent history into one SQLite database and serves it to
two audiences: a CLI plus agent skill that lets coding agents query their own
history with JS/SQL, and an Electron desktop app for humans to browse sessions,
manage memories, view usage stats, and read weekly recap cards. Covers Claude
Code, Codex, GitHub Copilot, DeepSeek Harness, Devin, Hermes Agent, Kiro, Kimi
Code, OMP, OpenCode, Pi, and ZCode, including subagent conversations.

- **Why it's similar:** Same core architecture as Sessions Chronicle — a local
  SQLite index over many assistants' session files, kept fresh by watching
  provider directories, with a desktop browsing UI.
- **How it differs from Sessions Chronicle:** Agent-first: the headline use is
  the agent querying its own past, with the desktop app as the human view.
  Electron rather than GTK/Libadwaita; AGPL-3.0.
- **Maintenance signal:** 957 stars, v0.2.5 released on 2026-10-08, pushed on
  2026-11-18. Created in May 2026.
- **Product implication:** The "one index, two audiences" shape is worth
  noting: the same SQLite index Sessions Chronicle already builds could serve
  an agent-facing CLI or MCP surface without changing the human UI.

### [Sessions Viewer](https://github.com/jerrywu001/cc-sessions-viewer)

Tauri 2 desktop app (Rust, Vue) for Windows, macOS, and Linux that reads,
searches, exports, and manages local transcripts from Claude Code, Codex, Grok
Build, Kimi Code, Pi, Antigravity CLI, and OpenCode. Adds a built-in chat to
start or resume Claude Code and Codex sessions, one-click resume in external
terminals, token/cost statistics, and a tool-management view for skills, MCP
servers, hooks, and instruction files.

- **Why it's similar:** Native-packaged, local-first, multi-assistant
  transcript browser with search, export, and resume — and Kimi Code and
  OpenCode overlap ours directly.
- **How it differs from Sessions Chronicle:** Cross-platform webview stack, and
  it is drifting toward an agent workbench (built-in chat, shell tabs, tool
  management). Its global search matches titles and user prompts only, not
  assistant answers or tool output.
- **Maintenance signal:** 420 stars, v0.6.3 released on 2026-11-02, pushed on
  2026-11-02. Created in May 2026.
- **Product implication:** Another sign that viewers are absorbing control
  features. Full-text search over assistant output and tool results remains a
  real differentiator.

## Visualization-first projects

These projects do not compete on browsing, search, or resume. They compete on
**comprehension**: making a session legible at a glance through an original
visual form rather than a transcript list. They are tracked separately because
they are the clearest source of design ideas for Sessions Chronicle, and because
this is the one cluster where the field is still thin.

### [Mindwalk](https://github.com/cosmtrek/mindwalk)

Replays coding-agent sessions as light moving across a 3D map of the codebase.
The repository is drawn as a night map; files are colored by the deepest state
they reached during the session (moss green for seen, moon white for read, warm
amber for edited, dark for unvisited), and the playback histogram follows the
same cool/warm spectrum so observation stays cool while mutations glow. Internally
split into a Trace layer that normalizes Claude Code and Codex events through
per-assistant adapters, and a Citymap layer that lays out the repository
deterministically. Go server serving a React/Three.js frontend, MIT.

- **Why it's similar:** Same input (local session logs, multi-assistant,
  normalized through adapters) and the same goal of understanding what an AI
  assistant actually did across a project.
- **How it differs from Sessions Chronicle:** Spatial replay of a single session
  rather than a durable, searchable archive across sessions. No browsing,
  search, or resume story.
- **Maintenance signal:** 1,401 stars, v0.5.0 released on 2026-08-07, last
  commit on 2026-08-10. Quiet since August.
- **Product implication:** The strongest proof that visual comprehension is a
  category of its own, and the benchmark to measure any Sessions Chronicle
  visualization against. Its Trace/Citymap split also validates separating
  normalization from rendering.

### [Agent Flow](https://github.com/patoles/agent-flow)

Real-time node graph of agent orchestration: tool calls, branching, and subagent
coordination on an interactive pan/zoom canvas, with companion timeline and
transcript panels and tabs for tracking several sessions at once. Detects Claude
Code and Codex sessions concurrently and shows them side by side. Architecture is
notable: Claude Code HTTP hooks feed an event relay server that streams to the
browser over SSE. Ships as a Next.js web app plus a VS Code extension.

- **Why it's similar:** Targets the same "what did the agent do, and why"
  question
  over the same local Claude Code and Codex sessions.
- **How it differs from Sessions Chronicle:** Live-first and hook-driven rather
  than reading sessions at rest from disk; graph topology rather than history.
- **Maintenance signal:** 1,802 stars, v0.9.6 released on 2026-11-20, pushed on
  2026-11-20.
- **Product implication:** Subagent structure is clearly worth showing as a
  shape, not a list. The hook/SSE approach is also the reference design if
  Sessions Chronicle ever wants live sessions.

### [Agents Trail](https://github.com/camtrik/agent-trail)

Local dashboard for Claude Code, Codex, OpenCode, OpenClaw, and Qoder. Combines
an aggregate overview (tokens, cost, live activity), turn-by-turn session replay
with expandable tool calls, hierarchical subagent trees, and a token/cost
inspector that breaks input, output, cache, and reasoning tokens down per turn
and aggregates by session and model. Local SQLite, no data leaves the machine.

- **Why it's similar:** Nearly identical premise to Sessions Chronicle — local
  SQLite index, multi-assistant coverage, privacy-first — with replay and cost
  analysis layered on top.
- **How it differs from Sessions Chronicle:** Web dashboard rather than native
  GNOME/GTK, and analytics-oriented rather than reading-oriented.
- **Maintenance signal:** 45 stars, v0.2.0 released on 2026-11-11, pushed on
  2026-11-11. Still early, but steadily maintained.
- **Product implication:** Its assistant coverage overlaps ours almost exactly.
  Worth watching as a shape, not yet as a competitive threat.

### [ClaudeScope](https://github.com/Liuziyu77/ClaudeScope)

Claude Code-only, parses `~/.claude/projects/<cwd-slug>/<sessionId>.jsonl` into
four views. The interesting one is a **Gantt-style timeline** that lays every
event on parallel lanes — user prompts, assistant text, thinking blocks, tool
calls — against a real-time x-axis, which makes wait periods and reasoning
clusters immediately visible. Also stacked-area token charts (input, cache_read,
cache_creation, output) for spotting runaway context growth, a session summary
line, and an event inspector. Python/Pandas/Plotly/Gradio, MIT, with the data
layer deliberately separated from visualization.

- **Why it's similar:** Same parsing target and the same intent to make a
  session
  legible rather than merely readable.
- **How it differs from Sessions Chronicle:** Single-assistant, notebook-grade
  Gradio app for analyzing one session at a time; no archive, search, or resume.
- **Maintenance signal:** 21 stars, no releases, last commit on 2026-09-03. Weak
  activity signal; listed for design relevance rather than momentum.
- **Product implication:** The most directly transposable idea in this section.
  A parallel-lane timeline is cheap to render in GTK (`DrawingArea`) and would
  reuse the existing parser output as-is.

### [claude-replay](https://github.com/es617/claude-replay)

Converts sessions from Claude Code, Cursor, Codex, Gemini, and OpenCode into
self-contained, embeddable HTML replays.

- **Why it's similar:** Multi-assistant session rendering aimed at
  comprehension, from the same local log formats.
- **How it differs from Sessions Chronicle:** Optimized for sharing a session
  outward, not for exploring your own history.
- **Maintenance signal:** 863 stars, v0.11.0 released on 2026-08-29, pushed on
  2026-11-13. Now also converts Kimi Code and Hermes sessions.
- **Product implication:** Real demand for exporting a session as an artifact
  someone else can read. Adjacent to, and compatible with, an archive product.

### [AI Agent Session Center](https://github.com/coding-by-feng/ai-agent-session-center)

Started as a way to turn live Claude Code, Gemini CLI, and Codex sessions into
animated 3D robots in a "cyberdrome". It now describes itself as a real-time
dashboard for Claude Code and Codex (live terminals, status alerts, prompt
queues, workspace resume) that runs as a desktop app or in the browser, with
the 3D view optional.

- **Why it's similar:** Shares the premise that session state deserves a visual
  form instead of a table.
- **How it differs from Sessions Chronicle:** Playful live-monitoring toy rather
  than a work-memory archive; the metaphor carries little analytical payload.
- **Maintenance signal:** 96 stars, v2.10.80 released on 2026-11-18, pushed on
  2026-11-18. Now describes itself as a Claude Code and Codex dashboard (live
  terminals, status alerts, prompt queues, workspace resume) with the 3D view
  optional.
- **Product implication:** Useful as a boundary marker. It shows where visual
  originality stops paying for itself and becomes decoration; the project
  itself has since demoted the 3D view to an option.

### [zoetrope](https://github.com/furkankly/zoetrope)

Rust terminal app (ratatui) that draws a Claude Code or Codex session as a live
flow graph: the main agent, the subagents it spawns, and the tools each one
runs. Replays finished sessions paced by their own timestamps, follows running
ones, and also offers a browser view. Read-only and local.

- **Why it's similar:** Same inputs (`~/.claude/projects/`,
  `~/.codex/sessions/`)
  and the same goal of making agent structure legible.
- **How it differs from Sessions Chronicle:** One session at a time, in the
  terminal; no archive, search, or cross-session view.
- **Maintenance signal:** 1,194 stars, v0.2.0 released on 2026-09-10, pushed on
  2026-11-03. Created in August 2026.
- **Product implication:** Confirms Agent Flow's lesson with a much lighter
  stack: a subagent/tool graph is useful even in a terminal, and replay paced
  by recorded timestamps is a cheap way to make a finished session feel alive.

## Adjacent projects

These are worth tracking, but they are not as directly comparable to Sessions
Chronicle's current scope.

### [ai-observer](https://github.com/tobilg/ai-observer)

Self-hosted single-binary OpenTelemetry-compatible observability backend for
local AI coding tools, covering Claude Code, Gemini CLI, Codex CLI, GitHub
Copilot, and OpenCode. Tracks tokens, costs, API latency, error rates, and
session activity in one dashboard.

- **Why it's adjacent:** Unifies the same fragmented multi-assistant local
  activity into a single view.
- **Why it is not a direct match:** Metrics/telemetry backend, not a transcript
  reading surface. Sits in the same cluster as claude-tap.
- **Maintenance signal:** 298 stars, v0.7.0 released on 2026-11-06, pushed on
  2026-11-06.

### [agents-observe](https://github.com/simple10/agents-observe)

Real-time observability dashboard for Claude Code and Codex with filtering,
search, multi-agent session visualization, full replay, and token usage stats.

- **Why it's adjacent:** Overlaps on search, replay, and multi-agent session
  comprehension.
- **Why it is not a direct match:** Live observability dashboard rather than a
  durable native history browser.
- **Maintenance signal:** 738 stars, v0.9.12 released on 2026-07-22, last commit
  on 2026-09-04. Quiet since September.

### [CC Switch](https://github.com/farion1231/cc-switch)

Cross-platform desktop all-in-one assistant for Claude Code, Codex, OpenCode,
DeepSeek Harness, and more, with provider/MCP/skills management.

- **Why it's adjacent:** Includes session management in a broad multi-assistant
  control plane.
- **Why it is not a direct match:** Provider switching, MCP sync, skills, WSL
  support, and operational management are the core surface; history browsing is
  one feature inside a much larger product.
- **Maintenance signal:** 156,062 stars, v4.0.37 released on 2026-11-20, pushed
  on 2026-11-21.
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
- **Maintenance signal:** 3,355 stars, v0.1.146 released on 2026-10-17, pushed
  on 2026-11-21.
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
- **Maintenance signal:** 1,377 stars, v2.17.0 released on 2026-11-06, pushed on
  2026-11-20.

### [Engineering Notebook](https://github.com/prime-radiant-inc/engineering-notebook)

Bun CLI and local web UI that ingests Claude Code and Codex session transcripts,
stores them in SQLite, and generates LLM-powered daily engineering-journal
entries. Presents a browsable journal with a date index, project timelines,
calendar/Gantt view, full-text search, transcript inspection, resume commands,
and an iCal feed.

- **Why it's adjacent:** Uses the same local Claude Code and Codex JSONL inputs
  and shares the local-first, SQLite-backed, human-readable reading goal.
- **Why it is not a direct match:** The core artifact is a generated daily
  narrative/summary rather than a faithful session archive; only two assistants;
  web app rather than native GTK/Libadwaita desktop.
- **Maintenance signal:** 356 stars, no releases, last commit on 2026-06-09 (a
  CI bump). No visible activity for more than five months; kept for the design
  direction, flagged as possibly dormant.
- **Product implication:** Shows that LLM summarization layered on top of raw
  sessions can create a different, potentially stickier product unit than
  search/browse alone. Worth watching as a design direction rather than as a
  feature-checklist competitor.

### [AI Engineer Coach](https://github.com/microsoft/AI-Engineering-Coach)

Microsoft's MIT-licensed VS Code extension (also runnable as a canvas in the
GitHub Copilot app) that reads local session logs and turns them into coaching:
practice scores and weekly trends, a Gantt-style session timeline, 45 editable
anti-pattern rules (prompt quality, session hygiene, code review, tool mastery,
context management), AI-generated code volume by language and model, a
repeated-prompt "Skill Finder", context-health checks, quizzes, and
achievements. Parsers cover VS Code Copilot Chat, Copilot CLI and the Copilot
app, Claude Code, Codex, OpenCode, and Xcode. Not published to the marketplace;
users build the `.vsix` themselves.

- **Why it's adjacent:** Same local session files, same privacy posture
  ("no data leaves your machine"), and a timeline view of sessions.
- **Why it is not a direct match:** It does not aim to let you read or search
  what happened. Its product unit is a score about how you work with
  assistants, not the transcript. Copilot-centric (several AI features use the
  VS Code language model API), and lives inside VS Code or the Copilot app.
- **Maintenance signal:** 4,422 stars, no releases (v0.1.0 in the changelog),
  pushed on 2026-11-21. Created in May 2026.
- **Product implication:** Strong attention signal for "analyze my usage" as a
  job distinct from "find my session". A light version is within reach for
  Sessions Chronicle — session timelines and simple per-project activity
  patterns from the existing index — but rule-based coaching is a different
  product and not worth chasing.

### [Claude Code Trace](https://github.com/delexw/claude-code-trace)

Rust/Tauri viewer for Claude Code session logs that runs as a desktop app, a
web app, or a Textual terminal UI over one backend. Renders sessions as
conversations with tool calls, thinking, sub-agents, word-level edit diffs, and
token counts; tails live sessions; and offers an opt-in efficiency analysis
that sends a redacted summary to an external service. Codex and Antigravity
are optional source plugins. A sibling project,
[codex-trace](https://github.com/PixelPaw-Labs/codex-trace), does the same for
Codex rollouts.

- **Why it's adjacent:** Faithful transcript reading of the same local JSONL,
  with careful rendering of tool calls and sub-agents.
- **Why it is not a direct match:** Claude Code first (other sources are
  opt-in plugins), and no cross-assistant archive or native GNOME UI.
- **Maintenance signal:** 392 stars, v0.20.0 released on 2026-11-17, pushed on
  2026-11-19.

### [ctx](https://github.com/ctxrs/ctx)

Rust CLI toolchain that searches local coding-agent history and "blames" code
to the sessions that wrote it, understanding parent sessions, subagents, and
forks. Aimed mainly at agents: it gives them fast recall of the real record
instead of a lossy memory summary.

- **Why it's adjacent:** Local, multi-assistant session search over the same
  JSONL and SQLite stores.
- **Why it is not a direct match:** CLI built for agents, with no human reading
  UI.
- **Maintenance signal:** 1,187 stars, v2.4.1 released on 2026-11-05, pushed on
  2026-11-21.
- **Product implication:** "Which session wrote this line?" is a compelling
  query. Sessions Chronicle already has file-edit data in tool calls and could
  answer it for humans.

### [deja-vu](https://github.com/vshulcz/deja-vu)

Single Go binary that builds a local inverted index over transcripts from 51
coding agents and exposes it as an MCP memory, a CLI search, cross-agent
handoff, pre-commit edit recovery, and line-level blame.

- **Why it's adjacent:** Retroactive, local, multi-assistant search across the
  same stores.
- **Why it is not a direct match:** Agent memory and handoff tool; no human
  reading UI.
- **Maintenance signal:** 1,640 stars, v0.24.3 released on 2026-11-19, pushed on
  2026-11-20. Created in July 2026.

### [abtop](https://github.com/graykode/abtop)

"btop for AI coding agents": a Rust TUI showing every running Claude Code,
Codex CLI, and OpenCode session with token use, context-window percentage, rate
limits, child processes, and open ports. Read-only, no API keys.

- **Why it's adjacent:** Discovers the same sessions from local process and
  file state.
- **Why it is not a direct match:** Live monitoring of running agents, not
  history.
- **Maintenance signal:** 3,868 stars, v0.5.5 released on 2026-09-14, pushed on
  2026-09-14.

### [ASB (Agent Switch Board)](https://github.com/Jamir-boop/ASB)

Native GTK 4 and Libadwaita "switch board" for GNOME (and Windows) that lists
Codex and Claude Desktop Code chats as cards with attention states, unread
marks, notifications, cross-machine sessions, inter-session messages, token
and cost usage (Codex, Claude Code, OpenCode, Antigravity, Grok, Pi), an
optional GNOME top-bar extension, and an Android companion.

- **Why it's adjacent:** The only other GTK 4/Libadwaita project found in this
  space, aimed at the same GNOME desktop.
- **Why it is not a direct match:** Live session dashboard and notifier, not a
  searchable history archive.
- **Maintenance signal:** 1 star, pushed on 2026-11-19. Created in October
  2026; very early, listed only because it shares our platform.

### [Recall](https://github.com/samzong/Recall)

Rust CLI/TUI that syncs local Claude Code, Codex, OpenCode, Antigravity, and
other sessions into one index with full-text and semantic search, usage
tracking, JSONL export/import, and resume back into the original CLI.

- **Why it's adjacent:** Same "one local index across assistants" premise,
  with search, export, and resume.
- **Why it is not a direct match:** Terminal-first power-user tool in the cass
  mold rather than a native desktop reading experience.
- **Maintenance signal:** 109 stars, v0.7.0 released on 2026-11-08, last commit
  on 2026-11-19.

### [sessions](https://github.com/nicknisi/sessions)

TypeScript CLI that builds a full-text index over Claude Code, Codex, Pi, and
OpenCode sessions, with fuzzy find and resume (or fork) from the terminal, an
MCP server that gives agents recall of past work, and token/cost usage reports.

- **Why it's adjacent:** Multi-assistant local search and resume over the same
  stores.
- **Why it is not a direct match:** CLI plus agent memory; no transcript
  reading UI.
- **Maintenance signal:** 52 stars, v1.34.1 released on 2026-11-16, last commit
  on 2026-11-16.

### [agf](https://github.com/subinium/agf)

Rust TUI for finding and resuming local sessions from 16 terminal agents,
including Claude Code, Codex, Antigravity, Grok Build, Kimi Code, Qwen Code,
Gemini, and Cursor CLI, with JSON commands and an MCP server.

- **Why it's adjacent:** Broad multi-assistant session discovery and search.
- **Why it is not a direct match:** Optimized for "find and resume in a
  keystroke", not for reading or reviewing what happened.
- **Maintenance signal:** 164 stars, v0.15.1 released on 2026-09-08, last commit
  on 2026-10-01.

### [claude-history](https://github.com/raine/claude-history)

Rust TUI and CLI history browser for Claude Code that, despite its name, also
supports Codex, Pi, and Oh My Pi. Offers fuzzy and semantic search, a
vim-style conversation viewer, export, and resume/fork from the TUI.

- **Why it's adjacent:** Search plus a real conversation viewer over the same
  local transcripts.
- **Why it is not a direct match:** Terminal UI with narrower assistant
  coverage; no native desktop app.
- **Maintenance signal:** 544 stars, v0.1.83 released on 2026-11-04, last commit
  on 2026-11-04.

### [agenttrace](https://github.com/luoyuctl/agenttrace)

Rust TUI and report generator that reads sessions from Claude Code, Codex CLI,
Qwen Code, Cline, Aider, Cursor exports, OpenCode, Pi, and others, and ranks
them by cost, duration, health, failures, and anomalies to diagnose slow or
expensive runs.

- **Why it's adjacent:** Same local session history, analyzed per session.
- **Why it is not a direct match:** Cost and health auditing rather than
  reading; sits with the trace/observability tools.
- **Maintenance signal:** 157 stars, v0.10.1 released on 2026-10-05, last commit
  on 2026-10-06.

### [Cursor Chat Browser](https://github.com/thomas-pedersen/cursor-chat-browser)

TypeScript web app for browsing, full-text searching, and exporting Cursor
editor chat and composer history from Cursor's workspace and global storage.

- **Why it's adjacent:** Local browse/search/export of AI coding conversations.
- **Why it is not a direct match:** Cursor-only web app; covers the IDE chat
  store rather than terminal agents.
- **Maintenance signal:** 516 stars, no releases, last commit on 2026-08-23.

### [claude-historian-mcp](https://github.com/Vvkmnn/claude-historian-mcp)

TypeScript MCP server for searching and retrieving Claude Code conversation
history from within Claude itself.

- **Why it's adjacent:** Solves local session search and retrieval by feeding
  history back into the AI assistant.
- **Why it is not a direct match:** Delivered as an MCP server with no human
  reading UI; Claude Code-only.
- **Maintenance signal:** 180 stars, no releases, pushed on 2026-11-02.
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
- **Maintenance signal:** 277 stars, v1.5.0 released on 2026-11-18, pushed on
  2026-11-18.

### [claude-view](https://github.com/tombelieber/claude-view)

Real-time monitoring dashboard for Claude Code sessions built with Rust and
React. Provides cost tracking, search, sub-agent visibility, reports, heatmaps,
and live monitoring.

- **Why it's adjacent:** Local-first Rust-based session search and analytics.
- **Why it is not a direct match:** Claude Code-only and live monitoring first,
  not a multi-assistant native desktop history browser.
- **Maintenance signal:** 128 stars, v0.46.3 released on 2026-11-02, pushed on
  2026-11-09.

### [ccpeek](https://github.com/ahmedelgabri/ccpeek)

Go local web app for exploring Claude Code history locally.

- **Why it's adjacent:** Active local session indexer and browser.
- **Why it is not a direct match:** Claude Code-only and local web app rather
  than native GTK/Libadwaita desktop.
- **Maintenance signal:** 33 stars, v2.3.2 released on 2026-11-15, pushed on
  2026-11-15.

### [session-graph](https://github.com/robertoshimizu/session-graph)

Python tool that converts scattered AI coding sessions into a queryable
knowledge graph using RDF/SPARQL and entity linking.

- **Why it's adjacent:** Turns session history into structured analysis data.
- **Why it is not a direct match:** Batch transformation pipeline rather than an
  interactive desktop session browser.
- **Maintenance signal:** 116 stars, v0.6.0 released on 2026-02-21, last commit
  on 2026-07-29.

### [Entropic](https://github.com/Dimension-AI-Technologies/Entropic)

GUI and CLI environment for Claude Code, Codex, and Gemini with repository
discovery, TODO tracking, chat history, and Git history views.

- **Why it's adjacent:** Overlaps with cross-repo agent history and workspace
  visibility.
- **Why it is not a direct match:** Broader agent workspace product with low
  adoption and less evidence of focused session-browser momentum.
- **Maintenance signal:** 9 stars, no releases, last commit on 2026-06-30.

### [opcode](https://github.com/winfunc/opcode)

Tauri/TypeScript GUI app and toolkit for Claude Code. It supports custom
agents, interactive session management, and secure background agents.

- **Why it's adjacent:** It shows large developer interest in GUI/control-plane
  surfaces around Claude Code.
- **Why it is not a direct match:** Claude Code-focused control plane rather
  than multi-assistant local history browser.
- **Maintenance signal:** 22,453 stars, v0.2.0 released on 2025-08-31. Commits
  resumed (latest merge on 2026-11-06), but there has been no release in over a
  year.

## Projects removed from the main list

These appeared in earlier versions of this document but are no longer in the
main shortlist because they are either less directly comparable, lower signal,
or no longer show clear current momentum.

### [cli-continues](https://github.com/yigitkonur/cli-continues)

TypeScript CLI for resuming an AI coding session in another tool (Claude Code,
Copilot, Gemini, Codex, Cursor). Removed in the 2026-11-21 review: no commit or
release in the last six months (last commit on 2026-05-07, which removed its CI
configs; 1,609 stars).

### [Chronicle / claude-history-manager](https://github.com/josephyaduvanshi/claude-history-manager)

Native macOS browser for Claude Code session history. Removed in the 2026-11-21
review: no commit or release in the last six months (v0.3.0 and last commit on
2026-04-26; 48 stars).

### [CodMate](https://github.com/loocor/codmate)

Native macOS SwiftUI app for Codex, Claude Code, and Gemini CLI sessions.
Removed in the 2026-11-21 review: the README now carries an archival note in
which the author says they plan to archive CodMate and shift focus away from a
heavyweight macOS GUI. Last commit and release are from 2026-03-31 and
2026-01-05 (665 stars).

### Copilot Chronicle

Interesting first-party feature direction inside GitHub Copilot CLI, but not a
standalone open-source project page comparable to Sessions Chronicle.

### [claude-sessions](https://github.com/iannuttall/claude-sessions)

Useful slash-command pack for documenting Claude Code sessions, but it solves
session capture/documentation rather than browsing and analysis. The repository
is now archived on GitHub (last push on 2025-06-16).

### [Codex Sessions Manager](https://github.com/coramba/codex-sessions-manager)

A small single-assistant Codex browser with a useful narrow scope, but current
repository activity appears limited compared with the actively maintained tools
listed above. No commit since 2025-11-20.

## Market read

The product space has become much more crowded since the March and April
assessments. The key shift is that "history viewer" is no longer a sufficient
category by itself. Competitors now cluster into a handful of shapes:

- **Broad local viewers:** AgentsView, Omnilogue, Obelisk, and Sessions Viewer.
- **Native platform viewers:** Agent Sessions on macOS and Sessions Chronicle
  on GNOME/Linux.
- **Operational control planes:** CC Switch, Nimbalyst, CCManager.
- **Trace/observability viewers:** claude-tap, ai-observer, agents-observe,
  abtop, agenttrace, and similar proxy/logging/monitoring tools.
- **Terminal search-and-resume tools:** cass, Recall, agf, claude-history, and
  sessions.
- **Context feeders:** claude-historian-mcp, SpecStory, ctx, deja-vu, and other
  tools that turn history into assistant-readable context.
- **Visualization-first tools:** Mindwalk, Agent Flow, zoetrope, ClaudeScope,
  Agents Trail, claude-replay.
- **Usage coaching:** AI Engineer Coach, which scores how you work with
  assistants rather than showing what they did.

Sessions Chronicle remains meaningfully differentiated only if it leans into
what those clusters do not fully cover: a native Linux/GNOME, local-first,
human-readable work memory for understanding what AI assistants did across
projects.

The July 2026 review added the visualization-first cluster, and it is the one
that changes the read. Every other cluster is crowded and converging on the same
feature checklist, so "browse, search, resume" is now table stakes rather than a
position. Visual comprehension is thinly populated by comparison: Mindwalk is
the only entrant with both an original form and real traction, Agent Flow owns
the live/orchestration angle, and the rest are early or single-assistant. That
gap is adjacent to work Sessions Chronicle has already done — the SQLite index
and the multi-assistant parsers are exactly the substrate these tools each had
to rebuild, and none of them pair a visualization with a durable searchable
archive. Two ideas transpose cheaply: ClaudeScope's parallel-lane timeline, and
a 2D treemap of touched files colored by interaction depth, which captures most
of Mindwalk's insight without the 3D engine.

The November 2026 review mostly confirms that read. The leaders kept compounding
(AgentsView doubled its stars and claims 50+ agents; Omnilogue renamed itself
and now lists 33 sources including Mistral Vibe and Kimi), and two new
SQLite-backed multi-assistant browsers appeared (Obelisk, Sessions Viewer).
The fastest-growing new entrants, though, are agent-facing search tools (ctx,
deja-vu) and Microsoft's usage coach, not human readers. Sessions Chronicle
remains the only native GNOME history browser found; the one other
GTK/Libadwaita project, ASB, is a live switch board. Ideas worth borrowing from
this round: line-level "which session wrote this" blame, and timestamp-paced
replay of a finished session.
