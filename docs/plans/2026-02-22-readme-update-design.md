# README Update Design

**Date:** 2026-02-22
**Status:** Approved

## Goal

Rewrite the README for a user-facing audience (people who want to install and use the app), not developers/contributors. Dev setup documentation stays in `AGENTS.md` and `docs/DEVELOPMENT_WORKFLOW.md`.

## Structure

### 1. Hero section
- App icon (existing SVG)
- Title: Sessions Chronicle
- Badges: CI (ci.yml) + codecov
- Tagline: "Browse, search, and resume your AI coding sessions — on GNOME."
- One-sentence description of what the app does

### 2. Features
Mix of user benefit (heading) + technical detail (inline):
- Find any session instantly — full-text search (SQLite FTS5)
- Browse & filter — sidebar tool filters, search highlighting
- Rich markdown rendering — code blocks, tables, task lists, blockquotes
- Inspect tool calls — inline expanders, subagent drill-down in utility pane
- Resume where you left off — terminal launch from the app
- Supports 4 AI tools — Claude Code, OpenCode, Codex, Mistral Vibe

### 3. Installation
1. Download `.flatpak` from GitHub Releases
2. `flatpak install sessions-chronicle-*.flatpak`
3. Launch from app menu or `flatpak run io.github.supermaciz.sessionschronicle`

### 4. Screenshots
Two existing screenshots with descriptive captions:
- Session list: "Browse and search your sessions"
- Session detail: "Read conversations with full markdown rendering and tool call inspection"

### 5. Inspiration
- [agent-sessions](https://github.com/jazzyalex/agent-sessions)
- [Copilot Chronicle](https://github.com/nichochar/copilot-chronicle)

### 6. License
MIT — link to LICENSE file.

## Removed sections
- Prerequisites (flatpak install instructions)
- Building the project
- Running the project
- Testing
- CI/CD

These remain in `AGENTS.md` and `docs/DEVELOPMENT_WORKFLOW.md`.
