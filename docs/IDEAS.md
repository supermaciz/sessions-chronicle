# Ideas Backlog

Raw ideas, weak signals, intuitions. Not concrete enough for a GitHub Issue
yet. When an idea matures, promote it to an issue or an exploration in
`docs/explorations/`.

The product lens is human comprehension of local AI-assisted work: help a
developer understand what happened, where to resume, what changed, and why a
session went wrong. Prefer progressive disclosure and deterministic analysis
before adding AI-generated interpretation.

## Search and navigation

- Saved searches / recent searches
- Search results as moments: show the surrounding message or tool call and jump
  directly to it
- Optional thematic grouping across sessions to recover a body of work whose
  project or wording is no longer remembered
- A cross-project atlas with zoomable time ranges and swimlanes for overlapping
  work

## Session comprehension and forensics

- A compact activity ribbon for a session: human prompts, reasoning, tool calls,
  errors, waits, subagents, and Git events
- A Git-like delegation tree: the main conversation is the trunk and subagents,
  resumes, and forks appear as branches only when they exist
- Explicit relation types in structural views: spawned, read, changed, produced,
  failed, retried, and resumed
- Expose injected context and system events separately from the visible
  conversation
- Detect error/retry loops, abandoned approaches, recovery points, and abrupt
  endings from structured events
- Compare two attempts at the same task as aligned trajectories, highlighting
  common, divergent, and unnecessary steps
- A workspace attention map showing which files or components were read,
  changed, tested, revisited, or rolled back over time
- A context-flow view showing which inputs were available before a decision or
  code change, without claiming causality that the transcript cannot prove

## Personal workflow intelligence

- Usage trends by week, project, task type, and AI assistant
- Lightweight user annotations: successful, failed, in progress, abandoned,
  worth reusing
- Correlate outcomes with duration, token use, tool-call patterns, and AI
  assistant; avoid vanity analytics without an actionable question
- Surface recurring friction patterns and suggest workflow improvements or
  candidate `SKILL.md` changes for explicit user review
- Per-turn token and cost estimates where the source data and pricing model are
  reliable enough to explain the calculation

## Import, export, and storytelling

- Export selected sessions as Markdown, HTML, JSON, Obsidian, or Logseq, with a
  privacy-first redaction preview
- Export a compact context bundle intended for deliberate reuse in a new AI
  assistant session
- A presentation/replay mode that tells the story of a change without exposing
  the full diagnostic transcript
- One-off JSONL import for inspecting a session outside the configured source
  directories

## UX and integration

- Global shortcut to open Sessions Chronicle on the latest session
- A quiet 2D overview of active or recent sessions that emphasizes which one is
  waiting, blocked, failed, or needs user attention
- Optional read-only local MCP endpoint over selected indexed history; keep it
  subordinate to the human-facing desktop workflow

## Git and context

- Link a session to the commit/branch it was working on
- Correlate session steps with code diffs, tests, human interventions, and
  reverted or abandoned changes
- Integration with Git-ai, Agent Trace Spec, entire.io

## Product guardrails

- Stay local-first, privacy-first, Linux-native, and useful across AI assistants
- Prefer overview, zoom and details-on-demand over a full-screen generic DAG
- Keep an exhaustive raw-event inspector as the source of truth behind every
  abstraction
- Do not turn the app into a provider control plane, generic knowledge manager,
  hosted team service, or cross-platform rewrite
- Add AI assistant formats based on demonstrated user value and format stability,
  not coverage for its own sake

