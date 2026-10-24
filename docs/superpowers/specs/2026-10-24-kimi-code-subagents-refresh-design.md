# Kimi Code Subagents Refresh Design

**Date:** 2026-10-24

**Status:** Ready for review

**References:** [Kimi Code format reference](../../session-formats/kimi-code.md),
[original Kimi Code support design](2026-07-29-kimi-code-support-design.md)

## Intent

Kimi Code support shipped in #196, but subagents have never worked on real
sessions. The 2.3.x format watch (2026-10-23) found that every real child
journal is rejected, so no Kimi subagent has ever been linked. The test suite
stays green because the fixtures use a prompt shape that the CLI never writes.
Local sampling on 2026-10-24 also surfaced resumed subagents and `AgentSwarm`
fan-outs, which the parser cannot represent today.

This work makes Kimi subagents visible and navigable on real sessions,
including resumed subagents and `AgentSwarm` launches. It also makes the token
totals complete.

Evidence used for this design (local `~/.kimi-code`, CLI 2.3.x):

- `session_d7f7b8ed-…`: four `Agent` calls, two of them re-driving `agent-1`
  (one with `args.resume`, one identified only by the result header).
- `session_acdf68b7-…`: one `AgentSwarm` call that spawns `agent-0`…`agent-4`.
- `session_3829c3d5-…`: an older `AgentSwarm` call whose result is
  `{"output":"Session closed","isError":true}`, with children still on disk.
- 629 `usage.record` entries across all local journals, all with
  `usageScope: "turn"`.

## Goals

1. Real child journals are parsed and linked to their parent call.
2. Child transcripts and titles start with the delegated task, not the
   injected `<git-context>` block.
3. Subagent rows show the subagent profile and a clean result summary.
4. Every `Agent` call that drives a subagent, including resumes, becomes a
   subagent item that links to the right child session.
5. An `AgentSwarm` call becomes one subagent item per spawned child, at the
   call's position in the transcript, like `Agent` calls.
6. Session token totals include every `usage.record`, compaction included.
7. The fixtures and tests use the real on-disk shapes, so they fail if the
   real-data path breaks again.

## Non-Goals

- Grouping or collapsing the subagent items of one swarm. This is a possible
  follow-up (a swarm is a natural group with a shared heading), but it is UI
  work outside this spec.
- Parsing the legacy `~/.kimi` layout.
- Showing subagent status (`completed` / `failed`) or swarm `outcome` as new UI.
  A matched call loses its generic tool call record, and with it the error
  flag: subagent items show no status. Decision: errors stay visible through
  `result_summary` instead (see sections 4 and 5). Keeping the tool call
  linked to the subagent row, as Mistral Vibe does, is a possible follow-up
  together with swarm grouping.
- Rolling child token usage into the parent session total. Current behavior is
  unchanged.
- Any change to other AI assistants' parsers.

## Anti-Goals

- **Wrong links.** If the evidence is ambiguous, a call stays a generic tool
  call. A missing link is acceptable; a link to the wrong child is not.
- **Loosening the main-session gate.** Injected prompts (`injection`,
  `system_trigger`, `skill_activation`, …) in the main journal must not become
  user messages, and must not make an otherwise empty bundle indexable.
- **Fixtures that diverge from reality again.** No fixture may use a prompt
  origin or record shape that has not been observed in real data or in the
  upstream source.

## Current Behavior (`crates/core/src/parsers/kimi_code.rs`)

- `is_real_turn_prompt` accepts only `origin.kind == "user"`. Child task
  prompts are `{"kind":"system_trigger","name":"subagent"}`, so every child
  journal fails with `ParseError::NoUserMessages` and is put in
  `omitted_agents`.
- `match_agent_calls` matches at most one call per child (`used_children`). A
  second explicit call to the same child stays generic.
- `Subagent.agent_name` comes from `args.subagent_type` only.
  `Subagent.result_summary` is the raw result text, including the
  `agent_id:` / `actual_subagent_type:` / `status:` header and the `[summary]`
  marker.
- Only `name == "Agent"` calls are collected as candidates. `AgentSwarm` stays a
  generic tool call, and its children (once parsable) would have no parent-side
  link. In the UI, child sessions are reachable only through
  `Subagent.child_session_id`, so the children would be indexed but
  unreachable.
- Tokens: sum of `usage.record` with `usageScope == "turn"`, with a fallback to
  `step.end.usage`.

## Design

### 1. Child prompt gate

The prompt rule depends on the journal:

- **Main journal:** unchanged. Only `turn.prompt` with `origin.kind == "user"`
  counts, with the existing `context.append_message` fallback.
- **Child journal** (any agent other than `main`): `turn.prompt` counts when
  `origin.kind == "user"`, or when `origin.kind == "system_trigger"` **and**
  `origin.name == "subagent"`. Other `system_trigger` names, `injection`, and
  the other kinds are still rejected.

Accepted child prompts become `Role::User` messages, as main prompts do. This
covers resumed subagents (the second and third task prompts in
`agent-1/wire.jsonl` are also `system_trigger`/`subagent`).

**Forked subagents** (experimental `fork: true`). The copied parent context
arrives as `context.append_message` records. Because a forked child also has
an accepted `turn.prompt`, the existing rule "read `context.append_message`
only when no real `turn.prompt` exists" already hides the copied context. This
rule must keep applying with the new gate: the scan and the parse must agree
on what "real prompt" means for that journal.

### 2. Stripping `<git-context>`

Kimi prepends a `<git-context>…</git-context>` block (working directory,
remote, branch, dirty files) to the first text part of a child task prompt.
It was seen on all 9 local child journals and never in a main prompt.

Rule, applied to accepted child prompts: if the first text part, after leading
whitespace, starts with `<git-context>` and contains `</git-context>`, remove
everything up to and including the closing tag, then trim leading whitespace.
Without a closing tag, or when nothing would remain, keep the text as is
(builder's call: losing the only user message is worse than showing the
block). The stripped text feeds the message, and through
`extract_first_prompt`, the child's title.

Main-journal prompts are not touched.

### 3. Agent metadata from `state.json`

`AgentMetadata` additionally reads `labels.profileName` and the swarm item.
The swarm item comes from `labels.swarmItem` (2.3.x), with a fallback to a
top-level `swarmItem` on the agent entry (older CLI, seen in
`session_3829c3d5-…`). "`labels.swarmItem`" below means this resolved value.
When the top-level `parentAgentId` is missing,
`labels.parentAgentId` is the fallback (builder's call: 2.3.x writes both,
and only the top-level one is read today). Invalid or non-string values are
ignored, as with the other optional metadata.

### 4. `Agent` calls: name, summary, resumes

**Agent name precedence:** the child's `labels.profileName`, then the result
header `actual_subagent_type:`, then `args.subagent_type`.

**Result summary cleanup:** when the result text starts with an `agent_id:`
line, drop the leading run of `key: value` header lines up to the first blank
line. Then drop a leading `[summary]` line if present, and trim. A failed run
keeps its body (`subagent error: …`), which is how the failure stays visible
once the generic tool call is gone. Text without the header is kept as is.
The cleaned text is also what feeds `result_summary` for indexing. The raw
output stays on the tool call record only if the call stays generic.

**Resumes (many calls → one child).** The matching rules change as follows:

1. Explicit evidence (`args.agent_id` / `agentId` / `resume`, structured result
   ids, the `agent_id:` header line) may link **several calls to the same
   child**. The existing consistency rules still apply per call: conflicting,
   blank, null, or unknown evidence keeps that call generic.
2. The chronological fallback is unchanged and still one-to-one. It only
   pairs calls with **no** evidence against children that **no** call has
   matched yet.
3. Children with a `labels.swarmItem` are excluded from the `Agent`
   chronological pool (see swarm matching below).

Each matched call keeps its own subagent row
(`kimi-agent-call::<session>::<raw call id>`), its own prompt (`continue`, …),
and its own cleaned summary. All of these rows share the same
`child_session_id`. The existing unit test
`duplicate_explicit_agent_id_does_not_fall_back_to_another_child` must change:
both calls now link to `agent-0`.

### 5. `AgentSwarm` calls

Observed call shape (2.3.x):

```json
{"name":"AgentSwarm","args":{"description":"…","subagent_type":"explore",
 "prompt_template":"… {{item}} …","items":["Claude Code (…)","OpenCode (…)", "…"]}}
```

Observed result shape (`result.output`, a string):

```text
<agent_swarm_result>
<summary>completed: 5</summary>
<subagent agent_id="agent-0" item="Claude Code (…)" outcome="completed">…markdown…</subagent>
…
</agent_swarm_result>
```

Each child has `labels.swarmItem` (equal to its item) and
`labels.profileName` in `state.json`.

**Candidates.** `AgentSwarm` calls are collected next to `Agent` calls, with
their transcript position, time, args, and result.

**Resolving children, per swarm call:**

1. **From the result** (preferred): parse each `<subagent agent_id="…"
   item="…" …>body</subagent>` block. Each `agent_id` must be a resolved child
   of the calling agent. If the child has a `labels.swarmItem`, it must equal
   the block's `item`. When `args.items` is present, the blocks must cover
   exactly its entries. Missing or extra blocks make the result unusable, and
   step 2 applies (builder's call).
2. **Without a usable result** (pending, interrupted, `Session closed`, or no
   `<agent_swarm_result>`): match each entry of `args.items` to the single
   child of the calling agent whose `labels.swarmItem` equals it, that is not
   already matched, and whose journal starts no earlier than the call. A
   duplicated item, or an item with zero or several candidates, makes the
   fallback fail.
3. **All or nothing:** expand the call only if every listed child resolves.
   Otherwise the call stays a generic tool call. Its children are still
   indexed as hidden child sessions, as with ambiguous `Agent` calls today.

Swarm matching runs before `Agent` matching for the same parent. Children
consumed by a swarm are not available to `Agent` explicit or chronological
matching, except through explicit evidence from a later resume call.

**Expansion.** A resolved swarm call is replaced in the parent transcript by
N consecutive `Subagent` items at the call's position, in the order of
`args.items`, falling back to result block order. The generic `AgentSwarm`
tool call is removed. Each item's `Subagent` row:

| Field | Value |
|-------|-------|
| `id` | `kimi-agent-call::<session>::<raw call id>::<agent_id>` |
| `agent_id` | child agent id |
| `agent_name` | child `labels.profileName`, else `args.subagent_type` |
| `title` | the swarm item text |
| `prompt` | `args.prompt_template` with every `{{item}}` replaced by the item |
| `result_summary` | the trimmed body of the matching `<subagent>` block; without one, the call's error output when `isError` is true (for example `Session closed`), else none |
| `child_session_id` | the synthetic child session id |
| `parser_ref` | raw call id |

**Index renumbering.** Inserting N items in place of one shifts every later
`item_index`. Expansion is done in a single pass after all call matching for
the journal is finished, never during streaming:

1. Rebuild `transcript_items` in order. Keep an old → new index map, where a
   swarm call's old index maps to the index of its first expanded item.
   Renumber `item_index` so the indexes stay contiguous from 0.
2. Apply the map to every structure that stores a transcript index:
   `ReasoningAttachment.transcript_item_index` and the `item_index` of context
   entries (`models/context.rs`, empty for Kimi today but covered for safety).
   Reasoning that was attached to the swarm call therefore moves to the first
   expanded item.
3. A test checks the invariant on an expanded fixture: indexes are contiguous,
   and every reasoning attachment and context entry points to an existing
   item of the expected kind.

`Agent` matches keep their in-place conversion (one item becomes one item),
and they run before the expansion pass so the map stays one-to-one for them.

**Result block parsing.** A small line-oriented scan of `result.output`, not
an XML parser. Both local swarm results contain no entities but plenty of raw
`<` and `"` inside bodies (markdown, code). One of them even quotes a literal
`<subagent agent_id="…" …>` tag inside a body. So:

- No entity decoding. Attribute values and bodies are used verbatim.
- A block starts only at a line that begins with `<subagent agent_id="`
  (after optional leading whitespace) and whose opening tag closes with `>` on
  the same line. `agent_id`, `item`, and `outcome` are read from that opening
  tag.
- A block's body runs to the **last** `</subagent>` before the next block
  start or `</agent_swarm_result>`, whichever comes first.
- A block without a closing tag, a missing `agent_id`, or a missing
  `</agent_swarm_result>` makes the whole result unusable, and the
  `args.items` fallback applies.

A unit test uses a body that contains a quoted `<subagent …>` line in the
middle of a paragraph (not at line start) and an inline `</subagent>`, taken
from the real sample.

Nested swarms or `Agent` calls inside a child journal follow the same rules
recursively, scoped to that child's own children.

### 6. Token usage

New precedence:

1. Sum every `usage.record` payload exactly once, whatever its `usageScope`
   (`turn`, `session`, missing, or unknown). Both scopes are deltas upstream.
2. If there is no `usage.record` at all, sum the `step.end.usage` values.
3. Otherwise leave usage absent.

The field mapping is unchanged. The test
`models_follow_steps_and_turn_usage_wins_without_double_counting` gains a
`session`-scoped record that must be counted.

### 7. Re-indexing existing data

Existing Kimi bundles have unchanged fingerprints, so they would not be
re-parsed. Following the established precedent (see the version notes in
`crates/core/src/database/schema.rs`), bump the database version and clear
fingerprints in the migration. Pick the next free version at implementation
time: version numbers have collided between branches before. Clearing only
Kimi fingerprints is preferable if the fingerprint storage makes that simple;
otherwise clear them all, as earlier migrations did.

## Fixtures and Tests

- Rewrite the child journals under `tests/fixtures/kimi_home` (`agent-0`,
  `agent-1`, `agent-nested`) with real prompt shapes:
  `origin: {"kind":"system_trigger","name":"subagent"}` and a leading
  `<git-context>` block in the first task prompt. Keep at most one deliberately
  synthetic case, and label it as such in the test name.
- Add a fixture bundle with an `AgentSwarm` call (result with `<subagent>`
  blocks, children with `labels.swarmItem` / `labels.profileName`).
- Add a fixture bundle with a resumed subagent (`args.resume` plus a call
  identified only by the result header), mirroring `session_d7f7b8ed`.
- Unit tests, at minimum:
  - main journal: a `system_trigger`/`subagent` prompt is still rejected;
  - child journal: accepted, `<git-context>` stripped from message and title,
    unclosed block kept;
  - result header and `[summary]` cleanup, including the failed form;
  - agent name precedence;
  - resume: three calls → three rows → one child;
  - swarm: result path, `args.items` fallback, all-or-nothing on mismatch,
    item order, `{{item}}` substitution, transcript index consistency after
    expansion;
  - forked child: copied `context.append_message` records are not surfaced;
  - usage: `session`-scoped records counted.
- Integration (`tests/kimi_code_integration.rs`): child sessions and subagent
  rows are persisted for the new fixtures, and re-indexing is stable.

## Documentation

Update the "Parser Behavior (Sessions Chronicle)" section of
`docs/session-formats/kimi-code.md`: remove the "known gap" notes this work
closes, and document `AgentSwarm` (call args, result block, `swarmItem`), the
`<git-context>` prefix, resume behavior, and the new usage rule. Update
`docs/PROJECT_STATUS.md` if it lists the gap.

## Acceptance Criteria

- CI-parity checks pass (`cargo fmt`, `cargo clippy -D warnings`, headless
  `cargo test --all`).
- On local data, after re-indexing:
  - `session_d7f7b8ed-…` shows four subagent items. Three of them open
    `agent-1`'s child session and one opens `agent-0`'s.
  - `session_acdf68b7-…` shows five consecutive subagent items titled with
    the swarm items, each opening its child.
  - `session_3829c3d5-…` (failed swarm result) links its three children through
    the `args.items` fallback, with `Session closed` as each item's summary.
    If the items do not match exactly, the call stays generic. Either outcome
    is acceptable as long as no link is wrong.
  - Child transcripts start with the task text, not `<git-context>`.
  - Main sessions that have no genuine user prompt are still not indexed.

## Builder's Calls (Summary)

- Keep the unstripped text when `<git-context>` is unclosed or would leave
  nothing.
- `labels.parentAgentId` as a fallback for the parent link.
- Swarm all-or-nothing expansion, and the `args.items` fallback rule.
- Swarm items are ordered by `args.items`.
- A swarm result whose `<subagent>` blocks do not cover exactly `args.items`
  counts as unusable.
- Fingerprint clearing: Kimi-only if simple, otherwise global.
