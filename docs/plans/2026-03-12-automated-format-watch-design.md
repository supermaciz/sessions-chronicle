# Automated Format Watch Design

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

Example source categories:

- schema definitions
- storage migrations
- persistence models and typed protocol definitions
- release notes or migration notes
- path-resolution code for session storage locations

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

### 3. Reports

Scheduled runs should generate a normalized Markdown report that summarizes:

- sources checked
- what changed
- why it matters
- risk level
- likely impacted repository files
- recommended next action

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

The detection stage should combine several extractor families.

### SQLite schema extractor

Used for assistants that persist data in SQLite or embed schema migrations.
Output examples:

- tables
- columns
- types
- indexes
- foreign-key relationships

### Structured type extractor

Used on TypeScript, Rust, Python, or JSON schema definitions that describe
persisted session records. Output examples:

- field names
- optional vs required markers when available
- enum or tagged-union variants
- known nested objects relevant to parsing

### Path and naming extractor

Captures parser-relevant storage conventions:

- root directory paths
- per-session file naming patterns
- multi-file directory structures
- known overrides or environment variables

### Documentation evidence extractor

Parses official migration notes, changelogs, and docs pages for explicit storage
or format statements. This extractor is lower confidence than code/schema
evidence but useful for contextual scoring.

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

Multiple medium signals for the same assistant in the same run may be promoted
to high risk when they converge on the same format area.

## Data flow

```text
Scheduled workflow
  -> load source registry
  -> fetch upstream evidence
  -> run extractors
  -> compare with baselines
  -> score deltas
  -> emit watch report
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
- Third-party analysis is supporting evidence only.

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

## Testing strategy

The watch system itself needs deterministic tests.

### Unit tests

- extractor behavior on representative source snippets
- baseline comparison logic
- scoring rules for optional vs required field changes
- dossier generation

### Fixture-style tests

- fake SQLite schema changes
- fake enum additions
- fake path migration examples
- fake documentation-only changes

### Output tests

- stable Markdown report rendering
- clear mapping from detected deltas to repository impact hints

## Rollout phases

### Phase 1

- critical watchlist only
- deterministic detection only
- scheduled report output
- issue creation on strong deterministic signals
- no PR automation

### Phase 2

- enable agentic qualification for high-risk signals
- allow limited automatic PRs for docs, fixtures, baselines, and regression tests

### Phase 3

- expand exploratory watchlist, starting with Gemini CLI
- track maturity of candidate assistants and whether their formats are stable

### Phase 4

- add optional external bot orchestration if GitHub Actions becomes too limiting
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
