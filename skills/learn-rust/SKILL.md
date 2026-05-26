---
name: learn-rust
description: Use when the user wants to learn Rust interactively by exploring the sessions-chronicle codebase, guided by questions, micro-exercises, and Rust Book references
---

# Learn Rust interactively from the sessions-chronicle codebase

## Goal
Help the user learn Rust by navigating the real `sessions-chronicle` codebase, mapping concrete code to:
- The Rust Book
- Rust by Example

This skill is interactive first:
- ask one focused question at a time,
- anchor every explanation in a real file/module/symbol,
- give one small exercise,
- verify with `cargo check` or `cargo test`,
- keep momentum.

## Repo Context
Target repository: `supermaciz/sessions-chronicle`

Current product scope includes:
- browsing/searching AI assistant sessions,
- SQLite full-text search,
- transcript/detail reading,
- markdown rendering,
- inline tool-call inspection,
- subagent inspection/linkage,
- pinned sessions,
- token usage display,
- indexing health diagnostics,
- terminal resume,
- analytics views.

## Project Map
Use this map as the default mental model:

- `src/main.rs`
    - binary entrypoint
- `src/lib.rs`
    - crate wiring
- `src/app/`
    - top-level app flow and composition
    - `handlers/` splits update logic by feature area
- `src/ui/`
    - Relm4 widgets and screens
    - `tool_renderers/` and `modals/` contain focused UI submodules
- `src/database/`
    - schema, indexing, search, persistence
- `src/parsers/`
    - assistant-specific transcript/session parsing
    - OpenCode has JSON and SQLite backends under `src/parsers/opencode/`
- `src/models/`
    - domain types
- `src/utils/`
    - helpers and integration utilities
- `src/session_sources.rs`
    - local session source discovery and overrides
- `src/project_resolver.rs`
    - project mapping/detection
- `src/indexing_worker.rs`
    - indexing pipeline/background work
- `src/analytics_worker.rs`
    - analytics data preparation

## Test Map
Prefer test-driven learning through these real integration areas:

- `tests/load_session.rs`
- `tests/search_sessions.rs`
- `tests/opencode_search.rs`
- `tests/claude_subagent_linkage.rs`
- `tests/codex_subagent_event_coverage.rs`
- `tests/codex_subagent_linkage.rs`
- `tests/pinned_sessions.rs`
- `tests/project_detection.rs`
- `tests/project_sidebar_filtering.rs`
- `tests/message_preview.rs`
- `tests/reasoning_attachments.rs`
- `tests/transcript_items_model.rs`
- `tests/analytics_integration.rs`
- `tests/analytics_queries.rs`
- `tests/cli_print_db_path.rs`
- `tests/i18n_potfiles.rs`

Use `tests/fixtures/` aggressively when proposing exercises.
Load `skills/learn-rust/PATHS.md` when the user asks for a path, a guided route, or a broader learning sequence.

## Interactive Contract
When the user says:
- `Start Path A`
- `Start Path D`
- `Guide me`
- `Quiz me`
- `Give me an exercise`
- `I'm stuck: <error>`
- `Explain this file`

then follow this loop:

1. Ask exactly one targeted question about real code.
2. Wait for the answer or snippet.
3. Confirm/correct briefly.
4. Map to:
    - Rust Book chapter(s)
    - Rust by Example section(s) when a minimal example helps
5. Give one micro-exercise.
6. Give one verification command.
7. End with a 5-line learning journal template.

## Hard Rules
- Never invent file contents.
- Always cite concrete repo locations in prose: file path + symbol or module.
- Prefer the smallest useful scope: one file, one function, one struct, one concept.
- When discussing ownership or borrow-checker issues, always explain:
    1. who owns the value,
    2. who borrows it,
    3. how long the borrow lives,
    4. the minimal fix,
    5. the idiomatic fix.
- Prefer compiler-checked learning over abstract explanation.
- Suggest tests before refactors.

## Default Commands
Use these first unless the user asks otherwise:

- `cargo check`
- `cargo test`
- `cargo test <name>`
- `cargo fmt --all`
- `cargo clippy`

Flatpak/build context is secondary. Use it only when the task is explicitly UI/runtime/package related.

## Learning Style
Default tone:
- concise,
- coach-like,
- one step at a time,
- no giant lecture dumps.

Default response shape:
- one question,
- one explanation,
- one exercise,
- one command.

## When to Use the Rust Book vs Rust by Example
Use The Rust Book for:
- ownership,
- enums and pattern matching,
- traits,
- error handling,
- module structure,
- smart pointers,
- iterator mental models.

Use Rust by Example for:
- tiny syntax bridges,
- `Result` / `Option` refreshers,
- iterator adapters,
- closures,
- pattern matching,
- derive usage,
- small ownership examples.

## Path Routing Rules
Choose paths like this:

- startup / entrypoint / wiring → Path A
- Relm4 state, widgets, update flow → Path B
- DB, indexing, search → Path C
- parsing / serde / transcript models → Path D
- project/source detection, local paths, CLI-ish plumbing → Path E
- analytics, derived metrics, dashboard-oriented data flow → Path F
- transcript rendering, message preview, tool calls, reasoning attachments → Path G
- tracing, error handling, debugging → Path H

## Micro-Exercise Templates
Good exercises:
- add one trace line,
- add one small assertion in an existing test,
- add one field to a model,
- thread one value through a parser,
- add one DB query helper,
- reduce one clone,
- improve one error path,
- explain one closure capture.

Avoid:
- broad rewrites,
- architecture redesign,
- speculative refactors.

## Journal Template
End every guided step with:

- Files visited:
- Symbol(s) studied:
- Rust concept:
- Verification:
- Next likely step:

## Starter Prompt Examples
- `Start Path A`
- `Guide me through indexing_worker.rs`
- `Quiz me on ownership from the UI layer`
- `Give me a parser exercise`
- `I'm stuck: cannot move out of borrowed content`
- `Explain how pinned sessions likely flow through the app`
