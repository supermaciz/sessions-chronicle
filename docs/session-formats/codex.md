# Codex — Session Format Reference

Format reference for Codex rollout session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

**Last checked: 2026-10-01.** Protocol, response-item and turn-item types,
recorder, and compression compared between upstream commits
`ac192cd7937b0d73edc6dffe009940ae53782dd4` and
`83cf88306e806b409f873b63544de3f1224f0fc3`. Protocol additions and persistence
policy also checked at stable `rust-v0.159.3` (released 2026-09-30).
The persistence policy is unchanged between the two inspected commits and
matches 0.159.3. Shared-history reconstruction evidence below remains pinned
to the September commit; its resolver files were not rechecked in this pass.
Version introduction dates and population-wide prevalence of paginated history
were not established. No new local session capture or reproduction test was run.
Local sampling below dates from 2026-09-06.

**Format watch 2026-10-25/26: stable `rust-v0.164.0`** (`6aaac694`, released
2026-10-23; `main` `970111f6` unchanged on 2026-10-26). Persistence policy,
protocol and turn-item types were diffed against `rust-v0.159.3`; see
[Rollout Changes in 0.162–0.164](#rollout-changes-in-0162-0164). Storage paths
(`sessions/`, `archived_sessions/`) and rollout file naming are unchanged. No
new local rollout was captured.

**Desktop/import watch: 2026-10-04; documented 2026-10-11.** The focused
check below does not refresh the protocol comparison above.

**Local desktop/import verification: 2026-10-11** (Codex CLI 0.159.3 installed;
latest upstream stable seen: 0.162.1). A read-only scan of `~/.codex` and the
upstream migration source showed that most local rollouts are now `paginated`
(see [Legacy-to-Paginated Migration](#legacy-to-paginated-migration-and-local-impact)).
This supersedes the 2026-09-06 local sampling in
[Local Evidence and Remaining Work](#local-evidence-and-remaining-work) as a
statement of what is on disk today. The protocol comparison was not refreshed.

## ChatGPT Desktop Linux and Imported Conversations

See the dedicated [ChatGPT desktop watch](chatgpt-desktop.md) for native
ChatGPT/Work storage unknowns and the capture checklist.

### Confirmed Product Boundaries

The official [Linux app documentation](https://learn.chatgpt.com/docs/linux/linux-app)
describes a preview desktop application with Codex and local-project access.
This product availability does not establish a new transcript format.

The [import documentation](https://learn.chatgpt.com/docs/import) describes
desktop imports from Claude Code, Claude Cowork, and Cursor, including automatic
updates of imported work. Codex CLI imports from Claude Code and Cursor.
Configuration migration and conversation migration are separate concerns.

The [projects and chats documentation](https://learn.chatgpt.com/docs/projects)
also describes adding an existing ChatGPT chat to a Codex chat, while explicitly
keeping Work conversation history separate from Codex history. Consequently,
not every conversation visible in the desktop application can be assumed to
exist as a local Codex rollout.

### Confirmed External-Agent Import Persistence

The inspected upstream
[session importer](https://github.com/openai/codex/blob/de3721a7be07054c8c2a41102b5a501f34155361/codex-rs/app-server/src/external_agent_migration/session_importer.rs)
creates Codex threads from converted external-session rollout items:

- New imports explicitly use `ThreadHistoryMode::Legacy` and `history_base: None`.
- Items pass through `into_persisted_rollout_items` before being appended through
  the thread store.
- Completed-import records associate `source_path`, `source_content_sha256`,
  and `imported_thread_id`, with optional title and connector information.
- The importer also supports appending to previously imported sessions.

This is evidence for the external-agent import path in upstream source, not
proof that every released Linux desktop build uses it. Its introduction version
was not established. It does not establish the persistence mechanism for adding
a native ChatGPT conversation to Codex.

### Likely Local Impact

New imports are written as `legacy`, but locally most imported rollouts are
`paginated` (older importer versions, or the startup migration), and the current
parser rejects those; see the ledger table below. Sessions Chronicle does not
consult the import provenance ledger. An original session and a still-`legacy`
imported counterpart appear as separate sessions (5 locally with identical first
prompts). Missing sessions are the larger risk than duplicates.

### Local Import Ledger (observed 2026-10-11)

`~/.codex/external_agent_session_imports.json` has `records[]` with
`source_path`, `content_sha256`, `imported_thread_id`, `imported_at`,
`source_modified_at`, `connector_names`, and `title`, plus
`detected_connector_records`. The importer always creates a new thread ID
(`ThreadId::new()`); the imported ID is unrelated to the source session ID, so
the ledger is the only provenance link.

The local ledger held 41 records, all from Claude Code transcripts. Counts below
are file counts from one machine, not prevalence estimates:

| Imported rollout | Claude Code original | Files | Effect in Sessions Chronicle on 2026-10-11 (by code inspection) |
|------------------|----------------------|-------|---------------------------------------------------|
| `paginated`, no `user_message` | gone | 16 | Rejected (`NoUserMessages`); the session is missing |
| `paginated`, no `user_message` | present | 14 | Copy rejected; original visible; no duplicate |
| `legacy`, has `user_message` | present | 11 | Copy indexed; 5 share the original's first prompt (duplicate), 6 differ (cause not established) |

The 11 `legacy` imports come from `0.162.0-alpha` and are expected to be rewritten
to `paginated` by the startup migration below, which would also remove the
duplicate. That expectation is not verified. Why 16 originals are gone was not
established.

**Update 2026-10-16:** the rejections in the table describe the parser before
paginated-message support. Paginated rollouts with `UserMessage` items are now
indexed (see [Paginated Item Shapes](#paginated-item-shapes-captured-2026-10-11)),
so the missing-session and duplicate-original effects need a fresh comparison.

### Unknowns and Next Evidence

The native ChatGPT/Work storage location and schema on Linux remain unknown
from the checked sources. Do not infer a supported ChatGPT source from desktop
integration alone. Conversion fidelity for tool calls, attachments, subagents,
and per-record timestamps also remains unverified.

The smallest justified follow-up is to compare anonymized captures of a native
Codex chat, an external-agent import, and a ChatGPT chat added to Codex. Check
message completeness, timestamps, tool call linkage, provenance, and repeated
imports before adding fixtures and compatibility tests. No parser change is
justified by this watch alone. Confidence is high for the inspected importer
behavior and medium for desktop coverage; compatibility/provenance risk is medium.

### Legacy-to-Paginated Migration and Local Impact

**Confirmed upstream:** Codex runs a startup migration (`legacy_to_paginated_v1`)
that rewrites legacy rollouts as `paginated`, tracking progress with a
creation-ordered cursor in SQLite and a `.pending` journal under
`~/.codex/rollout-migrations/`. Sources:
[`startup.rs`](https://github.com/openai/codex/blob/main/codex-rs/thread-store/src/local/rollout_migration/startup.rs)
(commits `4bb7ee347`, 2026-08-07; `465eafacb`, 2026-08-25) and
[`publish.rs`](https://github.com/openai/codex/blob/main/codex-rs/thread-store/src/local/rollout_migration/publish.rs).
The release that first shipped it was not established. `rollback.rs` was not read,
so reversibility is unknown.

**Observed 2026-10-11:** nearly all local rollouts are `paginated` with
`item_completed` and no `event_msg.user_message`, including very old ones (for
example a `0.58.0` file from 2025-11-14). Only 11 files are `legacy` (all
`0.162.0-alpha` desktop imports). This contradicts the earlier sampling (9 of 348
paginated), which predates the migration.

**Observed 2026-10-15:** a read-only scan of 413 local rollouts found every one
`paginated`, no legacy `event_msg.user_message`, 30 without any `UserMessage`
item (14 empty roots with at most 5 lines, 16 subagent children), no non-null
`history_base`, and 83 rollouts with `CommandExecution` items that all also
contain `function_call` response items. A child's inbound `agent_message`
response item carries a clear header (`Message Type`, `Task name`, `Sender`, an
empty `Payload:`) plus `encrypted_content`, so the delegated task prompt is
readable only on the parent side (the `spawn_agent` `message` argument).
Multi-agent `spawn_agent` output is `{"task_name": "/root/<name>"}` with no
`agent_id`.

**Implemented 2026-10-16:** `crates/core/src/parsers/codex.rs` reads the history
mode from the first `session_meta` and, in `paginated` mode, builds messages from
`item_completed` items instead of legacy events (see
[Parser Behavior](#parser-behavior-sessions-chronicle)). Before this change the
parser rejected such rollouts with `NoUserMessages`, which made almost every
local rollout unreadable by code inspection.

### Paginated Item Shapes (captured 2026-10-11)

Anonymized structural capture of a `codex_work_desktop` rollout (`0.160.0`,
`source: vscode`, `history_mode: paginated`, 174 lines). It was an ordinary
coding session stored in `~/.codex/sessions/`, not a ChatGPT chat. One capture
is a sample, not a contract.

`item_completed` carried 41 items:

| `item.type` | Count | Notes |
|-------------|-------|-------|
| `CommandExecution` | 19 | `command[]`, `cwd`, `status`, `stdout`, `stderr` (0.160.0 capture; since 0.162.0 only `aggregated_output` remains — see [Rollout Changes in 0.162–0.164](#rollout-changes-in-0162-0164)) |
| `AgentMessage` | 9 | `content[].text`, `phase` (for example `commentary`) |
| `UserMessage` | 4 | `content[].text`, `text_elements[]` |
| `Reasoning` | 4 | `summary_text[]`, `raw_content[]` |
| `Extension` | 3 | `kind: web.search`, `query`, `results[]` |
| `FileChange` | 2 | `changes{path -> {type, unified_diff}}`, `status` |

Each wrapper has `thread_id`, `turn_id`, `started_at_ms`, `completed_at_ms`.
Every line also carries an `ordinal`. User prompts exist only as `UserMessage`
items.

Other observed shapes:

- `session_meta.payload` adds `session_id`, `creator_user_id`,
  `creator_account_id`, `runtime_workspace_roots`, `thread_source`,
  `base_instructions.provenance`, `context_window.window_id`, and `git`.
- New top-level types: `token_usage_record` (22; `usage` and `turn_token_usage`)
  and `world_state` (2). New `event_msg` type: `thread_settings_applied` (5).
- `token_count` events were still present (22). `token_usage_record` is not
  handled by the parser; whether it carries data absent from `token_count` was not
  compared.
- Tool calls also appear as `response_item` `function_call` (3) and
  `custom_tool_call` (15) with matching outputs. The parser already handles both;
  `CommandExecution` items would be a second representation, and whether they
  duplicate those calls was not checked.

**Smallest justified parser change:** read `UserMessage` and `AgentMessage`
`item_completed` items so these rollouts are indexed. Deduplicating against tool
calls and imported originals needs separate evidence.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Active path** | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| **Archived path** | `~/.codex/archived_sessions/rollout-*.jsonl[.zst]` |
| **Patterns** | `rollout-*.jsonl`, `rollout-*.jsonl.zst` |
| **Example** | `rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl` |
| **Format** | Line-oriented JSON; active rollouts are append-only JSONL, while cold archived rollouts can be Zstandard-compressed JSONL |

**Date sharding:**

```
~/.codex/sessions/2026/01/18/rollout-2026-01-18T02-01-28-019bce9f-0a40-79e2-8351-8818e8487fb6.jsonl
                  └─────────┘          └─────────────────────────────────────────────────────────┘
                  Date sharding        Timestamp + thread id in filename
```

Archived sessions use a flat directory rather than date sharding. Current
upstream Codex can compress cold archived rollouts from `rollout-*.jsonl` to
`rollout-*.jsonl.zst` and transparently read either representation.

Sessions Chronicle discovers `rollout-*.jsonl` and `rollout-*.jsonl.zst` in the
configured session directory. When that directory is named `sessions` or
`codex_sessions`, discovery also includes a sibling `archived_sessions/`, even
if the active directory is absent. Compressed files are decoded as a stream.

The inspected recorder also supports `rollout-<timestamp>-<thread_id>_<rollout_id>.jsonl`
after `thread/revert`. A filename's rollout identity must not be assumed to
equal the stable thread ID in `session_meta.payload.id`.

---

## Event Structure

Codex rollout logs are envelope-based JSONL entries (`RolloutLine`). The following
is a schematic example of the historically supported envelope types, not an
exhaustive current enum:

```json
{
  "timestamp": "2026-01-18T01:01:30.000Z",
  "type": "session_meta" | "event_msg" | "response_item" | "turn_context" | "compacted",
  "payload": { "...": "..." }
}
```

### Session Metadata (`session_meta`)

```json
{
  "timestamp": "2026-01-18T01:01:28.000Z",
  "type": "session_meta",
  "payload": {
    "id": "019bce9f-0a40-79e2-8351-8818e8487fb6",
    "timestamp": "2026-01-18T01:01:28.000Z",
    "cwd": "/home/user/project",
    "originator": "codex_cli_rs",
    "cli_version": "0.117.0",
    "source": "cli",
    "model_provider": "openai"
  }
}
```

`source` is now a structured `SessionSource` in upstream types. It can still be a
simple source like `cli`, `vscode`, `exec`, or `unknown`, but it can also be a
structured subagent/custom value.

Representative shapes:

```json
"source": "cli"
```

```json
"source": {
  "sub_agent": {
    "thread_spawn": {
      "parent_thread_id": "thr_parent",
      "depth": 1,
      "agent_path": "/root/agent_a",
      "agent_nickname": "agent_a",
      "agent_role": "reviewer"
    }
  }
}
```

Additional `session_meta` fields now present in upstream types include
`forked_from_id`, direct `parent_thread_id`, `thread_source`, `agent_nickname`,
`agent_role`, `agent_path`, `base_instructions`, `dynamic_tools`,
`memory_mode`, and `multi_agent_version`.

**Confirmed additions since the September reference:** the
[0.159.3 protocol](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/protocol/src/protocol.rs)
also defines optional `creator_user_id`, `creator_account_id`, and
`runtime_workspace_roots`. The account identifier records the account selected
at thread creation and is not updated on resume. Workspace roots distinguish
an absent (unknown) value from an empty list (no roots). Sessions Chronicle
currently ignores these fields; they do not replace the thread `id` or `cwd`.

For spawned child sessions, current rollouts can carry the parent identifier in
both `session_meta.payload.parent_thread_id` and the structured
`source.subagent.thread_spawn.parent_thread_id`. Sessions Chronicle currently
uses the structured `source` value first, then direct `parent_thread_id` as a
child-session linkage fallback.

### Rollout Changes in 0.162–0.164

Confirmed by diffing `rust-v0.159.3` against
[`rust-v0.164.0`](https://github.com/openai/codex/releases/tag/rust-v0.164.0)
(format watch 2026-10-25). None of these changes touches a field the parser
requires (`turn_context.payload.model` / `cwd`, message items, tool-call
linkage); unknown `response_item` types are already ignored.

- **`additional_tools` response items are persisted (0.162.0+).**
  [`should_persist_response_item`](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/rollout/src/policy.rs)
  now returns `true` for `ResponseItem::AdditionalTools` (it was `false` at
  0.159.3), so rollouts can contain `response_item` lines of type
  `additional_tools`. Their on-disk shape has not been captured locally.
- **Command and MCP output truncation in paginated history (0.162.0+).**
  `CommandExecution` `aggregated_output` is truncated to 64 KiB
  (`PERSISTED_COMMAND_OUTPUT_MAX_BYTES`) with the marker
  `... command output truncated for persistence ...`; persisted MCP results are
  capped at 64 KiB (`PERSISTED_MCP_RESULT_MAX_BYTES`).
- **`CommandExecutionItem` / `ExecCommandEndEvent` lose `stdout`, `stderr` and
  `formatted_output` (0.162.0).** Only `aggregated_output` remains
  ([`items.rs`](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/protocol/src/items.rs)).
  `exec_command_end` is not persisted by the policy, so the parser's
  `stdout` / `stderr` fallback only matters for old legacy rollouts.
- **`turn_context` slimmed (0.162.0,
  [#51492](https://github.com/openai/codex/pull/51492)).**
  [`TurnContextItem`](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/protocol/src/protocol.rs)
  no longer persists `workspace_roots`, `current_date`, `timezone`, `network`,
  `personality` or `multi_agent_mode`; `summary` is optional (written as the
  legacy placeholder `none`). `model` and `cwd` remain required. The optional
  `turn_id`, `root_turn_id`, `disabled_plugin_ids` and
  `active_permission_profile` fields are already present at 0.159.3.
- **Sub-agent activities carry `model` and `reasoning_effort`** (optional,
  0.162.0, [#51463](https://github.com/openai/codex/pull/51463)).

### History Modes and Shared Prefixes

**Confirmed upstream:** `SessionMeta.history_mode` distinguishes `legacy` and
`paginated`; deserialization defaults to `legacy` when the field is absent.
In the [0.159.3 persistence policy](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rollout/src/policy.rs),
paginated history persists `event_msg.payload.type == "item_completed"` carrying
typed `TurnItem` values. Legacy `user_message` and `agent_message` events are
persisted only in `legacy` mode. Some `item_completed` events are also retained
in legacy mode, including function-call outputs, plans, and completed subagent
activities; the event alone does not identify the history mode.

**Implemented 2026-10-16:** in `paginated` mode the parser reads `UserMessage`
and `AgentMessage` items from `item_completed` and ignores legacy
`user_message` / `agent_message` events; in `legacy` mode (including an absent
or unrecognized `history_mode`) it reads only the legacy events. Only the first
`session_meta` selects the mode. The parser still ignores response-item
messages, which include injected developer, `AGENTS.md` and plugin context.

**Remaining gaps:** `history_base` shared-prefix reconstruction, typed
non-message items (`CommandExecution`, `FileChange`, `McpToolCall`, `WebSearch`,
`Extension`, `Plan`), non-text `UserMessage` content (`skill`, `mention`,
`local_image`), `sub_agent_activity`, and `wait_agent` / `close_agent` results
that target task names instead of agent IDs are not handled.

**Confirmed metadata contract:** the inspected
[protocol types](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/protocol/src/protocol.rs)
define optional `history_base` as a prefix reference with these fields:

| Field | Meaning in upstream types |
|-------|---------------------------|
| `thread_id` | Referenced rollout ID, despite the historical field name; not necessarily its stable thread ID |
| `end_ordinal_exclusive` | First rollout ordinal excluded from the inherited prefix |
| `end_byte_offset` | Byte offset immediately after the last included JSONL record |

Related optional metadata includes `forked_from_ordinal_exclusive` (the logical
fork boundary, independent of the physical `history_base`) and
`subagent_history_start_ordinal` (the first record belonging to the child's own
projected history; earlier records are inherited model context). Current types
also distinguish root `session_id` from thread `id`.

**Likely impact:** Sessions Chronicle reads each file independently and ignores
these history fields, so an inherited prefix may be missing from its transcript.

#### Resolving a Shared History

**Confirmed in source at `ac192cd`:** `history_base` freezes a prefix of a
specific physical rollout, not the latest state of its owning thread. Later
appends to the source do not extend the inherited history.

The upstream [`RolloutLineage` resolver](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/rollout_lineage.rs)
performs these steps:

1. Resolve the requested thread's current rollout. The
   [thread resolver](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/thread_rollout_resolver.rs)
   prefers its live writer, then SQLite's selected path. If SQLite identifies a
   paginated thread but its selected file cannot be resolved, it does not fall
   back to an older file with the same thread ID.
2. Read its metadata and follow `history_base.thread_id` recursively. Ancestor
   lookup uses the rollout ID parsed from filenames, scanning `sessions/` before
   `archived_sessions/`, including compressed representations. Unlike current-thread
   lookup, [ancestor lookup](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/rollout/src/list.rs#L1630)
   does not consult SQLite to choose a thread's current file.
3. Give each ancestor the frozen end boundary from its descendant's reference.
   The requested rollout itself has no inherited end bound. Reverse the collected
   segments to obtain oldest-ancestor-to-child order. Intermediate segments can
   be empty; they still link to earlier inherited history.

**Ordinals are record positions, not message counts or physical line numbers.**
The [recorder's ordinal state](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/rollout/src/ordinal.rs)
starts a standalone paginated file at ordinal `0`. With a base ending at `N`,
the new file's initial `session_meta` receives ordinal `N`, and its first local
record receives `N + 1`. The ancestor contributes records strictly below `N`.
The lineage's local segment starts at `1`, or `history_base.end_ordinal_exclusive + 1`,
so each file's initial metadata is excluded from projected history.

Illustrative boundaries (not a captured session):

| Segment | Initial metadata ordinal | Contributing record ordinals |
|---------|--------------------------|------------------------------|
| Root A, frozen by B at `4` | `0` | `[1, 4)` |
| B, inheriting A and frozen by C at `5` | `4` | `[5, 5)` — empty |
| C, inheriting B | `5` | `[6, ...)` |

This empty-middle case is explicitly covered by
[`resolves_nested_lineage_with_empty_intermediate_segments`](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/rollout_lineage_tests.rs#L50).
It would be incorrect to stop traversal when an intermediate file contributes
no messages.

#### Byte Bounds, Compression, and Invalid References

**Confirmed:** `end_byte_offset` addresses the original, uncompressed JSONL byte
stream, including record delimiters. It is not an offset into `.zst` bytes.
The upstream [seekable reader](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/rollout/src/seekable_reader.rs)
opens a plain file or decodes a compressed representation into an anonymous
temporary file for bounded-memory seek access. Normal reads do not publish a
plain copy beside the archive. Prefix-size validation checks the logical size;
for compressed data it can use frame-size information or decode up to the bound.

The lineage resolver rejects missing source rollouts, cycles, non-paginated
sources, ordinal overflow, zero exclusive-ordinal cutoffs, and byte bounds past
the logical end of the source. Missing files and cycles are errors, not empty
prefixes. Its cutoff validator checks the nonzero ordinal and available byte
length; it does **not** by itself prove that a supplied byte boundary matches
that ordinal or falls exactly on a JSONL record boundary. Do not describe it as
complete validation of arbitrary metadata. The
[upstream negative tests](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/rollout_lineage_tests.rs#L185)
cover missing ancestors, cycles, and out-of-bounds offsets.

Preparing a new reference differs from reading one: Codex can materialize a
previously standalone compressed source as plain JSONL for older-reader
compatibility. Already-shared compressed ancestors stay read-only. That creation
path also checks canonical managed roots to reject symlink escapes.

#### Fork, Revert, and Child Projection

**Confirmed:** [fork preparation](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/paginated_fork.rs)
flushes pending source writes and uses persisted projection positions to choose
`Latest`, `ThroughTurn`, or `BeforeTurn` boundaries. `ThroughTurn` excludes
in-progress turns. A boundary can lie in an ancestor; a boundary at the start
of a segment collapses to its inherited predecessor, or `None` at the root.
Boundaries beyond the already-inherited prefix are rejected.

[`thread/revert`](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/revert_thread.rs)
creates a replacement rollout referencing the retained prefix, preserves the
thread ID, and switches SQLite's selected path. Old files remain intact. Thus
`history_base.thread_id` can identify an older rollout of the **same thread**;
it is not a parent-session linkage field. The logical fork cutoff remains
separate and can shrink when reverting into inherited history. The
[metadata helper](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/rollout/src/metadata.rs#L117)
only infers a missing logical cutoff from `history_base` in unambiguous older
cases; it does not blindly substitute the physical base for the fork parent.

`subagent_history_start_ordinal` has another purpose: records below that boundary
are inherited model context and produce no child turn/item changes in
[history materialization](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/thread_history_materialization.rs#L221).
It also applies to inherited records physically copied into a child file with
no `history_base`.

**Transcript and model context are different consumers.** The
[model-context reader](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/model_context.rs)
scans bounded segments backwards, can stop at a usable compaction checkpoint
plus required turn context, and returns canonical metadata for the requested
thread. That optimized replay is not the complete visible transcript. The
[thread-history reader](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/thread-store/src/local/thread_history/read.rs)
uses projected turns/items within lineage ordinal bounds. A transcript importer
must not reuse model-context truncation as its display-history rule.

#### Local Evidence and Remaining Work

**Observed on 2026-09-06 (superseded for current disk state by the 2026-10-11
scan above):** a read-only scan of `~/.codex/sessions/` and
`~/.codex/archived_sessions/` found 348 plain rollouts and no `.jsonl.zst` files.
Initial metadata identified 9 paginated files (5 from `0.147.0`, 4 from `0.153.4`),
53 legacy files, and 286 files without `history_mode`. A subsequent scan of all
352 `session_meta` records, including later metadata in the same file, found
**no non-null `history_base` or `forked_from_ordinal_exclusive`**. Two `0.147.0`
records had `subagent_history_start_ordinal` values `20` and `43`, with no base.
The paginated files contained `item_completed` and no legacy `user_message` or
`agent_message` events. This is a live-directory snapshot, not a prevalence
estimate; no prompt content or private identifiers are reproduced here.

**Evidence limit:** chained, compressed, and reverted histories are established
from official source and inspected upstream tests, not local captures. Upstream
model-context tests exercise frozen cutoffs and compressed multi-segment replay;
revert tests exercise repeated reverts with a stable thread ID. These tests were
read, not executed during this documentation pass. Introduction versions remain
unknown; these reconstruction rules are pinned to the inspected commit.

**Recommended local work:** add synthetic, source-derived fixtures for frozen
prefixes, empty intermediate segments, archived/compressed ancestors, repeated
reverts, missing sources, cycles, and copied child context. Keep rollout identity
separate from session identity and preserve read-only, bounded-memory access.
How Sessions Chronicle selects the current rollout, reports incomplete lineages,
and invalidates its index when an ancestor changes still needs an implementation
decision; its current per-file parser does not implement these rules.

### User / Assistant Events (`event_msg`)

The examples below describe legacy message events. See history modes above for
the paginated representation.

```json
{
  "timestamp": "2026-01-18T01:01:30.000Z",
  "type": "event_msg",
  "payload": {
    "type": "user_message",
    "message": "Summarize the repo"
  }
}
{
  "timestamp": "2026-01-18T01:01:31.000Z",
  "type": "event_msg",
  "payload": {
    "type": "agent_message",
    "message": "Here is the summary"
  }
}
```

**Confirmed additions since the September reference:** legacy `user_message`
events can now carry `file_ids`, `file_id_details`, and `image_order`, alongside
the existing inline `images`. `image_order` contains `inline` / `file` values
that preserve the original ordering across the two reference lists. When it
is absent, upstream consumers retain inline-then-file ordering. File IDs are
opaque references, not local image paths. These fields are defined in the
[0.159.3 protocol](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/protocol/src/protocol.rs).
The local parser reads only `message` from these user events, so image-reference
coverage remains incomplete; no fresh multimodal fixture was captured.

The same protocol comparison confirms optional `root_turn_id` on
`turn_started`, `turn_id` on MCP begin/end events (defaulting for older records),
and optional `mcp_app_ui` on MCP events. The latter describes a UI resource and
display preference; it is also available on the typed MCP turn item. These
additions do not establish that every protocol event is persisted: the
[persistence policy](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rollout/src/policy.rs)
still excludes MCP begin events and persists MCP end events only in legacy mode.

### Turn Context (model captured per turn)

```json
{
  "timestamp": "2026-01-18T01:01:29.500Z",
  "type": "turn_context",
  "payload": {
    "cwd": "/home/user/project",
    "approval_policy": "on-request",
    "sandbox_policy": { "type": "workspace-write" },
    "model": "gpt-5.1-codex",
    "summary": "auto"
  }
}
```

### Session Configured Event (can include model + provider)

Historical/protocol shape supported by the parser. The inspected 0.159.3
persistence policy does not persist `session_configured`; use `turn_context`
and `session_meta` for durable model/provider metadata.

```json
{
  "timestamp": "2026-01-18T01:01:28.500Z",
  "type": "event_msg",
  "payload": {
    "type": "session_configured",
    "session_id": "019bce9f-0a40-79e2-8351-8818e8487fb6",
    "model": "codex-mini-latest",
    "model_provider_id": "openai"
  }
}
```

### Tool-Related Events

These are historical/protocol event shapes. In the inspected 0.153.4 policy,
`exec_command_begin`, `exec_command_end`, `mcp_tool_call_begin`, and
`web_search_begin` are transient; `mcp_tool_call_end` and `web_search_end` are
retained only in legacy mode. Protocol variants are not a persistence guarantee.

```json
{
  "type": "event_msg",
  "payload": {
    "type": "mcp_tool_call_begin",
    "call_id": "call_123",
    "invocation": {
      "server": "filesystem",
      "tool": "read_file",
      "arguments": { "path": "README.md" }
    }
  }
}
{
  "type": "event_msg",
  "payload": {
    "type": "mcp_tool_call_end",
    "call_id": "call_123",
    "result": { "Ok": { "is_error": false, "content": [] } }
  }
}
```

Other tool-related `event_msg.payload.type` variants: `exec_command_*`, `web_search_*`.

### Skill Invocation Format

Codex CLI skill usage is persisted as an explicit user invocation plus a
separate injected payload. In sampled rollouts, there is no dedicated
Codex-native skill tool-call event analogous to OpenCode's `tool == "skill"`.

**1. Explicit invocation** — `user_message`:

```json
{
  "type": "event_msg",
  "payload": {
    "type": "user_message",
    "message": "$logseq un fichier markdown",
    "text_elements": [
      {
        "byte_range": { "start": 0, "end": 7 },
        "placeholder": "$logseq"
      }
    ]
  }
}
```

**2. Injected skill payload** — `response_item` user message:

```xml
<skill>
<name>logseq</name>
<path>/home/user/project/skills/logseq/SKILL.md</path>
---
name: logseq
description: ...
---
...
</skill>
```

Observed semantics:

- Explicit invocation appears in `event_msg.payload.type == "user_message"`
  with a leading `$skill-name`
- The loaded skill payload is injected as `response_item.payload.type ==
  "message"` with `role == "user"`
- The injected payload uses a `<skill>` wrapper with `<name>` and `<path>`
  headers, followed by the skill frontmatter/body
- `text_elements[].placeholder` can preserve the exact `$skill-name` token but
  is not consistently populated
- In sampled local sessions, every injected `<skill>` payload was preceded by
  an explicit `$skill-name` user message
- If Codex reports that a named skill is unavailable, the rollout can contain
  the `$skill-name` user message without a following `<skill>` payload

### Collaboration / Subagent Events

Current upstream Codex protocol exposes subagent work through the `collab_*`
`event_msg.payload.type` family. These events are emitted by the multi-agent
callable surface (`spawn_agent`, `send_message`, `followup_task`,
`wait_agent`, `close_agent`, and resume flows).

**Persistence caveat (confirmed at 0.153.4):** the `collab_*` events listed below
are transient and are not written by the inspected persistence policy. These
examples remain relevant to older files and parser compatibility tests.
The policy instead retains `sub_agent_activity` events for non-completed
activities in legacy mode and completed subagent activities through
`item_completed`. Sessions Chronicle does not handle either activity carrier.

```json
{
  "type": "event_msg",
  "payload": {
    "type": "collab_agent_spawn_begin",
    "call_id": "spawn_1",
    "started_at_ms": 1770000000000,
    "sender_thread_id": "thr_parent",
    "prompt": "Investigate failing tests",
    "model": "gpt-5.1-codex",
    "reasoning_effort": "medium"
  }
}
{
  "type": "event_msg",
  "payload": {
    "type": "collab_agent_spawn_end",
    "call_id": "spawn_1",
    "completed_at_ms": 1770000001000,
    "sender_thread_id": "thr_parent",
    "new_thread_id": "thr_child",
    "new_agent_nickname": "reviewer-a",
    "new_agent_role": "reviewer",
    "prompt": "Investigate failing tests",
    "model": "gpt-5.1-codex",
    "reasoning_effort": "medium",
    "status": "completed"
  }
}
```

Current collab event types:

- `collab_agent_spawn_begin` / `collab_agent_spawn_end`
- `collab_agent_interaction_begin` / `collab_agent_interaction_end`
- `collab_waiting_begin` / `collab_waiting_end`
- `collab_close_begin` / `collab_close_end`
- `collab_resume_begin` / `collab_resume_end`

Current parser-relevant fields:

| Event | Key fields |
|-------|------------|
| `collab_agent_spawn_end` | `call_id`, `sender_thread_id`, `new_thread_id`, `new_agent_nickname`, `new_agent_role`, `prompt`, `model`, `reasoning_effort`, `status`, `completed_at_ms` |
| `collab_agent_interaction_end` | `call_id`, `sender_thread_id`, `receiver_thread_id`, `receiver_agent_nickname`, `receiver_agent_role`, `prompt`, `status`, `completed_at_ms` |
| `collab_waiting_end` | `call_id`, `sender_thread_id`, `agent_statuses[]`, `statuses{thread_id -> status}`, `completed_at_ms` |
| `collab_close_end` | `call_id`, `sender_thread_id`, `receiver_thread_id`, `receiver_agent_nickname`, `receiver_agent_role`, `status`, `completed_at_ms` |
| `collab_resume_end` | `call_id`, `sender_thread_id`, `receiver_thread_id`, `receiver_agent_nickname`, `receiver_agent_role`, `status`, `completed_at_ms` |

`AgentStatus` currently serializes with these variants:

- `"pending_init"`
- `"running"`
- `"interrupted"`
- `{ "completed": "<final assistant message>" }` or `{ "completed": null }`
- `{ "errored": "<error message>" }`
- `"shutdown"`
- `"not_found"`

Notes:

- Timing fields are useful for ordering and future duration display, but the
  current parser uses the rollout-line `timestamp` for session ordering.
- `model` and `reasoning_effort` describe the effective spawned agent settings.
  They are currently not stored by Sessions Chronicle's subagent model.
- `collab_resume_end` has the same status-bearing shape as close/interaction
  events. The current parser uses it for subagent summary enrichment, with
  priority over earlier waiting and close updates.
- `wait_agent` can emit `collab_waiting_end` with empty `agent_statuses` and
  empty `statuses`; treat this as no per-agent status update.

### Subagent Tool Calls in Response Items

Local Codex `0.130.0` rollouts can persist subagent operations as ordinary
`response_item` tool calls instead of `event_msg.payload.type == "collab_*"`.
This shape was observed in a real local parent rollout with
`originator == "codex-tui"` and `cli_version == "0.130.0"`.

Spawn call:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call",
    "call_id": "call_spawn",
    "name": "spawn_agent",
    "arguments": {
      "agent_type": "product-manager",
      "message": "...",
      "reasoning_effort": "medium"
    }
  }
}
```

Spawn output:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call_spawn",
    "output": "{\"agent_id\":\"019e382d-e986-7b62-9f97-b015c5cc70f5\",\"nickname\":\"Nord\"}"
  }
}
```

Wait output can also carry a per-agent status map:

```json
{
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call_wait",
    "output": "{\"status\":{\"019e382d-e986-7b62-9f97-b015c5cc70f5\":{\"completed\":\"...\"}},\"timed_out\":false}"
  }
}
```

After `wait_agent` resolves, the same rollout also persists a `response_item`
`message` with `role == "user"` whose `input_text` wraps a
`<subagent_notification>` payload. It duplicates the per-agent status under
`agent_path` and `status`:

```json
{
  "type": "response_item",
  "payload": {
    "type": "message",
    "role": "user",
    "content": [
      {
        "type": "input_text",
        "text": "<subagent_notification>\n{\"agent_path\":\"019e382d-e986-7b62-9f97-b015c5cc70f5\",\"status\":{\"completed\":\"...\"}}\n</subagent_notification>"
      }
    ]
  }
}
```

This marker is informational only: the status it carries is the same as the
`wait_agent` `function_call_output`. The current parser ignores `response_item`
`message` items, so it is not double-counted.

The linked child rollout still uses structured session provenance:

```json
{
  "type": "session_meta",
  "payload": {
    "id": "019e382d-e986-7b62-9f97-b015c5cc70f5",
    "source": {
      "subagent": {
        "thread_spawn": {
          "parent_thread_id": "019e3829-1153-77d3-acc5-8d683325f21d",
          "depth": 1,
          "agent_nickname": "Nord",
          "agent_role": "product-manager"
        }
      }
    },
    "thread_source": "subagent",
    "agent_nickname": "Nord",
    "agent_role": "product-manager"
  }
}
```

Parser implication:

- `spawn_agent` `function_call` creates an unlinked parent-side subagent from
  its arguments. The matching `function_call_output` enriches it via
  `output.agent_id` and optional `output.nickname`. When the output is missing,
  unparseable, or omits `agent_id` (a rejected spawn), the parser keeps the
  unlinked `Subagent` row rather than dropping the spawn from the transcript.
- `wait_agent` `function_call_output` carries terminal summaries in
  `output.status.{agent_id}` and enriches the matching parent-side `Subagent` rows.
- Sessions Chronicle maps response-item `spawn_agent` and `wait_agent` pairs
  into parent-side `Subagent` rows instead of generic tool calls.
- The trailing `<subagent_notification>` `response_item` `message` is ignored
  by the parser and carries no information beyond the `wait_agent` output.

### Encrypted Reasoning

```json
{
  "type": "response_item",
  "payload": {
    "type": "reasoning",
    "encrypted_content": "AAECAwQFBgcICQoL..."
  }
}
```

- Never decrypt locally
- Persist unchanged
- Forward to API to maintain context

### Multimodal Content

Two patterns:

1. **Inline Base64**: `data:image/png;base64,iVBORw0...`
2. **References**: HTTP(S) URLs or file identifiers

---

## Metadata Available in Events

| Field | Description |
|-------|-------------|
| `session_meta.payload.id` | Session/thread identifier |
| `session_meta.payload.cwd` | Working directory |
| `session_meta.payload.source` | Source provenance (`cli`, `vscode`, `exec`, structured subagent/custom variants, ...) |
| `session_meta.payload.model_provider` | Optional session-level provider id |
| `session_meta.payload.originator` | Recorder/runtime identifier |
| `session_meta.payload.cli_version` | CLI version that created the rollout |
| `turn_context.payload.model` | Active model slug for that turn |
| `event_msg` `session_configured` | Can provide `model` + `model_provider_id` |
| Skill invocation | Explicit `$skill-name` `user_message` plus injected `<skill>` payload |

**Model metadata:**

| Level | Field | Notes |
|-------|-------|-------|
| Per message | ❌ Not on `user_message`/`agent_message` payloads | |
| Per turn | ✅ `turn_context.payload.model` | Model slug, captured before sampling |
| Per session | ⚠️ `session_meta.payload.model_provider` | Optional, provider-only (no guaranteed model slug) |

---

## Token Usage

Codex rollouts can include `event_msg` entries with `payload.type == "token_count"`.
These provide **token usage totals for the session** and **the last model call**.

Example (abridged):

```json
{
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 14329,
        "cached_input_tokens": 10496,
        "output_tokens": 540,
        "reasoning_output_tokens": 477,
        "total_tokens": 14869
      },
      "last_token_usage": {
        "input_tokens": 15946,
        "cached_input_tokens": 14720,
        "output_tokens": 65,
        "reasoning_output_tokens": 13,
        "total_tokens": 16011
      },
      "model_context_window": 258400
    }
  }
}
```

Notes:

- `info.total_token_usage` is a running total for the current session/rollout file.
- `info.last_token_usage` is the usage for the most recent model call.
- `cached_input_tokens` is the cached subset of `input_tokens`, not an extra bucket to add on top.
- `reasoning_output_tokens` is exposed as a separate field in the payload and is normalized separately by
  Sessions Chronicle.
- Some `token_count` events can have `info: null` (treat as “unknown”, not zero).

See also: [Codex issue discussion around `token_count` logging](https://github.com/openai/codex/issues/5276).

---

## Parser Behavior (Sessions Chronicle)

Current implementation: [`crates/core/src/parsers/codex.rs`](../../crates/core/src/parsers/codex.rs)

- Selects the message source from the first `session_meta.payload.history_mode`:
  `"paginated"` reads `item_completed` `UserMessage` / `AgentMessage` items, any
  other value or an absent field means `legacy` and reads
  `event_msg.payload.type == user_message|agent_message`. Exactly one source is
  used per rollout
- Joins the `text` entries of an item's `content[]` (entry type compared
  case-insensitively, other entry types skipped) with `"\n"`; items without
  text produce no message. `commentary`, `final_answer` and absent `phase` are
  all kept, and the phase is not stored
- Skips transcript records below `session_meta.payload.subagent_history_start_ordinal`
  in `paginated` mode (the boundary ordinal itself is kept). The filter covers
  `event_msg` and `response_item` carriers before any state is touched, so no
  inherited message, tool call, reasoning or subagent activity leaks into the
  child; `turn_context` stays available for model attribution. Records without a
  valid non-negative integer `ordinal` are retained, and `legacy` mode has no
  such filter
- Gives a child session (resolved parent) with no user message a synthetic first
  prompt, `Delegated task: <name>`, taken from the last non-empty `agent_path`
  segment, else `agent_nickname`, else just `Delegated task`. It is a generated
  label, not the delegated prompt; the real task text stays on the parent's
  `Subagent.prompt`. A rollout without a resolved parent, such as a review
  subagent, and an empty root are still rejected with `NoUserMessages`
- Stores `session_meta.payload.agent_path` and the `task_name` from `spawn_agent`
  output (`Subagent.agent_name`) and links parent rows to child sessions by the
  immediate parent session ID plus the full task path, in either indexing order.
  A duplicated path leaves the row unlinked. An `agent_id` that resolves to a
  session keeps priority over the name
- Indexes tool lifecycle pairs for `mcp_tool_call_begin|end` and `exec_command_begin|end`
- Indexes Codex child rollouts as subagent sessions when `session_meta.payload.source.sub_agent.thread_spawn.parent_thread_id` or `source.subagent.thread_spawn.parent_thread_id` is present
- Uses direct `session_meta.payload.parent_thread_id` as a child-session linkage fallback
- Discovers plain and compressed rollouts, including sibling archives under the directory-name rules above
- Does not handle non-message `item_completed` items, `sub_agent_activity`, or shared-prefix reconstruction via `history_base`
- Indexes `collab_agent_spawn_end` as parent-side `Subagent` rows and transcript items
- Enriches parent-side subagents from `collab_waiting_end`, `collab_close_end`, `collab_resume_end`, and `collab_agent_interaction_end`
- Ignores collab timing fields, spawned-agent `model`, and spawned-agent `reasoning_effort`
- Maps response-item `spawn_agent` / `wait_agent` `function_call` pairs into
  parent-side `Subagent` rows and terminal summaries instead of generic tool calls
- Leaves response-item `close_agent`, `send_message`, and `followup_task` calls
  as generic tool calls
- Does not yet extract Codex skill invocations from `$skill-name` / `<skill>` pairs

**Title extraction:** The first user message from the active message source: the first `UserMessage` item in `paginated` mode, or the first `event_msg.payload.type == "user_message"` event (`payload.message`) in `legacy` mode. Children without one use the synthetic delegated-task label.

**Timestamp parsing:**

- `start_time`: from first-line `session_meta.payload.timestamp`
- `last_updated`: maximum valid top-level event timestamp across all subsequent envelope types, initialized from the session start time

**Content extraction (`legacy` mode only; `paginated` mode uses the item text rule above):**

```rust
fn extract_content_codex_event_msg(event: &Value) -> Option<(Role, String)> {
    let payload = event.get("payload")?;
    match payload.get("type")?.as_str()? {
        "user_message" => Some((Role::User, payload.get("message")?.as_str()?.to_string())),
        "agent_message" => Some((Role::Assistant, payload.get("message")?.as_str()?.to_string())),
        _ => None,
    }
}
```

**Tool call handling:**

- Historical/protocol data uses `event_msg.payload.type` variants:
  `exec_command_*`, `mcp_tool_call_*`, `web_search_*`, and collab `collab_*`;
  local rollouts can also emit tool calls as `response_item` `function_call`
  / `function_call_output`. The current persistence policy above determines
  which protocol events actually reach disk.
- Tool call correlation typically uses `call_id`.
- Current parser behavior: indexes `exec_command_*` and `mcp_tool_call_*`
  begin/end pairs plus `response_item` function calls as tool calls; maps Codex
  `collab_*` lifecycle events into parent `Subagent` rows plus child-session
  linkage when the child rollout is present.

**Streaming:** Use `BufReader` line-by-line iteration for plain JSONL and a
streaming Zstandard decoder for `*.jsonl.zst` — do not load entire rollout logs
into memory.

---

## Primary Sources

- [Codex stable 0.159.3 release](https://github.com/openai/codex/releases/tag/rust-v0.159.3)
- [Codex stable 0.164.0 release](https://github.com/openai/codex/releases/tag/rust-v0.164.0)
- [Codex persistence policy at 0.164.0 (`additional_tools`, 64 KiB truncation)](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/rollout/src/policy.rs)
- [Codex persistence policy, reverified at 0.159.3](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rollout/src/policy.rs)
- [Codex protocol additions, verified at 0.159.3](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/protocol/src/protocol.rs)
- [Codex typed turn items, October inspected commit](https://github.com/openai/codex/blob/83cf88306e806b409f873b63544de3f1224f0fc3/codex-rs/protocol/src/items.rs)
- [Codex persistence policy, verified at 0.153.4](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/policy.rs)
- [Codex protocol `SessionMeta`, `HistoryPosition`, `EventMsg`, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/protocol/src/protocol.rs)
- [Codex response-item types, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/protocol/src/models.rs)
- [Codex recorder, including revert filename support, inspected commit](https://github.com/openai/codex/blob/ac192cd7937b0d73edc6dffe009940ae53782dd4/codex-rs/rollout/src/recorder.rs)
- [Codex turn-context persistence](https://github.com/openai/codex/blob/main/codex-rs/core/src/codex.rs)
- [Codex rollout recorder](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs)
- [Codex rollout compression and compressed-file discovery](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/compression.rs)
- [Codex app-server thread/item event model](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [OpenAI Prompt Caching guide (`cached_tokens` is part of prompt/input usage)](https://developers.openai.com/api/docs/guides/prompt-caching)
