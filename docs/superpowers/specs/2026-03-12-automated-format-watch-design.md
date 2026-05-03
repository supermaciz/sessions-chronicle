# Automated Format Watch Design

**Date:** 2026-03-12  
**Status:** Draft

## Problem

Sessions Chronicle depends on storage and transcript formats owned by external AI
assistants. Those formats are moving targets.

The project already maintains parser documentation and fixtures for Claude Code,
OpenCode, Codex, and Mistral Vibe, but format changes are currently discovered
manually. That approach does not scale well as the project adds more supported
assistants or starts evaluating new sources such as Gemini CLI.

The main risk is not just missing a new format. It is discovering too late that
an existing parser assumption, fixture, or documentation page is now stale.

## Goal

Create an automated watch system that detects likely format changes early,
produces evidence that is easy to review, and helps keep parser docs, fixtures,
and regression coverage aligned with upstream reality.

Primary success criterion:

- Detect likely parser-breaking or documentation-invalidating changes quickly.

## Scope

This design covers a hybrid watch pipeline with two watch levels:

1. **Critical watchlist** for already supported AI assistants:
   - Claude Code
   - OpenCode
   - Codex
   - Mistral Vibe
2. **Exploratory watchlist** for candidate assistants that may matter later,
   such as Gemini CLI.

This design also covers:

- versioned watch configuration inside the repository
- deterministic signature extraction from upstream sources
- risk scoring and report generation
- optional agent-based qualification on strong signals
- automated issues and tightly scoped PRs for docs/tests/fixtures

Out of scope for the first implementation:

- automatic parser code changes
- fully autonomous bot-first infrastructure
- broad social/community monitoring as a primary signal source
- replacing human review for parser architecture decisions

## Design constraints

- Detection must stay auditable. The system should explain why it flagged a
  change and which evidence it used.
- Upstream sources should be treated as authoritative only when they are
  official or directly tied to the assistant's implementation.
- Community posts and reverse-engineering notes can enrich a report but should
  not, by themselves, trigger automatic repository changes.
- The watch system must prefer stable structural signals over fuzzy semantic
  interpretation.
- The project should be able to start inside GitHub Actions and keep the option
  open for a future external bot or agent orchestrator.

## Source accessibility

Not all upstream assistants expose their persistence code equally. The watch
system must account for this and adapt its extraction strategy per assistant.

| Assistant | Repo | Source code access | Extraction strategy |
|-----------|------|--------------------|---------------------|
| OpenCode | [sst/opencode](https://github.com/sst/opencode) (branch `dev`) | Full — open-source TypeScript, Drizzle ORM schema, session types | Direct: shallow clone, extract types and schema from source |
| Codex | [openai/codex](https://github.com/openai/codex) | Full — open-source Rust, protocol types in crate | Direct: shallow clone, extract enum variants and struct fields |
| Mistral Vibe | [mistralai/mistral-vibe](https://github.com/mistralai/mistral-vibe) | Full — open-source Python, session/message classes | Direct: shallow clone, extract dataclass/class fields |
| Claude Code | [anthropics/claude-code](https://github.com/anthropics/claude-code) | Partial — repo is public but does not contain source code; persistence format is not in the repo | Indirect: npm package diffing after updates, changelog keyword scanning, SDK type changes |

### Claude Code extraction strategy

The `anthropics/claude-code` GitHub repository contains documentation, plugins,
and issue tracking but not the application source code. The persistence layer
(JSONL event types, storage paths, message schemas) is embedded in the compiled
npm package `@anthropic-ai/claude-code`.

Available signals for Claude Code:

1. **npm package diffing**: after a version update, extract the installed
   package contents and diff bundled type definitions or generated schemas
   against the previous version snapshot.
2. **Changelog and release notes**: scan the repository's CHANGELOG.md and
   GitHub releases for keywords such as `session`, `storage`, `format`,
   `migration`, `JSONL`.
3. **SDK type changes**: the official TypeScript and Python SDKs expose
   `listSessions` and `getSessionMessages` functions with typed return values.
   Changes to those return types signal persistence format evolution.
4. **Local session diffing**: compare newly generated session files against
   existing fixtures after a Claude Code update to detect structural drift.

Because code-level extraction is not possible, Claude Code relies more heavily
on the documentation evidence extractor and on local session diffing than the
other assistants. Risk scores for Claude Code should reflect this reduced
confidence.

## Approach

Use a two-stage hybrid pipeline.

### Stage A: deterministic detection

On a scheduled GitHub Actions run, the watch system:

1. loads a versioned registry of watched sources
2. fetches official source changes for each watched assistant
3. extracts structural signatures from sensitive files
4. compares those signatures against repository baselines
5. scores the change risk and emits a report

This stage is the source of truth for detection.

### Stage B: agent-based qualification

Only when Stage A reports a strong or converging signal, the pipeline invokes an
agentic qualifier. The qualifier does not perform open-ended monitoring from
scratch. Instead, it receives a dossier assembled by the deterministic stage and
answers targeted questions such as:

- Is this likely a real format change or just an internal refactor?
- Which Sessions Chronicle docs or tests are probably stale?
- Can this be safely turned into a doc/test/fixture PR?

This keeps LLM usage focused on interpretation rather than primary detection.

## Repository artifacts

The watch system should live in repository-owned artifacts so that its behavior
is reviewable.

### 1. Watch source registry

Add a versioned registry such as:

```text
data/format-watch/sources.yaml
```

Each assistant entry should define:

- watch level: `critical` or `exploratory`
- official repositories and docs roots
- sensitive paths to inspect first
- extractor families to run
- risk heuristics
- output mapping to relevant Sessions Chronicle files

Example registry (abbreviated):

```yaml
assistants:
  opencode:
    watch_level: critical
    upstream:
      repo: sst/opencode
      branch: dev
    sensitive_paths:
      - "packages/opencode/src/session/index.ts"
      - "packages/opencode/src/session/message-v2.ts"
      - "packages/opencode/src/**/*.sql.ts"
    extractors:
      - family: structured_type
        language: typescript
        targets:
          - file: "packages/opencode/src/session/index.ts"
            types: ["Info"]
          - file: "packages/opencode/src/session/message-v2.ts"
            types: ["Part"]
      - family: sqlite_schema
        source: drizzle
        targets: ["session", "message", "part", "project"]
    impact_mapping:
      docs: docs/session-formats/opencode.md
      parser: src/parsers/opencode/
      fixtures: tests/fixtures/opencode_storage/

  codex:
    watch_level: critical
    upstream:
      repo: openai/codex
      branch: main
    sensitive_paths:
      - "codex-rs/protocol/src/protocol.rs"
      - "codex-rs/core/src/rollout/recorder.rs"
    extractors:
      - family: structured_type
        language: rust
        targets:
          - file: "codex-rs/protocol/src/protocol.rs"
            types: ["RolloutItem", "EventMsg", "SessionMeta"]
    impact_mapping:
      docs: docs/session-formats/codex.md
      parser: src/parsers/codex.rs
      fixtures: tests/fixtures/codex_sessions/

  claude-code:
    watch_level: critical
    upstream:
      repo: anthropics/claude-code
      branch: main
      # Repo does not contain source code. See "Claude Code extraction
      # strategy" for the indirect approach.
    sensitive_paths:
      - "CHANGELOG.md"
    extractors:
      - family: documentation
        targets:
          - path: "CHANGELOG.md"
            keywords: ["session", "storage", "format", "migration", "JSONL"]
      - family: sdk_types
        packages:
          - name: "@anthropic-ai/claude-code"
            registry: npm
          - name: "claude-code-sdk"
            registry: pypi
      - family: local_session_diff
        fixture_path: tests/fixtures/claude_sessions/
    impact_mapping:
      docs: docs/session-formats/claude-code.md
      parser: src/parsers/claude_code.rs
      fixtures: tests/fixtures/claude_sessions/

  mistral-vibe:
    watch_level: critical
    upstream:
      repo: mistralai/mistral-vibe
      branch: main
    sensitive_paths:
      - "vibe/core/session.py"
      - "vibe/**/*message*"
      - "vibe/**/*log*"
    extractors:
      - family: structured_type
        language: python
        targets:
          - file: "vibe/core/session.py"
            types: []  # to be refined after source inspection
    impact_mapping:
      docs: docs/session-formats/mistral-vibe.md
      parser: src/parsers/mistral_vibe.rs
      fixtures: tests/fixtures/vibe_sessions/
```

### 2. Baselines

Add versioned baseline files such as:

```text
data/format-watch/baselines/<assistant>.json
```

Each baseline stores a compact structural snapshot rather than a full mirror of
upstream code. Typical fields include:

- storage location patterns
- file naming rules
- top-level object fields
- enum or variant values
- SQLite tables, columns, and indexes
- key parser-relevant relationships such as parent-child linkage or tool call
  markers
- source evidence references (repo, path, commit)

Example baseline (OpenCode, abbreviated):

```json
{
  "assistant": "opencode",
  "source_commit": "abc1234def",
  "source_branch": "dev",
  "extracted_at": "2026-03-12T10:00:00Z",
  "session_schema": {
    "fields": {
      "id": { "type": "string", "required": true, "note": "Descending ULID" },
      "slug": { "type": "string", "required": true },
      "projectID": { "type": "string", "required": true },
      "workspaceID": { "type": "string", "required": false },
      "parentID": { "type": "string", "required": false },
      "title": { "type": "string", "required": true },
      "version": { "type": "string", "required": true },
      "summary": { "type": "object", "required": false },
      "share": { "type": "object", "required": false },
      "revert": { "type": "object", "required": false },
      "permission": { "type": "array", "required": false },
      "time": { "type": "object", "required": true }
    }
  },
  "part_types": [
    "text", "reasoning", "file", "tool", "step-start", "step-finish",
    "snapshot", "patch", "agent", "retry", "compaction", "subtask"
  ],
  "sqlite_tables": {
    "session": [
      "id", "project_id", "parentID", "slug", "title", "version",
      "summary_additions", "summary_deletions", "summary_files",
      "summary_diffs", "permission", "created_at", "updated_at"
    ],
    "message": ["id", "session_id", "data"],
    "part": ["id", "message_id", "type", "data"]
  },
  "storage_paths": {
    "root": "~/.local/share/opencode/",
    "db_file": "opencode.db",
    "db_mode": "WAL"
  },
  "id_prefix_convention": {
    "part": "prt_"
  }
}
```

### 3. Reports

Scheduled runs should generate a normalized Markdown report that summarizes:

- sources checked
- what changed
- why it matters
- risk level
- likely impacted repository files
- potentially stale fixtures with specific mismatch details
- recommended next action

Reports are committed in `data/format-watch/reports/YYYY-MM-DD.md`. Reports
older than 6 months may be removed during routine cleanup.

## Watch levels

### Critical watchlist

Critical assistants are already supported and can affect current parser
correctness. Changes here should produce at least a report when structural drift
is detected.

Examples of high-signal changes:

- new or renamed SQLite columns
- new message or part variant values
- changed storage paths or file naming rules
- new subagent or tool call relationship fields
- meaningful changes in documented session persistence behavior

### Exploratory watchlist

Exploratory assistants are not yet supported or are early candidates.

The purpose is to answer:

- Does the assistant persist local sessions in a parseable form?
- Are the storage paths stable enough to target later?
- Is the structure JSONL, JSON, SQLite, directory-based, or hybrid?
- Does the project expose enough primary evidence to support future design work?

Exploratory findings should usually land as reports or issues, not automatic PRs.

## Extractors and signatures

The detection stage combines several extractor families. Each assistant uses a
specific combination of extractors defined in the source registry.

### Per-assistant extractor mapping

**OpenCode** (full source access):
- Structured type extractor on `packages/opencode/src/session/index.ts` for
  `Session.Info` fields
- Structured type extractor on `packages/opencode/src/session/message-v2.ts`
  for the 12 part types
- SQLite schema extractor on Drizzle `*.sql.ts` files for tables `session`,
  `message`, `part`, `project`, `todo`, `permission`, `session_share`
- Path extractor for `~/.local/share/opencode/opencode.db`

**Codex** (full source access):
- Structured type extractor on `codex-rs/protocol/src/protocol.rs` for
  `RolloutItem` variants (`SessionMeta`, `EventMsg`, `ResponseItem`,
  `TurnContext`, `Compacted`) and `EventMsg` variants (`UserMessage`,
  `AgentMessage`, `TurnStarted`, `TurnEnded`, `ExecCommandBegin`,
  `ExecCommandEnd`, `ApprovalRequest`, `TurnDiffEvent`)
- Path extractor for `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
- Documentation extractor on release notes

**Mistral Vibe** (full source access):
- Structured type extractor on session/message Python classes for roles
  (`system`, `user`, `assistant`, `tool`) and message fields
- Path extractor for `~/.vibe/logs/session/` with pattern
  `{prefix}_{id}_{timestamp}.jsonl`
- Documentation extractor on config schema (`[session_logging]`)

**Claude Code** (no source access):
- Documentation extractor on `CHANGELOG.md` and GitHub releases for storage
  and format keywords
- SDK type extractor on `@anthropic-ai/claude-code` npm package and
  `claude-code-sdk` PyPI package for return types of `listSessions` and
  `getSessionMessages`
- Local session diff extractor comparing fresh session files against fixtures
  in `tests/fixtures/claude_sessions/`
- Path extractor for `~/.claude/projects/<encoded-cwd>/*.jsonl`

### Extractor families

#### SQLite schema extractor

Used for assistants that persist data in SQLite or embed schema migrations.
Output examples:

- tables
- columns
- types
- indexes
- foreign-key relationships

#### Structured type extractor

Used on TypeScript, Rust, Python, or JSON schema definitions that describe
persisted session records. Output examples:

- field names
- optional vs required markers when available
- enum or tagged-union variants
- known nested objects relevant to parsing

#### Path and naming extractor

Captures parser-relevant storage conventions:

- root directory paths
- per-session file naming patterns
- multi-file directory structures
- known overrides or environment variables

#### Documentation evidence extractor

Parses official migration notes, changelogs, and docs pages for explicit storage
or format statements. This extractor is lower confidence than code/schema
evidence but useful for contextual scoring.

#### SDK type extractor

Specific to assistants whose source code is not public. Monitors typed SDK
packages for changes to session-related return types and function signatures.

#### Local session diff extractor

Compares freshly generated session files from a locally installed assistant
against existing test fixtures. Useful as a supplementary signal for assistants
without public source code.

### Extractor output contract

All extractors must produce a normalized JSON object conforming to the baseline
schema so that the comparison and scoring stages are assistant-agnostic. The
output must include:

- `assistant`: assistant identifier
- `source_commit` or `source_version`: provenance reference
- `extracted_at`: ISO 8601 timestamp
- Zero or more of the baseline sections (`session_schema`, `part_types`,
  `sqlite_tables`, `storage_paths`, `event_variants`)

The scoring stage diffs the extractor output against the stored baseline using
field-level comparison.

## Risk scoring

The system should score both individual deltas and an aggregate assistant-level
risk.

### Low risk

Examples:

- wording-only documentation changes
- unrelated refactors in non-persistence code
- community discussion without official confirmation

Output:

- include in report only

### Medium risk

Examples:

- official docs mention storage updates but code evidence is incomplete
- type definitions change in non-critical fields
- new optional metadata fields appear

Output:

- report plus enriched recommendation
- optional issue if repeated or converging

### High risk

Examples:

- schema or file layout changes
- renamed fields required by current parsers
- new enum variants likely to affect transcript flattening or tool call parsing
- changed storage location behavior for supported assistants

Output:

- agentic qualification
- issue or tightly scoped PR based on confidence

### Signal aggregation rule

Two or more medium-risk deltas affecting the same domain (schema, storage paths,
or event/part types) in a single run are promoted to high risk. Domains are
defined per-assistant in the source registry.

## Data flow

```text
Scheduled workflow
  -> load source registry
  -> fetch upstream evidence
  -> run extractors (per-assistant adapter)
  -> normalize output to baseline schema
  -> compare with baselines
  -> score deltas
  -> emit watch report (including stale fixture hints)
  -> if score >= threshold: invoke qualifier with evidence dossier
  -> qualifier chooses issue or limited PR
```

The evidence dossier passed to the qualifier should contain:

- assistant name and watch level
- source URLs and commits
- before/after structural signatures
- changed sensitive files
- risk reasons
- likely impacted Sessions Chronicle paths

Expected impacted repository paths typically include:

- `docs/SESSION_FORMAT_ANALYSIS.md`
- `docs/PARSER_DESIGN.md`
- `docs/session-formats/`
- `src/parsers/`
- `tests/fixtures/`
- `tests/`

## Agentic qualifier policy

The qualifier should be GitHub-first, with optional future support for an
external bot.

### Initial execution target

- primary execution: GitHub Action with LLM-backed analysis
- future option: external bot or agent service using the same evidence contract

### Qualifier responsibilities

- classify the change as likely real, ambiguous, or low impact
- summarize expected impact on docs, fixtures, tests, and parsers
- recommend repository follow-up
- prepare a doc/test/fixture PR only when bounded and justified

### Explicit non-goals for the qualifier

- no automatic parser code edits in the first phase
- no autonomous repository-wide refactors
- no action based on community-only signals

## Guardrails

### Source reliability

- Official code, migrations, or typed protocol definitions are primary evidence.
- Official docs and release notes are secondary evidence.
- SDK type definitions are secondary evidence when source code is unavailable.
- Third-party analysis is supporting evidence only.
- Local session diffing is supporting evidence only, useful for confirmation
  but not sufficient to trigger automatic repository changes alone.

### Failure handling

- If a source cannot be fetched, mark it as `unreachable` and report coverage
  degradation.
- If an extractor fails on a source that previously worked, report the failure
  separately from a confirmed format drift.
- Missing data must never be interpreted as a successful "no change" result.

### PR gating

Automatic PRs are allowed only when all of the following are true:

- high-confidence signal from official sources
- scope is limited to docs, fixtures, baseline files, or regression tests
- the resulting change is reviewable in isolation
- the report can explain why parser code was not changed automatically

Otherwise the system opens an issue with evidence and recommendations.

## Baseline bootstrap

Before the watch system can run its first comparison, baselines must be created.

### Initial creation

1. For each assistant with source access (OpenCode, Codex, Mistral Vibe): run
   the extractors against the current upstream commit and produce the first
   `baselines/<assistant>.json`.
2. For Claude Code: manually assemble the baseline from current fixture
   analysis, SDK type inspection, and existing documentation in
   `docs/session-formats/claude-code.md`.
3. Cross-validate each baseline against the corresponding format documentation
   in `docs/session-formats/<assistant>.md`. Any divergence indicates either
   stale documentation or an incorrect extractor.
4. Commit validated baselines into the repository.

### Re-baseline after confirmed changes

After a confirmed upstream change has been fully integrated (parser updated,
fixtures updated, documentation updated), the maintainer updates the baseline:

```bash
./scripts/update-baseline.sh <assistant>
```

This re-runs the extractors and overwrites the stored baseline with the new
snapshot. The updated baseline is committed alongside the parser and fixture
changes so that subsequent watch runs do not re-flag the already-handled change.

## Operational parameters

- **Schedule**: weekly (Sunday 02:00 UTC)
- **GitHub Actions budget**: approximately 10 minutes per run (4 shallow clones
  plus extractors plus diff)
- **LLM budget (Stage B)**: approximately 1–2 qualifier calls per month,
  triggered only on high-risk signals
- **Retry policy**: 1 automatic retry after 5 minutes on network failure; mark
  source as `unreachable` after the second failure
- **Report retention**: reports committed in `data/format-watch/reports/`;
  reports older than 6 months may be removed
- **Health check**: if no report has been generated in 14 days, the workflow
  opens a self-diagnostic issue to flag possible scheduling problems (GitHub
  Actions disables scheduled workflows after 60 days of repository inactivity)

## Testing strategy

The watch system itself needs deterministic tests.

### Unit tests

- extractor behavior on representative source snippets
- baseline comparison logic
- scoring rules for optional vs required field changes
- dossier generation
- extractor output contract validation

### Fixture-style tests

- fake SQLite schema changes
- fake enum additions
- fake path migration examples
- fake documentation-only changes
- medium-to-high promotion with converging signals

### Output tests

- stable Markdown report rendering
- clear mapping from detected deltas to repository impact hints
- fixture staleness hints in report output

## Rollout phases

### Phase 1

- critical watchlist only
- deterministic detection only (Stage A)
- baseline bootstrap for all 4 assistants
- scheduled report output
- issue creation on strong deterministic signals
- no PR automation

### Phase 2

- enable agentic qualification for high-risk signals (Stage B)
- allow limited automatic PRs for docs, fixtures, baselines, and regression
  tests

### Phase 3

- expand exploratory watchlist, starting with Gemini CLI
- track maturity of candidate assistants and whether their formats are stable

### Phase 4

- add optional external bot orchestration (such as OpenClaw or Moltis) if
  GitHub Actions becomes too limiting for real-time or conversational workflows
- keep the same registry, baseline, and evidence dossier contract

## Expected outcome

- Faster detection of upstream format changes that could invalidate current
  parser assumptions.
- Less manual archaeology when supported assistants evolve their persistence
  formats.
- Better alignment between upstream evidence, local format docs, fixtures, and
  regression coverage.
- A clean path to monitor future assistants without mixing exploratory research
  with current parser breakage response.
