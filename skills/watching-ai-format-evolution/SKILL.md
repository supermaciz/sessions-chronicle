---
name: watching-ai-format-evolution
description: Use when manually monitoring, watching, tracking, or reviewing AI assistant storage, session, transcript, JSONL, or SQLite format drift after official upstream repository, changelog, or package updates, especially when fixtures, parser docs, tests, or parsers may be stale.
---

# Watching AI Format Evolution

## Overview

Run a manual format-watch pass that prioritizes official evidence, extracts only parser-relevant structural signals, and produces a bounded maintenance report.
Core principle: separate confirmed upstream evidence from local inference.

## Required Output Contract

Unless the user explicitly asks for a different structure, the response MUST be organized as a separate section for each assistant checked or considered.
This applies to general monthly watch passes, monthly summaries, watch updates, exploratory checks, and triage requests.

For each assistant section, use these exact headings in this exact order:

1. `Sources checked`
2. `Confirmed deltas`
3. `Likely deltas`
4. `Unknowns`
5. `Confidence`
6. `Risk`
7. `Inspect next`
8. `Recommended action`

Do not rename, merge, reorder, omit, or replace these headings.
If a heading has no content, write an explicit placeholder such as `None confirmed`, `Unknown`, or `Not checked`.
The exact heading contract is mandatory. Treat it like a schema, not a stylistic preference.
Use `## <Assistant name>` for the assistant section heading, then the exact eight `###` headings below it.
Do not answer this skill as freeform prose.

## Format Violations

The following are violations of this skill:

- Combining multiple assistants into one shared report
- Using only some of the required headings
- Replacing headings with close variants such as `Evidence`, `Findings`, `Next steps`, or `Action`
- Writing prose summaries instead of the required per-assistant template
- Using bold assistant names instead of `## <Assistant name>` headings
- Omitting headings because the answer feels obvious or evidence is thin

Partial compliance is non-compliance.

## When to Use

- User asks to watch, monitor, track, or review AI assistant format evolution.
- User wants a periodic manual check on supported or candidate assistants.
- User wants to know whether docs, fixtures, tests, or parser assumptions may be stale.
- User explicitly does not want GitHub Actions or an external bot/service.

## When NOT to Use

- User wants parser implementation now.
- User is debugging one concrete parser failure with a reproducible fixture.
- User wants broad market research rather than storage/transcript format evidence.

## Hard Rules

- Official source code, migrations, and typed protocol definitions are primary evidence.
- Official docs, changelogs, releases, and package metadata are secondary evidence.
- Local session capture and fixture diffing are supporting evidence by default.
- For closed-source assistants such as Claude Code, a fresh real session diff can justify docs, fixtures, and tests updates, but not parser edits by itself.
- Community posts are context only.
- For assistants with public persistence source, inspect sensitive source files before release notes.
- Label every conclusion as `confirmed`, `likely`, or `unknown`.
- Do not recommend parser edits without concrete format evidence.
- Do not default to GitHub Actions, OpenClaw-style services, or autonomous monitoring.

## Watch Loop

1. Build the watchlist.
Critical: already-supported assistants only.
In this repo that means Claude Code, OpenCode, Codex, and Mistral Vibe.
Exploratory: candidate assistants only if the user asks.
2. Collect authoritative evidence per assistant.
3. Extract parser-relevant signals only: storage paths, file naming, top-level fields, enum/variant values, SQLite tables/columns, tool call linkage, subagent markers.
4. Compare those signals to local docs, fixtures, tests, and parser assumptions.
5. Produce a bounded report with confidence, risk, impacted local paths, and smallest justified next step.

## Assistant Matrix

| Assistant | Watch level | Check first | Then check | Watch for |
|---|---|---|---|
| OpenCode | critical | session types, Drizzle schema, storage paths in official source | release notes | SQLite tables/columns, part types, path changes |
| Codex | critical | Rust protocol/session types, rollout recorder, session path code | release notes | enum variants, JSONL layout, storage naming |
| Mistral Vibe | critical | Python session/message classes, session logging config | release notes/docs | roles, fields, filename pattern |
| Claude Code | critical | official changelog/releases, package or SDK session types, local sessions | repo docs | path changes, event field drift, partial/incomplete transcript behavior |
| Gemini CLI | exploratory only | official docs plus chat/session persistence source | release notes | JSON structure, file layout, schema stability |

## Output Format

For each assistant, report exactly these sections:

- `Sources checked:` official URLs, packages, commits, or versions
- `Confirmed deltas:` only evidence-backed structural changes
- `Likely deltas:` plausible impact not yet proven
- `Unknowns:` what still needs direct evidence
- `Confidence:` low, medium, or high
- `Risk:` low, medium, or high
- `Inspect next:` existing local paths only
- `Recommended action:` docs, fixtures, tests, parser, or no action yet

Before sending the final answer, perform a heading check:

- count assistant sections
- verify all 8 headings are present in each section
- verify heading text matches exactly
- if any heading is missing, add it before responding

## Example

```markdown
## Claude Code

### Sources checked
- official changelog vX.Y.Z
- npm package @anthropic-ai/claude-code vX.Y.Z
- fresh local session sample from ~/.claude/projects/...

### Confirmed deltas
- changelog mentions storage migration

### Likely deltas
- local docs about storage paths may be stale

### Unknowns
- whether event field names changed or only storage location changed

### Confidence
medium

### Risk
medium

### Inspect next
- docs/session-formats/claude-code.md
- tests/fixtures/claude_sessions/
- src/parsers/claude_code.rs

### Recommended action
- refresh docs and fixtures first
- hold parser edits until a fresh fixture or source/type diff shows parser-impacting drift
```

## Common Mistakes

| Mistake | Fix |
|---|---|
| Treating release notes as schema proof | For open-source assistants, inspect source/types first |
| Mixing upstream facts with repo assumptions | Label repo-side impact as inference, not evidence |
| Recommending parser edits too early | Update docs/fixtures/tests first unless parser breakage is confirmed |
| Naming local files that were not verified | Only cite existing repository paths |
| Treating one fresh session as a stable contract | Use it as confirmation, not sole authority |

## Rationalization Table

| Excuse | Reality |
|---|---|
| "Release notes are enough" | They are secondary evidence for assistants with public source. |
| "Our parser assumptions imply upstream drift" | They show impact surface, not proof of a format change. |
| "One local session proves the contract changed" | It proves one sample changed, not that the upstream format is stable or general. |
| "I should suggest CI or a bot" | This skill is for manual watch passes unless the user expands scope. |
| "I can recommend parser edits now to be safe" | Parser edits need concrete structural evidence, not precaution alone. |

## Red Flags - Stop and Correct

- Claiming `confirmed` without a cited official source, package, or checked file
- Recommending parser edits without a specific field, variant, path, or schema delta
- Using community chatter as primary evidence
- Listing impacted local paths that do not exist
- Defaulting to GitHub Actions or external agent services
