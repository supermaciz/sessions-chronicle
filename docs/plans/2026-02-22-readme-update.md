# README Update Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rewrite README.md for a user-facing audience — clear install instructions, benefit-oriented features, and clean structure.

**Architecture:** Single file edit (`README.md`). No code changes. Removes developer-only sections (build, test, CI) that already live in `AGENTS.md` and `docs/DEVELOPMENT_WORKFLOW.md`.

**Tech Stack:** Markdown, GitHub Flavored Markdown.

---

### Task 1: Rewrite README.md

**Files:**
- Modify: `README.md`

**Step 1: Replace the full content of README.md**

New content:

```markdown
<img src="data/icons/io.github.supermaciz.sessionschronicle.svg" alt="App Icon" width="80" height="80" align="left"/>

# Sessions Chronicle
<br clear="left"/>

[![CI](https://github.com/supermaciz/sessions-chronicle/actions/workflows/ci.yml/badge.svg)](https://github.com/supermaciz/sessions-chronicle/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/supermaciz/sessions-chronicle/graph/badge.svg?token=)](https://codecov.io/gh/supermaciz/sessions-chronicle)

**Browse, search, and resume your AI coding sessions — on GNOME.**

Sessions Chronicle indexes all your local AI tool sessions into a searchable database,
so you can find any conversation, inspect tool calls, and pick up where you left off.


## Features

- **Find any session instantly** — full-text search across all conversations (SQLite FTS5)
- **Browse & filter** — sidebar filters by tool, keyword search with highlighted matches
- **Read conversations comfortably** — rich markdown rendering (code blocks, tables, task lists, blockquotes)
- **Inspect tool calls** — expand inline tool calls and drill down into subagents in the utility pane
- **Resume where you left off** — launch sessions directly from the app in your terminal
- **Supports 4 AI tools** — Claude Code, OpenCode, Codex, Mistral Vibe


## Installation

1. Download the latest `.flatpak` file from the [Releases page](https://github.com/supermaciz/sessions-chronicle/releases)
2. Install it:

```bash
flatpak install sessions-chronicle-*.flatpak
```

3. Launch **Sessions Chronicle** from your app menu, or run:

```bash
flatpak run io.github.supermaciz.sessionschronicle
```


## Screenshots

**Browse and search your sessions**
<img src="docs/screenshots/session_list.png" alt="Session List" width="800"/>

**Read conversations with full markdown rendering and tool call inspection**
<img src="docs/screenshots/session_detail.png" alt="Session Detail" width="800"/>


## Inspiration

Inspired by [agent-sessions](https://github.com/jazzyalex/agent-sessions) and [Copilot Chronicle](https://github.com/nichochar/copilot-chronicle).


## License

Licensed under MIT. See [LICENSE](LICENSE).
```

**Step 2: Verify the file looks correct**

Open `README.md` and confirm:
- Hero section with icon, title, badges, tagline present
- 6 feature bullets with benefit + technical detail
- Installation has 3 steps with code blocks
- 2 screenshots with descriptive captions
- Inspiration mentions both projects
- No Build/Test/CI/CD sections remain

**Step 3: Commit**

```bash
git add README.md docs/plans/2026-02-22-readme-update-design.md docs/plans/2026-02-22-readme-update.md
git commit -m "docs: rewrite README for user-facing audience"
```
