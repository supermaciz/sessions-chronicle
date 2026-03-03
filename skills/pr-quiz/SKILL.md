---
name: pr-quiz
description: Use when reviewing a Sessions Chronicle PR or local diff and the user wants an interactive A/B quiz focused on architecture, data safety, and Rust/Relm4 trade-offs.
---

# PR Quiz Facilitator (Sessions Chronicle)

## Overview

Turn a PR diff into an interactive review quiz for Sessions Chronicle.
Goal: evaluate trade-offs (not find one absolute correct answer), aligned with project guardrails.

## When to Use

Use when the user asks for:
- a code review quiz on a PR or diff
- learning through A/B implementation choices
- review coaching focused on Rust, Relm4, SQLite, or parser decisions

Do not use when user asks for direct refactoring or direct bugfix implementation.

## Workflow

1. Ask for input source if missing:
   - GitHub PR URL, or
   - local repo + base branch (usually `main`)
2. Build context before quiz:
   - PR intent (title, description, commits)
   - related design doc in `docs/plans/*-design.md` or `*-exploration.md` when available
   - full diff scope (`base...HEAD`)
3. Reduce scope if diff is large (>15 files or >400 changed lines):
   - announce scope reduction
   - keep core logic files and skip generated, lock, config, and cosmetic-only files
4. Select 1-5 comparisons with priority:
   1. `src/database/`, `src/parsers/`, `src/session_sources.rs`
   2. `src/ui/` behavior and data-flow files
   3. `src/models/` API shape and contracts
   4. tests covering changed behavior
5. Present one comparison at a time and wait for answer before moving on.
6. After each answer:
   - if user answers only "A" or "B", ask "Pourquoi ?"
   - reveal PR origin (A or B)
   - explain trade-offs fairly
   - recommend keep, improve, or context-dependent
7. End with review phase:
   - per comparison: chosen option, PR origin, trade-off analysis, recommendation
   - global: strengths + 1-2 actionable improvements

## Sessions Chronicle Evaluation Lens

Always evaluate options against these guardrails:

- **Data safety**
  - stream JSONL with `BufReader` line iteration
  - handle malformed or untrusted entries gracefully
  - continue indexing where possible
- **Path handling**
  - no hardcoded user or system paths
  - use existing path-resolution helpers (`session_sources.rs` patterns)
- **Architecture consistency**
  - keep Relm4 message and update flow clear
  - keep boundaries clear across `models/`, `database/`, and `ui/`
- **Performance**
  - consider startup and indexing cost
  - avoid loading large logs fully into memory
- **UX consistency**
  - preserve GNOME and Libadwaita interaction patterns
  - preserve keyboard and accessibility behavior
- **Verification discipline**
  - tests updated for behavior changes
  - CI-parity commands considered

## Comparison Quality Rules

- Avoid trivia (formatting, naming-only, or cosmetic-only changes)
- Focus on API shape, data flow, error handling, and complexity
- Both options must be plausible in this codebase
- Never reveal PR option before user answer
- Randomize PR placement between A and B
- Keep explanations specific to changed code and project constraints

## Session State

Track this state across the conversation:
- total number of comparisons planned
- current comparison index
- whether the user answered current comparison before moving on
- no early summary before all planned comparisons are completed

## Output Format

Start with:
"I found X comparisons across Y files. Let's go one by one."

For each item:

### Comparison N/Total - <file[:line]>
Context: <goal in this code path>
Question: <decision framing>
Option A:
```rust
<code>
```
Option B:
```rust
<code>
```
Your choice (A or B) and why?

## Review Phase

For each comparison:
- chosen option
- PR origin
- trade-off analysis
- recommendation (keep / improve / context-dependent)

Final wrap-up:
- what the PR does well
- one or two concrete improvements
- suggested verification commands when relevant:
  - `cargo fmt --all -- --check`
  - `cargo clippy --all -- -D warnings`
  - `cargo test --all --no-fail-fast`
  - Flatpak build check if packaging/build files changed

## Error Handling

If diff cannot be loaded:
- explain why clearly
- give next action (valid PR URL, branch name, or repo path check)
