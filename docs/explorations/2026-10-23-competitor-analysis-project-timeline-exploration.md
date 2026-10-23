# Competitor Analysis: Project Timeline View

**Issue:** [#92](https://github.com/supermaciz/sessions-chronicle/issues/92) — Project timeline view  
**Date:** 2026-10-23  
**Status:** Open  
**Lens:** competitive analysis — how existing tools approach a chronological, per-project view of AI assistant session history, and what Sessions Chronicle should adopt, ignore, or differentiate on.

## Context

Issue #92 asks for a chronological view of what happened in a project over time: which sessions ran, when, with which AI assistant, with what outcome and key actions. The product assessment of 2026-06-21 ranked this as the top P0 because it shifts the product from "find a transcript" to "reconstruct work."

This document reviews how the competitive field approaches that question. It draws on `docs/SIMILAR_PROJECTS.md`, the project-views explorations of 2026-03-14 and 2026-03-16, and fresh web research (READMEs, product docs, changelogs, marketplace listings, Reddit/HN discussions) conducted on 2026-10-23.

---

## 1. Comparative matrix

Legend: ● strong / ◐ partial / ○ absent / — not applicable

| Project | Per-project chronological view | Cross-project time overview | Granularity | Visualization form | Metadata shown per session | Git correlation |
|---|---|---|---|---|---|---|
| **AgentsView** (Go, web/desktop) | ◐ project filter on dashboard; project bar chart click-to-filter | ● Activity page: stacked concurrency timeline, clickable buckets → session drill-down; GitHub-style heatmap, click-to-filter by day | Session + message counts | Stacked bars (interactive/subagent/automated), heatmaps, hour-of-week grid, bar charts | Agent-minutes, cost, tokens, model, health grade, outcome dot (recency + how it ended) | ◐ Outcomes panel aggregates commits/lines from Git history (not placed on a timeline) |
| **cass** (Rust CLI/TUI) | ○ | ◐ Analytics Dashboard (`Alt+A`): 7 views (Dashboard, Explorer, Heatmap, Breakdowns, Tools, Plans, Coverage) with drill-down | Session + message (search-first) | TUI charts, KPI tiles, heatmap | Tokens, cost, model, tool breakdowns | ○ |
| **Nimbalyst** (Electron) | ● Project Graph (alpha, off by default): **Pulse** = grid of areas × day/week/month buckets with activity counts; **Atlas** = stable territory map; **Evidence Trails** = relation lanes (session↔file↔commit) | ◐ AI Usage Report with per-workspace cards | Record/artifact counts per bucket; session-level in session views | Grid/heatmap (Pulse), tile map (Atlas), lane graph (Evidence Trails), kanban | Tokens, cost, tool counts, session metadata; saved views; compare previous period | ◐ Evidence Trails links commits to sessions as relations, not as timeline events |
| **CCManager** (TypeScript TUI) | ○ | ○ | Live session state only | TUI session list with busy/waiting/idle states, worktree grouping | State, project, worktree | ◐ worktree/branch aware (operational, not historical) |
| **Agent Sessions** (macOS Swift) | ◐ session browser grouped by project | ◐ Analytics dashboard (time-series, agent breakdowns, time-of-day heatmaps, Git inspector), filterable to one project | Session | Grouped list rows, flip-cards with sparklines, heatmaps | Tokens, cost (priced per model), quota burn, model history | ◐ Git inspector in analytics |
| **Claude Code History Viewer** (Tauri) | ◐ conversation tree by project/session with worktree grouping | ◐ Analytics dashboard (token stats dual-mode, cost breakdown, provider distribution); **Session Board**: multi-session pixel view + attribute brushing + activity timeline | Session; multi-session scatter/timeline | Tree, charts, pixel view / brushing | Tokens, cost, provider, model | ○ |
| **claude-tap** (Python proxy) | ○ | ○ (live only) | API request | Trace waterfall of requests (prompts, tool schemas, streaming, token usage, request diffs) | Tokens, latency, cost per request | ○ |
| **Engineering Notebook** (Bun, web) | ● **Projects view: per-project timeline of journal entries**, on-demand summarization | ◐ Calendar view: week/month with Gantt-style bars of project activity | Session → LLM journal entry | Vertical timeline per project; Gantt-style calendar bars | LLM-written headline/summary/topics/open questions (cloud-dependent) | ○ |
| **agent-timeline** (Python, self-hosted web) | ● sessions grouped into **topic lanes** over a shared time axis; parent/child agent links | ● concurrency chart + time-range controls | Session | Swimlane-style topic lanes + concurrency chart | Timestamps, agent, model, parent links, topic labels (no raw transcripts stored) | ○ |
| **ai-timeline** (web/VS Code) | ◐ cross-session dashboard for benchmark runs | ◐ dashboard with mini-Gantt across many sessions | Tool call / message (within session) | Zoomable swimlane per session, mini-Gantt, per-tool duration stats | Tokens, cache hit rate, cost, per-event breakdown, auto-insights ("43% tokens wasted") | ○ |
| **brigade-tui** (Python TUI) | ◐ per-project tab with **wall-clock swimlane waterfall** (last 10 min) | ◐ grid of session cards across projects, factory summary line (spend today) | Session (live window) | Card grid + waterfall swimlane | Title, agent/role, elapsed, tokens, cost, state colors | ○ |
| **CCAssist** (VS Code ext) | ◐ **File Timeline**: per-file "git blame for AI edits" — every session/prompt that touched a file, with diffs | ◐ usage dashboard (cost per model, quota burn-down) | File → session/prompt | File-centric list + diffs; turn-changes cards | Cost, tokens, context fill, cache hit rate | ● per-file edit attribution |
| **Claudoscope** (macOS menu bar) | ◐ project insights + timeline history | ◐ real-time dashboard, 90-day activity heatmap | Session | Dashboard, heatmap, per-file diffs | Cost, tokens, secrets detection | ◐ per-file diffs of agent changes |
| **Mindwalk / Agent Flow / call-timeline / herdr** | ○ (single-session or live replay only) | ○ | Tool call / turn (within one session) | 3D map replay, live node graph + timeline panel, per-turn waterfall, live subagent swimlanes | Tokens, cost, durations | ◐ file attention (Mindwalk, Agent Flow) |
| **Sessions Chronicle** (today) | ◐ project sidebar filter (list filtering only) | ◐ analytics workspace | Session | Flat list, grouped rows possible | Assistant badge, summary, tokens, tool grouping, subagents, outcome | ○ (planned in IDEAS.md) |

**Reading of the matrix:** nobody ships a true *per-project narrative timeline* — sessions ordered in time within one project, with outcome and key actions, as the primary surface. The closest shapes are:

1. **AgentsView Activity** — a cross-project concurrency/time chart with drill-down into sessions (analytics-first, not project-narrative).
2. **Nimbalyst Pulse** — a per-project activity grid (areas × time buckets), but record-count based, alpha, and buried in a workspace product.
3. **Engineering Notebook Projects** — a per-project vertical timeline, but made of LLM-generated journal entries (cloud dependency, 2 assistants).
4. **agent-timeline** — topic lanes over time, self-hosted web, no transcripts.

---

## 2. Per-competitor analysis

### AgentsView (kenn-io/agentsview) — the strongest benchmark

**Approach.** A local-first web/desktop app (Go + SQLite + SSE) with a full analytics layer: dashboard, Usage page, Activity page, Trends, Quality. The timeline-ish surfaces are: a GitHub-style contribution heatmap (click a day → filters every chart and the session list), an hour-of-week heatmap, an "Activity Timeline" bar chart of message/session counts with Day/Week/Month bucketing and user/assistant/other message split, a Project Breakdown bar chart (click a project bar → filters dashboard + session list), and the **Activity page**: a stacked concurrency chart (interactive / subagents / automated) on one time axis, where clicking a bucket filters the sessions table to sessions active in that slot, with hover tooltips showing agent-minutes, tokens, and cost per bucket. Session rows carry a state dot encoding recency and how the session last ended. An **Outcomes panel** reads Git history of the repositories tied to matching sessions (commits, lines added/removed, files changed). Session Health grades sessions (completed/errored/abandoned).

**Strengths.** Drill-down consistency (every aggregate is clickable into the underlying sessions); honest metric semantics (documented per panel); git-aware outcomes; model-grain filtering; rolling vs pinned date ranges; very active development (v0.34+, 60+ agent formats).

**Weaknesses.** Analytics-first, not narrative-first: there is no "story of this project" surface. The per-project view is a filter on a global dashboard, not a dedicated project page. Concurrency metrics describe machine utilization, not work progression. Web/desktop-webview rather than native.

**Relevance to #92.** The clickable-bucket → filtered-session-list pattern, the day heatmap with click-to-filter, Day/Week/Month bucketing, and the recency/outcome state dot are all directly adoptable. The Outcomes panel proves the value of correlating sessions with git history — but as aggregates, not as timeline events.

### Nimbalyst (nimbalyst/nimbalyst) — the closest shape, the highest ambition

**Approach.** An Electron "visual workspace" around Claude Code/Codex/OpenCode. Its newest surface, **Project Graph** (alpha channel, off by default), indexes seven sources (sessions, trackers, commits, plans, documents, GitHub issues/PRs, memory) and offers three views: **Atlas** (a territory map of project "areas" whose positions are stable across date ranges, so users build spatial memory), **Pulse** ("what changed recently, and where did attention move?" — a grid of areas × calendar buckets (day/week/month), each cell counting distinct artifacts with recorded activity; includes "compare previous period" overlay), and **Evidence Trails** (focus one record, walk its recorded relations in named lanes — sessions a tracker item was worked on in, files a session edited, files a commit touched — with breadcrumbs). Time-range toolbar (7/30/90 days, stepping arrows, Now, compare previous period), up to 30 saved views ("lenses"), and an explicit **Sources & limitations** disclosure so the view never overstates coverage. Pulse is deliberately conservative: an empty cell means "no events loaded," not "nothing happened."

**Strengths.** The most thoughtful information design in the field: stable spatial memory, honest empty-cell semantics, explicit coverage disclosure, saved views, period comparison. Evidence Trails is the only competitor feature that connects sessions ↔ files ↔ commits as walkable relations. AI Usage Report breaks totals down per workspace.

**Weaknesses.** Alpha, off by default, inside a large workspace product (category-shift threat rather than a viewer). Record-count grids are abstract — a session is a count in a cell, not a row you can read. No transcript reading in the graph views. Electron, not native.

**Relevance to #92.** Pulse is the closest existing thing to a project timeline grid. Sessions Chronicle should adopt the *honesty patterns* (coverage disclosure, conservative empty states, saved views, period comparison) and the *session-as-first-class-row* principle (a timeline of sessions you can open, not counts in cells). Atlas-style spatial memory is out of scope for a v1 GNOME list-based view.

### Engineering Notebook (prime-radiant-inc) — the per-project timeline that exists

**Approach.** A Bun CLI + web UI that ingests Claude Code and Codex transcripts into SQLite, generates **LLM-written daily journal entries** (headline, summary, topics, open questions), and serves: a Journal view (date index / entries / transcript), a **Projects view** (all projects by recency, each with a **timeline of journal entries** and on-demand summarization of new sessions), a **Calendar view** (week/month with **Gantt-style bars showing project activity across dates**), search, and an iCal feed.

**Strengths.** It is the only tool whose primary per-project surface is a vertical chronological timeline. The calendar/Gantt view is a genuinely different temporal encoding. The "automatic engineering diary" framing creates a sticky product unit beyond search/browse.

**Weaknesses.** LLM summaries require a cloud API call (not local-first, not deterministic, only 2 assistants). The timeline is narrative, not navigational — entries link to transcripts, but there are no per-session metadata rows (duration, tokens, outcome, assistant). Web UI, not native.

**Relevance to #92.** Validates the demand for exactly the surface #92 describes. Sessions Chronicle can do the same shape **deterministically and locally**: the structured session summaries already exist (issue #91/PR #105), so a per-project timeline of sessions with their structured summary line needs no LLM. The Gantt-style calendar is a candidate for a later iteration, not v1.

### cass (Dicklesworthstone/coding_agent_session_search) — search-first, analytics as a side panel

**Approach.** A Rust CLI/TUI obsessed with search speed and power-user ergonomics across 30+ agent formats. Its only timeline-adjacent surface is an **Analytics Dashboard** (`Alt+A`): 7 views (Dashboard, Explorer, Heatmap, Breakdowns, Tools, Plans, Coverage) with interactive charts, KPI tiles, and drill-down filtering. The word "timeline" in its README refers to a "single, searchable timeline" of messages — i.e., search over time-ordered content, not a visual timeline.

**Strengths.** Best-in-class search ergonomics (ranking modes, fuzzy fallback, multi-open queue, find-in-detail). The analytics dashboard shows the TUI can carry heatmaps and drill-down.

**Weaknesses.** No per-project view, no chronological project surface, no session-state semantics. Analytics are descriptive, not navigational.

**Relevance to #92.** Benchmark for drill-down filtering and heatmap interactions in a keyboard-driven UI. Confirms that "timeline" as a *search* property is a different feature from #92's *comprehension* timeline — Sessions Chronicle should not confuse the two.

### Agent Sessions (jazzyalex/agent-sessions) — native grouped browsing + analytics

**Approach.** A native macOS Swift app: session browser with **rows grouped by project**, unified search, resume, archive management, an **Agent Cockpit** for live sessions, a **Quota Meter**, and an **Analytics** view (flippable stat cards with sparklines, time-series charts, agent breakdowns, time-of-day heatmaps, a Git inspector) that can be **filtered to a single project**. Session Info panel shows per-source telemetry (price at API rates, token mix, model history, request count) with explicit "honest boundaries" — unsupported fields stay blank instead of being inferred.

**Strengths.** The native-app polish Sessions Chronicle should aspire to on Linux; project grouping is first-class in the main browser; the "honest boundaries" telemetry principle matches Sessions Chronicle's deterministic-summary philosophy; per-model pricing (orchestrator vs subagent models).

**Weaknesses.** macOS-only. Analytics are aggregate cards, not a per-project chronological surface. No session-burst or work-progression semantics.

**Relevance to #92.** Confirms that grouping sessions by project in the main list is the expected native pattern (already decided in the 2026-03-14 exploration, Option A). The per-project-filtered analytics is a pattern to keep in mind for later, but #92 explicitly excludes analytics from v1.

### Claude Code History Viewer (jhlee0409) — tree + Session Board

**Approach.** A Tauri desktop app / headless server covering 31 assistants. Conversation browser as a **project/session tree with worktree grouping**; analytics dashboard (dual-mode token stats — billing vs conversation — cost breakdown, provider distribution); and a **Session Board**: multi-session visual analysis with a pixel view, attribute brushing, and an activity timeline. Real-time file watching, archive manager, server mode.

**Strengths.** The Session Board is the most "visual analytics over many sessions" feature in the field (scatter/pixel view with brushing). Broad provider coverage. Worktree-aware grouping matches how agents actually fork work.

**Weaknesses.** The Session Board is analysis-by-attribute, not a chronological project narrative. Cross-platform Tauri rather than native. Breadth over depth on comprehension.

**Relevance to #92.** The Session Board shows users will pay attention to multi-session visual analysis — a possible v2 direction for Sessions Chronicle (e.g., a compact per-project strip), but it validates keeping v1 list-based and navigational per issue #92's non-goals.

### CCManager (kbwo/ccmanager) — operational, not historical

**Approach.** A TypeScript TUI that orchestrates live sessions across assistants and git worktrees: parallel sessions, per-session state (busy/waiting/idle), multi-project discovery (`--multi-project`), worktree lifecycle, context copy between worktrees, notifications on state changes.

**Strengths.** Owns the "run many sessions across projects" job; worktree mental model; state-change automation.

**Weaknesses.** No history browsing, no timeline, no transcript reading. It manages sessions; it does not help you remember what happened.

**Relevance to #92.** A complementary threat: users may organize *forward* work in CCManager/Nimbalyst and never need a history tool. Sessions Chronicle's answer is the *backward* view — reconstructing what already happened. The worktree concept is worth remembering: a project timeline could later group or badge worktree-related sessions.

### claude-tap (liaohch3) — request-level trace forensics

**Approach.** A local reverse proxy that captures real API traffic (requests, streaming responses, tool schemas, token usage, request diffs) and renders it in a local trace viewer with light/dark modes, plus self-contained HTML export. Live-first; multi-agent (Claude Code, Codex, Gemini, Cursor, OpenCode, Kimi, Pi, Hermes, …).

**Strengths.** Ground truth at the API layer; privacy-forward (no hosted dashboard); raises the bar for "what did the agent do?" debugging. There is also an ecosystem of per-turn waterfall viewers (e.g., theREDspace/call-timeline: per-turn durations, call counts, tokens in/out, cost, context fill, per-call waterfall bars).

**Weaknesses.** Live capture only — nothing about sessions that ran before you installed the tap. Request/turn grain, not project grain. Not a history browser.

**Relevance to #92.** Different grain, different moment in time. But its per-turn waterfall (call-timeline) is the within-session counterpart of the per-project timeline: if Sessions Chronicle builds a project timeline, a matching *session activity ribbon* (already in IDEAS.md) would complete the two-grain story. Both should stay deterministic and local.

### agent-timeline (fstandhartinger) — topic lanes over time, privacy-first

**Approach.** A self-hosted, read-only web view: a collector snapshots transcript *metadata* (timestamps, agent, model, parent links, topic labels, source paths — explicitly **not** raw messages or tool output) into SQLite; the UI groups sessions into **user-defined topic lanes** (keyword matching, optional opt-in model classification), connects parent/child agents, shows activity over time with time-range controls, search, filters, and a concurrency chart.

**Strengths.** The clearest expression of "sessions as lanes on a time axis"; privacy design is exemplary (no transcripts stored, sanitized excerpts only, noindex headers, rate-limited login); small and single-purpose.

**Weaknesses.** Very early-stage. Topic lanes require manual keyword config; sessions are lane entries, not readable rows; no transcript, no outcome semantics, no per-session metadata beyond agent/model.

**Relevance to #92.** Validates the swimlane-over-time encoding for sessions (the 2026-03-14 exploration's Option E). Its weakness — sessions you cannot open or read — is exactly what Sessions Chronicle must not replicate: every timeline element must deep-link into the transcript.

### ai-timeline (msdickinson) — within-session swimlanes + cross-run patterns

**Approach.** A browser/VS Code tool that loads session files (drag & drop) and renders a **zoomable swimlane timeline of every response, tool call, and thinking block**, with playback and click-to-inspect, a dashboard with mini-Gantt and auto-insights ("file read 8 times," "43% of tokens spent after the fix"), per-model token/throughput/cache-hit views, and cross-session comparison for whole benchmark runs, compared by model. Plugin architecture (one file per parser/view).

**Strengths.** The best within-session timeline in the field; the "wasted work" insights are the most concrete comprehension payoff shown anywhere; cross-run comparison is unique.

**Weaknesses.** Session-at-a-time (or benchmark-folder-at-a-time); no durable archive, no project concept beyond folder loading; insights are heuristic, not auditable.

**Relevance to #92.** The within-session swimlane is the complement to #92's across-session timeline. Combined with ClaudeScope's parallel-lane Gantt (already flagged in `docs/SIMILAR_PROJECTS.md` as "the most directly transposable idea"), it forms a two-grain visualization roadmap: **project timeline (across sessions) → session ribbon (within session)**.

### brigade-tui (DanielPollitt01) — per-project waterfall, live

**Approach.** A read-only Python TUI: a grid of session cards across projects (state-colored: red blocked / green completed / blue running), one tab per project with a **wall-clock swimlane waterfall** (last 10 minutes, now-marker per row), a factory summary line (projects, running, spend today), explicit cap disclosure ("10 of 31 projects · 269 of 780 rows").

**Strengths.** The per-project swimlane waterfall is a direct, minimal template for a project timeline; the "stated, never silent" cap disclosure is an honesty pattern worth copying.

**Weaknesses.** Live window only (10 minutes); no history; timing-only waterfall (no outcome, no metadata beyond tokens/cost).

**Relevance to #92.** Confirms the per-project waterfall/swimlane encoding works even in a terminal. For Sessions Chronicle, the equivalent v1 is a vertical day-grouped list (GNOME-native); a waterfall encoding is a candidate once sessions carry reliable start/end timestamps across all five assistants.

### CCAssist / "Claude Code and Codex Assist" (VS Code extension) — the inverse timeline

**Approach.** An IDE extension with chat history browsing, diffs, full-text search, token/cost tracking, and a distinctive **File Timeline**: "git blame for AI edits" — pick a file, see every session and prompt that touched it, open the exact diff. Also per-turn "Turn changes" cards, a context-window graph, cache hit rate, cost per model, quota burn-down.

**Strengths.** The inverse view (file → sessions) is something no standalone tool offers; living inside the editor is a distribution advantage; 7,846 installs show demand.

**Weaknesses.** IDE-bound; per-file, not per-project; no chronological project surface.

**Relevance to #92.** The inverse index (which sessions touched which files) is a natural v2 for Sessions Chronicle's project timeline: day-grouped session rows that expand to show files touched, plus a per-file filter. It is also the cheapest bridge between "timeline" and "what changed in the code."

### Claudoscope (cordwainersmith) and the visualization-first cluster

Claudoscope (macOS menu bar) adds a real-time dashboard, per-file diffs of everything Claude changed, plan browsing, **timeline history**, and project insights. The wider cluster — Mindwalk (3D map replay of a single session), Agent Flow (live node graph + companion timeline and **file attention heatmap**), call-timeline (per-turn waterfall) — all answer "what did the agent do" at session-or-live grain, never at durable per-project history grain.

**Relevance to #92.** These tools define the *ceiling* of visual ambition and the *floor* of the archive story: none of them keep a searchable durable history. That pairing — visualization + durable local archive — is precisely the gap `docs/SIMILAR_PROJECTS.md` identifies, and #92 is the first feature that occupies it.

---

## 3. Emerging UX patterns

Across all competitors, a consistent set of patterns emerges for time-based session views:

1. **Click-through from aggregate to detail.** Every chart bucket, heatmap cell, and project bar is a filter into the underlying session list (AgentsView heatmap/concurrency/project bars, cass drill-down, CCHV brushing). Aggregates are navigation, not endpoints.
2. **Day/Week/Month bucketing toggle** on any time chart (AgentsView, Nimbalyst Pulse).
3. **GitHub-style contribution heatmap** with click-to-filter a single day (AgentsView, Agent Sessions, cass, Claudoscope's 90-day heatmap).
4. **Time-range presets with stepping and "compare previous period"** (Nimbalyst: 7/30/90 days, arrows, Now, compare overlay; AgentsView: rolling vs pinned ranges, link-across-pages).
5. **Honest coverage semantics.** Nimbalyst Pulse: an empty cell means "no events loaded," with a Sources & limitations disclosure; brigade-tui: caps stated ("10 of 31 projects"); Agent Sessions: unsupported telemetry fields stay blank. The field has collectively learned not to fabricate.
6. **State encoding on session rows.** A dot or color encoding recency + outcome (AgentsView), or live state (brigade red/green/blue, CCManager busy/waiting/idle). Outcome semantics (completed/errored/abandoned) appear in AgentsView's Session Health.
7. **Per-project grouping as first-class navigation** (Agent Sessions grouped rows, CCHV project/session tree, Engineering Notebook Projects list, Nimbalyst per-workspace cards).
8. **Two grains, two encodings.** Within-session: swimlanes/waterfalls (ClaudeScope, ai-timeline, call-timeline, claude-tap, herdr). Across-session: heatmaps, grids, bars (AgentsView, cass, Nimbalyst, agent-timeline, Engineering Notebook calendar). No tool yet connects the two grains in one product.
9. **The inverse view.** File → sessions (CCAssist File Timeline) exists only as an IDE extension feature; nobody has it in a standalone archive tool.
10. **Saved views / lenses** for recurring time-boxed questions (Nimbalyst, up to 30).
11. **Git correlation is valued but shallow.** AgentsView aggregates commits/lines per repo; Nimbalyst links commits to sessions as graph edges; nobody places commits *on* the session timeline as events.
12. **Local-first and deterministic are still differentiators.** Engineering Notebook needs cloud LLM calls; agent-timeline needs config and stores no transcripts; AgentsView is local but web-based. No competitor combines local + deterministic + multi-assistant + native desktop + durable archive.

---

## 4. Recommendations for Sessions Chronicle's Project Timeline View (#92)

Issue #92's scope is deliberately narrow: chronological sessions within a project, day grouping, compact rows linking to session detail, structural context; explicitly *not* analytics, charts, AI summaries, or cross-project comparison. The competitive analysis supports that scope and suggests concrete patterns to adopt inside it.

### 4.1 What to build (v1, aligned with #92)

1. **A per-project chronological view** reachable from the project sidebar (already decided: Option A, single-select ListBox) — selecting a project opens its timeline instead of (or as the default tab of) the filtered flat list. This matches Agent Sessions' grouped browsing and Engineering Notebook's Projects view, and it directly answers "where did I leave off on this project."
2. **Day-grouped vertical list** (`AdwPreferencesGroup` per day, or `ListView` with day header rows), most recent day first. Vertical and GNOME-native beats a horizontal frise for readability, keyboard navigation, and accessibility — the 2026-03-14 exploration already rated the swimlane ribbon (Option E) lowest on HIG compliance. Engineering Notebook proves the vertical per-project timeline is the shape users expect.
3. **Compact session rows** showing: assistant badge (existing), time + duration, structured summary line (already built, issue #91/PR #105 — deterministic, no LLM), tool-call count by category (existing grouping), token/cost where the source provides it (honest blanks otherwise — Agent Sessions' "honest boundaries" principle), and an outcome indicator (the 2026-03-27 outcome-display exploration). Clicking a row opens the session detail at the top; a secondary action resumes the session.
4. **"Where did I leave off" affordance**: the most recent session of the project is visually pinned/emphasized at the top of the timeline, with its resume action one click away.
5. **Session-burst grouping**: cluster sessions into "bursts of work" separated by inactivity gaps (e.g., > 4h or overnight). This answers issue #92's third question — "which sessions were part of the same burst of work" — and is something no competitor does. A subtle burst separator/header ("3 sessions · Tuesday morning") is enough in v1.
6. **Project header strip**: project name/path, session count, active-day count, assistant breakdown dots, and a 30-day activity sparkline (the Proposition D rich-card sparkline from the 2026-03-16 exploration, promoted to the timeline header). This is the project-overview payload without becoming an analytics dashboard.

### 4.2 Patterns to adopt from competitors

- **Click-through consistency** (AgentsView): the header sparkline days are clickable to filter the timeline to that day; a "filtered to <day>" chip with dismiss appears (AgentsView heatmap pattern, adapted).
- **Bucket toggle Day/Week/Month is out of scope for v1** (it belongs to charts), but keep the existing date-range filter working on the timeline.
- **Honest coverage** (Nimbalyst, brigade-tui, Agent Sessions): if a day has no indexed sessions, show nothing rather than implying inactivity; if token/cost data is missing for an assistant, leave it blank; state indexing coverage in the project header ("indexed since <date>").
- **Outcome dots** (AgentsView state dot): recency + outcome encoded as a small colored dot on each row, with tooltip — dense status at a glance without a status column.
- **Keyboard-first** (cass): the timeline is a `ListView` with full keyboard navigation, type-ahead, and Enter to open — consistent with the existing session list.

### 4.3 What to deliberately not build in v1

- No charts, heatmaps-as-dashboards, or project analytics (explicit #92 non-goal; AgentsView/cass/Agent Sessions already own that space and would beat us on breadth).
- No LLM-generated project narrative (Engineering Notebook's model — cloud dependency, non-deterministic; our structured summaries are the local, auditable equivalent).
- No cross-project swimlane/atlas (Nimbalyst/agent-timeline territory; revisit as the "cross-project atlas" idea already in `docs/IDEAS.md`).
- No live-session monitoring (CCManager/herdr/brigade-tui territory; separate issue #50).

### 4.4 Sequencing suggestion

1. **v1**: project timeline = day-grouped session rows + project header + burst separators + resume affordance (this issue, #92).
2. **v1.1**: per-row key actions (files edited count, commands run, error count — deterministic from existing tool-call data; this is the P1 "tool call timeline / file-change narrative" from the product assessment, scoped to row level).
3. **v2**: within-session activity ribbon (ClaudeScope/ai-timeline parallel-lane pattern, per `docs/SIMILAR_PROJECTS.md`) so the two grains connect: project timeline → session ribbon.
4. **v2**: file-touch inverse index (CCAssist's File Timeline, but local and multi-assistant): expandable per-file list inside the project timeline, plus a "filter timeline to file X" action.
5. **v2/v3**: git commit markers on the timeline (deterministic from `git log` + session timestamps — the one correlation nobody does as timeline events), then the cross-project atlas from `docs/IDEAS.md`.

---

## 5. Differentiation opportunities

What nobody does well, and where Sessions Chronicle can win:

1. **Per-project narrative timeline, native and local.** The closest competitors split the capability: Engineering Notebook has the per-project timeline but needs cloud LLM summaries and covers 2 assistants; Nimbalyst has the project graph but it is alpha, record-count-based, and inside a workspace product; AgentsView has the drill-down but no project narrative. A GNOME-native, deterministic, multi-assistant per-project timeline does not exist.
2. **Session bursts.** No tool groups sessions into "bursts of work." Issue #92 names this question explicitly; it is cheap to compute from timestamps and immediately useful for "what was I doing Tuesday?"
3. **Two-grain comprehension.** No tool connects across-session history to within-session structure in one product. Owning both grains (project timeline + session ribbon) with deep links between them is a structural advantage the visualization-first cluster cannot match, because they have no durable archive.
4. **Git commits as timeline events.** AgentsView aggregates commit stats; Nimbalyst links commits as graph edges; nobody renders commits *on* the project timeline interleaved with sessions. Deterministic correlation (session timestamps × `git log`) is feasible locally and is the single feature that turns the timeline from "when did I talk to the AI" into "what changed in the project."
5. **The inverse index done right.** CCAssist's File Timeline lives in an IDE extension and covers one editor's workflow. A local, multi-assistant file → sessions index inside the project timeline would be unique in the standalone category.
6. **Honesty as a feature.** The field's best pattern (Nimbalyst's coverage disclosure, Agent Sessions' blank-when-unknown, deterministic summaries) is philosophically aligned with Sessions Chronicle's existing guardrails (`docs/IDEAS.md`: deterministic analysis before AI-generated interpretation). Leaning into it — "every claim on the timeline is traceable to an indexed event" — is both a UX and a positioning differentiator against LLM-narrative competitors.
7. **GNOME/Linux native.** Every competitor with a timeline is web, Tauri, Electron, macOS, or CLI. On Linux, the choice is currently AgentsView (webview) or cass (TUI). A calm, HIG-compliant, keyboard-first native timeline is uncontested territory — the same argument the 2026-06-21 assessment makes for the product as a whole.

---

## Sources

Research conducted 2026-10-23 via web search and direct page/README fetches.

**Primary competitor docs**

- AgentsView — Usage Guide: https://www.agentsview.io/docs/usage/ · Activity reference: https://www.agentsview.io/docs/activity/ · Guide: https://www.agentsview.io/guide/ · Changelog: https://www.agentsview.io/changelog/
- Nimbalyst — Project Graph docs: https://nimbalyst.com/docs/file-management/project-graph/ · Changelog: https://nimbalyst.com/changelog/ · AI Usage Report: https://nimbalyst.com/docs/session-management/ai-usage-report/
- cass — README: https://github.com/Dicklesworthstone/coding_agent_session_search (Analytics Dashboard section) · Show HN: https://news.ycombinator.com/item?id=46130481
- Agent Sessions — site: https://jazzyalex.github.io/agent-sessions/ · DeepWiki: https://deepwiki.com/jazzyalex/agent-sessions
- Claude Code History Viewer — README: https://github.com/jhlee0409/claude-code-history-viewer · site: https://jhlee0409.github.io/claude-code-history-viewer/
- CCManager — README: https://github.com/kbwo/ccmanager · FollowAgents: https://followagents.com/en/agents/ccmanager
- claude-tap — site: https://liaohch3.com/claude-tap/ · repo: https://github.com/liaohch3/claude-tap

**Additional projects found during this research**

- Engineering Notebook — README: https://github.com/prime-radiant-inc/engineering-notebook · blog: https://primeradiant.com/blog/2026/what-we-are-working-on.html
- agent-timeline — README: https://github.com/fstandhartinger/agent-timeline
- ai-timeline — README: https://github.com/msdickinson/ai-timeline · web app: https://msdickinson.github.io/ai-timeline/
- brigade-tui — README: https://github.com/DanielPollitt01/brigade-tui
- CCAssist (Claude Code and Codex Assist) — Open VSX: https://open-vsx.org/extension/agsoft/claude-history-viewer · VS Marketplace: https://marketplace.visualstudio.com/items?itemName=agsoft.claude-history-viewer · comparison post: https://ccassist.dev/blog/best-claude-code-vscode-extensions-2026/
- Claudoscope — repo: https://github.com/cordwainersmith/Claudoscope · site: https://claudoscope.com/
- call-timeline — https://github.com/theREDspace/call-timeline
- AgentDashboard swimlane request — issue #116: https://github.com/xhargh/AgentDashboard/issues/116
- Mindwalk — https://github.com/cosmtrek/mindwalk · Agent Flow — https://github.com/patoles/agent-flow · ClaudeScope — https://github.com/Liuziyu77/ClaudeScope · herdr — https://herdr.dev/

**Community signals**

- Reddit r/ClaudeCode "AI Coding Agent Dev Tools 2026": https://www.reddit.com/r/ClaudeCode/comments/1r6hy2c/ai_coding_agent_dev_tools_2026/ — recurring pain: "managing what session is what and finding the right session again later down the line."
- HN "A year of vibes" (recording/publishing agent sessions): https://news.ycombinator.com/item?id=46352875

**Internal references**

- `docs/SIMILAR_PROJECTS.md` (2026-08-04 review, visualization-first cluster)
- `docs/IDEAS.md` — "Search and navigation" (cross-project atlas) and "Session comprehension" (activity ribbon, workspace attention map)
- `docs/product-assessments/2026-06-21-product-assessment.md` — P0: Project timeline view
- `docs/explorations/2026-03-14-project-views-exploration.md` — Options A–E, decision A+D
- `docs/explorations/2026-03-16-project-sidebar-filtering-exploration.md` — Propositions A–E, decision A
- Issue #92: https://github.com/supermaciz/sessions-chronicle/issues/92
